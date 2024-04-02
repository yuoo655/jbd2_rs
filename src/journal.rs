use super::defs::*;
use super::prelude::*;
use super::consts::*;

impl JbdJournal {
    pub fn new() -> Self {
        JbdJournal {
            first: 0,
            start: 0,
            last: 0,
            trans_id: 0,
            alloc_trans_id: 0,
            block_size: 4096,
            cp_queue: CheckpointQueue::new(),
            block_rec_root: BlockRecordRoot::new(),
            jbd_fs: core::ptr::null_mut(),
        }
    }


    pub fn jbd_journal_write_sb(&mut self) {
        // This method should write the journal superblock information to the actual storage.
        // Here, we're just simulating the update of the journal's superblock within the JbdFs structure.
        let jbd_fs = unsafe { &mut *self.jbd_fs };

        jbd_fs.sb.start = self.start.to_be();
        jbd_fs.sb.sequence = self.trans_id.to_be();

        jbd_fs.dirty = true; // Marking the filesystem as dirty, meaning changes need to be written to disk.

        let bdev = &jbd_fs.bdev;
        jbd_fs.sb.sync_to_disk(bdev);
    }




    pub fn commit_trans(&mut self, trans: &mut Transaction){
        let bdev = &unsafe { &*self.jbd_fs }.bdev;
        let last = self.last;

        trans.trans_id = self.alloc_trans_id;

        // desc block
        self.write_descriptor_block(trans);

        // revoke block
        self.write_revoke_block(trans);

        // commit block
        self.write_commit_block(trans);

        // Update the journal start and transaction ID based on whether the checkpoint queue is empty
        if self.cp_queue.queue.is_empty() {
            // If there's data to write, update the journal start to this transaction's start block
            if trans.data_cnt > 0 {
                self.start = trans.start_iblock;
                self.trans_id = trans.trans_id;
                // Add transaction to the checkpoint queue
                self.cp_queue.queue.push_back(trans.clone());
            } else {
                // If no data to write, move the start beyond this transaction's allocated blocks
                self.start = trans.start_iblock + trans.alloc_blocks as u32;
                self.trans_id = trans.trans_id + 1;
            }

            // Write the updated journal superblock to disk
            // self.write_superblock();

        } else {
            // If the checkpoint queue is not empty, just add this transaction
            self.cp_queue.queue.push_back(trans.clone());
        }

        // Increment the allocation transaction ID for the next transaction
        self.alloc_trans_id += 1;

    }


    pub fn write_descriptor_block(&mut self, trans: &mut Transaction){
        
        let bhdr = JbdBhdr {
            magic: JBD_MAGIC_NUMBER,
            blocktype: JBD_DESCRIPTOR_BLOCK,
            sequence: trans.trans_id.to_be(),
        };

        let desc_iblock = self.jbd_journal_alloc_block(trans);

        // 将描述符块头部写入块缓冲区的开始位置
        let mut desc_block_data = vec![0u8; BLOCK_SIZE as usize];
        desc_block_data[0..core::mem::size_of::<JbdBhdr>()].copy_from_slice(&bhdr.to_be_bytes());


        // 计算标签开始位置
        let mut tag_ptr = unsafe{desc_block_data.as_mut_ptr().add(core::mem::size_of::<JbdBhdr>())};


        let mut tag_ptr_offset: usize = core::mem::size_of::<JbdBhdr>() + 1;

        // 遍历事务中的所有缓冲区，为每个缓冲区创建标签
        for (index, jbd_buf) in trans.buf_queue.iter().enumerate() {

            // 标记最后一个缓冲区的标签
            let is_last_tag = index == trans.buf_queue.len() - 1;

            let tag_info: TagInfo = TagInfo {
                block: jbd_buf.block.lb_id,
                is_escape: false, // 这里应该根据实际情况判断是否需要转义
                checksum: 0, // 实际应用中应计算校验和
                last_tag: is_last_tag,
                ..Default::default()
            };

            let tag_slice = &mut desc_block_data[tag_ptr_offset..];
            self.jbd_write_block_tag(tag_slice, &tag_info).expect("Failed to write tag");

            tag_ptr_offset += self.jbd_tag_bytes();
        }

        if trans.start_iblock == 0 {
            
            // 如果是第一个事务，将描述符块的起始块号写入日志超级块
            let jbd_fs = unsafe { &mut *self.jbd_fs };
            jbd_fs.sb.first = desc_iblock;
            jbd_fs.dirty = true;
            trans.start_iblock = desc_iblock;
        }
        let bdev = &unsafe { &*self.jbd_fs }.bdev;
        bdev.write_offset(desc_iblock as usize * BLOCK_SIZE as usize, &desc_block_data);


        // let block = bdev.read_offset((desc_iblock as usize) * BLOCK_SIZE);
        // let header = block.as_ptr() as *const JbdBhdr; // Cast data to JbdBhdr
        // let magic = unsafe { (*header).magic.to_be() };

        // if magic != JBD_MAGIC_NUMBER {
        //     log::info!("Invalid magic number found in journal.");
        // }
        // let tag_ptr = unsafe{block.as_ptr().add(core::mem::size_of::<JbdBhdr>())};
        // let jbdtag = unsafe { &*(tag_ptr as *const JbdBlockTag3) };

    }


