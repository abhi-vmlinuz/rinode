use chrono::Utc;
use std::ffi::CString;
use std::fs::{self, Permissions};
use std::io::{Error, ErrorKind, Result};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::db::{Db, EntryRecord, NewEntry};
use crate::hasher::compute_quick_fingerprint;
use crate::syscalls::{renameat2_path, statx_path};

pub struct VaultManager {
    config: Config,
}

impl VaultManager {
    pub fn new(config: Config) -> Self {
        VaultManager { config }
    }

    /// Resolve the root of the mount/subvolume using statx STATX_MNT_ID,
    /// with device boundary fallback when the kernel does not return STATX_MNT_ID.
    pub fn resolve_mount_root(&self, path: &Path) -> Result<PathBuf> {
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };

        // Start from parent if path is a file/symlink
        let mut current = if abs_path.is_dir() {
            abs_path.clone()
        } else {
            abs_path.parent().unwrap_or(&abs_path).to_path_buf()
        };

        let current_info = statx_path(&current)?;
        let use_mnt_id = current_info.has_mnt_id;
        let target_mnt_id = current_info.mnt_id;
        let target_dev = (current_info.dev_major, current_info.dev_minor);

        // Traverse upwards while mount / device ID remains identical
        while let Some(parent) = current.parent() {
            match statx_path(parent) {
                Ok(parent_info) => {
                    let matches = if use_mnt_id && parent_info.has_mnt_id {
                        parent_info.mnt_id == target_mnt_id
                    } else {
                        (parent_info.dev_major, parent_info.dev_minor) == target_dev
                    };
                    if matches {
                        current = parent.to_path_buf();
                    } else {
                        break;
                    }
                }
                _ => break, // Reached boundary
            }
        }

