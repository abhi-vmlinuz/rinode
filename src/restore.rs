use std::fs::{self, File};
use std::io::{Error, ErrorKind, Result};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::db::{Db, EntryRecord};
use crate::syscalls::{reflink_clone, renameat2_path, RENAME_NOREPLACE};
use crate::vault::VaultManager;

/// Restore an entry by its ID or by searching for its latest name
pub fn restore_by_id_or_name(
    db: &Db,
    identifier: &str,
    keep_vault: bool,
    force: bool,
) -> Result<EntryRecord> {
    let entry = if let Ok(id) = identifier.parse::<i64>() {
        db.get_by_id(id)
            .map_err(Error::other)?
            .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Entry with ID {} not found", id)))?
    } else {
        let matches = db
            .find_by_filename(identifier)
            .map_err(Error::other)?;

        if matches.is_empty() {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("No deleted entries found matching '{}'", identifier),
            ));
        }
        matches[0].clone() // Most recent match
    };

    restore_entry(db, &entry, keep_vault, force)?;
    Ok(entry)
}

fn apply_original_metadata(path: &Path, entry: &EntryRecord) {
    // Restore ownership first; ignore failures for non-root callers (EPERM)
    // Chown before chmod so setuid/setgid bits are not cleared by kernel
    unsafe {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        if let Ok(c) = CString::new(path.as_os_str().as_bytes()) {
            libc::chown(c.as_ptr(), entry.uid, entry.gid);
        }
    }
    // Restore exact permission bits
    let _ = fs::set_permissions(
        path,
        fs::Permissions::from_mode((entry.mode as u32) & 0o7777),
    );
    // Flush content to disk before reporting success (regular files only)
    if !entry.is_directory {
        if let Ok(f) = File::open(path) {
            use std::os::unix::io::AsRawFd;
            unsafe {
                libc::fsync(f.as_raw_fd());
            }
        }
    }
}