    pub fn write_revoke_block(&mut self, trans: &mut Transaction) {
        let bhdr = JbdBhdr {
            magic: JBD_MAGIC_NUMBER,
            blocktype: JBD_REVOKE_BLOCK,
            sequence: trans.trans_id.to_be(),
        };
    
        let revoke_iblock = self.jbd_journal_alloc_block(trans);
    
        let mut revoke_block_data = vec![0u8; BLOCK_SIZE as usize];
        revoke_block_data[0..core::mem::size_of::<JbdBhdr>()].copy_from_slice(&bhdr.to_be_bytes());
    

        let mut offset = core::mem::size_of::<JbdBhdr>();
        for rec in &trans.revoke_root {
            let rec_data = rec.1.lba.to_be_bytes();  
            let rec_slice = &mut revoke_block_data[offset..offset + rec_data.len()];
            rec_slice.copy_from_slice(&rec_data);
            offset += rec_data.len();
        }
    
        let bdev = &unsafe { &*self.jbd_fs }.bdev;
        bdev.write_offset(revoke_iblock as usize * BLOCK_SIZE as usize, &revoke_block_data);
    }

    pub fn write_commit_block(&mut self, trans: &mut Transaction) {
        let bhdr = JbdBhdr {
            magic: JBD_MAGIC_NUMBER,
            blocktype: JBD_COMMIT_BLOCK,
            sequence: trans.trans_id.to_be(),
        };
    
        let commit_iblock = self.jbd_journal_alloc_block(trans);
    
        let mut commit_block_data = vec![0u8; BLOCK_SIZE as usize];
        commit_block_data[0..core::mem::size_of::<JbdBhdr>()].copy_from_slice(&bhdr.to_be_bytes());
    
    
        let bdev = &unsafe { &*self.jbd_fs }.bdev;
        bdev.write_offset(commit_iblock as usize * BLOCK_SIZE as usize, &commit_block_data);
    }

    // 分配一个新的块并返回其块号
    pub fn jbd_journal_alloc_block(&mut self, trans: &mut Transaction) -> u32 {
        let start_block = self.last + 1;
        self.last += 1;
        trans.alloc_blocks += 1;
        // 确保 last 指针没有超过日志的边界
        // self.wrap();

        // 检查是否还有足够的空间分配块
        if self.last == self.start {
            // 没有空间时，尝试清理已提交的事务
            self.jbd_journal_purge_cp_trans(true, true);
            assert!(self.last != self.start, "No space left in the journal");
        }

        start_block
    }

