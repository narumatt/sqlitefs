pub mod sqlite;
use std::time::SystemTime;
use crate::sqerror::SqError;

pub trait DbModule {
    /// Create database and initialize table
    fn init_database(&self) -> Result<(),SqError>;
    /// Add file inode data
    fn add_inode(&self, attr: &DBFileAttr) -> Result<(), SqError>;
    /// Add a single directry entry
    fn add_dentry(&self, entry: &DEntry) -> Result<(), SqError>;
    /// Add 1 to nlink
    fn increase_nlink(&self, inode: u32) -> Result<u32, SqError>;
    /// Remove 1 from nlink
    fn decrease_nlink(&self, inode: u32) -> Result<u32, SqError>;
    fn execute(&self);
}

// Imported from rust-fuse 4.0-dev
// This time format differs from v3.1
pub struct DBFileAttr {
    /// Inode number
    pub ino: u32,
    /// Size in bytes
    pub size: u32,
    /// Time of last access
    pub atime: SystemTime,
    /// Time of last modification
    pub mtime: SystemTime,
    /// Time of last change
    pub ctime: SystemTime,
    /// Time of creation (macOS only)
    pub crtime: SystemTime,
    /// Permissions
    pub perm: u32,
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

pub struct DEntry {
    pub parent_ino: u32,
    pub child_ino: u32,
    pub filename: String,
    pub file_type: u32
}
