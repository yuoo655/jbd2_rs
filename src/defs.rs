use super::consts::*;
use super::prelude::*;

#[derive(Debug, Clone)]
#[repr(C)]
pub struct JbdSb {
    // JbdSb header
    pub header: JbdBhdr,

    // Static information describing the journal
    pub blocksize: u32,
    pub maxlen: u32,
    pub first: u32,

    // Dynamic information describing the current state of the log
    pub sequence: u32,
    pub start: u32,

    // Error value, as set by journal_abort().
    pub error_val: i32,

    // Remaining fields are only valid in a version-2 superblock
    pub feature_compat: u32,
    pub feature_incompat: u32,
    pub feature_ro_compat: u32,

    // 128-bit uuid for journal
    pub uuid: [u8; UUID_SIZE],

    // Nr of filesystems sharing log
    pub nr_users: u32,

    // Blocknr of dynamic superblock copy
    pub dynsuper: u32,

    // Limit of journal blocks per transaction
    pub max_transaction: u32,
    pub max_trandata: u32,

    // Checksum type
    pub checksum_type: u8,
    pub padding2: [u8; 3],
    pub padding: [u32; 42],
    pub checksum: u32,

    // IDs of all filesystems sharing the log
    pub users: [u8; JBD_USERS_SIZE],
}

impl TryFrom<Vec<u8>> for JbdSb {
    type Error = u64;
    fn try_from(value: Vec<u8>) -> core::result::Result<Self, u64> {
        let data = &value[..core::mem::size_of::<JbdSb>()];
        Ok(unsafe { core::ptr::read(data.as_ptr() as *const _) })
    }
}

impl JbdSb {
    pub fn sync_to_disk(&self, bdev: &Arc<dyn BlockDevice>) {
        let data = any_as_u8_slice(self);
        bdev.write_offset(0x20000, data);
    }
}

#[derive(Debug, Clone)]
pub struct JbdJournal {
    pub first: u32,
    pub start: u32,
    pub last: u32,
    pub trans_id: u32,
    pub alloc_trans_id: u32,
    pub block_size: u32,
    pub cp_queue: CheckpointQueue, // Queue for managing checkpointing
    pub block_rec_root: BlockRecordRoot, // Root of the block record tree
    pub jbd_fs: *mut JbdFs,        // Back-reference to the JbdFs
}

#[derive(Debug, Clone)]
pub struct Transaction {
    pub trans_id: u32,
    pub start_iblock: u32,
    pub alloc_blocks: i32,
    pub data_cnt: i32,
    pub data_csum: u32,
    pub written_cnt: i32,
    pub error: i32,
    pub journal: Arc<JbdJournal>,
    pub buf_queue: VecDeque<JbdBuf>,
    pub revoke_root: BTreeMap<u32, JbdRevokeRec>,
    pub tbrec_list: Vec<JbdBlockRec>,
}

#[derive(Debug, Clone)]
pub struct JbdRevokeRec {
    pub lba: u32,
}

