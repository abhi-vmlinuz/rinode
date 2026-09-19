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

    // No arguments -> display active rules and usage guide
    display_active_rules(config);
    println!("\nUsage:");
    println!("  rinode exclude <pattern>     Add a rule (e.g. *.log, build, /var/log)");
    println!("  rinode exclude -l            List all active rules");
    println!("  rinode exclude -t <path>     Test whether a path matches any rule");
    println!("  rinode exclude -r <rule>     Remove an existing rule");
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
        .unwrap_or_default();

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
}
