use fuse::{
    Filesystem,
    ReplyAttr,
    ReplyData,
    ReplyDirectory,
    ReplyEntry,
    ReplyWrite,
    ReplyCreate,
    ReplyEmpty,
    Request,
    FileType
};
use libc::{c_int, ENOENT, ENOTEMPTY, EISDIR, ENOTDIR, EPERM, EEXIST, EINVAL, ENAMETOOLONG};
use std::path::Path;
use std::ffi::OsStr;
use crate::db_module::{DbModule, DBFileAttr};
use crate::db_module::sqlite::Sqlite;
use crate::sqerror::{Error, ErrorKind};
use time::Timespec;
use std::time::SystemTime;
use std::sync::Mutex;
use std::collections::HashMap;

const ONE_SEC: Timespec = Timespec{
    sec: 1,
    nsec: 0
};

pub struct SqliteFs{
    db: Sqlite,
    lookup_count: Mutex<HashMap<u32, u32>>
}

impl SqliteFs {
    pub fn new(path: &str) -> Result<SqliteFs, Error> {
        let db = match Sqlite::new(Path::new(path)) {
            Ok(n) => n,
            Err(err) => return Err(err)
        };
        let lookup_count = Mutex::new(HashMap::<u32, u32>::new());
        Ok(SqliteFs{db, lookup_count})
    }
}

impl Filesystem for SqliteFs {
    fn init(&mut self, _req: &Request<'_>) -> Result<(), c_int> {
        match self.db.delete_all_noref_inode() {
            Ok(n) => n,
            Err(err) => debug!("{}", err)
        };
        Ok(())
    }

    fn destroy(&mut self, _req: &Request<'_>) {
        let lc_list = self.lookup_count.lock().unwrap();
        for key in lc_list.keys() {
            match self.db.delete_inode_if_noref(*key) {
                Ok(n) => n,
                Err(err) => debug!("{}", err)
            }
        }
    }

    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let parent = parent as u32;
        let child = match self.db.lookup(parent, name.to_str().unwrap()) {
            Ok(n) => {
                match n {
                    Some(v) => {
                        reply.entry(&ONE_SEC, &v.get_file_attr() , 0);
                        debug!("filesystem:lookup, return:{:?}", v.get_file_attr());
                        v.ino
                    },
                    None => { reply.error(ENOENT); return;}
                }
            },
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
        let mut lc_list = self.lookup_count.lock().unwrap();
        let lc = lc_list.entry(child).or_insert(0);
        *lc += 1;
        debug!("filesystem:lookup, lookup count:{:?}", *lc);
    }

