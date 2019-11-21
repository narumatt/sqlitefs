use sqlite_fs::db_module::{sqlite, DbModule};

mod helpers;
#[test]
fn sqlite_create_db() {
    let db = sqlite::Sqlite::new_in_memory();
    match db {
        Ok(_) => assert!(true),
        Err(_) => assert!(false, "failed to create db in memory"),
    }
}

#[test]
fn sqlite_create_db_file() {
    let mut dbf = helpers::DBWithTempFile::new();
    match dbf.db.init() {
        Ok(_) => assert!(true),
        Err(_) => assert!(false, "failed to create db file"),
    }
}

#[test]
fn sqlite_init_db() {
    let mut db = match sqlite::Sqlite::new_in_memory() {
        Ok(n) => n,
        Err(_) => {
            assert!(false, "failed to create db in memory");
            return;
        }
    };
    match db.init() {
        Ok(_) => assert!(true),
        Err(_) => assert!(false, "failed to init db"),
    }
}
