use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{Utc, DateTime, NaiveDateTime, Timelike};
use rusqlite::types::ToSql;
use rusqlite::{params, Connection, NO_PARAMS, Statement};
use crate::db_module::{DbModule, DBFileAttr, DEntry};
use crate::sqerror::{Error, Result, ErrorKind};
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

fn string_to_systemtime(text: String, nsec: u32) -> SystemTime {
    SystemTime::from(DateTime::<Utc>::from_utc(
        NaiveDateTime::parse_from_str(&text, "%Y-%m-%d %H:%M:%S").unwrap().with_nanosecond(nsec).unwrap(), Utc
    ))
}

fn file_type_to_const(kind: FileType) -> u32 {
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

fn const_to_file_type(kind: u32) -> FileType {
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

fn release_data(inode: u32, offset: u32, tx: &Connection) -> Result<()> {
    if offset == 0 {
        tx.execute("DELETE FROM data WHERE file_id=$1", params![inode])?;
    } else {
        let mut block = offset / BLOCK_SIZE;
        if offset % BLOCK_SIZE != 0 {
            block = offset / BLOCK_SIZE + 1;
            let sql = "SELECT data FROM data WHERE file_id=$1 and block_num = $2";
            let mut stmt = tx.prepare(sql)?;
            let mut data: Vec<u8> = match stmt.query_row(params![inode, block], |row| row.get(0)) {
                Ok(n) => n,
                Err(err) => {
                    if err == rusqlite::Error::QueryReturnedNoRows {
                        vec![0; BLOCK_SIZE as usize]
                    } else {
                        return Err(Error::from(err))
                    }
                }
            };
            data.resize((offset % BLOCK_SIZE) as usize, 0);
            tx.execute("REPLACE INTO data \
            (file_id, block_num, data)
            VALUES($1, $2, $3)",
                       params![inode, block, data])?;
        }
        tx.execute("DELETE FROM data WHERE file_id=$1 and block_num > $2", params![inode, block])?;
    }
    Ok(())
}

fn update_time(inode: u32, sql: &str, time: DateTime<Utc>, tx: &Connection) -> Result<()> {
    let mut stmt = tx.prepare(sql)?;
    let params = params![&time.format("%Y-%m-%d %H:%M:%S").to_string(), time.timestamp_subsec_nanos(), inode];
    stmt.execute(params)?;
    Ok(())
}

fn update_atime(inode: u32, time: DateTime<Utc>, tx: &Connection) -> Result<()> {
    let sql = "UPDATE metadata SET atime=datetime($1), atime_nsec=$2 WHERE id=$3";
    update_time(inode, sql, time, tx)
}

fn update_mtime(inode: u32, time: DateTime<Utc>, tx: &Connection) -> Result<()> {
    let sql = "UPDATE metadata SET mtime=datetime($1), mtime_nsec=$2 WHERE id=$3";
    update_time(inode, sql, time, tx)
}

fn update_ctime(inode: u32, time: DateTime<Utc>, tx: &Connection) -> Result<()> {
    let sql = "UPDATE metadata SET ctime=datetime($1), ctime_nsec=$2 WHERE id=$3";
    update_time(inode, sql, time, tx)
}

fn add_dentry(entry: DEntry, tx: &Connection) -> Result<()> {
    let sql = "INSERT INTO dentry VALUES($1, $2, $3, $4)";
    tx.execute(
        sql,
        params![
            entry.parent_ino,
            entry.child_ino,
            file_type_to_const(entry.file_type),
            entry.filename
            ]
    )?;
    Ok(())
}

fn inclease_nlink(inode: u32, tx: &Connection) -> Result<u32> {
    let mut nlink: u32 = tx.query_row("SELECT nlink FROM metadata WHERE id=$1", params![inode], |row| row.get(0))?;
    nlink += 1;
    tx.execute("Update metadata SET nlink=$1 where id=$2",
               params![nlink, inode])?;
    Ok(nlink)
}

fn declease_nlink(inode: u32, tx: &Connection) -> Result<u32> {
    let mut nlink: u32 = tx.query_row("SELECT nlink FROM metadata WHERE id=$1", params![inode], |row| row.get(0))?;
    if nlink != 0 {
        nlink -= 1;
    }
    tx.execute("Update metadata SET nlink=$1 where id=$2",
               params![nlink, inode])?;
    Ok(nlink)
}

fn parse_attr(mut stmt: Statement, params: &[&dyn ToSql]) -> Result<DBFileAttr> {
    let rows = stmt.query_map(params, |row| {
        Ok(DBFileAttr {
            ino: row.get(0)?,
            size: row.get(1)?,
            blocks: row.get(17).unwrap_or(0),
            atime: string_to_systemtime(row.get(2)?, row.get(3)?),
            mtime: string_to_systemtime(row.get(4)?, row.get(5)?),
            ctime: string_to_systemtime(row.get(6)?, row.get(7)?),
            crtime: string_to_systemtime(row.get(8)?, row.get(9)?),
            kind: const_to_file_type(row.get(10)?),
            perm: row.get(11)?,
            nlink: row.get(12)?,
            uid: row.get(13)?,
            gid: row.get(14)?,
            rdev: row.get(15)?,
            flags: row.get(16)?
        })
    })?;
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

fn get_inode_local(inode: u32, tx: &Connection) -> Result<DBFileAttr> {
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
    let stmt = tx.prepare(sql)?;
    let params = params![inode];
    parse_attr(stmt, params)
}

fn get_dentry_and_filetype(parent: u32, name: &str, tx: &Connection) -> Result<(u32, u32)> {
    let sql = "SELECT child_id, file_type FROM dentry WHERE  parent_id=$1 and name=$2";
    let mut stmt = tx.prepare(sql)?;
    let res: (u32, u32) = match stmt.query_row(
        params![parent, name], |row| Ok((row.get(0)?, row.get(1)?))
    ) {
        Ok(n) => n,
        Err(err) => {
            if err == rusqlite::Error::QueryReturnedNoRows {
                (0, 0)
            } else {
                return Err(Error::from(err))
            }
        }
    };
    Ok(res)
}

fn delete_dentry_local(parent: u32, name: &str, tx: &Connection) -> Result<()> {
    let sql = "DELETE FROM dentry WHERE parent_id=$1 and name=$2";
    tx.execute(sql, params![parent, name])?;
    Ok(())
}

fn check_directory_is_empty_local(inode: u32, tx: &Connection) -> Result<bool> {
    let sql = "SELECT name FROM dentry where parent_id=$1";
    let mut stmt = tx.prepare(sql)?;
    let rows = stmt.query_map(params![inode], |row| {
        Ok({
            let name: String;
            name = row.get(0)?;
            name
        })
    })?;
    for row in rows {
        let name = row?;
        if &name != "." && &name != ".." {
            return Ok(false);
        }
    }
    Ok(true)
}

pub struct Sqlite {
    conn: Connection
}

impl Sqlite {
    pub fn new(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        // enable foreign key. Sqlite ignores foreign key by default.
        conn.execute("PRAGMA foreign_keys=ON", NO_PARAMS)?;
        Ok(Sqlite { conn })
    }
}

impl DbModule for Sqlite {
    fn get_inode(&self, inode: u32) -> Result<DBFileAttr> {
        get_inode_local(inode, &self.conn)
    }

    fn add_inode(&mut self, parent: u32, name: &str, attr: &DBFileAttr) -> Result<u32> {
        let sql = "INSERT INTO metadata \
            (size,\
            atime,\
            atime_nsec,\
            mtime,\
            mtime_nsec,\
            ctime,\
            ctime_nsec,\
            crtime,\
            crtime_nsec,\
            kind, \
            mode,\
            nlink,\
            uid,\
            gid,\
            rdev,\
            flags\
            ) \
            VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)";
        let atime = DateTime::<Utc>::from(attr.atime);
        let mtime = DateTime::<Utc>::from(attr.mtime);
        let ctime = DateTime::<Utc>::from(attr.ctime);
        let crtime = DateTime::<Utc>::from(attr.crtime);
        let tx = self.conn.transaction()?;
        {
            tx.execute(sql, params![
            attr.size,
            atime.format("%Y-%m-%d %H:%M:%S").to_string(),
            atime.timestamp_subsec_nanos(),
            mtime.format("%Y-%m-%d %H:%M:%S").to_string(),
            mtime.timestamp_subsec_nanos(),
            ctime.format("%Y-%m-%d %H:%M:%S").to_string(),
            ctime.timestamp_subsec_nanos(),
            crtime.format("%Y-%m-%d %H:%M:%S").to_string(),
            crtime.timestamp_subsec_nanos(),
            file_type_to_const(attr.kind),
            attr.perm,
            0,
            attr.uid,
            attr.gid,
            attr.rdev,
            attr.flags,
        ])?;
        }
        let sql = "SELECT last_insert_rowid()";
        let child: u32;
        {
            let mut stmt = tx.prepare(sql)?;
            child = stmt.query_row(params![], |row| row.get(0))?;
        }
        let dentry = DEntry{parent_ino: parent, child_ino: child, filename: String::from(name), file_type: attr.kind};
        add_dentry(dentry, &tx)?;
        inclease_nlink(child, &tx)?;
        if attr.kind == FileType::Directory {
            let dentry = DEntry{parent_ino: child, child_ino: parent, filename: String::from(".."), file_type: attr.kind};
            add_dentry(dentry, &tx)?;
            let dentry = DEntry{parent_ino: child, child_ino: child, filename: String::from("."), file_type: attr.kind};
            add_dentry(dentry, &tx)?;
        }
        tx.commit()?;
        Ok(child)
    }

    fn update_inode(&mut self, attr: DBFileAttr, truncate: bool) -> Result<()> {
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
        let tx = self.conn.transaction()?;
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
        if truncate {
            release_data(attr.ino, attr.size, &tx)?;
        }
        tx.commit()?;
        Ok(())
    }

    fn delete_inode_if_noref(&mut self, inode: u32) -> Result<()> {
        let sql = "SELECT nlink FROM metadata WHERE id=$1";
        let tx = self.conn.transaction()?;
        let nlink: u32;
        {
            let mut stmt = tx.prepare(sql)?;
            nlink = stmt.query_row(params![inode], |row| row.get(0))?;
        }
        if nlink == 0 {
            let sql = "DELETE FROM metadata WHERE id=$1";
            tx.execute(sql, params![inode])?;
        }
        tx.commit()?;
        Ok(())
    }

    fn get_dentry(&self, inode: u32) -> Result<Vec<DEntry>> {
        let sql = "SELECT child_id, file_type, name FROM dentry WHERE parent_id=$1 ORDER BY name";
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![inode], |row| {
            Ok(DEntry{parent_ino: inode,
                child_ino: row.get(0)?,
                file_type: const_to_file_type(row.get(1)?),
                filename: row.get(2)?,
            })
        })?;
        let mut entries: Vec<DEntry> = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    fn delete_dentry(&mut self, parent: u32, name: &str) -> Result<u32> {
        let sql = "SELECT child_id FROM dentry WHERE parent_id=$1 and name=$2";
        let tx = self.conn.transaction()?;
        let child: u32;
        {
            let mut stmt = tx.prepare(sql)?;
            child = stmt.query_row(params![parent, name], |row| row.get(0))?;
        }
        delete_dentry_local(parent, name, &tx)?;
        declease_nlink(child, &tx)?;
        tx.commit()?;
        Ok(child)
    }

    fn move_dentry(&mut self, parent: u32, name: &str, new_parent: u32, new_name: &str) -> Result<u32> {
        let sql = "UPDATE dentry SET parent_id=$1, name=$2 where parent_id=$3 and name=$4";
        let tx = self.conn.transaction()?;
        let (child_id, file_type) = get_dentry_and_filetype(parent, name, &tx)?;
        if child_id == 0 {
            return Err(Error::from(ErrorKind::FsNoEnt {description: format!("parent: {} name:{}", parent, name)}));
        }
        let (exist_id, exist_file_type) = get_dentry_and_filetype(new_parent, new_name, &tx)?;
        if exist_id != 0 {
            if file_type != exist_file_type {
                if exist_file_type == DB_IFDIR {
                    return Err(Error::from(ErrorKind::FsIsDir {description: format!(
                        "parent: {} name:{}",
                        new_parent, new_name
                    )}));
                } else if exist_file_type == DB_IFREG {
                    return Err(Error::from(ErrorKind::FsIsNotDir {description: format!(
                        "parent: {} name:{}",
                        new_parent,
                        new_name
                    )}));
                } else {
                    return Err(Error::from(ErrorKind::Undefined {description: format!(
                        "parent: {} name:{} has invalid type: {:?}",
                        new_parent,
                        new_name,
                        const_to_file_type(exist_file_type)
                    )}));
                }
            }
            if exist_file_type == DB_IFDIR {
                let empty = check_directory_is_empty_local(exist_id, &tx)?;
                if !empty {
                    return Err(Error::from(ErrorKind::FsNotEmpty {description: format!(
                        "parent: {} name:{} is not empty",
                        new_parent,
                        new_name
                    )}));
                }
            }
            delete_dentry_local(new_parent, new_name, &tx)?;
            declease_nlink(exist_id, &tx)?;
        }
        tx.execute(sql, params![new_parent, new_name, parent, name])?;

        tx.commit()?;
        Ok(exist_id)
    }

    fn check_directory_is_empty(&self, inode: u32) -> Result<bool> {
        check_directory_is_empty_local(inode,&self.conn)
    }

    fn lookup(&self, parent: u32, name: &str) -> Result<DBFileAttr> {
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
        let stmt = self.conn.prepare(sql)?;
        let params = params![parent, name];
        parse_attr(stmt, params)
    }

    fn get_data(&mut self, inode:u32, block: u32, length: u32) -> Result<Vec<u8>> {
        let tx = self.conn.transaction()?;
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
                        return Err(Error::from(err))
                    }
                }
            };
        }
        update_atime(inode, Utc::now(), &tx)?;
        tx.commit()?;
        Ok(row)
    }

    fn write_data(&mut self, inode:u32, block: u32, data: &[u8], size: u32) -> Result<()> {
        let tx = self.conn.transaction()?;
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
        update_mtime(inode, time, &tx)?;
        update_ctime(inode, time, &tx)?;
        tx.commit()?;
        Ok(())
    }

    fn release_data(&self, inode: u32) -> Result<()> {
        self.conn.execute("DELETE FROM data WHERE file_id=$1", params![inode])?;
        Ok(())
    }

    fn delete_all_noref_inode(&mut self) -> Result<()> {
        self.conn.execute("DELETE FROM metadata WHERE nlink=0", params![])?;
        Ok(())
    }

    fn get_db_block_size(&self) -> u32 {
        BLOCK_SIZE
    }
}
