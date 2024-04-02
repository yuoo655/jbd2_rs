use crate::journal;

use super::consts::*;
use super::defs::*;
use super::prelude::*;
use super::transaction::*;

impl JbdFs {
    pub fn journal_start(&mut self) {
        let mut journal = JbdJournal {
            first: self.sb.first,
            start: self.sb.first,
            last: 0,
            trans_id: self.sb.sequence + 1,
            alloc_trans_id: 0,
            block_size: self.sb.blocksize,
            cp_queue: CheckpointQueue::new(),
            block_rec_root: BlockRecordRoot::new(),
            jbd_fs: self,
        };

        journal.jbd_journal_write_sb();

        self.journal = journal;
    }

    pub fn trans_start(&mut self) {
        let new_trans = Transaction::new(Arc::new(self.journal.clone()));
        self.curr_trans = Some(Arc::new(RefCell::new(new_trans)));
    }

    pub fn trans_stop(&mut self) {
        let curr_trans = self.curr_trans.as_mut().unwrap();
        let mut trans = curr_trans.borrow_mut();
        self.journal.commit_trans(&mut trans);

        self.journal.jbd_journal_write_sb();
    }

    pub fn write_trans(&mut self, block: Ext4Block) {
        let curr_trans = self.curr_trans.as_mut().unwrap();
        let mut trans = curr_trans.borrow_mut();
        trans.jbd_trans_set_block_dirty(block);
    }

    pub fn recover(&mut self) -> Result<(), String> {
        if self.sb.start == 0 {
            log::info!("Journal is empty.");
            return Ok(());
        }
        let mut info = RecoverInfo::new();

        self.iterate_log(&mut info, "ACTION_SCAN")?;
        // self.iterate_log(&mut info, "ACTION_REVOKE")?;
        self.iterate_log(&mut info, "ACTION_RECOVER")?;

        self.sb.start = 0;
        self.sb.sequence = info.last_trans_id;
        // self.inode_ref.fs.sb.features_incompatible = features_incompatible;
        self.dirty = true;

        log::info!("Recovery complete.");

        Ok(())
    }

    // layout
    // +------------------+
    // |     Header       |
    // +------------------+
    // | Tag 1            |
    // | - Block Number   |
    // | - Flags          |
    // | - UUID (Optional)|
    // | - Checksum       |
    // +------------------+
    // | Tag 2            |
    // | - Block Number   |
    // | - Flags          |
    // | ...              |
    // +------------------+
    // | ...              |
    // +------------------+
    // | Tag N            |
    // +------------------+
    pub fn iterate_log(&self, info: &mut RecoverInfo, action: &str) -> Result<(), String> {
        log::info!("Iterating log: {}", action);
        let mut log_end = false;
        let mut this_block = self.sb.start.to_be();
        let mut this_trans_id = self.sb.sequence.to_be();

        log::debug!(
            "start_trans_id: {:x?} start_block {:x?}",
            this_trans_id, this_block
        );

        // let mut start_trans_id =
        if action == "ACTION_SCAN" {
            info.trans_cnt = 0;
        } else if info.trans_cnt == 0 {
            log::info!("No transactions to recover.");
            log_end = true;
        }

        // log::info!("Start of journal at trans id: {}", this_trans_id);

        while !log_end {
            let block = self.bdev.read_offset((this_block as usize) * BLOCK_SIZE);

            let header = block.as_ptr() as *const JbdBhdr; // Cast data to JbdBhdr

            if action != "ACTION_SCAN" && trans_id_diff(this_trans_id, info.last_trans_id) > 0 {
                log_end = true;
                continue;
            }

            if unsafe { (*header).magic.to_be() } != JBD_MAGIC_NUMBER {
                log::info!("Invalid magic number found in journal.");
                log_end = true;
                continue;
            }

            let blocktype = unsafe { (*header).blocktype.to_be() };
            match blocktype {
                JBD_DESCRIPTOR_BLOCK => {
                    // log::info!("Descriptor block: {:x?}", this_block);
                    if action == "ACTION_RECOVER" {
                        // log::info!("Replaying descriptor block: {:x?}", this_block);
                        let mut replay_arg = ReplayArg {
                            info,
                            this_block: &mut this_block,
                            this_trans_id: this_trans_id,
                        };

                        self.jbd_replay_descriptor_block(header, &mut replay_arg);
                    } else {
                        self.debug_descriptor_block(header, &mut this_block);
                    }
                }
                JBD_COMMIT_BLOCK => {
                    // log::info!("Commit block: {:x?}", this_block);
                    this_trans_id += 1;

                    if action == "ACTION_SCAN" {
                        info.trans_cnt += 1;
                    }
                }
                JBD_REVOKE_BLOCK => {
                    log::info!("Revoke block: {:x?}", this_block);
                    if action == "ACTION_REVOKE" {
                        // self.jbd_build_revoke_tree(info, header);
                        // Process revoke block
                    }
                }
                _ => log_end = true,
            }

            this_block += 1;
            if this_block == self.sb.start {
                log_end = true;
            }
        }

        log::info!("End of journal");
        if action == "ACTION_SCAN" {
            info.start_trans_id = self.sb.sequence;
            info.last_trans_id = if trans_id_diff(this_trans_id, self.sb.sequence) > 0 {
                this_trans_id - 1
            } else {
                this_trans_id
            };
        }

        Ok(())
    }

