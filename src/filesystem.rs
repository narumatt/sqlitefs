use fuse::{
    Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, ReplyWrite, Request,
};
use libc::ENOENT;
use std::path::Path;
use std::ffi::OsStr;
use crate::db_module::DbModule;
use crate::db_module::sqlite::Sqlite;
use crate::sqerror::SqError;
use time::Timespec;
use std::time::SystemTime;

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
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
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

    fn setattr(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        atime: Option<Timespec>,
        mtime: Option<Timespec>,
        _fh: Option<u64>,
        crtime: Option<Timespec>,
        _chgtime: Option<Timespec>,
        _bkuptime: Option<Timespec>,
        flags: Option<u32>,
        reply: ReplyAttr
    ) {
        let mut attr = match self.db.get_inode(ino as u32) {
            Ok(n) => n,
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
        if let Some(n) = mode {attr.perm = n as u16};
        if let Some(n) = uid {attr.uid = n};
        if let Some(n) = gid {attr.gid = n};
        if let Some(n) = size {attr.size = n as u32};
        if let Some(n) = atime {attr.atime = attr.datetime_from(&n)};
        if let Some(n) = mtime {attr.mtime = attr.datetime_from(&n)};
        attr.ctime = SystemTime::now();
        if let Some(n) = crtime {attr.crtime = attr.datetime_from(&n)};
        if let Some(n) = flags {attr.flags = n};
        match self.db.update_inode(attr) {
            Ok(_n) => (),
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
        reply.attr(&ONE_SEC, &attr.get_file_attr());
    }

    fn read(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, size: u32, reply: ReplyData) {
        let mut data: Vec<u8> = Vec::with_capacity(size as usize);
        let block_size = self.db.get_db_block_size();
        let mut size = size;
        let mut offset = offset as u32;
        while size > 0 {
            let b_num = offset / block_size + 1;
            let mut block_data = match self.db.get_data(ino as u32, b_num, block_size) {
                Ok(n) => n,
                Err(_err) => {reply.error(ENOENT); return; }
            };
            let b_offset = offset % block_size;
            let b_end = if (size + b_offset) / block_size >= 1 {block_size} else {size + b_offset};
            if block_data.len() < b_end as usize {
                block_data.resize(b_end as usize, 0);
            }
            data.append(&mut block_data[b_offset as usize..b_end as usize].to_vec());
            offset += b_end - b_offset;
            size -= b_end - b_offset;
        }
        reply.data(&data);
    }

    fn write(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, data: &[u8], _flags: u32, reply: ReplyWrite) {
        let block_size = self.db.get_db_block_size();
        let ino = ino as u32;
        let size = data.len() as u32;
        let offset = offset as u32;
        let start_block = offset / block_size + 1;
        let end_block = (offset + size - 1) / block_size + 1;
        for i in start_block..=end_block {
            let mut block_data: Vec<u8> = Vec::with_capacity(block_size as usize);
            let b_start_index = if i == start_block {offset % block_size} else {0};
            let b_end_index = if i == end_block {(offset+size-1) % block_size +1} else {block_size};
            let data_offset = (i - start_block) * block_size;
            if (b_start_index != 0) || (b_end_index != block_size) {
                let mut data_pre = match self.db.get_data(ino, i, block_size) {
                    Ok(n) => n,
                    Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
                };
                if data_pre.len() < block_size as usize {
                    data_pre.resize(block_size as usize, 0);
                }
                if b_start_index != 0 {
                    block_data.extend_from_slice(&data_pre[0..b_start_index as usize]);
                }
                block_data.extend_from_slice(&data[data_offset as usize..(data_offset + b_end_index - b_start_index) as usize]);
                if b_end_index != block_size {
                    block_data.extend_from_slice(&data_pre[b_end_index as usize..block_size as usize]);
                }
            } else {
                block_data.extend_from_slice(&data[data_offset as usize..(data_offset + block_size) as usize]);
            }
            match self.db.write_data(ino, i, &block_data, (i-1) * block_size + b_end_index) {
                Ok(n) => n,
                Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
            }
        }
        reply.written(size);
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
