pub mod sqlite;
use std::time::SystemTime;
use crate::sqerror::Result;
use fuse::{FileAttr, FileType};
use time::Timespec;
use chrono::{DateTime, Utc, NaiveDateTime};

pub trait DbModule {
    /// Get metadata. If not found, return None
    fn get_inode(&self, inode: u32) -> Result<Option<DBFileAttr>>;
    /// Add a file or a directory.
    /// Update atime, mtime, ctime. Update mtime and ctime of the parent directory.
    fn add_inode(&mut self, parent: u32, name: &str, attr: &DBFileAttr) -> Result<u32>;
    /// Update file metadata.
    /// Update ctime. Update mtime if filesize is changed.
    fn update_inode(&mut self, attr: DBFileAttr, truncate: bool) -> Result<()>;
    // Delete an inode if the link count is zero.
    fn delete_inode_if_noref(&mut self, inode: u32) -> Result<()>;
    /// Get directory entries
    fn get_dentry(&self, inode: u32) -> Result<Vec<DEntry>>;
    /// Add a new directory entry which is hard link
    /// Update mtime, Update mtime and ctime of the parent directory.
    fn link_dentry(&mut self, inode: u32, parent: u32, name: &str) -> Result<DBFileAttr>;
    /// Delete a dentry. returns target inode.
    /// Update ctime. Update mtime and ctime of the parent directory.
    fn delete_dentry(&mut self, parent: u32, name: &str) -> Result<u32>;
    /// Move dentry to another parent or name. Return inode number if a new file is overwrote.
    /// Update ctime, and mtime and ctime of the parent directories.
    fn move_dentry(&mut self, parent: u32, name: &str, new_parent: u32, new_name: &str) -> Result<Option<u32>>;
    /// check a directory if it is empty.
    fn check_directory_is_empty(&self, inode: u32) -> Result<bool>;
    /// lookup a directory entry table and get a file attribute.
    /// If not found, return None.
    /// Update atime.
    fn lookup(&mut self, parent: u32, name: &str) -> Result<Option<DBFileAttr>>;
    /// Read data from a whole block.
    /// Update atime.
    fn get_data(&mut self, inode: u32, block: u32, length: u32) -> Result<Vec<u8>>;
    /// Write data into a whole block.
    /// Update mtime and ctime.
    fn write_data(&mut self, inode:u32, block: u32, data: &[u8], size: u32) -> Result<()>;
    /// Release all data related to an inode number.
    fn release_data(&self, inode: u32) -> Result<()>;
    /// Delete all inodes which nlink is 0.
    fn delete_all_noref_inode(&mut self) -> Result<()>;
    /// Get block size of the filesystem
    fn get_db_block_size(&self) -> u32;
}

// Imported from rust-fuse 4.0-dev
// This time format differs from v3.1
#[derive(Clone, Copy, Debug, Hash, PartialEq)]
pub struct DBFileAttr {
    /// Inode number
    pub ino: u32,
    /// Size in bytes
    pub size: u32,
    /// block size
    pub blocks: u32,
    /// Time of last access
    pub atime: SystemTime,
    /// Time of last modification
    pub mtime: SystemTime,
    /// Time of last change
    pub ctime: SystemTime,
    /// Time of creation (macOS only)
    pub crtime: SystemTime,
    /// file type
    pub kind: FileType,
    /// Permissions
    pub perm: u16,
    /// Number of hard links
    pub nlink: u32,
    /// User id
    pub uid: u32,
    /// Group id
    pub gid: u32,
    /// Rdev
    pub rdev: u32,
    /// Flags (macOS only, see chflags(2))
    pub flags: u32,
}

impl DBFileAttr {
    fn timespec_from(&self, st: &SystemTime) -> Timespec {
        if let Ok(dur_since_epoch) = st.duration_since(std::time::UNIX_EPOCH) {
            Timespec::new(dur_since_epoch.as_secs() as i64,
                          dur_since_epoch.subsec_nanos() as i32)
        } else {
            Timespec::new(0, 0)
        }
    }

    pub fn datetime_from(&self, ts: &Timespec) -> SystemTime {
        let dt = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(ts.sec, ts.nsec as u32), Utc);
        SystemTime::from(dt)
    }

    pub fn get_file_attr(&self) -> FileAttr {
        FileAttr {
            ino: self.ino as u64,
            size: self.size as u64,
            blocks: self.blocks as u64,
            atime: self.timespec_from(&self.atime),
            mtime: self.timespec_from(&self.mtime),
            ctime: self.timespec_from(&self.ctime),
            crtime: self.timespec_from(&self.crtime),
            kind: self.kind,
            perm: self.perm,
            nlink: self.nlink,
            uid: self.uid,
            gid: self.gid,
            rdev: self.rdev,
            flags: self.flags,
        }
    }
}

pub struct DEntry {
    pub parent_ino: u32,
    pub child_ino: u32,
    pub filename: String,
    pub file_type: FileType,
}
