pub mod consts;
pub mod defs;
pub mod jbd;
pub mod journal;
pub mod prelude;
pub mod transaction;

pub use consts::*;
pub use defs::*;
pub use jbd::*;
pub use journal::*;
pub use prelude::*;
pub use transaction::*;


use log::{Level, LevelFilter, Metadata, Record};

macro_rules! with_color {
    ($color_code:expr, $($arg:tt)*) => {{
        format_args!("\u{1B}[{}m{}\u{1B}[m", $color_code as u8, format_args!($($arg)*))
    }};
}

struct SimpleLogger;

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        let level = record.level();
        let args_color = match level {
            Level::Error => ColorCode::Red,
            Level::Warn => ColorCode::Yellow,
            Level::Info => ColorCode::Green,
            Level::Debug => ColorCode::Cyan,
            Level::Trace => ColorCode::BrightBlack,
        };

        if self.enabled(record.metadata()) {
            println!(
                "{} - {}",
                record.level(),
                with_color!(args_color, "{}", record.args())
            );
        }
    }

    fn flush(&self) {}
}

#[repr(u8)]
enum ColorCode {
    Red = 31,
    Green = 32,
    Yellow = 33,
    Cyan = 36,
    BrightBlack = 90,
}



#[derive(Debug)]
pub struct Disk;

impl BlockDevice for Disk {
    fn read_offset(&self, offset: usize) -> Vec<u8> {
        use std::fs::OpenOptions;
        use std::io::{Read, Seek};
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("ex4.img")
            .unwrap();
        let mut buf = vec![0u8; BLOCK_SIZE as usize];
        let r = file.seek(std::io::SeekFrom::Start(offset as u64));
        let r = file.read_exact(&mut buf);

        buf
    }

    fn write_offset(&self, offset: usize, data: &[u8]) {
        use std::fs::OpenOptions;
        use std::io::{Read, Seek, Write};
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("ex4.img")
            .unwrap();

        let r = file.seek(std::io::SeekFrom::Start(offset as u64));
        let r = file.write_all(&data);
    }
}


#[derive(Debug)]
pub struct Ext4;


impl Ext4Fs for Ext4{
    fn get_journal_block(&self) -> Vec<u8> {
        let offset = 0x20000 * BLOCK_SIZE;
        use std::fs::OpenOptions;
        use std::io::{Read, Seek};
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("ex4.img")
            .unwrap();
        let mut buf = vec![0u8; BLOCK_SIZE as usize];
        let r = file.seek(std::io::SeekFrom::Start(offset as u64));
        let r = file.read_exact(&mut buf);
        buf
    }

    fn get_superblock(&self) -> Vec<u8> {
        let offset = 0x1000;
        use std::fs::OpenOptions;
        use std::io::{Read, Seek};
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("ex4.img")
            .unwrap();
        let mut buf = vec![0u8; BLOCK_SIZE as usize];
        let r = file.seek(std::io::SeekFrom::Start(offset as u64));
        let r = file.read_exact(&mut buf);
        buf
    }
}




fn main() {
    log::set_logger(&SimpleLogger).unwrap();
    log::set_max_level(LevelFilter::Info);
    let disk = Arc::new(Disk);
    let ext4 = Arc::new(Ext4);
    let data = disk.read_offset(0x20000);
    let jbd_sb = JbdSb::try_from(data).unwrap();

    let mut jbd_fs = JbdFs {
        sb: jbd_sb,
        journal: JbdJournal::new(),
        bdev: disk,
        ext4fs: ext4,
        dirty: false,
        curr_trans: None,
    };

    // journal start at mount
    jbd_fs.journal_start();

    jbd_fs.trans_start();


    // write a block
    let block = Ext4Block {
        lb_id: 0x2,
        data: vec![0x41u8; 4096],
    };

    jbd_fs.bdev.write_offset((block.lb_id as usize) * BLOCK_SIZE, &block.data);

    // write a transaction
    jbd_fs.write_trans(block);

    // commit the transaction
    jbd_fs.trans_stop();

    log::info!("recovering...");
    let r: Result<(), String> = jbd_fs.recover();
}
