use std::env;
use std::path::Path;
use std::ffi::OsStr;
#[macro_use] extern crate failure;
#[macro_use] extern crate log;
mod db_module;
mod sqerror;
mod filesystem;
use crate::db_module::{DbModule, DBFileAttr, DEntry};
use crate::db_module::sqlite::Sqlite;
use std::time::UNIX_EPOCH;
use crate::sqerror::SqError;
use crate::filesystem::SqliteFs;

const HELLO_ATTR: DBFileAttr = DBFileAttr {
    ino: 2,
    size: 13,
    blocks: 1,
    atime: UNIX_EPOCH,
    mtime: UNIX_EPOCH,
    ctime: UNIX_EPOCH,
    crtime: UNIX_EPOCH,
    perm: 0o100644,
    nlink: 0,
    uid: 0,
    gid: 0,
    rdev: 0,
    flags: 0
};


fn main() {
    env_logger::init();
    let path = Path::new("./filesystem.sqlite");
    let db : Sqlite;
    match db_module::sqlite::Sqlite::new(path) {
        Ok(n) => db = n,
        Err(err) => {println!("{:?}", err); return}
    }
    match db.table_exists() {
        Ok(n) => if n {
            println!("true");
        } else {
            println!("false");
            match db.init_database() {
                Ok(_n) => {},
                Err(err) => println!("{}", err)
            }
        },
        Err(err) => println!("{:?}", err)
    }
    let res: Result<(), SqError> = match db.get_inode(2) {
        Ok(n) => {
            if n.ino == 0 {
                let hello_dentry: DEntry = DEntry {
                    parent_ino: 1,
                    child_ino: 2,
                    filename: String::from("hello.txt"),
                    file_type: 0o100000
                };
                db.add_inode(&HELLO_ATTR);
                db.add_dentry(&hello_dentry);
                db.increase_nlink(HELLO_ATTR.ino);
                db.add_data(HELLO_ATTR.ino, 1, "Hello World!\n".as_bytes());
            }
            Ok(())
        },
        Err(err) => Err(err)
    };
    res.map_err(|err|  println!("{:?}", err));

    let mountpoint = env::args_os().nth(1).unwrap();
    let options = ["-o", "ro", "-o", "fsname=hello"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();
    let fs: SqliteFs = match SqliteFs::new("./filesystem.sqlite") {
        Ok(n) => n,
        Err(err) => {println!("{:?}", err); return;}
    };
    fuse::mount(fs, &mountpoint, &options).unwrap();
}
