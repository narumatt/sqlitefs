use fuse::{
    Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request,
};
use libc::ENOENT;
use std::path::Path;
use std::ffi::OsStr;
use crate::db_module::DbModule;
use crate::db_module::sqlite::Sqlite;
use crate::sqerror::SqError;
use time::Timespec;

const ONE_SEC: Timespec = Timespec{
    sec: 1,
    nsec: 0
};

pub struct SqliteFs{
    db: Sqlite,
}

impl SqliteFs {
    pub fn new(path: &str) -> Result<SqliteFs, SqError> {
        let db = match Sqlite::new(Path::new(path)) {
            Ok(n) => n,
            Err(err) => return Err(err)
        };
        Ok(SqliteFs{db})
    }
}

impl Filesystem for SqliteFs {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        match self.db.lookup(parent as u32, name.to_str().unwrap()) {
            Ok(n) => {
                reply.entry(&ONE_SEC, &n.get_file_attr() , 0);
                debug!("filesystem:lookup, return:{:?}", n.get_file_attr());
            },
            Err(_err) => reply.error(ENOENT)
        };
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        match self.db.get_inode(ino as u32) {
            Ok(n) => {
                reply.attr(&ONE_SEC, &n.get_file_attr());
                debug!("filesystem:getattr, return:{:?}", n.get_file_attr());
            },
            Err(_err) => reply.error(ENOENT)
        };
    }

    fn read(&mut self, _req: &Request, ino: u64, _fh: u64, _offset: i64, _size: u32, reply: ReplyData) {
        let mut data: Vec<u8> = Vec::with_capacity(_size as usize);
        let block_size = self.db.get_db_block_size();
        let mut size = _size;
        let mut offset = _offset as u32;
        while size > 0 {
            let b_num = offset / block_size + 1;
            let mut b_data = match self.db.get_data(ino as u32, b_num, block_size) {
                Ok(n) => n,
                Err(_err) => {reply.error(ENOENT); return; }
            };
            let b_offset = offset % block_size;
            let b_end = if (size + b_offset) / block_size >= 1 {block_size} else {size + b_offset};
            if b_data.len() < b_end as usize {
                b_data.resize(b_end as usize, 0);
            }
            data.append(&mut b_data[b_offset as usize..b_end as usize].to_vec());
            offset += b_end - b_offset;
            size -= b_end - b_offset;
        }
        reply.data(&data);
    }

    fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        let db_entries = match self.db.get_dentry(ino as u32) {
            Ok(n) => n,
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
        for (i, entry) in db_entries.into_iter().enumerate().skip(offset as usize) {
            let full = reply.add(entry.child_ino as u64, (i + 1) as i64, entry.file_type, &entry.filename);
            if full {
                break;
            }
            debug!("filesystem:readdir, ino: {:?} offset: {:?} kind: {:?} name: {}", entry.child_ino as u64, (i + 1) as i64, entry.file_type, entry.filename);
        }
        reply.ok();
    }
}