#[derive(Debug, Clone)]
pub struct CheckpointQueue {
    pub queue: VecDeque<Transaction>,
}
impl CheckpointQueue {
    pub fn new() -> Self {
        CheckpointQueue {
            queue: VecDeque::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BlockRecord {
    lba: u64,                        // 块地址
    trans: Option<Arc<Transaction>>, // 关联的事务
    dirty_buf_queue: Vec<JbdBuf>,    // 存储脏缓冲区的集合
}

#[derive(Debug, Clone)]
pub struct BlockRecordRoot {
    records: BTreeMap<u32, BlockRecord>,
}
impl BlockRecordRoot {
    pub fn new() -> Self {
        BlockRecordRoot {
            records: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct JbdBlockRec {
    lba: u64,
    trans: Option<Arc<Transaction>>,
    dirty_buf_queue: Vec<JbdBuf>,
}

#[derive(Debug, Clone)]
pub struct JbdBuf {
    pub jbd_lba: u32,
    pub block: Ext4Block,
    pub buffer: Buffer,
    pub trans: Option<Arc<Transaction>>,
    pub block_rec: Option<Arc<JbdBlockRec>>,
    pub dirty: bool, // 指示缓冲区是否被修改过
}

#[derive(Clone, Default)]
pub struct Ext4Block {
    pub lb_id: u64,
    pub data: Vec<u8>,
}

impl Debug for Ext4Block {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Ext4Block")
            .field("lb_id", &self.lb_id)
            .field("data sample {:x}", &self.data[0])
            .finish()
    }
}

#[derive(Clone)]
pub struct Buffer {
    pub block_num: u32, // 块号
    pub data: Vec<u8>,  // 缓冲区数据
    pub dirty: bool,    // 是否已修改但未写回
    pub uptodate: bool, // 数据是否为最新
}

impl Debug for Buffer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Buffer")
            .field("lb_id", &self.block_num)
            .field("data sample {:x}", &self.data[0])
            .finish()
    }
}

impl Default for JbdJournal {
    fn default() -> Self {
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
}

#[derive(Debug, Clone)]
pub struct JbdFs {
    pub journal: JbdJournal,
    pub sb: JbdSb,
    pub bdev: Arc<dyn BlockDevice>,
    pub dirty: bool,
    pub curr_trans: Option<Arc<RefCell<Transaction>>>,
}

pub struct RecoverInfo {
    pub revoke_tree: BTreeMap<u64, RevokeEntry>,
    pub last_trans_id: u32,
    pub trans_cnt: u32,
    pub start_trans_id: u32,
    pub this_trans_id: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct JbdBhdr {
    pub magic: u32,
    pub blocktype: u32,
    pub sequence: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct JbdRevokeHeader {
    header: JbdBhdr,
    count: u32,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RevokeEntry {
    pub block: u64,
    pub trans_id: u32,
}

pub struct ReplayArg<'a> {
    pub info: &'a mut RecoverInfo,
    pub this_block: &'a mut u32,
    pub this_trans_id: u32,
}

#[derive(Debug, Clone, Default)]
#[repr(C)]
pub struct TagInfo {
    pub tag_bytes: usize,
    pub block: u64,
    pub is_escape: bool,
    pub uuid_exist: bool,
    pub uuid: Vec<u8>,
    pub last_tag: bool,
    pub checksum: u32,
}

impl TagInfo {
    pub fn new() -> Self {
        TagInfo {
            tag_bytes: 0,
            block: 0,
            is_escape: false,
            uuid_exist: false,
            uuid: vec![0; UUID_SIZE], // Initialize UUID with zeros.
            last_tag: false,
            checksum: 0,
        }
    }
}

impl RecoverInfo {
    pub fn new() -> Self {
        RecoverInfo {
            start_trans_id: 0,
            last_trans_id: 0,
            this_trans_id: 0,
            trans_cnt: 0,
            revoke_tree: BTreeMap::new(),
        }
    }
}

#[repr(C)]
#[derive(Default, Clone)]
pub struct JbdBlockTag3 {
    pub blocknr: u32,      /* The on-disk block number */
    pub checksum: u16,     /* crc32c(uuid+seq+block) */
    pub flags: u16,        /* See below */
    pub blocknr_high: u32, /* most-significant high 32bits. */
}

impl JbdBlockTag3 {
    pub fn to_be_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.blocknr.to_le_bytes());
        bytes.extend_from_slice(&self.checksum.to_be_bytes());
        bytes.extend_from_slice(&self.flags.to_be_bytes());
        bytes.extend_from_slice(&self.blocknr_high.to_be_bytes());
        bytes
    }
}

impl TryFrom<Vec<u8>> for JbdBlockTag3 {
    type Error = u64;
    fn try_from(value: Vec<u8>) -> core::result::Result<Self, u64> {
        let data = &value[..core::mem::size_of::<JbdBlockTag3>()];
        Ok(unsafe { core::ptr::read(data.as_ptr() as *const _) })
    }
}

impl Debug for JbdBlockTag3 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("JbdBlockTag3")
            .field("blocknr", &self.blocknr.to_be())
            .field("checksum", &self.checksum.to_be())
            .field("flags", &self.flags.to_be())
            .field("blocknr_high", &self.blocknr_high.to_be())
            .finish()
    }
}

pub fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
    unsafe { core::slice::from_raw_parts((p as *const T) as *const u8, core::mem::size_of::<T>()) }
}

pub fn trans_id_diff(id1: u32, id2: u32) -> i32 {
    // Logic to calculate the difference between two transaction IDs
    id1 as i32 - id2 as i32
}

pub trait BlockDevice: Send + Sync + Any + Debug {
    // 读取指定偏移量的数据
    fn read_offset(&self, offset: usize) -> Vec<u8>;

    // 将数据写入指定偏移量
    fn write_offset(&self, offset: usize, data: &[u8]);

    // // 查找并获取一个缓冲区，如果不存在则返回 None
    // fn find_get_buffer(&self, block_num: u32) -> Option<Buffer>;

    // // 获取一个数据块
    // fn get_block(&self, block_num: u32) -> Result<Block, String>;

    // // 设置一个数据块
    // fn set_block(&self, block: &Block) -> Result<(), String>;

    // // 直接设置多个数据块
    // fn set_blocks_direct(&self, data: &[u8], block_num: u32, count: u32) -> Result<(), String>;

    // // 刷新缓冲区
    // fn flush_buffer(&self, buffer: &Buffer);
}
