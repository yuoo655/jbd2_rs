pub const ACTION_SCAN: i32 = 1;
pub const ACTION_REVOKE: i32 = 2;
pub const ACTION_RECOVER: i32 = 3;
pub const JBD_DESCRIPTOR_BLOCK: u32 = 1;
pub const JBD_COMMIT_BLOCK: u32 = 2;
pub const JBD_REVOKE_BLOCK: u32 = 5;
pub const JBD_SUPERBLOCK_V2: u32 = 4;
pub const UUID_SIZE: usize = 16;
pub const JBD_FLAG_ESCAPE: u32 = 1;
pub const JBD_FLAG_SAME_UUID: u32 = 2;
pub const JBD_FLAG_LAST_TAG: u16 = 8;
pub const JBD_FEATURE_INCOMPAT_64BIT: u32 = 1;

pub const JBD_USERS_SIZE: usize = 16 * 48;
pub const JBD_MAGIC_NUMBER: u32 = 0xc03b3998;
pub const JBD_FEATURE_INCOMPAT_CSUM_V3: u16 = 0x00000010;

pub const BLOCK_SIZE: usize = 4096;
