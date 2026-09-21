use chrono::Utc;
use std::fs::{self, Permissions};
use std::io::{Error, Result};
use std::os::unix::fs::PermissionsExt;
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

    /// Resolve the root of the mount/subvolume using statx STATX_MNT_ID
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
        let target_mnt_id = current_info.mnt_id;

        // Traverse upwards while mount ID remains identical
        while let Some(parent) = current.parent() {
            match statx_path(parent) {
                Ok(parent_info) if parent_info.mnt_id == target_mnt_id => {
                    current = parent.to_path_buf();
                }
                _ => break, // Reached boundary of subvolume / mount point
            }
        }

        Ok(current)
    }

    /// Get or create the secure storage directory for the given target path
    pub fn get_vault_dir(&self, target_path: &Path) -> Result<PathBuf> {
        let mount_root = self.resolve_mount_root(target_path)?;
        let candidate_storage = mount_root.join(".rinode-storage");
        let candidate_legacy = mount_root.join(".rinode-vault");

        let candidate = if candidate_legacy.exists() && !candidate_storage.exists() {
            candidate_legacy
        } else {
            candidate_storage
        };

        // Check if we can create/write to this mount's storage directory
        let vault_dir = if fs::create_dir_all(&candidate).is_ok()
            && fs::metadata(&candidate)
                .map(|m| !m.permissions().readonly())
                .unwrap_or(false)
        {
            candidate
        } else {
            // Fallback to user home data dir if mount root is not writable (e.g. non-root on /)
            let user_storage = crate::config::base_data_dir()
                .join("rinode")
                .join("storage");
            fs::create_dir_all(&user_storage)?;
            user_storage
        };

        // Ensure 0700 permissions (rwx------) for privacy
        fs::set_permissions(&vault_dir, Permissions::from_mode(0o700))?;
        Ok(vault_dir)
    }

    /// Generate an collision-resistant unique filename for vault storage
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
            // Fallback: mix pid, nanos, and address entropy (still 64-bit space)
            let pid = std::process::id() as u64;
            (pid.wrapping_mul(0x9e3779b97f4a7c15))
                ^ (now_nanos as u64).wrapping_mul(0xbf58476d1ce4e5b9)
        };
        format!("{}__{}_{}_{}_{:016x}", sanitized, inode, dev, now_nanos, rand_val)
    }

    /// Durable cross-device file copy: copy -> fsync -> caller renames atomically.
    /// Writes to `tmp_dest` only; never leaves a half-written `vault_dest` behind.
    fn copy_file_durable(src: &Path, tmp_dest: &Path, mode: u32) -> Result<()> {
        fs::copy(src, tmp_dest)?;
        let f = fs::File::open(tmp_dest)?;
        // Preserve original permission bits on the copy
        let _ = fs::set_permissions(tmp_dest, Permissions::from_mode(mode & 0o7777));
        use std::os::unix::io::AsRawFd;
        let fd = f.as_raw_fd();
        let rc = unsafe { libc::fsync(fd) };
        if rc != 0 {
            let _ = fs::remove_file(tmp_dest);
            return Err(Error::last_os_error());
        }
        Ok(())
    }

    /// Recursive directory copy for EXDEV fallback (src dir -> tmp dir).
    fn copy_dir_all(src: &Path, tmp_dest: &Path) -> Result<()> {
        fs::create_dir_all(tmp_dest)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let dst = tmp_dest.join(entry.file_name());
            let ft = entry.file_type()?;
            if ft.is_dir() {
                Self::copy_dir_all_static(&entry.path(), &dst)?;
            } else if ft.is_symlink() {
                let target = fs::read_link(entry.path())?;
                std::os::unix::fs::symlink(target, &dst)?;
            } else {
                fs::copy(entry.path(), &dst)?;
            }
        }
        Ok(())
    }

    fn copy_dir_all_static(src: &Path, tmp_dest: &Path) -> Result<()> {
        Self::copy_dir_all(src, tmp_dest)
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

        let is_excluded = self.config.is_excluded(&abs_path, &filename);

        // If permanently requested (-p / --no-storage / --permanent), unlink directly and record in history as PURGED
        if permanent {
            let info = statx_path(&abs_path)?;
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
            if let Ok(info) = statx_path(&abs_path) {
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
            }

            if abs_path.is_dir() {
                fs::remove_dir_all(&abs_path)?;
            } else {
                fs::remove_file(&abs_path)?;
            }
            return Ok(None);
        }

        // Stat the target
        let info = statx_path(&abs_path)?;

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
                status: "PRESERVED".to_string(),
                is_directory: false,
                symlink_target: Some(symlink_target),
                link_type: "SYMLINK".to_string(),
            };

            // Remove symlink
            fs::remove_file(&abs_path)?;
            let id = db.insert_entry(&new_entry).map_err(Error::other)?;
            return db.get_by_id(id).map_err(Error::other);
        }

        // Handle Regular Files and Directories
        let vault_dir = self.get_vault_dir(&abs_path)?;
        let vault_filename = self.generate_vault_filename(&filename, info.ino, info.dev_minor);
        let vault_dest = vault_dir.join(&vault_filename);

        // Compute instant O(1) fingerprint before move if regular file
        let quick_fingerprint = if info.is_regular {
            compute_quick_fingerprint(&abs_path, info.size)
        } else {
            None
        };

        // Compute accurate directory size before move if directory
        let dir_size = if info.is_dir {
            Self::compute_directory_size(&abs_path)
        } else {
            0
        };

        // Perform atomic rename into the vault
        let link_type = match renameat2_path(&abs_path, &vault_dest, 0) {
            Ok(_) => "RENAME_MOVE".to_string(),
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                // Cross-device fallback (finding 3): never leave half-written
                // storage behind. Copy to tmp -> fsync -> atomic rename -> unlink src.
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
                // Atomically publish temp -> final destination
                if let Err(re) = fs::rename(&tmp_dest, &vault_dest) {
                    let _ = if tmp_dest.is_dir() {
                        fs::remove_dir_all(&tmp_dest)
                    } else {
                        fs::remove_file(&tmp_dest)
                    };
                    return Err(re);
                }
                // Only unlink source after durable copy is on disk.
                // If unlink fails, vault copy is complete; surface error as-is.
                if info.is_dir {
                    fs::remove_dir_all(&abs_path)?;
                } else {
                    fs::remove_file(&abs_path)?;
                }
                "COPY".to_string()
            }
            Err(e) => return Err(e),
        };

        let new_entry = NewEntry {
            dev_major: info.dev_major,
            dev_minor: info.dev_minor,
            mnt_id: info.mnt_id,
            inode_no: info.ino,
            original_path: abs_path.to_string_lossy().to_string(),
            filename,
            file_size: if info.is_dir { dir_size } else { info.size },
            mode: info.mode,
            uid: info.uid,
            gid: info.gid,
            quick_fingerprint,
            vault_path: vault_dest.to_string_lossy().to_string(),
            deleted_at: Utc::now(),
            status: "PRESERVED".to_string(),
            is_directory: info.is_dir,
            symlink_target: None,
            link_type,
        };

        let id = db.insert_entry(&new_entry).map_err(Error::other)?;
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
