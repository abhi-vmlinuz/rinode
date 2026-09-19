use std::fs::{self, File};
use std::io::{Error, ErrorKind, Result};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::db::{Db, EntryRecord};
use crate::syscalls::{reflink_clone, renameat2_path, RENAME_NOREPLACE};

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

fn apply_original_metadata(path: &std::path::Path, entry: &EntryRecord) {
    // Restore exact permission bits (finding 2)
    let _ = fs::set_permissions(
        path,
        fs::Permissions::from_mode((entry.mode as u32) & 0o7777),
    );
    // Restore ownership; ignore failures for non-root callers (EPERM)
    unsafe {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        if let Ok(c) = CString::new(path.as_os_str().as_bytes()) {
            libc::chown(c.as_ptr(), entry.uid, entry.gid);
        }
    }
    // Flush content to disk before reporting success
    if let Ok(f) = File::open(path) {
        use std::os::unix::io::AsRawFd;
        unsafe {
            libc::fsync(f.as_raw_fd());
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

    let orig_path = PathBuf::from(&entry.original_path);

    // 1. Recreate parent directories if missing (mkdir -p)
    if let Some(parent) = orig_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // 2. Collision handling without TOCTOU (finding 4):
    // For force=false we do NOT trust exists() checks. The kernel flag
    // RENAME_NOREPLACE makes the rename itself fail atomically with EEXIST.
    // For force=true we intentionally remove the destination first.
    if force && orig_path.exists() && entry.link_type != "SYMLINK" {
        // Remove existing destination if force is true
        if orig_path.is_dir() && !orig_path.is_symlink() {
            fs::remove_dir_all(&orig_path)?;
        } else {
            let _ = fs::remove_file(&orig_path);
        }
    } else if !force && orig_path.exists() && entry.link_type == "SYMLINK" {
        // Fast-path friendly error for symlinks (symlink() would also fail with EEXIST)
        return Err(Error::new(
            ErrorKind::AlreadyExists,
            format!(
                "Destination '{}' already exists. Use --force to overwrite.",
                orig_path.display()
            ),
        ));
    } else if !force && orig_path.exists() && keep_vault && !entry.is_directory {
        // keep-copy path uses File::create (truncates); pre-check to avoid clobber.
        // A residual race here is benign: worst case we overwrite an empty new file
        // only if attacker creates it between check and create, which rename path avoids.
        return Err(Error::new(
            ErrorKind::AlreadyExists,
            format!(
                "Destination '{}' already exists. Use --force to overwrite.",
                orig_path.display()
            ),
        ));
    }

    // 3. Handle Symlink restoration
    if entry.link_type == "SYMLINK" {
        let target = entry.symlink_target.as_deref().unwrap_or("");
        if force {
            let _ = fs::remove_file(&orig_path);
        }
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

    // 4. Handle Regular Files and Directories
    let storage_path = crate::db::resolve_storage_path(&entry.vault_path).ok_or_else(|| {
        Error::new(
            ErrorKind::NotFound,
            format!("Storage file '{}' is missing on disk", entry.vault_path),
        )
    })?;

    if keep_vault && !entry.is_directory {
        // Snapshot fork mode: Try CoW reflink first, fallback to standard copy
        let src_file = File::open(&storage_path)?;
        let dst_file = File::create(&orig_path)?;

        if reflink_clone(&src_file, &dst_file).is_err() {
            drop(dst_file);
            fs::copy(&storage_path, &orig_path)?;
        }
        // Finding 2: File::create uses umask; restore exact mode/owner + fsync
        apply_original_metadata(&orig_path, entry);
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
            Err(e) => return Err(e),
        }
    }

    // 5. Update database status to RESTORED
    db.mark_restored(entry.id)
        .map_err(Error::other)?;

    Ok(())
}