    fn debug_descriptor_block(&self, header: *const JbdBhdr, iblock: &mut u32) {
        let tag_bytes = 0x8;
        let mut tag_ptr = unsafe { header.offset(1) as *const u8 };
        let mut tag_tbl_size = BLOCK_SIZE as isize - core::mem::size_of::<JbdBhdr>() as isize;

        while tag_tbl_size > 0 {
            let mut tag_info = TagInfo::new();
            if let Err(e) =
                self.jbd_extract_block_tag(tag_ptr, tag_bytes, tag_tbl_size as usize, &mut tag_info)
            {
                log::info!("Error extracting block tag: {}", e);
                break;
            }
            self.jbd_display_block_tags(&tag_info, iblock);

            if tag_info.last_tag {
                break;
            }

            tag_ptr = unsafe { tag_ptr.offset(tag_info.tag_bytes as isize) };
            tag_tbl_size -= tag_info.tag_bytes as isize;
        }
    }
    fn jbd_replay_block_tags(&self, tag_info: &TagInfo, replay_arg: &mut ReplayArg) {

        *replay_arg.this_block += 1;

        // self.wrap(replay_arg.this_block);

        // Check if we should replay this block
        let revoke_entry = replay_arg.info.revoke_tree.get(&tag_info.block);
        if let Some(entry) = revoke_entry {
            if trans_id_diff(replay_arg.this_trans_id, entry.trans_id) <= 0 {
                // Skip replaying this block
                return;
            }
        }
        let mut journal_block = self
            .bdev
            .read_offset((*replay_arg.this_block as usize) * BLOCK_SIZE);
        let mut ext4_block_data = vec![0u8; BLOCK_SIZE]; // Placeholder for the actual block data

        // Special handling for different blocks
        if tag_info.block != 0 {
            // Regular block
            ext4_block_data.copy_from_slice(&journal_block);
            if tag_info.is_escape {
                let mut bhdr = unsafe { *(ext4_block_data.as_ptr() as *mut JbdBhdr) };
                log::info!("bhdr: {:x?}", bhdr);
                bhdr.magic = JBD_MAGIC_NUMBER.to_be();
            }

            // Write back the modified block data
            self.bdev.write_offset(
                (*replay_arg.this_block as usize) * BLOCK_SIZE,
                &ext4_block_data,
            );
        } else {
            // Superblock special handling
        }
    }
    fn wrap(&self, iblock: &mut u32) {
        // if *iblock >= self.sb.maxlen {
        //     *iblock -= self.sb.maxlen - self.sb.first;
        // }
    }
    fn jbd_display_block_tags(&self, tag_info: &TagInfo, iblock: &mut u32) {
        log::info!("Block in block_tag: {}", tag_info.block);
        *iblock += 1;
        self.wrap(iblock);
    }
    pub fn jbd_replay_descriptor_block(&self, header: *const JbdBhdr, replay_arg: &mut ReplayArg) {
        let tag_bytes = 0x8; // For demonstration
        let mut tag_ptr = unsafe { header.offset(1) as *const u8 };
        let mut tag_tbl_size = BLOCK_SIZE as isize - core::mem::size_of::<JbdBhdr>() as isize;

        while tag_tbl_size > 0 {
            let mut tag_info = TagInfo::new();
            if let Err(e) =
                self.jbd_extract_block_tag(tag_ptr, tag_bytes, tag_tbl_size as usize, &mut tag_info)
            {
                log::info!("Error extracting block tag: {}", e);
                break;
            }
            self.jbd_replay_block_tags(&tag_info, replay_arg);

            if tag_info.last_tag {
                break;
            }

            tag_ptr = unsafe { tag_ptr.offset(tag_info.tag_bytes as isize) };
            tag_tbl_size -= tag_info.tag_bytes as isize;
        }
    }

