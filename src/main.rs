mod cli;
mod config;
mod db;
mod exclusion_cli;
mod hasher;
mod init;
mod restore;
mod syscalls;
mod theme;
mod tui;
mod vault;

use clap::Parser;
use cli::{Cli, Commands};
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, Row, Table};
use config::Config;
use db::Db;
use restore::restore_by_id_or_name;
use vault::VaultManager;

pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

pub fn format_display_path(original_path: &str, max_refs: usize) -> String {
    let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
    let path = std::path::Path::new(original_path);

    let (is_home, rel_path) = if let Some(h) = &home {
        if let Ok(rel) = path.strip_prefix(h) {
            (true, rel)
        } else {
            (false, path)
        }
    } else {
        (false, path)
    };

    let mut parts: Vec<&str> = Vec::new();
    for comp in rel_path.components() {
        if let std::path::Component::Normal(s) = comp {
            if let Some(s_str) = s.to_str() {
                parts.push(s_str);
            }
        }
    }

    if parts.is_empty() {
        return if is_home { "~".to_string() } else { original_path.to_string() };
    }

    // Directory parts (excluding filename at the end)
    let dir_parts = if parts.len() > 1 {
        &parts[..parts.len() - 1]
    } else {
        &parts[..0]
    };

    if dir_parts.is_empty() {
        return if is_home { "~".to_string() } else { "/".to_string() };
    }

    let prefix = if is_home { "~" } else { "" };

    if dir_parts.len() <= max_refs {
        format!("{}/{}", prefix, dir_parts.join("/"))
    } else {
        format!("{}/{}/...", prefix, dir_parts[..max_refs].join("/"))
    }
}

