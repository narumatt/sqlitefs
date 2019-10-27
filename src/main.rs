use std::env;
use std::ffi::OsStr;
#[macro_use] extern crate failure;
#[macro_use] extern crate log;
mod db_module;
mod sqerror;
mod filesystem;
use crate::filesystem::SqliteFs;

fn main() {
    env_logger::init();
    let mountpoint = env::args_os().nth(1).unwrap();
    let db_path = env::args_os().nth(2).unwrap();
    let options = ["-o", "ro", "-o", "fsname=sqlitefs"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();
    let fs: SqliteFs = match SqliteFs::new(db_path.to_str().unwrap()) {
        Ok(n) => n,
        Err(err) => {println!("{:?}", err); return;}
    };
    fuse::mount(fs, &mountpoint, &options).unwrap();
}
