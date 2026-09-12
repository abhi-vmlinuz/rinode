use std::fs::{self, File};
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use crate::db::{Db, EntryRecord};
use crate::syscalls::{reflink_clone, renameat2_path};

/// Restore an entry by its ID or by searching for its latest name
pub fn restore_by_id_or_name(
    db: &Db,
    identifier: &str,
    keep_vault: bool,
    force: bool,
) -> Result<EntryRecord> {
    let entry = if let Ok(id) = identifier.parse::<i64>() {
        db.get_by_id(id)
            .map_err(|e| Error::new(ErrorKind::Other, e))?
            .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Entry with ID {} not found", id)))?
    } else {
        let matches = db
            .find_by_filename(identifier)
            .map_err(|e| Error::new(ErrorKind::Other, e))?;

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

/// Perform the restoration of an entry
pub fn restore_entry(db: &Db, entry: &EntryRecord, keep_vault: bool, force: bool) -> Result<()> {
    let orig_path = PathBuf::from(&entry.original_path);

    // 1. Recreate parent directories if missing (mkdir -p)
    if let Some(parent) = orig_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // 2. Collision detection
    if orig_path.exists() {
        if !force {
            return Err(Error::new(
                ErrorKind::AlreadyExists,
                format!(
                    "Destination '{}' already exists. Use --force to overwrite.",
                    orig_path.display()
                ),
            ));
        }

        // Remove existing destination if force is true
        if orig_path.is_dir() {
            fs::remove_dir_all(&orig_path)?;
        } else {
            fs::remove_file(&orig_path)?;
        }
    }

    // 3. Handle Symlink restoration
    if entry.link_type == "SYMLINK" {
        let target = entry.symlink_target.as_deref().unwrap_or("");
        std::os::unix::fs::symlink(target, &orig_path)?;
        db.mark_restored(entry.id)
            .map_err(|e| Error::new(ErrorKind::Other, e))?;
        return Ok(());
    }

    // 4. Handle Regular Files and Directories
    let vault_path = Path::new(&entry.vault_path);
    if !vault_path.exists() {
        return Err(Error::new(
            ErrorKind::NotFound,
            format!("Vault file '{}' is missing on disk", vault_path.display()),
        ));
    }

    if keep_vault && !entry.is_directory {
        // Snapshot fork mode: Try CoW reflink first, fallback to standard copy
        let src_file = File::open(vault_path)?;
        let dst_file = File::create(&orig_path)?;

        if reflink_clone(&src_file, &dst_file).is_err() {
            drop(dst_file);
            fs::copy(vault_path, &orig_path)?;
        }
    } else {
        // Default Consume mode: Atomic rename back to original path
        renameat2_path(vault_path, &orig_path, 0)?;
    }

    // 5. Update database status to RESTORED
    db.mark_restored(entry.id)
        .map_err(|e| Error::new(ErrorKind::Other, e))?;

    Ok(())
}