/// Perform the restoration of an entry
pub fn restore_entry(db: &Db, entry: &EntryRecord, keep_vault: bool, force: bool) -> Result<()> {
    if entry.status == "EXCLUDED" {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("Cannot restore '{}': file was excluded by config and permanently unlinked without vaulting", entry.filename),
        ));
    }

    if entry.status == "PURGED" {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("Cannot restore '{}': file was permanently unlinked and not stored in storage", entry.filename),
        ));
    }

    // 1. Resolve and validate storage source on disk BEFORE touching destination
    let storage_path = if entry.link_type != "SYMLINK" {
        let p = crate::db::resolve_storage_path(&entry.vault_path).ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                format!("Storage file '{}' is missing on disk", entry.vault_path),
            )
        })?;
        if !p.exists() {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("Storage file '{}' is missing on disk", entry.vault_path),
            ));
        }
        Some(p)
    } else {
        None
    };

    let orig_path = PathBuf::from(&entry.original_path);

    // 2. Recreate parent directories if missing (mkdir -p)
    if let Some(parent) = orig_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // 3. Collision handling: inspect destination safely
    let dst_exists = orig_path.exists() || orig_path.is_symlink();
    if dst_exists {
        if !force {
            return Err(Error::new(
                ErrorKind::AlreadyExists,
                format!(
                    "Destination '{}' already exists. Use --force to overwrite.",
                    orig_path.display()
                ),
            ));
        }

        // Under --force:
        let dst_is_dir = orig_path.is_dir() && !orig_path.is_symlink();
        if dst_is_dir {
            if !entry.is_directory {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "Cannot overwrite existing directory '{}' with file '{}'",
                        orig_path.display(),
                        entry.filename
                    ),
                ));
            }
            fs::remove_dir_all(&orig_path)?;
        } else {
            let _ = fs::remove_file(&orig_path);
        }
    }

    // 4. Handle Symlink restoration
    if entry.link_type == "SYMLINK" {
        let target = entry.symlink_target.as_deref().unwrap_or("");
        match std::os::unix::fs::symlink(target, &orig_path) {
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!(
                        "Destination '{}' already exists. Use --force to overwrite.",
                        orig_path.display()
                    ),
                ));
            }
            Err(e) => return Err(e),
        }
        db.mark_restored(entry.id)
            .map_err(Error::other)?;
        return Ok(());
    }

    // 5. Handle Regular Files and Directories
    let storage_path = storage_path.unwrap();

    if keep_vault {
        if entry.is_directory {
            VaultManager::copy_dir_all(&storage_path, &orig_path)?;
            apply_original_metadata(&orig_path, entry);
        } else {
            // Snapshot fork mode: Try CoW reflink first, fallback to durable atomic copy
            let parent = orig_path.parent().unwrap_or_else(|| Path::new("."));
            let tmp_dest = parent.join(format!(
                ".tmp.restore_fork.{}.{}",
                entry.filename,
                std::process::id()
            ));

            let copy_res = (|| -> Result<()> {
                let src_file = File::open(&storage_path)?;
                let dst_file = File::create(&tmp_dest)?;
                if reflink_clone(&src_file, &dst_file).is_err() {
                    drop(dst_file);
                    VaultManager::copy_file_durable(&storage_path, &tmp_dest, entry.mode as u32)?;
                }
                fs::rename(&tmp_dest, &orig_path)?;
                Ok(())
            })();

            if let Err(e) = copy_res {
                let _ = fs::remove_file(&tmp_dest);
                return Err(e);
            }
            apply_original_metadata(&orig_path, entry);
        }
    } else {
        // Default Consume mode: atomic rename back to original path.
        // force=false -> RENAME_NOREPLACE (kernel-enforced, no TOCTOU).
        let flags = if force { 0 } else { RENAME_NOREPLACE };
        match renameat2_path(&storage_path, &orig_path, flags) {
            Ok(_) => {}
            Err(e) if e.raw_os_error() == Some(libc::EEXIST) => {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!(
                        "Destination '{}' already exists. Use --force to overwrite.",
                        orig_path.display()
                    ),
                ));
            }
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                // Cross-device EXDEV fallback (copy to tmp in target parent, fsync, atomic rename, unlink vault copy)
                let parent = orig_path.parent().unwrap_or_else(|| Path::new("."));
                let tmp_dest = parent.join(format!(
                    ".tmp.restore.{}.{}",
                    entry.filename,
                    std::process::id()
                ));

                if entry.is_directory {
                    VaultManager::copy_dir_all(&storage_path, &tmp_dest)?;
                    fs::rename(&tmp_dest, &orig_path)?;
                    apply_original_metadata(&orig_path, entry);
                    let _ = fs::remove_dir_all(&storage_path);
                } else {
                    VaultManager::copy_file_durable(&storage_path, &tmp_dest, entry.mode as u32)?;
                    fs::rename(&tmp_dest, &orig_path)?;
                    apply_original_metadata(&orig_path, entry);
                    let _ = fs::remove_file(&storage_path);
                }
                let meta_path = PathBuf::from(format!("{}.meta", storage_path.display()));
                let _ = fs::remove_file(meta_path);
            }
            Err(e) => return Err(e),
        }
    }

    // 6. Update database status to RESTORED
    db.mark_restored(entry.id)
        .map_err(Error::other)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_record(status: &str, is_dir: bool, orig_path: &Path, vault_path: &Path) -> EntryRecord {
        EntryRecord {
            id: 1,
            dev_major: 1,
            dev_minor: 1,
            mnt_id: 1,
            inode_no: 100,
            original_path: orig_path.to_string_lossy().to_string(),
            filename: orig_path.file_name().unwrap().to_string_lossy().to_string(),
            file_size: 42,
            mode: 0o644,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            quick_fingerprint: None,
            vault_path: vault_path.to_string_lossy().to_string(),
            deleted_at: Utc::now(),
            status: status.to_string(),
            is_directory: is_dir,
            symlink_target: None,
            link_type: "RENAME".to_string(),
            restored_at: None,
            purged_at: None,
        }
    }

    #[test]
    fn test_restore_refuses_excluded_or_purged() {
        let temp = std::env::temp_dir().join(format!("rinode_test_rst_refuse_{}", std::process::id()));
        fs::create_dir_all(&temp).unwrap();
        let db = Db::open(&temp.join("test.db")).unwrap();

        let excluded = sample_record("EXCLUDED", false, &temp.join("a.txt"), &temp.join("vault_a.txt"));
        assert!(restore_entry(&db, &excluded, false, false).is_err());

        let purged = sample_record("PURGED", false, &temp.join("b.txt"), &temp.join("vault_b.txt"));
        assert!(restore_entry(&db, &purged, false, false).is_err());

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn test_restore_missing_source_fails_without_modifying_destination() {
        let temp = std::env::temp_dir().join(format!("rinode_test_rst_missing_{}", std::process::id()));
        fs::create_dir_all(&temp).unwrap();
        let db = Db::open(&temp.join("test.db")).unwrap();

        let dst = temp.join("target.txt");
        fs::write(&dst, "important content").unwrap();

        let missing_vault = temp.join("nonexistent_vault.txt");
        let rec = sample_record("PRESERVED", false, &dst, &missing_vault);

        let res = restore_entry(&db, &rec, false, true);
        assert!(res.is_err());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "important content");

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn test_restore_force_refuses_wiping_directory_with_file() {
        let temp = std::env::temp_dir().join(format!("rinode_test_rst_dirguard_{}", std::process::id()));
        fs::create_dir_all(&temp).unwrap();
        let db = Db::open(&temp.join("test.db")).unwrap();

        let dst_dir = temp.join("dir_target");
        fs::create_dir_all(dst_dir.join("sub")).unwrap();
        fs::write(dst_dir.join("sub/keep.txt"), "keep").unwrap();

        let vault_file = temp.join("vault_file.txt");
        fs::write(&vault_file, "file payload").unwrap();

        let rec = sample_record("PRESERVED", false, &dst_dir, &vault_file);

        let res = restore_entry(&db, &rec, false, true);
        assert!(res.is_err(), "Must refuse overwriting directory with file");
        assert!(dst_dir.join("sub/keep.txt").exists());

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn test_restore_keep_copy_file() {
        let temp = std::env::temp_dir().join(format!("rinode_test_rst_keepcopy_{}", std::process::id()));
        fs::create_dir_all(&temp).unwrap();
        let db = Db::open(&temp.join("test.db")).unwrap();

        let dst = temp.join("restored.txt");
        let vault_file = temp.join("vault_copy.txt");
        fs::write(&vault_file, "restored data").unwrap();

        let rec = sample_record("PRESERVED", false, &dst, &vault_file);
        let res = restore_entry(&db, &rec, true, false);
        assert!(res.is_ok());

        assert_eq!(fs::read_to_string(&dst).unwrap(), "restored data");
        assert!(vault_file.exists(), "Vault file must remain on keep-copy");

        fs::remove_dir_all(&temp).ok();
    }
}
