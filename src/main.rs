use std::path::Path;
#[macro_use] extern crate failure;
mod db_module;
mod sqerror;
use crate::db_module::DbModule;
use crate::db_module::sqlite::Sqlite;

fn main() {
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
}
