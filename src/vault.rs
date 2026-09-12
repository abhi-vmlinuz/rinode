use chrono::Utc;
use std::fs::{self, Permissions};
use std::io::{Error, ErrorKind, Result};
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

    /// Get or create the secure vault directory for the given target path
    pub fn get_vault_dir(&self, target_path: &Path) -> Result<PathBuf> {
        let mount_root = self.resolve_mount_root(target_path)?;
        let candidate_vault = mount_root.join(".rinode-vault");

        // Check if we can create/write to this mount's .rinode-vault
        let vault_dir = if fs::create_dir_all(&candidate_vault).is_ok()
            && fs::metadata(&candidate_vault)
                .map(|m| !m.permissions().readonly())
                .unwrap_or(false)
        {
            candidate_vault
        } else {
            // Fallback to user home data dir if mount root is not writable (e.g. non-root on /)
            let user_vault = directories::BaseDirs::new()
                .map(|b| b.data_local_dir().join("recent-inode").join("vault"))
                .unwrap_or_else(|| PathBuf::from("/tmp/.rinode-vault"));
            fs::create_dir_all(&user_vault)?;
            user_vault
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
        let mut rand_bytes = [0u8; 4];
        let rand_val = if unsafe { libc::getrandom(rand_bytes.as_mut_ptr() as *mut libc::c_void, 4, libc::GRND_NONBLOCK) } == 4 {
            u32::from_ne_bytes(rand_bytes)
        } else {
            std::process::id() ^ (now_nanos as u32)
        };
        format!("{}__{}_{}_{}_{:06x}", sanitized, inode, dev, now_nanos, rand_val & 0xffffff)
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

        // If permanently requested or excluded by config, delete directly without vaulting
        if permanent || is_excluded {
            if is_excluded && !permanent {
                if let Ok(info) = statx_path(&abs_path) {
                    let symlink_target = if info.is_symlink {
                        fs::read_link(&abs_path).ok().map(|p| p.to_string_lossy().to_string())
                    } else {
                        None
                    };
                    let new_entry = NewEntry {
                        dev_major: info.dev_major,
                        dev_minor: info.dev_minor,
                        mnt_id: info.mnt_id,
                        inode_no: info.ino,
                        original_path: abs_path.to_string_lossy().to_string(),
                        filename: filename.clone(),
                        file_size: if info.is_symlink { 0 } else { info.size },
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
            let id = db.insert_entry(&new_entry).map_err(|e| Error::new(ErrorKind::Other, e))?;
            return Ok(db.get_by_id(id).map_err(|e| Error::new(ErrorKind::Other, e))?);
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

        // Perform atomic rename into the vault
        let link_type = match renameat2_path(&abs_path, &vault_dest, 0) {
            Ok(_) => "RENAME_MOVE".to_string(),
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                // Cross-device fallback
                if info.is_dir {
                    return Err(Error::new(
                        ErrorKind::Other,
                        "Cannot move directory across mount boundaries",
                    ));
                }
                fs::copy(&abs_path, &vault_dest)?;
                fs::remove_file(&abs_path)?;
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
            file_size: info.size,
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

        let id = db.insert_entry(&new_entry).map_err(|e| Error::new(ErrorKind::Other, e))?;
        Ok(db.get_by_id(id).map_err(|e| Error::new(ErrorKind::Other, e))?)
    }
}
