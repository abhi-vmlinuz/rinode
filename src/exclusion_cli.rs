use std::io::{self, Write};
use std::path::Path;

use crate::config::{Config, ExclusionKind};

pub fn handle_exclude_command(
    config: &mut Config,
    pattern: Option<String>,
    list: bool,
    test: Option<String>,
    remove: Option<String>,
) {
    if let Some(sample) = test {
        test_sample_path(config, &sample);
        return;
    }

    if let Some(target) = remove {
        remove_exclusion_rule(config, &target);
        return;
    }

    if list {
        display_active_rules(config);
        return;
    }

    if let Some(pat) = pattern {
        add_single_pattern(config, &pat);
        return;
    }

    // No arguments -> Interactive prompt menu
    run_interactive_prompt_menu(config);
}

fn add_single_pattern(config: &mut Config, input: &str) {
    let (kind, rule) = Config::classify_input(input);
    let kind_label = match kind {
        ExclusionKind::SystemPath => "System path prefix",
        ExclusionKind::PathRegex => "Path regex pattern",
        ExclusionKind::FilenameRegex => "Filename regex pattern",
    };

    match config.add_rule(kind, rule.clone()) {
        Ok(path) => {
            println!("[+] Added exclusion rule:");
            println!("    Type:    {}", kind_label);
            println!("    Rule:    {}", rule);
            println!("    Saved:   {}", path.display());
        }
        Err(e) => {
            eprintln!("[!] Failed to add exclusion rule: {}", e);
        }
    }
}

fn remove_exclusion_rule(config: &mut Config, target: &str) {
    match config.remove_rule(target) {
        Ok(true) => {
            println!("[+] Removed exclusion rule '{}'", target);
        }
        Ok(false) => {
            println!("[-] Rule '{}' was not found in active exclusions.", target);
        }
        Err(e) => {
            eprintln!("[!] Failed to remove rule: {}", e);
        }
    }
}

fn test_sample_path(config: &Config, sample: &str) {
    let p = Path::new(sample);
    let filename = p
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "".to_string());

    println!("Testing path: '{}'", sample);
    match config.test_path(p, &filename) {
        Some(reason) => {
            println!("-> MATCHED: Excluded by {}", reason);
            println!("-> Result: Files matching this path will NOT be vaulted (directly unlinked).");
        }
        None => {
            println!("-> NOT EXCLUDED: No matching filter rule.");
            println!("-> Result: This file will be safely preserved in the vault on deletion.");
        }
    }
}

fn display_active_rules(config: &Config) {
    println!("rinode active exclusion rules");
    println!("------------------------------------------------------------");

    println!("System Paths (Prefix match):");
    if config.exclusions.system_paths.is_empty() {
        println!("  (none)");
    } else {
        for s in &config.exclusions.system_paths {
            println!("  • {}", s);
        }
    }

    println!("\nPath Patterns (Regex match):");
    if config.exclusions.path_regex.is_empty() {
        println!("  (none)");
    } else {
        for r in &config.exclusions.path_regex {
            println!("  • {}", r);
        }
    }

    println!("\nFilename Patterns (Regex match):");
    if config.exclusions.filename_regex.is_empty() {
        println!("  (none)");
    } else {
        for f in &config.exclusions.filename_regex {
            println!("  • {}", f);
        }
    }
    println!("------------------------------------------------------------");
}

fn run_interactive_prompt_menu(config: &mut Config) {
    let stdin = io::stdin();

    loop {
        println!("\nrinode exclusion manager");
        println!("------------------------------------------------------------");
        println!("  [1] Quick-add: type any folder, file glob, or path");
        println!("  [2] Test sample path (dry-run check)");
        println!("  [3] View all active rules");
        println!("  [4] Remove an existing rule");
        println!("  [q] Quit");
        print!("\nSelect option: ");
        io::stdout().flush().ok();

        let mut choice = String::new();
        if stdin.read_line(&mut choice).is_err() {
            break;
        }

        match choice.trim() {
            "1" => {
                print!("\nEnter folder name, glob (e.g. *.log), or path: ");
                io::stdout().flush().ok();
                let mut input = String::new();
                if stdin.read_line(&mut input).is_ok() {
                    let trimmed = input.trim();
                    if !trimmed.is_empty() {
                        add_single_pattern(config, trimmed);
                    }
                }
            }
            "2" => {
                print!("\nEnter sample path to test: ");
                io::stdout().flush().ok();
                let mut sample = String::new();
                if stdin.read_line(&mut sample).is_ok() {
                    let trimmed = sample.trim();
                    if !trimmed.is_empty() {
                        test_sample_path(config, trimmed);
                    }
                }
            }
            "3" => {
                display_active_rules(config);
            }
            "4" => {
                print!("\nEnter rule string to remove: ");
                io::stdout().flush().ok();
                let mut target = String::new();
                if stdin.read_line(&mut target).is_ok() {
                    let trimmed = target.trim();
                    if !trimmed.is_empty() {
                        remove_exclusion_rule(config, trimmed);
                    }
                }
            }
            "q" | "Q" | "exit" => {
                break;
            }
            _ => {
                println!("[!] Invalid choice. Please select 1, 2, 3, 4, or q.");
            }
        }
    }
}
