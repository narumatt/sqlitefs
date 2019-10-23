use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{Utc, DateTime, NaiveDateTime, Timelike};
use rusqlite::types::ToSql;
use rusqlite::{params, Connection, NO_PARAMS, Transaction, Statement};
use crate::db_module::{DbModule, DBFileAttr, DEntry};
use crate::sqerror::SqError;
use std::sync::{Mutex, MutexGuard};

const EMPTY_ATTR: DBFileAttr = DBFileAttr {
ino: 0,
size: 0,
blocks: 0,
atime: UNIX_EPOCH,
mtime: UNIX_EPOCH,
ctime: UNIX_EPOCH,
crtime: UNIX_EPOCH,
perm: 0,
nlink: 0,
uid: 0,
gid: 0,
rdev: 0,
flags: 0
};

const BLOCK_SIZE: u32 = 4096;

pub struct Sqlite {
    conn: Mutex<Connection>
}

impl Sqlite {
    pub fn new(path: &Path) -> Result<Self, SqError> {
        let conn = Connection::open(path)?;
        // enable foreign key. Sqlite ignores foreign key by default.
        conn.execute("PRAGMA foreign_keys=true", NO_PARAMS)?;
        Ok(Sqlite { conn: Mutex::new(conn) })
    }

    fn string_to_systemtime(&self, text: String, nsec: u32) -> SystemTime {
        SystemTime::from(DateTime::<Utc>::from_utc(
            NaiveDateTime::parse_from_str(&text, "%Y-%m-%d %H:%M:%S").unwrap().with_nanosecond(nsec).unwrap(), Utc
        ))
    }

    fn get_inode_local(&self, inode: u32, tx: Option<&Transaction>) -> Result<DBFileAttr, SqError> {
        let sql = "SELECT \
            metadata.id,\
            metadata.size,\
            metadata.atime,\
            metadata.atime_nsec,\
            metadata.mtime,\
            metadata.mtime_nsec,\
            metadata.ctime,\
            metadata.ctime_nsec,\
            metadata.crtime,\
            metadata.crtime_nsec,\
            metadata.mode,\
            metadata.nlink,\
            metadata.uid,\
            metadata.gid,\
            metadata.rdev,\
            metadata.flags,\
            blocknum.block_num \
            FROM metadata \
            LEFT JOIN (SELECT count(block_num) block_num from data where file_id=$1) as blocknum
            where id=$1";
        let connect: MutexGuard<Connection>;
        let stmt = match tx {
            Some(tx) => {
                tx.prepare(sql)?
            },
            None => {
                connect = self.conn.lock().unwrap();
                connect.prepare(sql)?
            }
        };
        let params = params![inode];
        self.parse_attr(stmt, params)
    }

    fn parse_attr_row(&self, row: &rusqlite::Row) -> Result<DBFileAttr, rusqlite::Error> {
        Ok(DBFileAttr {
            ino: row.get(0)?,
            size: row.get(1)?,
            blocks: row.get(16)?,
            atime: self.string_to_systemtime(row.get(2)?, row.get(3)?),
            mtime: self.string_to_systemtime(row.get(4)?, row.get(5)?),
            ctime: self.string_to_systemtime(row.get(6)?, row.get(7)?),
            crtime: self.string_to_systemtime(row.get(8)?, row.get(9)?),
            perm: row.get(10)?,
            nlink: row.get(11)?,
            uid: row.get(12)?,
            gid: row.get(13)?,
            rdev: row.get(14)?,
            flags: row.get(15)?
        })
    }


    fn parse_attr(&self, mut stmt: Statement, params: &[&dyn ToSql]) -> Result<DBFileAttr, SqError> {
        let rows = stmt.query_map(params, |row| self.parse_attr_row(row))?;
        let mut attrs = Vec::new();
        for row in rows {
            attrs.push(row?);
        }
        if attrs.len() == 0 {
            Ok(EMPTY_ATTR)
        } else {
            Ok(attrs[0])
        }
    }
}

impl DbModule for Sqlite {
    fn get_inode(&self, inode: u32) -> Result<DBFileAttr, SqError> {
        self.get_inode_local(inode, None)
    }

    fn get_dentry(&self, inode: u32) -> Result<Vec<DEntry>, SqError> {
        let sql = "SELECT child_id, file_type, name FROM dentry WHERE parent_id=$1 ORDER BY name";
        let connect = self.conn.lock().unwrap();
        let mut stmt = connect.prepare(sql)?;
        let rows = stmt.query_map(params![inode], |row| {
            Ok(DEntry{parent_ino: inode,
                child_ino: row.get(0)?,
                file_type: row.get(1)?,
                filename: row.get(2)?,
            })
        })?;
        let mut entries: Vec<DEntry> = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    fn lookup(&self, parent: u32, name: &str) -> Result<DBFileAttr, SqError> {
        let sql = "SELECT \
            metadata.id,\
            metadata.size,\
            metadata.atime,\
            metadata.atime_nsec,\
            metadata.mtime,\
            metadata.mtime_nsec,\
            metadata.ctime,\
            metadata.ctime_nsec,\
            metadata.crtime,\
            metadata.crtime_nsec,\
            metadata.mode,\
            metadata.nlink,\
            metadata.uid,\
            metadata.gid,\
            metadata.rdev,\
            metadata.flags, \
            blocknum.block_num \
            FROM dentry \
            INNER JOIN metadata \
            ON metadata.id=dentry.child_id \
            AND dentry.parent_id=$1 \
            AND dentry.name=$2 \
            LEFT JOIN (SELECT file_id file_id, count(block_num) block_num from data) as blocknum \
            ON dentry.child_id = blocknum.file_id
            ";
        let connect = self.conn.lock().unwrap();
        let stmt = connect.prepare(sql)?;
        let params = params![parent, name];
        self.parse_attr(stmt, params)
    }

    fn get_data(&self, inode:u32, block: u32, length: u32) -> Result<Vec<u8>, SqError> {
        let connect = self.conn.lock().unwrap();
        let mut stmt = connect.prepare(
            "SELECT \
                data FROM data where file_id=$1 and block_num=$2")?;
        let row: Vec<u8> = match stmt.query_row(params![inode, block], |row| row.get(0)) {
            Ok(n) => n,
            Err(err) => {
                if err == rusqlite::Error::QueryReturnedNoRows {
                    vec![0; length as usize]
                } else {
                    return Err(SqError::from(err))
                }
            }
        };
        Ok(row)
    }

    fn get_db_block_size(&self) -> u32 {
        BLOCK_SIZE
    }
}
