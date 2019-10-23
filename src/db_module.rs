pub mod sqlite;
use std::time::SystemTime;
use crate::sqerror::SqError;
use fuse::{FileAttr, FileType};
use time::Timespec;
use libc::{S_IFIFO, S_IFCHR, S_IFBLK, S_IFDIR, S_IFREG, S_IFLNK, S_IFSOCK};

pub trait DbModule {
    /// Create database and initialize table
    fn init_database(&self) -> Result<(),SqError>;
    /// Add file inode data
    fn add_inode(&self, attr: &DBFileAttr) -> Result<(), SqError>;
    /// Get file metadata. If not found, return ino 0
    fn get_inode(&self, inode: u32) -> Result<DBFileAttr, SqError>;
    /// Add a single directory entry
    fn add_dentry(&self, entry: &DEntry) -> Result<(), SqError>;
    /// Get directory entries
    fn get_dentry(&self, inode: u32) -> Result<Vec<DEntry>, SqError>;
    /// lookup a directory entry table and get a file attribute
    fn lookup(&self, parent: u32, name: &str) -> Result<DBFileAttr, SqError>;
    /// Add 1 to nlink
    fn increase_nlink(&self, inode: u32) -> Result<u32, SqError>;
    /// Remove 1 from nlink
    fn decrease_nlink(&self, inode: u32) -> Result<u32, SqError>;
    /// Write data.
    fn add_data(&self, inode: u32, block: u32, data: &[u8]) -> Result<(), SqError>;
    /// Read data.
    fn get_data(&self, inode: u32, block: u32, length: u32) -> Result<Vec<u8>, SqError>;
    /// Get block size of filesystem
    fn get_db_block_size(&self) -> u32;
}

// Imported from rust-fuse 4.0-dev
// This time format differs from v3.1
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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

    fn kind_from(&self, _perm: u16) -> FileType {
        let perm = _perm as u32;
        if perm & S_IFREG != 0 {
            FileType::RegularFile
        } else if perm & S_IFDIR != 0 {
            FileType::Directory
        } else if perm & S_IFLNK != 0 {
            FileType::Symlink
        } else if perm & S_IFIFO != 0{
            FileType::NamedPipe
        } else if perm & S_IFCHR != 0 {
            FileType::CharDevice
        } else if perm & S_IFBLK != 0 {
            FileType::BlockDevice
        } else {
            FileType::Socket
        }
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
            kind: self.kind_from(self.perm),
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
    pub file_type: u32
}
