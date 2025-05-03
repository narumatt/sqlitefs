#[macro_use] extern crate failure;
#[macro_use] extern crate log;
#[macro_use] extern crate clap;
use std::env;
use std::ffi::OsStr;
use sqlite_fs::filesystem::SqliteFs;
use clap::{Arg};
use sqlite_fs::db_module::sqlite::Sqlite;
use sqlite_fs::db_module::DbModule;
use std::path::PathBuf;

fn main() {
    env_logger::init();

    let mount_option_arg = Arg::new("mount_option")
        .short('o')
        .long("option")
        .help("Additional mount option for this filesystem")
        .value_parser(clap::builder::NonEmptyStringValueParser::new())
        .num_args(1..);

    let mount_point_arg = Arg::new("mount_point")
        .help("Target mountpoint path")
        .index(1)
        .required(true);

    let db_path_arg = Arg::new("db_path")
        .help("Sqlite database file path. If not set, open database in memory.")
        .index(2);

    let matches = command!()
        .about("Sqlite database as a filesystem.")
        .version(crate_version!())
        .arg(mount_option_arg)
        .arg(mount_point_arg)
        .arg(db_path_arg)
        .get_matches();

    let mut option_vals = ["-o", "fsname=sqlitefs", "-o", "default_permissions", "-o", "allow_other"].to_vec();
    if let Some(v) = matches.get_many::<String>("mount_option") {
        for i in v {
            option_vals.push("-o");
            option_vals.push(i);
        }
    }

    let mountpoint = matches.get_one::<String>("mount_point").expect("Mount point path is missing.");
    let db_path = matches.get_one::<String>("db_path");
    let options = option_vals
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();
    let fs: SqliteFs;
    match db_path {
        Some(path) => {
            fs = match SqliteFs::new(path) {
                Ok(n) => n,
                Err(err) => {println!("{:?}", err); return;}
            };
        }
        None => {
            let mut db = match Sqlite::new_in_memory() {
                Ok(n) => n,
                Err(err) => {println!("{:?}", err); return;}
            };
            match db.init() {
                Ok(n) => n,
                Err(err) => {println!("{:?}", err); return;}
            };
            fs = match SqliteFs::new_with_db(db) {
                Ok(n) => n,
                Err(err) => {println!("{:?}", err); return;}
            };
        }
    }
    match fuse::mount(fs, &mountpoint, &options) {
        Ok(n) => n,
        Err(err) => error!("{}", err)
    }
}
