use std::path::Path;
use std::time::SystemTime;
use chrono::{Utc, DateTime};
use rusqlite::types::ToSql;
use rusqlite::{params, Connection, NO_PARAMS};
use crate::db_module::{DbModule, DBFileAttr, DEntry};
use libc::{S_IRWXU, S_IFDIR};
use crate::sqerror::SqError;

pub struct Sqlite {
    conn: Connection
}

impl Sqlite {
    pub fn new(path: &Path) -> Result<Self, SqError> {
        let conn = Connection::open(path)?;
        // enable foreign key. Sqlite ignores foreign key by default.
        conn.execute("PRAGMA foreign_keys=true", NO_PARAMS)?;
        Ok(Sqlite { conn })
    }

    pub fn table_exists(&self) -> Result<bool, SqError> {
        let mut stmt = self.conn
            .prepare("SELECT count(name) FROM sqlite_master WHERE type='table' AND name=$1")?;
        let res: i32 = stmt.query_row(params!["metadata"], |row| {row.get(0)})?;
        if res != 0 {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn get_nlink(&self, inode: u32) -> Result<u32, SqError> {
        let mut stmt = self.conn
            .prepare("SELECT nlink FROM metadata WHERE id=$1")?;
        let res: u32 = stmt.query_row(params![inode], |row| { row.get(0) })?;
        Ok(res)
    }
}

impl DbModule for Sqlite {
    /// Create table and insert root entry for empty database
    fn init_database(&self) -> Result<(), SqError> {
        // inode table
        self.conn.execute("CREATE TABLE metadata(
            id integer primary key,
            size int default 0 not null,
            atime text,
            atime_nsec int,
            mtime text,
            mtime_nsec int,
            ctime text,
            ctime_nsec int,
            crtime text,
            crtime_nsec int,
            mode int,
            nlink int default 0 not null,
            uid int default 0,
            gid int default 0,
            rdev int default 0,
            flags int default 0
        )", NO_PARAMS)?;
        // data table
        self.conn.execute("CREATE TABLE data(
            file_id int,
            block_num int,
            data blob,
            foreign key (file_id) references metadata(id) on delete cascade,
            primary key (file_id, block_num)
        )", NO_PARAMS)?;
        // directory entry table
        self.conn.execute("CREATE TABLE dentry(
            parent_id int,
            child_id int,
            file_type int,
            name text,
            foreign key (parent_id) references metadata(id) on delete cascade,
            foreign key (child_id) references metadata(id) on delete cascade,
            primary key (parent_id, name)
        )", NO_PARAMS)?;
        // extended attribute table
        self.conn.execute("CREATE TABLE xattr(
            file_id int,
            name text,
            value blob,
            foreign key (file_id) references metadata(id) on delete cascade,
            primary key (file_id, name)
        )", NO_PARAMS)?;
        // insert root dir info.
        let now = SystemTime::now();
        self.add_inode(&DBFileAttr {
            ino: 1,
            size: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            perm: S_IFDIR | S_IRWXU,
            nlink: 0,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
        })?;
        self.add_dentry(&DEntry {
            parent_ino: 1,
            child_ino: 1,
            file_type: S_IFDIR,
            filename: String::from(".")
        })?;
        self.add_dentry(&DEntry {
            parent_ino: 1,
            child_ino: 1,
            file_type: S_IFDIR,
            filename: String::from("..")
        })?;
        self.increase_nlink(1)?;
        Ok(())
    }

    fn add_inode(&self, attr: &DBFileAttr) -> Result<(), SqError> {
        let atime: DateTime<Utc> = attr.atime.into();
        let mtime: DateTime<Utc> = attr.mtime.into();
        let ctime: DateTime<Utc> = attr.ctime.into();
        let crtime: DateTime<Utc> = attr.crtime.into();
        self.conn.execute("INSERT INTO metadata
            (id,
            size,
            atime,
            atime_nsec,
            mtime,
            mtime_nsec,
            ctime,
            ctime_nsec,
            crtime,
            crtime_nsec,
            mode,
            nlink,
            uid,
            gid,
            rdev,
            flags)
            VALUES($1,
            $2,
            datetime($3),
            $4,
            datetime($5),
            $6,
            datetime($7),
            $8,
            datetime($9),
            $10,
            $11, $12, $13, $14, $15, $16
            )", params![
                attr.ino,
                attr.size,
                atime.format("%Y-%m-%d %H:%M:%S").to_string(),
                atime.timestamp_subsec_nanos(),
                mtime.format("%Y-%m-%d %H:%M:%S").to_string(),
                mtime.timestamp_subsec_nanos(),
                ctime.format("%Y-%m-%d %H:%M:%S").to_string(),
                ctime.timestamp_subsec_nanos(),
                crtime.format("%Y-%m-%d %H:%M:%S").to_string(),
                crtime.timestamp_subsec_nanos(),
                attr.perm,
                attr.nlink,
                attr.uid,
                attr.gid,
                attr.rdev,
                attr.flags
        ])?;
        Ok(())
    }

    fn add_dentry(&self, entry: &DEntry) -> Result<(), SqError> {
        self.conn.execute("INSERT INTO dentry \
            (parent_id, child_id, file_type, name)
            VALUES($1, $2, $3, $4)",
         params![
                entry.parent_ino, entry.child_ino, entry.file_type, entry.filename
            ])?;
        Ok(())
    }

    fn increase_nlink(&self, inode: u32) -> Result<u32, SqError> {
        let num = self.get_nlink(inode)? + 1;
        self.conn.execute("UPDATE metadata SET nlink=$1 where id=$2",
      params![num, inode])?;
        Ok(num)
    }

    fn decrease_nlink(&self, inode: u32) -> Result<u32, SqError> {
        let num = self.get_nlink(inode)? - 1;
        self.conn.execute("UPDATE metadata SET nlink=$1 where id=$2",
                          params![num, inode])?;
        Ok(num)
    }

    fn execute(&self) {
        /*self.connection
            .iterate("SELECT * FROM users WHERE age > 15", |pairs| {
                for &(column, value) in pairs.iter() {
                    println!("{} = {}", column, value.unwrap());
                }
                true
            })
            .unwrap();*/
    }
}
