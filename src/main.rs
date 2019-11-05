#[macro_use] extern crate failure;
#[macro_use] extern crate log;
#[macro_use] extern crate clap;
mod db_module;
mod sqerror;
mod filesystem;
use std::env;
use std::ffi::OsStr;
use crate::filesystem::SqliteFs;
use clap::{App, Arg};

fn main() {
    env_logger::init();

    let mount_option_arg = Arg::with_name("mount_option")
        .short("o")
        .long("option")
        .help("Additional mount option for this filesystem")
        .takes_value(true)
        .multiple(true);

    let mount_point_arg = Arg::with_name("mount_point")
        .help("Target mountpoint path")
        .index(1)
        .required(true);

    let db_path_arg = Arg::with_name("db_path")
        .help("Sqlite database file path")
        .index(2)
        .required(true);

    let matches = App::new("sqlitefs")
        .about("Sqlite database as a filesystem.")
        .version(crate_version!())
        .arg(mount_option_arg)
        .arg(mount_point_arg)
        .arg(db_path_arg)
        .get_matches();

    let mut option_vals = ["-o", "fsname=sqlitefs"].to_vec();
    if let Some(v) = matches.values_of("mount_option") {
        for i in v {
            option_vals.push("-o");
            option_vals.push(i);
        }
    }

    let mountpoint = matches.value_of("mount_point").expect("Mount point path is missing.");
    let db_path = matches.value_of("db_path").expect("Database file path is missing.");
    let options = option_vals
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();
    let fs: SqliteFs = match SqliteFs::new(db_path) {
        Ok(n) => n,
        Err(err) => {println!("{:?}", err); return;}
    };
    match fuse::mount(fs, &mountpoint, &options) {
        Ok(n) => n,
        Err(err) => error!("{}", err)
    }
}
