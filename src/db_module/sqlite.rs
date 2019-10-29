use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{Utc, DateTime, NaiveDateTime, Timelike};
use rusqlite::types::ToSql;
use rusqlite::{params, Connection, NO_PARAMS, Transaction, Statement};
use crate::db_module::{DbModule, DBFileAttr, DEntry};
use crate::sqerror::SqError;
use std::sync::{Mutex, MutexGuard};
use fuse::FileType;

const DB_IFIFO: u32 = 0o0010000;
const DB_IFCHR: u32 = 0o0020000;
const DB_IFDIR: u32 = 0o0040000;
const DB_IFBLK: u32 = 0o0060000;
const DB_IFREG: u32 = 0o0100000;
const DB_IFLNK: u32 = 0o0120000;
const DB_IFSOCK: u32 = 0o0140000;

const EMPTY_ATTR: DBFileAttr = DBFileAttr {
ino: 0,
size: 0,
blocks: 0,
atime: UNIX_EPOCH,
mtime: UNIX_EPOCH,
ctime: UNIX_EPOCH,
crtime: UNIX_EPOCH,
kind: FileType::RegularFile,
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
        conn.execute("PRAGMA foreign_keys=ON", NO_PARAMS)?;
        Ok(Sqlite { conn: Mutex::new(conn) })
    }

    fn string_to_systemtime(&self, text: String, nsec: u32) -> SystemTime {
        SystemTime::from(DateTime::<Utc>::from_utc(
            NaiveDateTime::parse_from_str(&text, "%Y-%m-%d %H:%M:%S").unwrap().with_nanosecond(nsec).unwrap(), Utc
        ))
    }

    fn file_type_to_const(&self, kind: FileType) -> u32 {
        match kind {
            FileType::RegularFile => DB_IFREG,
            FileType::Socket => DB_IFSOCK,
            FileType::Directory => DB_IFDIR,
            FileType::Symlink => DB_IFLNK,
            FileType::BlockDevice => DB_IFBLK,
            FileType::CharDevice => DB_IFCHR,
            FileType::NamedPipe => DB_IFIFO,
        }
    }

    fn const_to_file_type(&self, kind: u32) -> FileType {
        match kind {
            DB_IFREG => FileType::RegularFile,
            DB_IFSOCK => FileType::Socket,
            DB_IFDIR => FileType::Directory,
            DB_IFLNK => FileType::Symlink,
            DB_IFBLK => FileType::BlockDevice,
            DB_IFCHR => FileType::CharDevice,
            DB_IFIFO => FileType::NamedPipe,
            _ => FileType::RegularFile,
        }
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
            metadata.kind, \
            metadata.mode,\
            metadata.nlink,\
            metadata.uid,\
            metadata.gid,\
            metadata.rdev,\
            metadata.flags,\
            blocknum.block_num \
            FROM metadata \
            LEFT JOIN (SELECT count(block_num) block_num FROM data WHERE file_id=$1) AS blocknum
            WHERE id=$1";
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
            blocks: row.get(17)?,
            atime: self.string_to_systemtime(row.get(2)?, row.get(3)?),
            mtime: self.string_to_systemtime(row.get(4)?, row.get(5)?),
            ctime: self.string_to_systemtime(row.get(6)?, row.get(7)?),
            crtime: self.string_to_systemtime(row.get(8)?, row.get(9)?),
            kind: self.const_to_file_type(row.get(10)?),
            perm: row.get(11)?,
            nlink: row.get(12)?,
            uid: row.get(13)?,
            gid: row.get(14)?,
            rdev: row.get(15)?,
            flags: row.get(16)?
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

    fn update_time(&self, inode: u32, sql: &str, time: DateTime<Utc>, tx: Option<&Transaction>) -> Result<(), SqError> {
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
        let params = params![&time.format("%Y-%m-%d %H:%M:%S").to_string(), time.timestamp_subsec_nanos(), inode];
        stmt.execute(params)?;
        Ok(())
    }

    fn update_atime(&self, inode: u32, time: DateTime<Utc>, tx: Option<&Transaction>) -> Result<(), SqError> {
        let sql = "UPDATE metadata SET atime=datetime($1), atime_nsec=$2 WHERE id=$3";
        self.update_time(inode, sql, time, tx)
    }

    fn update_mtime(&self, inode: u32, time: DateTime<Utc>, tx: Option<&Transaction>) -> Result<(), SqError> {
        let sql = "UPDATE metadata SET mtime=datetime($1), mtime_nsec=$2 WHERE id=$3";
        self.update_time(inode, sql, time, tx)
    }

    fn update_ctime(&self, inode: u32, time: DateTime<Utc>, tx: Option<&Transaction>) -> Result<(), SqError> {
        let sql = "UPDATE metadata SET ctime=datetime($1), ctime_nsec=$2 WHERE id=$3";
        self.update_time(inode, sql, time, tx)
    }

    fn release_data_local(&self, inode: u32, tx: Option<&Transaction>) -> Result<(), SqError> {
        let sql = "DELETE FROM data WHERE file_id=$1";
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
        stmt.execute(params![inode])?;
        Ok(())
    }
}

