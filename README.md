# rinode

<p align="center">
  <strong>Zero-copy deleted file tracking and instant restoration for Linux.</strong>
</p>

<p align="center">
  <a href="https://github.com/abhi-vmlinuz/rinode/releases"><img src="https://img.shields.io/github/v/release/abhi-vmlinuz/rinode?style=flat-square&color=blue" alt="Release"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/Rust-1.75+-orange?style=flat-square&logo=rust" alt="Rust Version"></a>
  <a href="https://kernel.org"><img src="https://img.shields.io/badge/Linux_Kernel-5.8+-007ACC?style=flat-square&logo=linux" alt="Kernel Version"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="License"></a>
</p>

---

`rinode` (recent-inode) is a Linux utility that preserves deleted files and directories in constant time without copying data blocks, allowing instant restoration.

## Problem

Standard Linux deletion tools have two main drawbacks:

1. **Immediate extent tree deallocation**: When you delete a file with `rm`, the kernel drops the inode link count to zero. Filesystems like ext4 immediately clear the inode extent tree and mark blocks as free. Once that happens, undeleting requires scanning raw disk blocks with tools like `ext4magic` or `photorec`, which often fails for fragmented or large files.
2. **Slow cross-filesystem copies**: Desktop trash specifications (FreeDesktop Trash) copy files byte-by-byte when moving across mount points or subvolumes. Deleting a 50GB directory tree or large virtual machine image can take minutes and wastes write cycles.

## How it works

`rinode` keeps inodes alive at the filesystem level:

1. **Mount-boundary discovery**: It walks up the directory tree using `statx(2)` with `STATX_MNT_ID` to find the exact mount point or Btrfs subvolume root.
2. **Constant-time move**: It relocates deleted entries into a permissions-locked vault (`.rinode-vault`, mode `0700`) on the same filesystem using `renameat2(2)`. The inode number, extent tree, permissions, and timestamps stay intact on disk.
3. **Audit indexing**: Metadata (original path, inode, device ID, owner, permissions, and a 64KB xxHash fingerprint) is recorded into an embedded SQLite database (`rinode.db`) in Write-Ahead Log (WAL) mode.
4. **Instant restoration**:
   - **Move back (default)**: Recreates any missing parent directories (`mkdir -p`) and moves the inode back to its original location via `rename()`.
   - **Snapshot fork (`--keep-vault`)**: On filesystems that support Copy-on-Write (Btrfs, XFS), it issues an `ioctl(FICLONE)` system call to point a new directory entry to the existing data blocks with zero duplication. On ext4, it copies the file.

## Installation

Requirements:
- Linux kernel 5.8 or newer (required for `STATX_MNT_ID`)
- Rust 1.75+ (for building from source)

### Building from source

```bash
make
```

### Systemwide installation

Installs the binary to `/usr/local/bin`, man page to `/usr/local/share/man/man1`, and shell completions for Fish, Bash, and Zsh:

```bash
sudo make install
```

### Local user installation

Installs to `~/.local/bin` and user completion directories:

```bash
make install-user
```

To uninstall:
```bash
sudo make uninstall
```

## Usage

### Interactive terminal dashboard

Running `rinode` without arguments opens a split-screen terminal interface:

```bash
rinode
```

- **Left pane**: Table of preserved files (ID, filename, size, deletion time, inode number).
- **Right pane**: Inode metadata, permissions, ownership, timestamps, and vault path.
- **Actions**: Press `Enter` on any row to open the action menu (Restore, Reflink fork, Inspect, Purge, Copy path).
- **Shortcuts**: `j`/`k` to navigate, `r` to restore, `x` to purge, `e` to view active exclusion rules, `/` to filter, `q` to exit.

### Command-line interface

#### Deleting files

`rinode rm` moves items into the local vault. It accepts standard POSIX `rm` flags (`-r`, `-R`, `-f`, `-v`, `-i`, `-d`) for drop-in compatibility:

```bash
# Delete a single file
rinode rm report.pdf

# Delete a directory hierarchy
rinode rm -rf ./build_output/

# Permanently delete without vaulting
rinode rm -p unwanted_cache.tar
```

#### Listing vaulted files

```bash
rinode ls
```

Shows a formatted table of active entries in the vault:

```text
╭────┬──────────────┬────────┬─────────────────────┬─────────┬─────────────────────────────────────╮
│ ID ┆ Name         ┆ Size   ┆ Deleted At          ┆ Inode   ┆ Original Path                       │
╞════╪══════════════╪════════╪═════════════════════╪═════════╪═════════════════════════════════════╡
│ 1  ┆ report.pdf   ┆ 2.4 MB ┆ 2026-09-10 17:55:20 ┆ 1982182 ┆ /home/user/documents/report.pdf     │
╰────┴──────────────┴────────┴─────────────────────┴─────────┴─────────────────────────────────────╯
```

#### Restoring files

Restore by ID or filename:

```bash
# Restore by entry ID (moves file back to original location)
rinode restore 1

# Restore by name
rinode restore report.pdf

# Overwrite if a file already exists at the destination path
rinode restore --force report.pdf

# Restore a copy while keeping the snapshot in the vault
rinode restore --keep-vault 1
```

#### Inspecting metadata

```bash
rinode inspect 1
```

Displays complete inode provenance:
```text
Inode Metadata for Entry #1
-----------------------------------------
  Filename:          report.pdf
  Original Path:     /home/user/documents/report.pdf
  Inode Number:      1982182
  Mount/Device:      252:0 (mnt_id: 47)
  File Size:         2.4 MB (2516582 bytes)
  Permissions (Oct): 100644
  Owner UID / GID:   1000 / 1000
  Link / Move Type:  RENAME_MOVE
  Status:            PRESERVED
  Deleted At:        2026-09-10T17:55:20.171592361+00:00
  Fast Fingerprint:  dc1025ce6c498bd0
  Vault Location:    /home/user/.rinode-vault/report.pdf__1982182_0_1789062920171503320_cac889
```

#### Managing exclusions

You can prevent temporary build artifacts, log files, or specific paths from entering the vault:

```bash
# Open the interactive menu
rinode exclude

# Add a glob pattern (converted to a filename regex)
rinode exclude "*.log"

# Add a folder name (converted to a path regex)
rinode exclude "node_modules"

# Add an absolute system path prefix
rinode exclude "/var/cache"

# List active rules
rinode exclude --list

# Test whether a candidate path would be vaulted or unlinked directly
rinode exclude --test /home/user/repo/node_modules/pkg/index.js

# Remove a rule
rinode exclude --remove "*.log"
```

#### Cleaning up (Purging)

Permanently unlinks files from the vault that have exceeded the retention threshold:

```bash
# Purge entries older than the configured retention period (default: 14 days)
rinode purge

# Purge entries older than N days
rinode purge --days 7

# Purge all vaulted files immediately
rinode purge --all
```

## Shell integration

`rinode init` generates a shell function named `r`:
- `r` without arguments launches the terminal UI.
- `r <files...>` vaults files using `rinode rm`.
- `r ls`, `r restore <id>`, `r inspect <id>`, and `r purge` forward to their respective subcommands.

To load it, add the following to your shell configuration file:

### Fish (`~/.config/fish/config.fish`)
```fish
rinode init fish | source
```

### Bash (`~/.bashrc`)
```bash
eval "$(rinode init bash)"
```

### Zsh (`~/.zshrc`)
```zsh
eval "$(rinode init zsh)"
```

To also replace standard `rm` with `rinode rm`, add the `--alias-rm` flag:
```fish
rinode init fish --alias-rm | source
```

## Configuration

Configuration files are resolved in this order:
1. `./rinode.toml` (Current directory)
2. `~/.config/rinode/config.toml` (User configuration)
3. `/etc/rinode/config.toml` (System fallback)

Default configuration:

```toml
[storage]
retention_days = 14
max_vault_size_gb = 20
max_hash_file_size_mb = 50

[exclusions]
system_paths = [
    "/proc",
    "/sys",
    "/dev",
    "/run",
    "/tmp",
]

path_regex = [
    ".*/node_modules/.*",
    ".*/\\.git/(?!config|HEAD).*",
    ".*/target/(debug|release)/.*",
    ".*/build/.*",
    ".*/\\.cache/.*",
    ".*/__pycache__/.*",
]

filename_regex = [
    "^\\..*\\.swp$",
    ".*~$",
    ".*\\.tmp$",
    "^core(\\.\\d+)?$",
]
```

## Filesystem support

| Filesystem | Deletion mechanism | Restore mechanism | Snapshot fork (`--keep-vault`) |
|---|---|---|---|
| **ext4** | `renameat2` | `rename` | Kernel file copy |
| **Btrfs** | `renameat2` within subvolume | `rename` | `ioctl(FICLONE)` (reflink CoW) |
| **XFS** | `renameat2` | `rename` | `ioctl(FICLONE)` (reflink CoW) |

## Tests

Integration tests run against a local test hierarchy:

```bash
make test
```