        Ok(current)
    }

    /// Clean up any abandoned .tmp.* staging files older than 30 minutes in vault directory
    pub(crate) fn sweep_orphaned_tmp(vault_dir: &Path) {
        let Ok(entries) = fs::read_dir(vault_dir) else { return; };
        let now = std::time::SystemTime::now();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with(".tmp.") {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(mtime) = meta.modified() {
                        if let Ok(age) = now.duration_since(mtime) {
                            if age.as_secs() > 1800 {
                                if meta.is_dir() {
                                    let _ = fs::remove_dir_all(entry.path());
                                } else {
                                    let _ = fs::remove_file(entry.path());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Fsync directory to durably commit directory entries to disk
    pub(crate) fn fsync_dir(dir: &Path) -> Result<()> {
        if let Ok(f) = fs::File::open(dir) {
            use std::os::unix::io::AsRawFd;
            let rc = unsafe { libc::fsync(f.as_raw_fd()) };
            if rc != 0 {
                return Err(Error::last_os_error());
            }
        }
        Ok(())
    }

    fn user_fallback_storage() -> Result<PathBuf> {
        let user_storage = crate::config::base_data_dir()
            .join("rinode")
            .join("storage");
        if !user_storage.exists() {
            fs::DirBuilder::new().mode(0o700).recursive(true).create(&user_storage)?;
        } else {
            let _ = fs::set_permissions(&user_storage, Permissions::from_mode(0o700));
        }
        Ok(user_storage)
    }

    /// Get or create the secure storage directory for the given target path.
    /// Uses .rinode-storage-$UID on shared mounts to prevent cross-user permission conflicts.
    pub fn get_vault_dir(&self, target_path: &Path) -> Result<PathBuf> {
        let mount_root = self.resolve_mount_root(target_path)?;
        let uid = unsafe { libc::geteuid() };
        let candidate_storage = mount_root.join(format!(".rinode-storage-{}", uid));
        let candidate_legacy = mount_root.join(".rinode-storage");

        let candidate = if candidate_storage.exists() {
            candidate_storage
        } else if candidate_legacy.exists() {
            if let Ok(meta) = fs::symlink_metadata(&candidate_legacy) {
                if !meta.is_symlink() && meta.uid() == uid {
                    candidate_legacy
                } else {
                    candidate_storage
                }
            } else {
                candidate_storage
            }
        } else {
            candidate_storage
        };

        let vault_dir = if candidate.exists() {
            if let Ok(meta) = fs::symlink_metadata(&candidate) {
                if !meta.is_symlink() && meta.uid() == uid {
                    let _ = fs::set_permissions(&candidate, Permissions::from_mode(0o700));
                    candidate
                } else {
                    Self::user_fallback_storage()?
                }
            } else {
                Self::user_fallback_storage()?
            }
        } else {
            let is_writable = if let Ok(mount_c) = CString::new(mount_root.as_os_str().as_bytes()) {
                unsafe { libc::access(mount_c.as_ptr(), libc::W_OK) == 0 }
            } else {
                false
            };

            if is_writable {
                if fs::DirBuilder::new().mode(0o700).create(&candidate).is_ok() {
                    candidate
                } else {
                    Self::user_fallback_storage()?
                }
            } else {
                Self::user_fallback_storage()?
            }
        };

        Self::sweep_orphaned_tmp(&vault_dir);
        Ok(vault_dir)
    }

    /// Generate a collision-resistant unique filename for vault storage,
    /// truncating the stem so total filename length never exceeds NAME_MAX (255 bytes).
    fn generate_vault_filename(&self, filename: &str, inode: u64, dev: u32) -> String {
        let sanitized: String = filename
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
            .collect();
        let now_nanos = Utc::now().timestamp_nanos_opt().unwrap_or(0);
        let mut rand_bytes = [0u8; 8];
        let rand_val = if unsafe { libc::getrandom(rand_bytes.as_mut_ptr() as *mut libc::c_void, 8, libc::GRND_NONBLOCK) } == 8 {
            u64::from_ne_bytes(rand_bytes)
        } else {
            let pid = std::process::id() as u64;
            (pid.wrapping_mul(0x9e3779b97f4a7c15))
                ^ (now_nanos as u64).wrapping_mul(0xbf58476d1ce4e5b9)
        };
        let suffix = format!("__{}_{}_{}_{:016x}", inode, dev, now_nanos, rand_val);
        let max_stem_len = 255usize.saturating_sub(suffix.len());
        let truncated_stem: String = sanitized.chars().take(max_stem_len).collect();
        format!("{}{}", truncated_stem, suffix)
    }

    /// Durable cross-device file copy: copy -> fsync -> caller renames atomically.
    /// Handles FIFOs and special files safely without hanging. Chowns before chmod.
    pub(crate) fn copy_file_durable(src: &Path, tmp_dest: &Path, mode: u32) -> Result<()> {
        let meta = fs::symlink_metadata(src)?;
        if meta.file_type().is_fifo() {
            let dst_c = CString::new(tmp_dest.as_os_str().as_bytes())
                .map_err(|e| Error::new(ErrorKind::InvalidInput, e))?;
            let rc = unsafe { libc::mkfifo(dst_c.as_ptr(), mode as libc::mode_t) };
            if rc != 0 {
                return Err(Error::last_os_error());
            }
            return Ok(());
        }
        if meta.file_type().is_socket() {
            return Err(Error::new(ErrorKind::InvalidInput, "cannot copy Unix domain socket"));
        }

        fs::copy(src, tmp_dest)?;
        let f = fs::File::open(tmp_dest)?;
        use std::os::unix::io::AsRawFd;
        let fd = f.as_raw_fd();
        let rc = unsafe { libc::fsync(fd) };
        if rc != 0 {
            let _ = fs::remove_file(tmp_dest);
            return Err(Error::last_os_error());
        }

        // Restore ownership before permissions so setuid/setgid bits are not cleared
        unsafe {
            if let Ok(c) = CString::new(tmp_dest.as_os_str().as_bytes()) {
                libc::chown(c.as_ptr(), meta.uid(), meta.gid());
            }
        }
        let _ = fs::set_permissions(tmp_dest, Permissions::from_mode(mode & 0o7777));
        Ok(())
    }

    /// Recursive directory copy for EXDEV fallback (src dir -> tmp dir).
    /// Preserves directory metadata, handles FIFOs safely, and skips sockets.
    pub(crate) fn copy_dir_all(src: &Path, tmp_dest: &Path) -> Result<()> {
        fs::DirBuilder::new().mode(0o700).create(tmp_dest)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let dst = tmp_dest.join(entry.file_name());
            let ft = entry.file_type()?;
            if ft.is_dir() {
                Self::copy_dir_all(&entry.path(), &dst)?;
                if let Ok(m) = entry.metadata() {
                    unsafe {
                        if let Ok(c) = CString::new(dst.as_os_str().as_bytes()) {
                            libc::chown(c.as_ptr(), m.uid(), m.gid());
                        }
                    }
                    let _ = fs::set_permissions(&dst, Permissions::from_mode(m.mode() & 0o7777));
                }
            } else if ft.is_symlink() {
                let target = fs::read_link(entry.path())?;
                std::os::unix::fs::symlink(target, &dst)?;
            } else if ft.is_fifo() {
                let dst_c = CString::new(dst.as_os_str().as_bytes())
                    .map_err(|e| Error::new(ErrorKind::InvalidInput, e))?;
                let mode = entry.metadata().map(|m| m.mode()).unwrap_or(0o600);
                let rc = unsafe { libc::mkfifo(dst_c.as_ptr(), mode as libc::mode_t) };
                if rc != 0 {
                    return Err(Error::last_os_error());
                }
            } else if ft.is_socket() {
                continue;
            } else {
                let mode = entry.metadata().map(|m| m.mode()).unwrap_or(0o600);
                Self::copy_file_durable(&entry.path(), &dst, mode)?;
                if let Ok(m) = entry.metadata() {
                    unsafe {
                        if let Ok(c) = CString::new(dst.as_os_str().as_bytes()) {
                            libc::chown(c.as_ptr(), m.uid(), m.gid());
                        }
                    }
                }
            }
        }
        if let Ok(m) = fs::metadata(src) {
            unsafe {
                if let Ok(c) = CString::new(tmp_dest.as_os_str().as_bytes()) {
                    libc::chown(c.as_ptr(), m.uid(), m.gid());
                }
            }
            let _ = fs::set_permissions(tmp_dest, Permissions::from_mode(m.mode() & 0o7777));
        }
        Ok(())
    }

    /// Iteratively compute total recursive directory size in bytes.
    /// Resilient against permission errors (skips unreadable subtrees like `du`).
    /// Does not follow symlinks, deduplicates hard links, and stays within root mount device.
    pub fn compute_directory_size(root: &Path) -> u64 {
        use std::collections::HashSet;
        use std::os::unix::fs::MetadataExt;

        let mut total_size: u64 = 0;
        let mut stack = vec![root.to_path_buf()];
        let mut seen_inodes: HashSet<(u64, u64)> = HashSet::new();

        let root_dev = match fs::symlink_metadata(root) {
            Ok(m) => {
                total_size = total_size.saturating_add(m.len());
                seen_inodes.insert((m.dev(), m.ino()));
                m.dev()
            }
            Err(_) => return 0,
        };

        while let Some(current_dir) = stack.pop() {
            let entries = match fs::read_dir(&current_dir) {
                Ok(entries) => entries,
                Err(_) => continue, // Permission denied or unreadable directory (e.g. containers overlay)
            };

            for entry in entries.flatten() {
                let path = entry.path();
                let meta = match fs::symlink_metadata(&path) {
                    Ok(m) => m,
                    Err(_) => continue, // Cannot stat entry (broken symlink, permission denied, race)
                };

                // Do not cross into other filesystems/mounts
                if meta.dev() != root_dev {
                    continue;
                }

                // If symlink, do not follow; add length of symlink target path itself
                if meta.is_symlink() {
                    total_size = total_size.saturating_add(meta.len());
                    continue;
                }

                // Deduplicate hard links
                let key = (meta.dev(), meta.ino());
                if !seen_inodes.insert(key) {
                    continue;
                }

                total_size = total_size.saturating_add(meta.len());

                if meta.is_dir() {
                    stack.push(path);
                }
            }
        }

        total_size
    }

    fn write_sidecar_metadata(vault_path: &Path, entry: &NewEntry) -> Result<()> {
        let meta_path = PathBuf::from(format!("{}.meta", vault_path.display()));
        let content = format!(
            "original_path={}\nfilename={}\ninode={}\ndev_major={}\ndev_minor={}\nmnt_id={}\nsize={}\nmode={:o}\nuid={}\ngid={}\ndeleted_at={}\nlink_type={}\nis_directory={}\n",
            entry.original_path,
            entry.filename,
            entry.inode_no,
            entry.dev_major,
            entry.dev_minor,
            entry.mnt_id,
            entry.file_size,
            entry.mode,
            entry.uid,
            entry.gid,
            entry.deleted_at.to_rfc3339(),
            entry.link_type,
            entry.is_directory,
        );
        fs::write(meta_path, content)?;
        Ok(())
    }

    /// Preserve a file or directory into the vault
    pub fn preserve(&self, path: &Path, db: &Db, permanent: bool) -> Result<Option<EntryRecord>> {
        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };

        let info = statx_path(&abs_path)?;
        let is_excluded = self.config.is_excluded(&abs_path, &filename, info.is_dir);

        // Pre-check quota: refuse if single file alone exceeds total storage quota
        if !permanent && !is_excluded && self.config.storage.max_storage_bytes > 0 {
            let max = self.config.storage.max_storage_bytes;
            let item_size = if info.is_dir { Self::compute_directory_size(&abs_path) } else { info.size };
            if item_size > max {
                return Err(Error::other(format!(
                    "File size ({} bytes) exceeds total storage quota ({} bytes). Use -p / --permanent to delete directly or adjust max_storage_bytes.",
                    item_size, max
                )));
            }
        }

        // If permanently requested (-p / --no-storage / --permanent), unlink directly and record in history as PURGED
        if permanent {
            let symlink_target = if info.is_symlink {
                fs::read_link(&abs_path).ok().map(|p| p.to_string_lossy().to_string())
            } else {
                None
            };
            let file_size = if info.is_symlink {
                0
            } else if info.is_dir {
                Self::compute_directory_size(&abs_path)
            } else {
                info.size
            };

            if abs_path.is_dir() {
                fs::remove_dir_all(&abs_path)?;
            } else {
                fs::remove_file(&abs_path)?;
            }

            let new_entry = NewEntry {
                dev_major: info.dev_major,
                dev_minor: info.dev_minor,
                mnt_id: info.mnt_id,
                inode_no: info.ino,
                original_path: abs_path.to_string_lossy().to_string(),
                filename: filename.clone(),
                file_size,
                mode: info.mode,
                uid: info.uid,
                gid: info.gid,
                quick_fingerprint: None,
                vault_path: "".to_string(),
                deleted_at: Utc::now(),
                status: "PURGED".to_string(),
                is_directory: info.is_dir,
                symlink_target,
                link_type: if info.is_symlink { "SYMLINK".to_string() } else { "UNLINK".to_string() },
            };
            let id = db.insert_entry(&new_entry).map_err(Error::other)?;
            return db.get_by_id(id).map_err(Error::other);
        }

        // If excluded by config, delete directly and record in history as EXCLUDED
        if is_excluded {
            let symlink_target = if info.is_symlink {
                fs::read_link(&abs_path).ok().map(|p| p.to_string_lossy().to_string())
            } else {
                None
            };
            let file_size = if info.is_symlink {
                0
            } else if info.is_dir {
                Self::compute_directory_size(&abs_path)
            } else {
                info.size
            };
            let new_entry = NewEntry {
                dev_major: info.dev_major,
                dev_minor: info.dev_minor,
                mnt_id: info.mnt_id,
                inode_no: info.ino,
                original_path: abs_path.to_string_lossy().to_string(),
                filename: filename.clone(),
                file_size,
                mode: info.mode,
                uid: info.uid,
                gid: info.gid,
                quick_fingerprint: None,
                vault_path: "".to_string(),
                deleted_at: Utc::now(),
                status: "EXCLUDED".to_string(),
                is_directory: info.is_dir,
                symlink_target,
                link_type: if info.is_symlink { "SYMLINK".to_string() } else { "UNLINK".to_string() },
            };
            let _ = db.insert_entry(&new_entry);

            if abs_path.is_dir() {
                fs::remove_dir_all(&abs_path)?;
            } else {
                fs::remove_file(&abs_path)?;
            }
            return Ok(None);
        }

        // Handle Symlinks
        if info.is_symlink {
            let symlink_target = fs::read_link(&abs_path)?.to_string_lossy().to_string();
            let new_entry = NewEntry {
                dev_major: info.dev_major,
                dev_minor: info.dev_minor,
                mnt_id: info.mnt_id,
                inode_no: info.ino,
                original_path: abs_path.to_string_lossy().to_string(),
                filename: filename.clone(),
                file_size: 0,
                mode: info.mode,
                uid: info.uid,
                gid: info.gid,
                quick_fingerprint: None,
                vault_path: "".to_string(),
                deleted_at: Utc::now(),
                status: "PENDING".to_string(),
                is_directory: false,
                symlink_target: Some(symlink_target),
                link_type: "SYMLINK".to_string(),
            };

            let id = db.insert_entry(&new_entry).map_err(Error::other)?;
            fs::remove_file(&abs_path)?;
            db.confirm_entry_preserved(id, "", "SYMLINK", 0, None).map_err(Error::other)?;
            return db.get_by_id(id).map_err(Error::other);
        }

        // Handle Regular Files and Directories
        let vault_dir = self.get_vault_dir(&abs_path)?;
        let vault_filename = self.generate_vault_filename(&filename, info.ino, info.dev_minor);
        let vault_dest = vault_dir.join(&vault_filename);

        let quick_fingerprint = if info.is_regular {
            compute_quick_fingerprint(&abs_path, info.size)
        } else {
            None
        };

        let dir_size = if info.is_dir {
            Self::compute_directory_size(&abs_path)
        } else {
            0
        };
        let file_size = if info.is_dir { dir_size } else { info.size };

        let pending_entry = NewEntry {
            dev_major: info.dev_major,
            dev_minor: info.dev_minor,
            mnt_id: info.mnt_id,
            inode_no: info.ino,
            original_path: abs_path.to_string_lossy().to_string(),
            filename: filename.clone(),
            file_size,
            mode: info.mode,
            uid: info.uid,
            gid: info.gid,
            quick_fingerprint: quick_fingerprint.clone(),
            vault_path: vault_dest.to_string_lossy().to_string(),
            deleted_at: Utc::now(),
            status: "PENDING".to_string(),
            is_directory: info.is_dir,
            symlink_target: None,
            link_type: "PENDING".to_string(),
        };
        let id = db.insert_entry(&pending_entry).map_err(Error::other)?;

        // Perform atomic rename into the vault
        let link_type = match renameat2_path(&abs_path, &vault_dest, 0) {
            Ok(_) => {
                let _ = Self::fsync_dir(&vault_dir);
                "RENAME_MOVE".to_string()
            }
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                // Pre-check that source is unlinkable before copying
                let parent = abs_path.parent().unwrap_or(Path::new("/"));
                let is_unlinkable = if let Ok(parent_c) = CString::new(parent.as_os_str().as_bytes()) {
                    unsafe { libc::access(parent_c.as_ptr(), libc::W_OK) == 0 }
                } else {
                    false
                };
                if !is_unlinkable {
                    let _ = db.purge_entry(&EntryRecord {
                        id,
                        dev_major: info.dev_major,
                        dev_minor: info.dev_minor,
                        mnt_id: info.mnt_id,
                        inode_no: info.ino,
                        original_path: abs_path.to_string_lossy().to_string(),
                        filename: filename.clone(),
                        file_size,
                        mode: info.mode,
                        uid: info.uid,
                        gid: info.gid,
                        quick_fingerprint: quick_fingerprint.clone(),
                        vault_path: "".to_string(),
                        deleted_at: Utc::now(),
                        status: "PENDING".to_string(),
                        is_directory: info.is_dir,
                        symlink_target: None,
                        link_type: "PENDING".to_string(),
                        restored_at: None,
                        purged_at: None,
                    });
                    return Err(Error::new(
                        ErrorKind::PermissionDenied,
                        format!("Cannot remove '{}': parent directory is write-protected", abs_path.display()),
                    ));
                }

                let tmp_dest = vault_dir.join(format!(
                    ".tmp.{}.{}",
                    vault_filename,
                    std::process::id()
                ));
                let copy_res = if info.is_dir {
                    Self::copy_dir_all(&abs_path, &tmp_dest)
                } else {
                    Self::copy_file_durable(&abs_path, &tmp_dest, info.mode as u32)
                };
                if let Err(ce) = copy_res {
                    let _ = if tmp_dest.is_dir() {
                        fs::remove_dir_all(&tmp_dest)
                    } else {
                        fs::remove_file(&tmp_dest)
                    };
                    return Err(ce);
                }

                if let Err(re) = fs::rename(&tmp_dest, &vault_dest) {
                    let _ = if tmp_dest.is_dir() {
                        fs::remove_dir_all(&tmp_dest)
                    } else {
                        fs::remove_file(&tmp_dest)
                    };
                    return Err(re);
                }

                let _ = Self::fsync_dir(&vault_dir);

                // Unlink source after durable copy is published
                if info.is_dir {
                    fs::remove_dir_all(&abs_path)?;
                } else {
                    fs::remove_file(&abs_path)?;
                }
                "COPY".to_string()
            }
            Err(e) => return Err(e),
        };

        // Write sidecar metadata file
        let mut final_entry = pending_entry;
        final_entry.link_type = link_type.clone();
        let _ = Self::write_sidecar_metadata(&vault_dest, &final_entry);

        // Confirm in DB
        db.confirm_entry_preserved(
            id,
            &vault_dest.to_string_lossy(),
            &link_type,
            file_size,
            quick_fingerprint.as_deref(),
        ).map_err(Error::other)?;

        db.get_by_id(id).map_err(Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_compute_directory_size_nested() {
        let temp_dir = std::env::temp_dir().join(format!("rinode_size_test_{}", std::process::id()));
        fs::create_dir_all(temp_dir.join("sub1/sub2")).unwrap();

        let mut f1 = File::create(temp_dir.join("sub1/file1.bin")).unwrap();
        f1.write_all(&[0u8; 1024]).unwrap();

        let mut f2 = File::create(temp_dir.join("sub1/sub2/file2.bin")).unwrap();
        f2.write_all(&[0u8; 2048]).unwrap();

        let size = VaultManager::compute_directory_size(&temp_dir);
        // Size must include at least 1024 + 2048 = 3072 bytes (plus directory inodes)
        assert!(size >= 3072, "Computed size {} was less than file content 3072", size);

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_compute_directory_size_unreadable_subfolder() {
        let temp_dir = std::env::temp_dir().join(format!("rinode_unreadable_test_{}", std::process::id()));
        let sub = temp_dir.join("restricted");
        fs::create_dir_all(&sub).unwrap();

        let mut f1 = File::create(temp_dir.join("visible.bin")).unwrap();
        f1.write_all(&[0u8; 1000]).unwrap();

        let mut f2 = File::create(sub.join("secret.bin")).unwrap();
        f2.write_all(&[0u8; 2000]).unwrap();

        // Make restricted subfolder unreadable (000)
        fs::set_permissions(&sub, Permissions::from_mode(0o000)).unwrap();

        // Must not crash or error; returns at least the visible file size
        let size = VaultManager::compute_directory_size(&temp_dir);
        assert!(size >= 1000, "Computed size {} should include accessible files", size);

        // Restore permissions for cleanup
        let _ = fs::set_permissions(&sub, Permissions::from_mode(0o755));
        fs::remove_dir_all(&temp_dir).unwrap();
    }
}
