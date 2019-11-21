#![allow(dead_code)]
extern crate tempfile;
use nix::dir::Dir;
use sqlite_fs::db_module::{sqlite, DbModule};
use std::fs::File;
use std::mem;

enum DirOrNot {
    Empty,
    Exist(tempfile::TempDir),
}

pub struct DBWithTempFile {
    pub db: sqlite::Sqlite,
    dir: DirOrNot,
}

impl DBWithTempFile {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("filesystem.db");
        let db = sqlite::Sqlite::new(file_path.as_path()).unwrap();
        let dir = DirOrNot::Exist(dir);
        Self { db, dir }
    }
}

impl Drop for DBWithTempFile {
    fn drop(&mut self) {
        let dir = mem::replace(&mut self.dir, DirOrNot::Empty);
        match dir {
            DirOrNot::Empty => (),
            DirOrNot::Exist(temp) => {
                temp.close().unwrap();
            }
        }
    }
}