pub fn format_display_name(filename: &str, max_len: usize) -> String {
    if filename.chars().count() <= max_len {
        return filename.to_string();
    }

    let path = std::path::Path::new(filename);
    if let (Some(stem), Some(ext)) = (path.file_stem().and_then(|s| s.to_str()), path.extension().and_then(|e| e.to_str())) {
        if !ext.is_empty() && ext.len() <= 6 && max_len > ext.len() + 4 {
            let stem_budget = max_len.saturating_sub(ext.len() + 3);
            let stem_prefix: String = stem.chars().take(stem_budget).collect();
            return format!("{}...{}", stem_prefix, ext);
        }
    }

    let prefix: String = filename.chars().take(max_len.saturating_sub(3)).collect();
    format!("{}...", prefix)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();
    let mut config = Config::load();
    let db = Db::open_default()?;

    // Default to interactive TUI if no subcommand is passed
    let command = args.command.unwrap_or(Commands::Tui);

    match command {
        Commands::Tui => {
            tui::run_tui(&db, &config)?;
        }

        Commands::Rm {
            paths,
            permanent,
            force,
            recursive: _,
            verbose,
            interactive: _,
            dir: _,
        } => {
            let vault = VaultManager::new(config);
            let mut preserved_count = 0;
            let mut permanent_count = 0;
            let mut excluded_count = 0;

            for path in paths {
                if !path.exists() && !path.is_symlink() {
                    if !force {
                        eprintln!("rinode: cannot remove '{}': No such file or directory", path.display());
                    }
                    continue;
                }

                if verbose {
                    if permanent {
                        println!("rinode: removing '{}' permanently", path.display());
                    } else {
                        println!("rinode: preserving '{}'", path.display());
                    }
                }

                match vault.preserve(&path, &db, permanent) {
                    Ok(Some(entry)) => {
                        if entry.status == "PURGED" {
                            permanent_count += 1;
                            println!(
                                "Deleted [{}] '{}' permanently (inode: {}, size: {})",
                                entry.id,
                                entry.filename,
                                entry.inode_no,
                                format_bytes(entry.file_size)
                            );
                        } else {
                            preserved_count += 1;
                            println!(
                                "Deleted [{}] '{}' (inode: {}, size: {})",
                                entry.id,
                                entry.filename,
                                entry.inode_no,
                                format_bytes(entry.file_size)
                            );
                        }
                    }
                    Ok(None) => {
                        excluded_count += 1;
                        println!("Excluded by config: '{}'", path.display());
                    }
                    Err(e) => {
                        eprintln!("rinode: failed to remove '{}': {}", path.display(), e);
                    }
                }
            }

            if preserved_count > 0 {
                if permanent_count > 0 || excluded_count > 0 {
                    let mut details = Vec::new();
                    if permanent_count > 0 {
                        details.push(format!("{} permanently", permanent_count));
                    }
                    if excluded_count > 0 {
                        details.push(format!("{} excluded", excluded_count));
                    }
                    println!(
                        "Deleted {} item(s) ({}). Run 'rinode ls' or 'rinode restore <id>' to undo.",
                        preserved_count + permanent_count,
                        details.join(", ")
                    );
                } else {
                    println!(
                        "Deleted {} item(s). Run 'rinode ls' or 'rinode restore <id>' to undo.",
                        preserved_count
                    );
                }
            } else if permanent_count > 0 {
                if excluded_count > 0 {
                    println!(
                        "Deleted {} item(s) permanently ({} excluded, tracked in history).",
                        permanent_count, excluded_count
                    );
                } else {
                    println!(
                        "Deleted {} item(s) permanently (tracked in history).",
                        permanent_count
                    );
                }
            } else if excluded_count > 0 {
                println!("Deleted 0 item(s) ({} excluded).", excluded_count);
            }
        }

        Commands::Ls { limit, all, ids } => {
            let entries = if all {
                db.list_all(None)?
            } else {
                db.list_active(Some(limit))?
            };

            if ids {
                for entry in entries {
                    println!("{}\t{}", entry.id, entry.filename);
                }
                return Ok(());
            }

            if entries.is_empty() {
                println!("No deleted files found.");
                return Ok(());
            }

            let term_width = crossterm::terminal::size().ok().map(|(w, _)| w as usize);
            let (max_refs, max_name_len) = match term_width {
                Some(w) if w >= 135 => (2, 36),
                Some(w) if w >= 115 => (2, 28),
                Some(w) if w >= 100 => (1, if all { 20 } else { 24 }),
                Some(w) if w >= 80 => (1, if all { 15 } else { 18 }),
                Some(w) => (1, 14.max(w.saturating_sub(if all { 80 } else { 68 }))),
                None => (2, 26),
            };

            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .apply_modifier(UTF8_ROUND_CORNERS);

            let mut headers = vec![
                Cell::new("ID").fg(Color::Cyan),
                Cell::new("Name").fg(Color::Green),
                Cell::new("Size").fg(Color::Yellow),
                Cell::new("Deleted At").fg(Color::Magenta),
                Cell::new("Inode").fg(Color::Blue),
            ];
            if all {
                headers.push(Cell::new("Status").fg(Color::White));
            }
            headers.push(Cell::new("Original Path"));
            table.set_header(headers);

            for entry in entries {
                let formatted_date = entry.deleted_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string();
                let display_size = if entry.is_directory {
                    "<DIR>".to_string()
                } else if entry.link_type == "SYMLINK" {
                    "<SYMLINK>".to_string()
                } else {
                    format_bytes(entry.file_size)
                };

                let display_name = format_display_name(&entry.filename, max_name_len);
                let display_path = format_display_path(&entry.original_path, max_refs);

                let mut row_cells = vec![
                    Cell::new(entry.id.to_string()).fg(Color::Cyan),
                    Cell::new(display_name).fg(Color::Green),
                    Cell::new(display_size).fg(Color::Yellow),
                    Cell::new(formatted_date),
                    Cell::new(entry.inode_no.to_string()),
                ];
                if all {
                    let status_cell = match entry.status.as_str() {
                        "PRESERVED" => Cell::new("DELETED").fg(Color::Green),
                        "RESTORED" => Cell::new("RESTORED").fg(Color::Cyan),
                        "PURGED" => Cell::new("PURGED").fg(Color::Red),
                        "EXCLUDED" => Cell::new("EXCLUDED").fg(Color::Yellow),
                        _ => Cell::new(&entry.status),
                    };
                    row_cells.push(status_cell);
                }
                row_cells.push(Cell::new(display_path));
                table.add_row(Row::from(row_cells));
            }

            println!("{table}");
        }

        Commands::Restore {
            target,
            keep_vault,
            force,
        } => {
            match restore_by_id_or_name(&db, &target, keep_vault, force) {
                Ok(entry) => {
                    println!(
                        "Restored [{}] '{}' to '{}'",
                        entry.id, entry.filename, entry.original_path
                    );
                    if keep_vault {
                        println!("Retained snapshot copy in storage.");
                    }
                }
                Err(e) => {
                    eprintln!("rinode: restore failed: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Inspect { id } => {
            match db.get_by_id(id)? {
                Some(entry) => {
                    println!("Inode Metadata for Entry #{}", entry.id);
                    println!("-----------------------------------------");
                    println!("  Filename:          {}", entry.filename);
                    println!("  Original Path:     {}", entry.original_path);
                    println!("  Inode Number:      {}", entry.inode_no);
                    println!("  Mount/Device:      {}:{} (mnt_id: {})", entry.dev_major, entry.dev_minor, entry.mnt_id);
                    println!("  File Size:         {} ({} bytes)", format_bytes(entry.file_size), entry.file_size);
                    println!("  Permissions (Oct): {:04o}", entry.mode);
                    println!("  Owner UID / GID:   {} / {}", entry.uid, entry.gid);
                    println!("  Link / Move Type:  {}", entry.link_type);
                    let display_status = if entry.status == "PRESERVED" { "DELETED" } else { &entry.status };
                    println!("  Status:            {}", display_status);
                    println!("  Deleted At:        {}", entry.deleted_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S %z"));
                    if let Some(restored_at) = entry.restored_at {
                        println!("  Restored At:       {}", restored_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S %z"));
                    }
                    if let Some(purged_at) = entry.purged_at {
                        println!("  Purged At:         {}", purged_at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S %z"));
                    }
                    if let Some(target) = entry.symlink_target {
                        println!("  Symlink Target:    {}", target);
                    }
                    if let Some(fp) = entry.quick_fingerprint {
                        println!("  Fast Fingerprint:  {}", fp);
                    }
                    let vault_loc = if entry.status == "EXCLUDED" {
                        "(none - excluded by rule)"
                    } else if entry.status == "PURGED" && entry.vault_path.is_empty() {
                        "(none - permanently unlinked)"
                    } else if entry.vault_path.is_empty() {
                        "(none)"
                    } else {
                        &entry.vault_path
                    };
                    println!("  Storage Location:  {}", vault_loc);
                }
                None => {
                    eprintln!("rinode: no entry found with ID {}", id);
                    std::process::exit(1);
                }
            }
        }

        Commands::Purge {
            targets,
            days,
            all,
            force,
        } => {
            if !targets.is_empty() {
                let mut purged_count = 0;
                let mut had_error = false;

                for target in &targets {
                    let entry_opt = if let Ok(id) = target.parse::<i64>() {
                        db.get_by_id(id)?
                    } else {
                        let matches = db.find_by_filename(target)?;
                        matches
                            .iter()
                            .find(|e| e.status == "PRESERVED")
                            .cloned()
                            .or_else(|| matches.into_iter().next())
                    };

                    match entry_opt {
                        Some(entry) => {
                            if entry.status == "PURGED" {
                                if !force {
                                    eprintln!("rinode: entry [{}] '{}' is already purged.", entry.id, entry.filename);
                                }
                                continue;
                            }
                            if entry.status == "EXCLUDED" {
                                if !force {
                                    eprintln!("rinode: entry [{}] was excluded and never stored in storage.", entry.id);
                                    had_error = true;
                                }
                                continue;
                            }
                            if entry.status == "RESTORED" {
                                let disk_path = db::resolve_storage_path(&entry.vault_path);
                                if disk_path.is_none() {
                                    if !force {
                                        eprintln!("rinode: entry [{}] '{}' was already restored and is not in storage.", entry.id, entry.filename);
                                        had_error = true;
                                    }
                                    continue;
                                }
                            }
                            match db.purge_entry(&entry) {
                                Ok(_) => {
                                    println!("Purged [{}] '{}' from storage.", entry.id, entry.filename);
                                    purged_count += 1;
                                }
                                Err(e) => {
                                    eprintln!("rinode: failed to purge [{}] '{}': {}", entry.id, entry.filename, e);
                                    had_error = true;
                                }
                            }
                        }
                        None => {
                            if !force {
                                eprintln!("rinode: no entry found matching '{}'", target);
                                had_error = true;
                            }
                        }
                    }
                }

                if had_error && !force && purged_count == 0 {
                    std::process::exit(1);
                }
            } else {
                let retention_days = if all {
                    0
                } else {
                    days.unwrap_or(config.storage.retention_days)
                };

                let expired = db.get_expired(retention_days)?;
                let mut purged_count = 0;

                for entry in expired {
                    if db.purge_entry(&entry).is_ok() {
                        purged_count += 1;
                    }
                }

                println!("Purged {} expired file(s) from storage.", purged_count);
            }
        }

        Commands::Init { shell, alias, alias_rm } => {
            match init::generate_init(shell.as_deref(), &alias, alias_rm) {
                Ok(script) => print!("{}", script),
                Err(e) => {
                    eprintln!("rinode: init error: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Exclude {
            pattern,
            list,
            test,
            remove,
        } => {
            exclusion_cli::handle_exclude_command(&mut config, pattern, list, test, remove);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_display_name_short() {
        assert_eq!(format_display_name("file.txt", 20), "file.txt");
        assert_eq!(format_display_name("exact_length.txt", 16), "exact_length.txt");
    }

    #[test]
    fn test_format_display_name_with_extension() {
        let long_name = "sample_very_long_document_title_for_testing.docx";
        let formatted = format_display_name(long_name, 26);
        assert_eq!(formatted, "sample_very_long_do...docx");
        assert_eq!(formatted.chars().count(), 26);
        assert!(formatted.ends_with(".docx"));
    }

    #[test]
    fn test_format_display_name_no_extension() {
        let long_dir = "very_long_directory_name_without_extension";
        let formatted = format_display_name(long_dir, 20);
        assert_eq!(formatted, "very_long_directo...");
        assert_eq!(formatted.chars().count(), 20);
    }

    #[test]
    fn test_format_display_name_hidden_file() {
        let swp = ".sample_notes_file_data.txt.swp";
        let formatted = format_display_name(swp, 20);
        assert_eq!(formatted, ".sample_notes_...swp");
        assert_eq!(formatted.chars().count(), 20);
    }
}