    fn forget(&mut self, _req: &Request<'_>, ino: u64, nlookup: u64) {
        let ino = ino as u32;
        let mut lc_list = self.lookup_count.lock().unwrap();
        let lc = lc_list.entry(ino).or_insert(0);
        *lc -= nlookup as u32;
        debug!("filesystem:forget, lookup count:{:?}", *lc);
        if *lc <= 0 {
            lc_list.remove(&ino);
            match self.db.delete_inode_if_noref(ino) {
                Ok(n) => n,
                Err(err) => debug!("{}", err)
            }
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        match self.db.get_inode(ino as u32) {
            Ok(n) => {
                match n {
                    Some(v) => {
                        reply.attr(&ONE_SEC, &v.get_file_attr());
                        debug!("filesystem:getattr, return:{:?}", v.get_file_attr());
                    },
                    None => reply.error(ENOENT)
                }

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
            Ok(n) => {
                match n {
                    Some(v) => v,
                    None => {reply.error(ENOENT); return;}
                }
            },
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
        let old_size = attr.size;
        if let Some(n) = mode {attr.perm = n as u16};
        if let Some(n) = uid {attr.uid = n};
        if let Some(n) = gid {attr.gid = n};
        if let Some(n) = size {attr.size = n as u32};
        if let Some(n) = atime {attr.atime = attr.datetime_from(&n)};
        if let Some(n) = mtime {attr.mtime = attr.datetime_from(&n)};
        attr.ctime = SystemTime::now();
        if let Some(n) = crtime {attr.crtime = attr.datetime_from(&n)};
        if let Some(n) = flags {attr.flags = n};
        match self.db.update_inode(attr, old_size > attr.size) {
            Ok(_n) => (),
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
        reply.attr(&ONE_SEC, &attr.get_file_attr());
    }

    fn readlink(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyData) {
        let ino = ino as u32;
        let attr = match self.db.get_inode(ino) {
            Ok(n) => match n {
                Some(attr) => attr,
                None => {reply.error(ENOENT); return;}
            },
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };

        if attr.kind != FileType::Symlink {
            reply.error(EINVAL);
            return;
        }
        let size = attr.size;
        let mut data = match self.db.get_data(ino as u32, 1, size) {
            Ok(n) => n,
            Err(_err) => {reply.error(ENOENT); return; }
        };
        data.resize(size as usize, 0);
        reply.data(&data);
    }

    fn mkdir(&mut self, req: &Request<'_>, parent: u64, name: &OsStr, mode: u32, reply: ReplyEntry) {
        let now = SystemTime::now();
        let mut attr = DBFileAttr {
            ino: 0,
            size: 0,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: FileType::Directory,
            perm: mode as u16,
            nlink: 0,
            uid: req.uid(),
            gid: req.gid(),
            rdev: 0,
            flags: 0
        };
        let ino =  match self.db.add_inode(parent as u32, name.to_str().unwrap(), &attr) {
            Ok(n) => n,
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
        attr.ino = ino;
        reply.entry(&ONE_SEC, &attr.get_file_attr(), 0);
        let mut lc_list = self.lookup_count.lock().unwrap();
        let lc = lc_list.entry(ino).or_insert(0);
        *lc += 1;
        debug!("filesystem:mkdir, inode: {:?} lookup count:{:?}", ino, *lc);
    }

    fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let ino = match self.db.delete_dentry(parent as u32, name.to_str().unwrap()) {
            Ok(n) => n,
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
        let lc_list = self.lookup_count.lock().unwrap();
        if !lc_list.contains_key(&ino) {
            match self.db.delete_inode_if_noref(ino) {
                Ok(n) => n,
                Err(err) => {
                    reply.error(ENOENT);
                    debug!("{}", err);
                    return;
                }
            };
        }
        reply.ok();
    }

    fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let parent = parent as u32;
        let name = name.to_str().unwrap();
        let attr = match self.db.lookup(parent, name) {
            Ok(n) => {
                match n {
                    Some(v) => v,
                    None => {reply.error(ENOENT); return;}
                }
            },
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
        let empty = match self.db.check_directory_is_empty(attr.ino){
            Ok(n) => n,
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
        if !empty {
            reply.error(ENOTEMPTY);
            return;
        }
        let ino = match self.db.delete_dentry(parent, name) {
            Ok(n) => n,
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
        let lc_list = self.lookup_count.lock().unwrap();
        if !lc_list.contains_key(&ino) {
            match self.db.delete_inode_if_noref(ino) {
                Ok(n) => n,
                Err(err) => {
                    reply.error(ENOENT);
                    debug!("{}", err);
                    return;
                }
            };
        }
        reply.ok();
    }

    fn symlink(&mut self, req: &Request, parent: u64, name: &OsStr, link: &Path, reply: ReplyEntry) {
        let now = SystemTime::now();
        let mut attr = DBFileAttr {
            ino: 0,
            size: 0,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: FileType::Symlink,
            perm: 0o777, // never used
            nlink: 0,
            uid: req.uid(),
            gid: req.gid(),
            rdev: 0,
            flags: 0
        };
        let ino = match self.db.add_inode(parent as u32, name.to_str().unwrap(), &attr) {
            Ok(n) => n,
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
        let data = link.to_str().unwrap().as_bytes();
        let block_size = self.db.get_db_block_size() as usize;
        if data.len() > block_size {
            reply.error(ENAMETOOLONG);
            return;
        }
        match self.db.write_data(ino, 1, &data, data.len() as u32) {
            Ok(n) => n,
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        }
        attr.ino = ino;
        reply.entry(&ONE_SEC, &attr.get_file_attr(), 0);
        let mut lc_list = self.lookup_count.lock().unwrap();
        let lc = lc_list.entry(ino).or_insert(0);
        *lc += 1;
        debug!("filesystem:symlink, inode: {:?} lookup count:{:?}", ino, *lc);
    }

    fn rename(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        reply: ReplyEmpty
    ) {
        let parent = parent as u32;
        let name = name.to_str().unwrap();
        let newparent = newparent as u32;
        let newname = newname.to_str().unwrap();
        let entry =  match self.db.move_dentry(parent, name, newparent, newname) {
            Ok(n) => n,
            Err(err) => match err.kind() {
                ErrorKind::FsNotEmpty {description} => {reply.error(ENOTEMPTY); debug!("{}", &description); return;},
                ErrorKind::FsIsDir{description} => {reply.error(EISDIR); debug!("{}", &description); return;},
                ErrorKind::FsIsNotDir{description} => {reply.error(ENOTDIR); debug!("{}", &description); return;},
                _ => {reply.error(ENOENT); debug!("{}", err); return;},
            }
        };
        if let Some(ino) = entry {
            let lc_list = self.lookup_count.lock().unwrap();
            if !lc_list.contains_key(&ino) {
                match self.db.delete_inode_if_noref(ino) {
                    Ok(n) => n,
                    Err(err) => {reply.error(ENOENT); debug!("{}", err); return;},
                };
            }
        }
        reply.ok();
    }

    fn link(&mut self, _req: &Request<'_>, ino: u64, newparent: u64, newname: &OsStr, reply: ReplyEntry) {
        let attr = match self.db.link_dentry(ino as u32, newparent as u32, newname.to_str().unwrap()) {
            Ok(n) => n,
            Err(err) => match err.kind() {
                ErrorKind::FsParm{description} => {reply.error(EPERM); debug!("{}", &description); return;},
                ErrorKind::FsFileExist{description} => {reply.error(EEXIST); debug!("{}", &description); return;},
                _ => {reply.error(ENOENT); debug!("{}", err); return;}
            }
        };
        reply.entry(&ONE_SEC, &attr.get_file_attr(), 0);
        let mut lc_list = self.lookup_count.lock().unwrap();
        let lc = lc_list.entry(ino as u32).or_insert(0);
        *lc += 1;
        debug!("filesystem:link, lookup count:{:?}", *lc);
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

    fn create(&mut self, req: &Request<'_>, parent: u64, name: &OsStr, mode: u32, _flags: u32, reply: ReplyCreate) {
        let ino;
        let parent = parent as u32;
        let name = name.to_str().unwrap();
        let lookup_result = match self.db.lookup(parent, name) {
            Ok(n) => n,
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
        let mut attr: DBFileAttr;
        if lookup_result.is_none() {
            let now = SystemTime::now();
            attr = DBFileAttr {
                ino: 0,
                size: 0,
                blocks: 0,
                atime: now,
                mtime: now,
                ctime: now,
                crtime: now,
                kind: FileType::RegularFile,
                perm: mode as u16,
                nlink: 0,
                uid: req.uid(),
                gid: req.gid(),
                rdev: 0,
                flags: 0
            };
            ino = match self.db.add_inode(parent, name, &attr) {
                Ok(n) => n,
                Err(err) => {
                    reply.error(ENOENT);
                    debug!("{}", err);
                    return;
                }
            };
            attr.ino = ino;
            debug!("filesystem:create, created:{:?}", attr);
        } else {
            attr = lookup_result.unwrap();
            ino = attr.ino;
            debug!("filesystem:create, existed:{:?}", attr);
        }
        let mut lc_list = self.lookup_count.lock().unwrap();
        let lc = lc_list.entry(ino).or_insert(0);
        *lc += 1;
        reply.created(&ONE_SEC, &attr.get_file_attr(), 0, 0, 0);
    }
}
