use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{Utc, DateTime, NaiveDateTime, Timelike};
use rusqlite::types::ToSql;
use rusqlite::{params, Connection, NO_PARAMS, Transaction, Statement};
use crate::db_module::{DbModule, DBFileAttr, DEntry};
use libc::{S_IRWXU, S_IFDIR};
use crate::sqerror::SqError;
use std::sync::{Mutex, MutexGuard};
use std::fs::OpenOptions;
use std::error::Error;

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

macro_rules! parse_attr_row {
    ($row:ident) => {{}};
}

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

    pub fn table_exists(&self) -> Result<bool, SqError> {
        let connect = self.conn.lock().unwrap();
        let mut stmt = connect
            .prepare("SELECT count(name) FROM sqlite_master WHERE type='table' AND name=$1")?;
        let res: i32 = stmt.query_row(params!["metadata"], |row| {row.get(0)})?;
        if res != 0 {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn get_nlink(&self, inode: u32, tx: Option<&Transaction>) -> Result<u32, SqError> {
        let sql = "SELECT nlink FROM metadata WHERE id=$1";
        let res:u32;
        match tx {
            Some(tx) => {
                let mut stmt = tx.prepare(sql)?;
                res = stmt.query_row(params![inode], |row| { row.get(0) })?;
            },
            None => {
                let connect = self.conn.lock().unwrap();
                let mut stmt = connect.prepare(sql)?;
                res = stmt.query_row(params![inode], |row| { row.get(0) })?;
            }
        }
        Ok(res)
    }

    fn string_to_systemtime(&self, text: String, nsec: u32) -> SystemTime {
        SystemTime::from(DateTime::<Utc>::from_utc(
            NaiveDateTime::parse_from_str(&text, "%Y-%m-%d %H:%M:%S").unwrap().with_nanosecond(nsec).unwrap(), Utc
        ))
    }

    fn search_inode_from_name(&self, parent: u32, name: &str, tx: Option<&Transaction>) -> Result<u32, SqError> {
        let sql = "SELECT child_id FROM dentry where parent_id=$1 and name=$2";
        let res: u32;
        let connect: MutexGuard<Connection>;
        let mut stmt: Statement = match tx {
            Some(tx) => {
                tx.prepare(sql)?

            },
            None => {
                connect = self.conn.lock().unwrap();
                connect.prepare(sql)?
            }
        };
        let row: u32 = match stmt.query_row(params![parent, name], |row| row.get(0)) {
            Ok(n) => n,
            Err(err) => {
                if err == rusqlite::Error::QueryReturnedNoRows {
                    0
                } else {
                    return Err(SqError::from(err))
                }
            }
        };
        Ok(row)
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
        let mut stmt = match tx {
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

    fn get_block_count(&self, inode: u32) -> Result<u32, SqError> {
        let sql = "SELECT count(block_num) from data where file_id=$1";
        let connect = self.conn.lock().unwrap();
        let mut stmt = connect.prepare(sql)?;
        let res: u32 = stmt.query_row(params![inode], |row| {row.get(0)})?;
        Ok(res)
    }
}

impl DbModule for Sqlite {
    /// Create table and insert root entry for empty database
    fn init_database(&self) -> Result<(), SqError> {
        // inode table
        {
            let connect = self.conn.lock().unwrap();
            connect.execute("CREATE TABLE metadata(\
            id integer primary key,\
            size int default 0 not null,\
            atime text,\
            atime_nsec int,\
            mtime text,\
            mtime_nsec int,\
            ctime text,\
            ctime_nsec int,\
            crtime text,\
            crtime_nsec int,\
            mode int,\
            nlink int default 0 not null,\
            uid int default 0,\
            gid int default 0,\
            rdev int default 0,\
            flags int default 0\
            )", NO_PARAMS)?;
            // data table
            connect.execute("CREATE TABLE data(\
            file_id int,\
            block_num int,\
            data blob,\
            foreign key (file_id) references metadata(id) on delete cascade,\
            primary key (file_id, block_num)\
            )", NO_PARAMS)?;
            // directory entry table
            connect.execute("CREATE TABLE dentry(\
            parent_id int,\
            child_id int,\
            file_type int,\
            name text,\
            foreign key (parent_id) references metadata(id) on delete cascade,\
            foreign key (child_id) references metadata(id) on delete cascade,\
            primary key (parent_id, name)\
            )", NO_PARAMS)?;
            // extended attribute table
            connect.execute("CREATE TABLE xattr(\
            file_id int,\
            name text,\
            value blob,\
            foreign key (file_id) references metadata(id) on delete cascade,\
            primary key (file_id, name)\
            )", NO_PARAMS)?;
        }
        // insert root dir info.
        let now = SystemTime::now();
        self.add_inode(&DBFileAttr {
            ino: 1,
            size: 0,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            perm: (S_IFDIR | S_IRWXU) as u16,
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
            filename: String::from("."),
        })?;
        self.add_dentry(&DEntry {
            parent_ino: 1,
            child_ino: 1,
            file_type: S_IFDIR,
            filename: String::from(".."),
        })?;
        self.increase_nlink(1)?;
        Ok(())
    }

    fn add_inode(&self, attr: &DBFileAttr) -> Result<(), SqError> {
        let atime: DateTime<Utc> = attr.atime.into();
        let mtime: DateTime<Utc> = attr.mtime.into();
        let ctime: DateTime<Utc> = attr.ctime.into();
        let crtime: DateTime<Utc> = attr.crtime.into();
        self.conn.lock().unwrap().execute("INSERT INTO metadata\
            (id,\
            size,\
            atime,\
            atime_nsec,\
            mtime,\
            mtime_nsec,\
            ctime,\
            ctime_nsec,\
            crtime,\
            crtime_nsec,\
            mode,\
            nlink,\
            uid,\
            gid,\
            rdev,\
            flags) \
            VALUES($1,\
            $2,\
            datetime($3),\
            $4,\
            datetime($5),\
            $6,\
            datetime($7),\
            $8,\
            datetime($9),\
            $10,\
            $11, $12, $13, $14, $15, $16\
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

    fn get_inode(&self, inode: u32) -> Result<DBFileAttr, SqError> {
        self.get_inode_local(inode, None)
    }

     fn add_dentry(&self, entry: &DEntry) -> Result<(), SqError> {
        self.conn.lock().unwrap().execute("INSERT INTO dentry \
            (parent_id, child_id, file_type, name)
            VALUES($1, $2, $3, $4)",
         params![
                entry.parent_ino, entry.child_ino, entry.file_type, entry.filename
            ])?;
        Ok(())
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
        let mut stmt = connect.prepare(sql)?;
        let params = params![parent, name];
        self.parse_attr(stmt, params)
    }

    fn increase_nlink(&self, inode: u32) -> Result<u32, SqError> {
        let mut connect = self.conn.lock().unwrap();
        let tx = connect.transaction()?;
        let num = self.get_nlink(inode, Some(&tx))? + 1;
        tx.execute("UPDATE metadata SET nlink=$1 where id=$2",
      params![num, inode])?;
        tx.commit()?;
        Ok(num)
    }

    fn decrease_nlink(&self, inode: u32) -> Result<u32, SqError> {
        let mut connect = self.conn.lock().unwrap();
        let tx = connect.transaction()?;
        let num = self.get_nlink(inode, Some(&tx))? - 1;
        tx.execute("UPDATE metadata SET nlink=$1 where id=$2",
                          params![num, inode])?;
        tx.commit()?;
        Ok(num)
    }

    fn add_data(&self, inode:u32, block: u32, data: &[u8]) -> Result<(), SqError> {
        self.conn.lock().unwrap().execute("REPLACE INTO data \
            (file_id, block_num, data)
            VALUES($1, $2, $3)",
          params![inode, block, data])?;
        Ok(())
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