    pub fn jbd_journal_purge_cp_trans(&mut self, flush: bool, once: bool) {
        while let Some(trans) = self.cp_queue.queue.front().cloned() {
            if trans.data_cnt == 0 || (flush && trans.data_cnt == trans.written_cnt) {
                // 更新日志开始位置
                self.start = trans.start_iblock + trans.alloc_blocks as u32;
                // self.wrap();

                // 更新事务ID
                self.trans_id = trans.trans_id + 1;

                // 从检查点队列中移除事务
                self.cp_queue.queue.pop_front();

                // 如果需要，这里可以添加更多清理事务的逻辑
                // 比如释放事务占用的资源等

                // 如果只处理一次，退出循环
                if once {
                    break;
                }
            } else if !flush {
                // 如果不刷新数据，更新日志开始位置并退出循环
                self.start = trans.start_iblock;
                // self.wrap();
                self.trans_id = trans.trans_id;
                break;
            } else {
                // 如果需要刷新数据，调用 jbd_journal_flush_trans 处理事务
                self.jbd_journal_flush_trans(&trans);
            }
        }
    }

    pub fn jbd_journal_flush_trans(&mut self, trans: &Transaction) {
    }

    pub fn jbd_write_block_tag(
        &mut self,
        tag: &mut [u8],
        tag_info: &TagInfo,
    ) -> Result<(), String> {
        let tag_bytes = self.jbd_tag_bytes();

        // 确保标签缓冲区足够大
        if tag.len() < tag_bytes {
            return Err("Buffer size is too small".to_string());
        }

        if self.has_feature(JBD_FEATURE_INCOMPAT_CSUM_V3 as u32) {
            // 使用 JbdBlockTag3 结构
            if tag_info.uuid_exist && tag.len() < tag_bytes + UUID_SIZE {
                return Err("Buffer size is too small for UUID".to_string());
            }

            let mut tag3 = JbdBlockTag3 {
                blocknr: tag_info.block as u32, 
                checksum: tag_info.checksum as u16, 
                flags: 0,                       
                blocknr_high: (tag_info.block >> 32) as u32,
            };

            // 设置标志位
            if tag_info.is_escape {
                tag3.flags |= JBD_FLAG_ESCAPE as u16;
            }
            if tag_info.last_tag {
                tag3.flags |= JBD_FLAG_LAST_TAG as u16;
            }
            if !tag_info.uuid_exist {
                tag3.flags |= JBD_FLAG_SAME_UUID as u16;
            }

            let tag3_bytes: &[u8] = unsafe { any_as_u8_slice(&tag3) };
            tag[..tag3_bytes.len()].copy_from_slice(tag3_bytes);

            // 如果存在 UUID，将其追加到标签之后
            if tag_info.uuid_exist {
                let uuid_start = tag_bytes;
                tag[uuid_start..uuid_start + UUID_SIZE].copy_from_slice(&tag_info.uuid);
            }
        } else {
            // 使用 JbdBlockTag 结构
            // 类似地处理 JbdBlockTag，如上所示
        }

        Ok(())
    }

    pub fn has_feature(&self, feature: u32) -> bool {
        true
    }

    fn jbd_tag_bytes(&self) -> usize {
        core::mem::size_of::<JbdBlockTag3>()
    }
}



impl JbdBhdr {
    pub fn from_be_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return Err("Byte slice is too short".into());
        }

        let magic = u32::from_be_bytes(bytes[0..4].try_into().unwrap());
        let blocktype = u32::from_be_bytes(bytes[4..8].try_into().unwrap());
        let sequence = u32::from_be_bytes(bytes[8..12].try_into().unwrap());

        Ok(Self {
            magic,
            blocktype,
            sequence,
        })
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.magic.to_be_bytes());
        bytes.extend_from_slice(&self.blocktype.to_be_bytes());
        bytes.extend_from_slice(&self.sequence.to_be_bytes());
        bytes
    }
}