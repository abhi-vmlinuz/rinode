use std::ffi::CString;
use std::fs::File;
use std::io::{Error, Result};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::path::Path;

// Linux constants for statx
const AT_FDCWD: libc::c_int = -100;
const AT_SYMLINK_NOFOLLOW: libc::c_int = 0x100;
const STATX_ALL: libc::c_uint = 0x00000fff;
const STATX_MNT_ID: libc::c_uint = 0x00001000;

// ioctl FICLONE for CoW reflinks on btrfs/XFS
const FICLONE: libc::c_ulong = 0x40049409;

// renameat2 flags
#[allow(dead_code)]
pub const RENAME_NOREPLACE: libc::c_uint = 1;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StatxInfo {
    pub dev_major: u32,
    pub dev_minor: u32,
    pub mnt_id: u64,
    pub ino: u64,
    pub mode: u16,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub is_regular: bool,
}

/// Call Linux statx(2) to fetch file metadata including STATX_MNT_ID
pub fn statx_path(path: &Path) -> Result<StatxInfo> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|e| Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let mut statxbuf: libc::statx = unsafe { std::mem::zeroed() };

    let flags = AT_SYMLINK_NOFOLLOW;
    let mask = STATX_ALL | STATX_MNT_ID;

    let res = unsafe {
        libc::statx(
            AT_FDCWD,
            c_path.as_ptr(),
            flags,
            mask,
            &mut statxbuf as *mut libc::statx,
        )
    };

    if res != 0 {
        return Err(Error::last_os_error());
    }

    let mode = statxbuf.stx_mode as u16;
    let file_type = mode & (libc::S_IFMT as u16);

    Ok(StatxInfo {
        dev_major: statxbuf.stx_dev_major,
        dev_minor: statxbuf.stx_dev_minor,
        mnt_id: statxbuf.stx_mnt_id,
        ino: statxbuf.stx_ino,
        mode,
        nlink: statxbuf.stx_nlink,
        uid: statxbuf.stx_uid,
        gid: statxbuf.stx_gid,
        size: statxbuf.stx_size,
        is_dir: file_type == (libc::S_IFDIR as u16),
        is_symlink: file_type == (libc::S_IFLNK as u16),
        is_regular: file_type == (libc::S_IFREG as u16),
    })
}

/// Atomic rename using renameat2(2)
pub fn renameat2_path(old_path: &Path, new_path: &Path, flags: libc::c_uint) -> Result<()> {
    let old_c = CString::new(old_path.as_os_str().as_bytes())
        .map_err(|e| Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let new_c = CString::new(new_path.as_os_str().as_bytes())
        .map_err(|e| Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let res = unsafe {
        libc::renameat2(
            AT_FDCWD,
            old_c.as_ptr(),
            AT_FDCWD,
            new_c.as_ptr(),
            flags,
        )
    };

    if res != 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Zero-copy CoW clone via ioctl(FICLONE)
pub fn reflink_clone(src: &File, dst: &File) -> Result<()> {
    let res = unsafe {
        libc::ioctl(dst.as_raw_fd(), FICLONE, src.as_raw_fd())
    };

    if res != 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Read symlink target safely
pub fn read_symlink_target(path: &Path) -> Result<String> {
    let target = std::fs::read_link(path)?;
    Ok(target.to_string_lossy().to_string())
}

/// Recreate symlink safely
pub fn create_symlink(target: &str, link_path: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link_path)
}
