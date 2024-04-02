use super::consts::*;
use super::defs::*;
use super::prelude::*;


impl Transaction {
    pub fn new(journal: Arc<JbdJournal>) -> Transaction {
        Transaction {
            trans_id: 0,
            start_iblock: 0,
            alloc_blocks: 0,
            data_cnt: 0,
            data_csum: 0,
            written_cnt: 0,
            error: 0,
            journal: journal,
            buf_queue: VecDeque::new(),
            revoke_root: BTreeMap::new(),
            tbrec_list: Vec::new(),
        }
    }

    pub fn jbd_trans_set_block_dirty(&mut self, block: Ext4Block) {
        let buffer = Buffer{
            block_num: block.lb_id as u32, 
            data: block.data.clone(),  
            dirty: true,    
            uptodate: true, 
        };

        let buf = JbdBuf {
            jbd_lba: block.lb_id as _,
            block: block,
            buffer: buffer,
            trans: Some(Arc::new(self.clone())),
            block_rec: None,
            dirty: true,
        };

        self.buf_queue.push_back(buf);

        self.data_cnt += 1;


        log::debug!("buf queue {:x?}", self.buf_queue);
    }
}