    pub fn jbd_extract_block_tag(
        &self,
        tag_ptr: *const u8,
        tag_bytes: usize,
        remain_buf_size: usize,
        tag_info: &mut TagInfo,
    ) -> Result<(), String> {
        if remain_buf_size < tag_bytes {
            return Err("Buffer size is too small".to_string());
        }

        let jbdtag = unsafe { &*(tag_ptr as *const JbdBlockTag3) };

        log::info!("blocknr: {:x?}", jbdtag.blocknr.to_le());
        let blocknr = jbd_get32_le(tag_ptr).to_le();

        let flags = jbd_get32(unsafe { tag_ptr.offset(4) });

        tag_info.tag_bytes = tag_bytes;
        tag_info.block = blocknr as u64;

        if self.has_feature(JBD_FEATURE_INCOMPAT_64BIT) {
            let blocknr_high = jbd_get32(unsafe { tag_ptr.offset(8) });
            tag_info.block |= (blocknr_high as u64) << 32;
        }

        tag_info.is_escape = flags & JBD_FLAG_ESCAPE != 0;

        if flags & JBD_FLAG_SAME_UUID == 0 {
            if remain_buf_size < tag_bytes + UUID_SIZE {
                return Err("Buffer size is too small for UUID".to_string());
            }

            let uuid_ptr = unsafe { tag_ptr.add(tag_bytes) };
            tag_info.uuid_exist = true;
            tag_info.uuid.resize(UUID_SIZE, 0);
            tag_info
                .uuid
                .copy_from_slice(unsafe { core::slice::from_raw_parts(uuid_ptr, UUID_SIZE) });
            tag_info.tag_bytes += UUID_SIZE;
        }

        tag_info.last_tag = flags & JBD_FLAG_LAST_TAG as u32 != 0;

        Ok(())
    }

    pub fn jbd_write_block_tag(&self, tag: &mut [u8], tag_info: &TagInfo) -> Result<(), String> {
        let tag_bytes = core::mem::size_of::<JbdBlockTag3>();

        // 检查是否有足够的空间来存储标签
        if tag.len() < tag_bytes {
            return Err("Buffer size is too small for tag".to_string());
        }

        if self.has_feature(JBD_FEATURE_INCOMPAT_CSUM_V3 as u32) {
            let mut tag3 = JbdBlockTag3::default();
            tag3.blocknr = tag_info.block as u32;
            // tag3.blocknr_high = (tag_info.block >> 32) as u32;

            // tag3.blocknr = (tag_info.block & 0xFFFFFFFF) as u32;
            // tag3.blocknr_high = (tag_info.block >> 32) as u32;

            if tag_info.uuid_exist {
                if tag.len() < tag_bytes + UUID_SIZE {
                    return Err("Buffer size is too small for UUID".to_string());
                }
                tag3.flags |= JBD_FLAG_SAME_UUID as u16;
            }

            if tag_info.is_escape {
                tag3.flags |= JBD_FLAG_ESCAPE as u16;
            }

            if tag_info.last_tag {
                tag3.flags |= JBD_FLAG_LAST_TAG;
            }

            // 这里应该有逻辑来设置校验和
            tag3.checksum = tag_info.checksum as _;

            // 将 tag3 写入 tag 缓冲区
            let tag3_bytes = tag3.to_be_bytes();
            tag[..tag3_bytes.len()].copy_from_slice(&tag3_bytes);
        } else {
            // JbdBlockTag
        }

        Ok(())
    }

    pub fn has_feature(&self, feature: u32) -> bool {
        false
    }

    pub fn jbd_tag_bytes(&self) -> usize {
        // 根据 journal 特性返回合适的标签大小
        if self.has_feature(JBD_FEATURE_INCOMPAT_CSUM_V3 as u32) {
            core::mem::size_of::<JbdBlockTag3>()
        } else {
            4
            // core::mem::size_of::<JbdBlockTag>()
        }
    }
}

fn jbd_get32(ptr: *const u8) -> u32 {
    assert!(!ptr.is_null(), "Pointer must not be null");

    let slice = unsafe { core::slice::from_raw_parts(ptr, 4) };
    u32::from_be_bytes(slice.try_into().expect("Slice should have a length of 4"))
}

fn jbd_get32_le(ptr: *const u8) -> u32 {
    assert!(!ptr.is_null(), "Pointer must not be null");

    let slice = unsafe { core::slice::from_raw_parts(ptr, 4) };
    u32::from_le_bytes(slice.try_into().expect("Slice should have a length of 4"))
}
