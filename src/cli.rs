use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "rinode", version, about = "Zero-copy deleted file tracking & instant restoration")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Safely delete files or directories by moving them into the vault
    #[command(name = "rm")]
    Rm {
        /// Files or directories to delete
        #[arg(required = true)]
        paths: Vec<PathBuf>,

        /// Permanently delete without preserving in vault
        #[arg(short, long)]
        permanent: bool,

        /// Force deletion without warnings
        #[arg(short, long)]
        force: bool,

        /// Remove directories and their contents recursively (POSIX rm compatibility)
        #[arg(short = 'r', short_alias = 'R', long)]
        recursive: bool,

        /// Explain what is being done (POSIX rm compatibility)
        #[arg(short = 'v', long)]
        verbose: bool,

        /// Prompt before every removal (POSIX rm compatibility)
        #[arg(short = 'i', long)]
        interactive: bool,

        /// Remove empty directories (POSIX rm compatibility)
        #[arg(short = 'd', long)]
        dir: bool,
    },

    /// List recently deleted files
    #[command(name = "ls")]
    Ls {
        /// Maximum number of entries to display
        #[arg(short = 'n', long, default_value = "20")]
        limit: usize,

        /// Show all entries including already restored or purged
        #[arg(short, long)]
        all: bool,

        /// Output only IDs and filenames for shell completion
        #[arg(long, hide = true)]
        ids: bool,
    },

    /// Restore a deleted file or directory by its ID or filename
    #[command(name = "restore")]
    Restore {
        /// ID or filename of the entry to restore
        target: String,

        /// Keep a copy in the vault (Snapshot Fork via CoW/Reflink)
        #[arg(short, long)]
        keep_vault: bool,

        /// Overwrite if the destination already exists
        #[arg(short, long)]
        force: bool,
    },

    /// Inspect detailed metadata (inode, dev, permissions, hashes) of an entry
    #[command(name = "inspect")]
    Inspect {
        /// Entry ID to inspect
        id: i64,
    },

    /// Purge expired files from the vault to reclaim disk space
    #[command(name = "purge")]
    Purge {
        /// Purge entries older than N days (default: configured retention days)
        #[arg(short, long)]
        days: Option<u32>,

        /// Force purge all preserved entries
        #[arg(long)]
        all: bool,
    },

    /// Launch the interactive terminal user interface (TUI)
    #[command(name = "tui")]
    Tui,

    /// Generate shell integration script (supports: fish, bash, zsh)
    #[command(name = "init")]
    Init {
        /// Shell type (fish, bash, or zsh). If omitted, auto-detects from $SHELL.
        shell: Option<String>,

        /// Also alias rm="rinode rm" in your shell
        #[arg(long)]
        alias_rm: bool,
    },

    /// Manage or test pattern and directory exclusions
    #[command(name = "exclude", alias = "ignore")]
    Exclude {
        /// Pattern, folder name, or glob to exclude (e.g. '*.log', 'node_modules')
        pattern: Option<String>,

        /// List all currently active exclusion rules
        #[arg(short, long)]
        list: bool,

        /// Test whether a sample path would be excluded (dry-run check)
        #[arg(short, long)]
        test: Option<String>,

        /// Remove an existing exclusion rule
        #[arg(short, long)]
        remove: Option<String>,
    },
}
