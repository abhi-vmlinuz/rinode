mod cli;
mod config;
mod db;
mod exclusion_cli;
mod hasher;
mod init;
mod restore;
mod syscalls;
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
            let mut excluded_count = 0;

            for path in paths {
                if !path.exists() && !path.is_symlink() {
                    if !force {
                        eprintln!("rinode: cannot remove '{}': No such file or directory", path.display());
                    }
                    continue;
                }

                if verbose {
                    println!("rinode: preserving '{}'", path.display());
                }

                match vault.preserve(&path, &db, permanent) {
                    Ok(Some(entry)) => {
                        preserved_count += 1;
                        println!(
                            "Preserved [{}] '{}' (inode: {}, size: {})",
                            entry.id,
                            entry.filename,
                            entry.inode_no,
                            format_bytes(entry.file_size)
                        );
                    }
                    Ok(None) => {
                        excluded_count += 1;
                        if permanent {
                            println!("Permanently removed excluded path '{}'", path.display());
                        } else {
                            println!("Excluded by config: '{}'", path.display());
                        }
                    }
                    Err(e) => {
                        eprintln!("rinode: failed to remove '{}': {}", path.display(), e);
                    }
                }
            }

            if preserved_count > 0 || excluded_count > 0 {
                println!(
                    "Preserved {} item(s) ({} excluded). Run 'rinode ls' or 'rinode restore <id>' to undo.",
                    preserved_count, excluded_count
                );
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
                println!("No entries found in the vault.");
                return Ok(());
            }

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

                let mut row_cells = vec![
                    Cell::new(entry.id.to_string()).fg(Color::Cyan),
                    Cell::new(entry.filename).fg(Color::Green),
                    Cell::new(display_size).fg(Color::Yellow),
                    Cell::new(formatted_date),
                    Cell::new(entry.inode_no.to_string()),
                ];
                if all {
                    let status_cell = match entry.status.as_str() {
                        "PRESERVED" => Cell::new("PRESERVED").fg(Color::Green),
                        "RESTORED" => Cell::new("RESTORED").fg(Color::Cyan),
                        "PURGED" => Cell::new("PURGED").fg(Color::Red),
                        "EXCLUDED" => Cell::new("EXCLUDED").fg(Color::Yellow),
                        _ => Cell::new(&entry.status),
                    };
                    row_cells.push(status_cell);
                }
                row_cells.push(Cell::new(entry.original_path));
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
                        println!("Retained snapshot copy in the vault.");
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
                    println!("  Status:            {}", entry.status);
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
                    } else {
                        &entry.vault_path
                    };
                    println!("  Vault Location:    {}", vault_loc);
                }
                None => {
                    eprintln!("rinode: no entry found with ID {}", id);
                    std::process::exit(1);
                }
            }
        }

        Commands::Purge { days, all } => {
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

            println!("Purged {} expired file(s) from the vault.", purged_count);
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