impl DbModule for Sqlite {
    fn get_inode(&self, inode: u32) -> Result<DBFileAttr, SqError> {
        self.get_inode_local(inode, None)
    }

    fn update_inode(&self, attr: DBFileAttr) -> Result<(), SqError> {
        let sql = "UPDATE metadata SET \
            size=$1,\
            atime=datetime($2),\
            atime_nsec=$3,\
            mtime=datetime($4),\
            mtime_nsec=$5,\
            ctime=datetime($6),\
            ctime_nsec=$7,\
            crtime=datetime($8),\
            crtime_nsec=$9,\
            mode=$10,\
            uid=$11,\
            gid=$12,\
            rdev=$13,\
            flags=$14 \
             WHERE id=$15";
        let atime = DateTime::<Utc>::from(attr.atime);
        let mtime = DateTime::<Utc>::from(attr.mtime);
        let ctime = DateTime::<Utc>::from(attr.ctime);
        let crtime = DateTime::<Utc>::from(attr.crtime);
        let mut connect = self.conn.lock().unwrap();
        let tx = connect.transaction()?;
        {
            let mut stmt = tx.prepare(sql)?;
            stmt.execute(params![
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
            attr.uid,
            attr.gid,
            attr.rdev,
            attr.flags,
            attr.ino
            ])?;
        }
        if attr.size == 0 {
            self.release_data_local(attr.ino, Some(&tx))?;
        }
        Ok(())
    }

    fn get_dentry(&self, inode: u32) -> Result<Vec<DEntry>, SqError> {
        let sql = "SELECT child_id, file_type, name FROM dentry WHERE parent_id=$1 ORDER BY name";
        let connect = self.conn.lock().unwrap();
        let mut stmt = connect.prepare(sql)?;
        let rows = stmt.query_map(params![inode], |row| {
            Ok(DEntry{parent_ino: inode,
                child_ino: row.get(0)?,
                file_type: self.const_to_file_type(row.get(1)?),
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
            metadata.kind, \
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
            LEFT JOIN (SELECT file_id file_id, count(block_num) block_num from data) AS blocknum \
            ON dentry.child_id = blocknum.file_id
            ";
        let connect = self.conn.lock().unwrap();
        let stmt = connect.prepare(sql)?;
        let params = params![parent, name];
        self.parse_attr(stmt, params)
    }

    fn get_data(&self, inode:u32, block: u32, length: u32) -> Result<Vec<u8>, SqError> {
        let mut connect = self.conn.lock().unwrap();
        let tx = connect.transaction()?;
        let row: Vec<u8>;
        {
            let mut stmt = tx.prepare(
                "SELECT \
                data FROM data WHERE file_id=$1 AND block_num=$2")?;
            row = match stmt.query_row(params![inode, block], |row| row.get(0)) {
                Ok(n) => n,
                Err(err) => {
                    if err == rusqlite::Error::QueryReturnedNoRows {
                        vec![0; length as usize]
                    } else {
                        return Err(SqError::from(err))
                    }
                }
            };
        }
        self.update_atime(inode, Utc::now(), Some(&tx))?;
        tx.commit()?;
        Ok(row)
    }

    fn write_data(&self, inode:u32, block: u32, data: &[u8], size: u32) -> Result<(), SqError> {
        let mut connect = self.conn.lock().unwrap();
        let tx = connect.transaction()?;
        {
            let db_size: u32 = tx.query_row("SELECT size FROM metadata WHERE id=$1", params![inode], |row| row.get(0))?;
            tx.execute("REPLACE INTO data \
            (file_id, block_num, data)
            VALUES($1, $2, $3)",
                       params![inode, block, data])?;
            if size > db_size {
                tx.execute("UPDATE metadata SET size=$1 WHERE id=$2", params![size, inode])?;
            }
        }
        let time = Utc::now();
        self.update_mtime(inode, time, Some(&tx))?;
        self.update_ctime(inode, time, Some(&tx))?;
        tx.commit()?;
        Ok(())
    }

    fn release_data(&self, inode: u32) -> Result<(), SqError> {
        let connect = self.conn.lock().unwrap();
        connect.execute("DELETE FROM data WHERE file_id=$1", params![inode])?;
        Ok(())
    }

    fn get_db_block_size(&self) -> u32 {
        BLOCK_SIZE
    }
}
