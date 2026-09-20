use chrono::Utc;
use std::fs;
use std::io::{self, Error, Result, Write};
use std::path::{Component, Path, PathBuf};

use crate::config::Config;
use crate::db::{Db, SafetyAuditRecord};
use crate::syscalls::statx_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyTier {
    T0Refuse,
    T1ProtectedRoot,
    T2Sensitive,
    T3SemiProtected,
    Standard,
}

impl SafetyTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            SafetyTier::T0Refuse => "T0 (REFUSE)",
            SafetyTier::T1ProtectedRoot => "T1 (PROTECTED ROOT)",
            SafetyTier::T2Sensitive => "T2 (SENSITIVE)",
            SafetyTier::T3SemiProtected => "T3 (SEMI-PROTECTED)",
            SafetyTier::Standard => "Standard",
        }
    }
}

pub const COMPILED_T0_PATHS: &[&str] = &[
    "/",
    "/proc",
    "/sys",
    "/dev",
    "/run",
];

pub const COMPILED_T1_PATHS: &[&str] = &[
    "/etc",
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/lib32",
    "/boot",
    "/root",
    "/var",
];

pub const COMPILED_T3_PATHS: &[&str] = &[
    "/opt",
    "/srv",
];

#[derive(Debug, Clone)]
pub struct SafetyVerdict {
    pub raw_path: PathBuf,
    pub normalized_path: PathBuf,
    pub canonical_path: Option<PathBuf>,
    pub tier: SafetyTier,
    pub is_mount_root: bool,
    pub is_dir: bool,
    pub size: u64,
    pub is_symlink: bool,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SafetyFlags {
    pub is_interactive: bool,
    pub allow_protected: bool,
    pub no_preserve_root: bool,
    pub force: bool,
    pub permanent: bool,
    pub verbose: bool,
}

/// Lexically normalize path by resolving relative components, parent '..',
/// and duplicate slashes without touching the filesystem.
pub fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                if out.as_os_str() != "/" {
                    out.pop();
                    if out.as_os_str().is_empty() && path.is_absolute() {
                        out.push("/");
                    }
                }
            }
            Component::Normal(c) => out.push(c),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        out
    }
}

fn is_t2_sensitive(path: &Path, filename: &str, mode: u16) -> bool {
    let path_str = path.to_string_lossy();

    // Check directory component names
    for comp in path.components() {
        if let Component::Normal(os_str) = comp {
            let s = os_str.to_string_lossy();
            if s == ".ssh" || s == ".gnupg" || s == ".pki" || s == ".aws" {
                return true;
            }
        }
    }

    // Explicit system credential files
    if path_str == "/etc/shadow"
        || path_str == "/etc/gshadow"
        || path_str.starts_with("/etc/sudoers")
    {
        return true;
    }

    // Common credential / key filenames
    if filename.starts_with("id_")
        || filename.ends_with(".pem")
        || filename.ends_with(".key")
        || filename.ends_with(".pfx")
        || filename.ends_with(".p12")
        || filename.ends_with("_rsa")
        || filename.ends_with("_ed25519")
        || filename == ".env"
        || filename.starts_with(".env.")
    {
        return true;
    }

    // Mode 0600 / 0400 credential files with security naming
    let is_reg = (mode & (libc::S_IFMT as u16)) == (libc::S_IFREG as u16);
    if is_reg && mode > 0 && (mode & 0o077) == 0 && (mode & 0o400) != 0 {
        let lower = filename.to_lowercase();
        if lower.contains("secret")
            || lower.contains("credential")
            || lower.contains("token")
            || lower.contains("auth")
        {
            return true;
        }
    }

    false
}

fn classify_path(path: &Path, config: &Config) -> Result<SafetyTier> {
    let s = path.to_string_lossy();

    // 1. T0 check
    for t0 in COMPILED_T0_PATHS {
        if *t0 == "/" {
            if s == "/" {
                return Ok(SafetyTier::T0Refuse);
            }
        } else if s == *t0 || s.starts_with(&format!("{}/", t0)) {
            return Ok(SafetyTier::T0Refuse);
        }
    }

    // Special check for /tmp and /var/tmp:
    // The directory itself is T0, but child contents are Standard.
    if s == "/tmp" || s == "/var/tmp" {
        return Ok(SafetyTier::T0Refuse);
    }
    if s.starts_with("/tmp/") || s.starts_with("/var/tmp/") {
        return Ok(SafetyTier::Standard);
    }

    // 2. T1 check
    for t1 in COMPILED_T1_PATHS {
        if s == *t1 || s.starts_with(&format!("{}/", t1)) {
            return Ok(SafetyTier::T1ProtectedRoot);
        }
    }

    // User extra-protected from config (treated as T1)
    for extra in &config.safety.extra_protected {
        if s == *extra || s.starts_with(&format!("{}/", extra)) {
            return Ok(SafetyTier::T1ProtectedRoot);
        }
    }

    // 3. T3 check
    for t3 in COMPILED_T3_PATHS {
        if s == *t3 || s.starts_with(&format!("{}/", t3)) {
            return Ok(SafetyTier::T3SemiProtected);
        }
    }

    Ok(SafetyTier::Standard)
}

fn tier_reason(tier: SafetyTier) -> &'static str {
    match tier {
        SafetyTier::T0Refuse => "critical system path or root (T0)",
        SafetyTier::T1ProtectedRoot => "protected system directory or mount root (T1)",
        SafetyTier::T2Sensitive => "sensitive credentials or private keys (T2)",
        SafetyTier::T3SemiProtected => "semi-protected system hierarchy /opt or /srv (T3)",
        SafetyTier::Standard => "standard path",
    }
}

/// Resolves a single path against the safety policy, checking lexical traps,
/// symlink double-checking, mount boundaries, ancestor walk, and credential patterns.
pub fn resolve_checked(raw_path: &Path, config: &Config) -> Result<SafetyVerdict> {
    let s = raw_path.to_string_lossy();
    let trimmed = s.trim_end_matches('/');

    // 1. Lexical refusal for . and ..
    if trimmed == "." || trimmed == ".." || trimmed.ends_with("/.") || trimmed.ends_with("/..") {
        return Err(Error::other("refusing to remove '.' or '..' directory entry"));
    }

    // 2. Lexical normalization against CWD
    let abs_path = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(raw_path)
    };
    let normalized = normalize_lexical(&abs_path);
    let norm_str = normalized.to_string_lossy();

    // 3. Root check
    if normalized == Path::new("/") || norm_str == "/" {
        return Ok(SafetyVerdict {
            raw_path: raw_path.to_path_buf(),
            normalized_path: normalized,
            canonical_path: Some(PathBuf::from("/")),
            tier: SafetyTier::T0Refuse,
            is_mount_root: true,
            is_dir: true,
            size: 0,
            is_symlink: false,
            reason: "root filesystem (/)",
        });
    }

    // 4. Self-protection check
    let filename = normalized
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    if filename == ".rinode-storage"
        || filename == ".rinode-vault"
        || norm_str.ends_with("/.rinode-storage")
        || norm_str.ends_with("/.rinode-vault")
        || norm_str.contains("/.rinode-storage/")
        || norm_str.contains("/.rinode-vault/")
    {
        return Ok(SafetyVerdict {
            raw_path: raw_path.to_path_buf(),
            normalized_path: normalized,
            canonical_path: None,
            tier: SafetyTier::T0Refuse,
            is_mount_root: false,
            is_dir: true,
            size: 0,
            is_symlink: false,
            reason: "rinode internal storage directory",
        });
    }

    if filename == "rinode.db" || norm_str.ends_with("/rinode.db") {
        return Ok(SafetyVerdict {
            raw_path: raw_path.to_path_buf(),
            normalized_path: normalized,
            canonical_path: None,
            tier: SafetyTier::T0Refuse,
            is_mount_root: false,
            is_dir: false,
            size: 0,
            is_symlink: false,
            reason: "rinode metadata database",
        });
    }

    if filename == "rinode.toml" || filename == "policy.toml" || norm_str.starts_with("/etc/rinode") {
        return Ok(SafetyVerdict {
            raw_path: raw_path.to_path_buf(),
            normalized_path: normalized,
            canonical_path: None,
            tier: SafetyTier::T0Refuse,
            is_mount_root: false,
            is_dir: false,
            size: 0,
            is_symlink: false,
            reason: "rinode policy or configuration file",
        });
    }

    // 5. Inspect symlinks
    let sym_meta = fs::symlink_metadata(&normalized).or_else(|_| fs::symlink_metadata(raw_path));
    let is_symlink = sym_meta.as_ref().map(|m| m.is_symlink()).unwrap_or(false);
    let sym_target_tier = if is_symlink {
        if let Ok(target) = fs::read_link(&normalized) {
            let target_abs = if target.is_absolute() {
                target
            } else {
                normalized.parent().unwrap_or(Path::new("/")).join(target)
            };
            let target_norm = normalize_lexical(&target_abs);
            classify_path(&target_norm, config)?
        } else {
            SafetyTier::Standard
        }
    } else {
        SafetyTier::Standard
    };

    // 6. Statx metadata & mount root detection
    let statx_res = statx_path(&normalized);
    let (is_dir, size, mode, is_mount_root) = match statx_res {
        Ok(info) => {
            let is_mnt = if let Some(parent) = normalized.parent() {
                if let Ok(p_info) = statx_path(parent) {
                    info.mnt_id != p_info.mnt_id
                } else {
                    false
                }
            } else {
                true
            };
            (info.is_dir, info.size, info.mode, is_mnt)
        }
        Err(_) => (false, 0, 0, false),
    };

    // 7. Base classification
    let path_tier = classify_path(&normalized, config)?;

    // Strictest tier between path and symlink target wins
    let mut effective_tier = match (path_tier, sym_target_tier) {
        (SafetyTier::T0Refuse, _) | (_, SafetyTier::T0Refuse) => SafetyTier::T0Refuse,
        (SafetyTier::T1ProtectedRoot, _) | (_, SafetyTier::T1ProtectedRoot) => SafetyTier::T1ProtectedRoot,
        (SafetyTier::T2Sensitive, _) | (_, SafetyTier::T2Sensitive) => SafetyTier::T2Sensitive,
        (SafetyTier::T3SemiProtected, _) | (_, SafetyTier::T3SemiProtected) => SafetyTier::T3SemiProtected,
        _ => SafetyTier::Standard,
    };

    // Mount roots default to T1
    if is_mount_root && effective_tier == SafetyTier::Standard {
        effective_tier = SafetyTier::T1ProtectedRoot;
    }

    // T2 sensitive credential pattern
    if effective_tier == SafetyTier::Standard && is_t2_sensitive(&normalized, &filename, mode) {
        effective_tier = SafetyTier::T2Sensitive;
    }

    let canonical = normalized.canonicalize().ok();

    Ok(SafetyVerdict {
        raw_path: raw_path.to_path_buf(),
        normalized_path: normalized,
        canonical_path: canonical,
        tier: effective_tier,
        is_mount_root,
        is_dir,
        size,
        is_symlink,
        reason: tier_reason(effective_tier),
    })
}

fn format_bytes_brief(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GiB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MiB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KiB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn record_audit_log(
    db: Option<&Db>,
    verdict: &SafetyVerdict,
    flags: &SafetyFlags,
    result: &str,
) {
    if let Some(db) = db {
        let (uid, euid, pid, ppid) = unsafe {
            (
                libc::getuid(),
                libc::geteuid(),
                libc::getpid() as u32,
                libc::getppid() as u32,
            )
        };
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let canonical_str = verdict
            .canonical_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| verdict.normalized_path.to_string_lossy().to_string());

        let mut flag_list = Vec::new();
        if flags.allow_protected {
            flag_list.push("--allow-protected");
        }
        if flags.no_preserve_root {
            flag_list.push("--no-preserve-root");
        }
        if flags.force {
            flag_list.push("-f/--force");
        }
        if flags.permanent {
            flag_list.push("-p/--permanent");
        }

        let rec = SafetyAuditRecord {
            timestamp: Utc::now().to_rfc3339(),
            uid,
            euid,
            pid,
            ppid,
            cwd,
            canonical_path: canonical_str,
            tier: verdict.tier.as_str().to_string(),
            flags: flag_list.join(" "),
            result: result.to_string(),
        };
        let _ = db.record_safety_override(&rec);
    }
}

/// Enforce safety checks and flag-based ceremonies across all targeted paths.
pub fn verify_safety(
    verdicts: &[SafetyVerdict],
    flags: &SafetyFlags,
    config: &Config,
    db: Option<&Db>,
) -> Result<()> {
    let is_terminal = flags.is_interactive;

    // 1. T0 checks
    let t0_items: Vec<_> = verdicts.iter().filter(|v| v.tier == SafetyTier::T0Refuse).collect();
    if !t0_items.is_empty() {
        let first = t0_items[0];
        if flags.permanent {
            record_audit_log(db, first, flags, "DENIED_PERMANENT_ON_T0");
            return Err(Error::other(format!(
                "refusing to permanently delete T0 protected path '{}' (vaulting required, --no-storage is not permitted)",
                first.normalized_path.display()
            )));
        }

        if !flags.no_preserve_root || !flags.allow_protected || !flags.force {
            record_audit_log(db, first, flags, "DENIED_MISSING_T0_FLAGS");
            return Err(Error::other(format!(
                "refusing to remove T0 protected path '{}'. Overriding requires: --no-preserve-root --allow-protected --force",
                first.normalized_path.display()
            )));
        }

        for item in &t0_items {
            record_audit_log(db, item, flags, "ALLOWED_T0_OVERRIDE");
        }
    }

    // 2. T1 checks
    let t1_items: Vec<_> = verdicts.iter().filter(|v| v.tier == SafetyTier::T1ProtectedRoot).collect();
    if !t1_items.is_empty() {
        let total_size: u64 = t1_items.iter().map(|v| v.size).sum();
        let is_bulk = t1_items.len() > config.safety.bulk_count || total_size > 1024 * 1024 * 1024;

        if flags.permanent {
            let first = t1_items[0];
            record_audit_log(db, first, flags, "DENIED_PERMANENT_ON_T1");
            return Err(Error::other(format!(
                "refusing to permanently delete T1 protected path '{}' (vaulting required, --no-storage is not permitted)",
                first.normalized_path.display()
            )));
        }

        if is_bulk {
            if flags.allow_protected && flags.force {
                for item in &t1_items {
                    record_audit_log(db, item, flags, "ALLOWED_T1_BULK_FLAGS");
                }
            } else if is_terminal {
                eprint!(
                    "rinode: T1 bulk operation detected ({} protected items, {}). Move to storage? [y/N]: ",
                    t1_items.len(),
                    format_bytes_brief(total_size)
                );
                io::stderr().flush().ok();
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                let trimmed = input.trim().to_lowercase();
                if trimmed != "y" && trimmed != "yes" {
                    record_audit_log(db, t1_items[0], flags, "DENIED_T1_BULK_PROMPT");
                    return Err(Error::other("operation cancelled by user"));
                }
                for item in &t1_items {
                    record_audit_log(db, item, flags, "ALLOWED_T1_BULK_PROMPT");
                }
            } else {
                record_audit_log(db, t1_items[0], flags, "DENIED_T1_BULK_NON_TTY");
                return Err(Error::other(
                    "refusing bulk deletion of T1 protected items in non-interactive session. Requires: --allow-protected --force",
                ));
            }
        } else if flags.allow_protected {
            for item in &t1_items {
                record_audit_log(db, item, flags, "ALLOWED_T1_FLAG");
            }
        } else if is_terminal {
            for item in &t1_items {
                eprint!(
                    "rinode: '{}' is a T1 protected path. Move to storage? [y/N]: ",
                    item.normalized_path.display()
                );
                io::stderr().flush().ok();
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                let trimmed = input.trim().to_lowercase();
                if trimmed != "y" && trimmed != "yes" {
                    record_audit_log(db, item, flags, "DENIED_T1_PROMPT");
                    return Err(Error::other("operation cancelled by user"));
                }
                record_audit_log(db, item, flags, "ALLOWED_T1_PROMPT");
            }
        } else {
            record_audit_log(db, t1_items[0], flags, "DENIED_T1_NON_TTY");
            return Err(Error::other(format!(
                "refusing to remove T1 protected path '{}' in non-interactive session. Use --allow-protected to proceed",
                t1_items[0].normalized_path.display()
            )));
        }
    }

    // 3. T2 checks
    let t2_items: Vec<_> = verdicts.iter().filter(|v| v.tier == SafetyTier::T2Sensitive).collect();
    if !t2_items.is_empty() {
        if !flags.force {
            if is_terminal {
                for item in &t2_items {
                    if flags.permanent {
                        eprint!(
                            "rinode: WARNING: '{}' contains sensitive credentials. Permanently delete from disk? [y/N]: ",
                            item.normalized_path.display()
                        );
                    } else {
                        eprint!(
                            "rinode: WARNING: '{}' contains sensitive credentials. Move to storage (will remain in vault until purged)? [y/N]: ",
                            item.normalized_path.display()
                        );
                    }
                    io::stderr().flush().ok();
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    let trimmed = input.trim().to_lowercase();
                    if trimmed != "y" && trimmed != "yes" {
                        record_audit_log(db, item, flags, "DENIED_T2_PROMPT");
                        return Err(Error::other("operation cancelled by user"));
                    }
                    record_audit_log(db, item, flags, "ALLOWED_T2_PROMPT");
                }
            } else {
                record_audit_log(db, t2_items[0], flags, "DENIED_T2_NON_TTY");
                return Err(Error::other(format!(
                    "refusing to remove T2 sensitive path '{}' without confirmation. Use -f / --force to confirm in scripts",
                    t2_items[0].normalized_path.display()
                )));
            }
        } else {
            for item in &t2_items {
                record_audit_log(db, item, flags, "ALLOWED_T2_FORCE");
            }
        }
    }

    // 4. T3 checks
    let t3_items: Vec<_> = verdicts.iter().filter(|v| v.tier == SafetyTier::T3SemiProtected).collect();
    if !t3_items.is_empty() {
        if !flags.force && !flags.allow_protected {
            if is_terminal {
                for item in &t3_items {
                    eprint!(
                        "rinode: '{}' is in a T3 semi-protected path (/opt or /srv). Move to storage? [y/N]: ",
                        item.normalized_path.display()
                    );
                    io::stderr().flush().ok();
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    let trimmed = input.trim().to_lowercase();
                    if trimmed != "y" && trimmed != "yes" {
                        record_audit_log(db, item, flags, "DENIED_T3_PROMPT");
                        return Err(Error::other("operation cancelled by user"));
                    }
                    record_audit_log(db, item, flags, "ALLOWED_T3_PROMPT");
                }
            } else {
                record_audit_log(db, t3_items[0], flags, "DENIED_T3_NON_TTY");
                return Err(Error::other(format!(
                    "refusing to remove T3 semi-protected path '{}' in non-interactive session. Use -f / --force or --allow-protected",
                    t3_items[0].normalized_path.display()
                )));
            }
        } else {
            for item in &t3_items {
                record_audit_log(db, item, flags, "ALLOWED_T3_FLAG");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lexical_dot_refusal() {
        let cfg = Config::default();
        assert!(resolve_checked(Path::new("."), &cfg).is_err());
        assert!(resolve_checked(Path::new(".."), &cfg).is_err());
        assert!(resolve_checked(Path::new("foo/."), &cfg).is_err());
        assert!(resolve_checked(Path::new("foo/.."), &cfg).is_err());
        assert!(resolve_checked(Path::new("./"), &cfg).is_err());
        assert!(resolve_checked(Path::new("../"), &cfg).is_err());
        assert!(resolve_checked(Path::new("/."), &cfg).is_err());
        assert!(resolve_checked(Path::new("/./"), &cfg).is_err());
    }

    #[test]
    fn test_slash_normalization_and_root() {
        let cfg = Config::default();
        let v1 = resolve_checked(Path::new("/"), &cfg).unwrap();
        assert_eq!(v1.tier, SafetyTier::T0Refuse);

        let v2 = resolve_checked(Path::new("///"), &cfg).unwrap();
        assert_eq!(v2.tier, SafetyTier::T0Refuse);

        let v3 = resolve_checked(Path::new("//"), &cfg).unwrap();
        assert_eq!(v3.tier, SafetyTier::T0Refuse);
    }

    #[test]
    fn test_ancestor_matching_t1() {
        let cfg = Config::default();
        let v = resolve_checked(Path::new("/etc/nginx/sites-available/old.conf"), &cfg).unwrap();
        assert_eq!(v.tier, SafetyTier::T1ProtectedRoot);

        let v_bin = resolve_checked(Path::new("/usr/bin/some_binary"), &cfg).unwrap();
        assert_eq!(v_bin.tier, SafetyTier::T1ProtectedRoot);
    }

    #[test]
    fn test_tmp_contents_allowed() {
        let cfg = Config::default();
        let v_tmp = resolve_checked(Path::new("/tmp"), &cfg).unwrap();
        assert_eq!(v_tmp.tier, SafetyTier::T0Refuse);

        let v_file = resolve_checked(Path::new("/tmp/scratch.txt"), &cfg).unwrap();
        assert_eq!(v_file.tier, SafetyTier::Standard);
    }

    #[test]
    fn test_t2_sensitive_matching() {
        let cfg = Config::default();
        let v_env = resolve_checked(Path::new("/home/user/project/.env"), &cfg).unwrap();
        assert_eq!(v_env.tier, SafetyTier::T2Sensitive);

        let v_key = resolve_checked(Path::new("/home/user/.ssh/id_ed25519"), &cfg).unwrap();
        assert_eq!(v_key.tier, SafetyTier::T2Sensitive);

        let v_pem = resolve_checked(Path::new("/home/user/certs/cert.pem"), &cfg).unwrap();
        assert_eq!(v_pem.tier, SafetyTier::T2Sensitive);
    }

    #[test]
    fn test_t3_semi_protected() {
        let cfg = Config::default();
        let v_opt = resolve_checked(Path::new("/opt/my_service/config"), &cfg).unwrap();
        assert_eq!(v_opt.tier, SafetyTier::T3SemiProtected);
    }

    #[test]
    fn test_self_protection_rinode_storage_and_db() {
        let cfg = Config::default();
        let v_storage = resolve_checked(Path::new("/data/.rinode-storage"), &cfg).unwrap();
        assert_eq!(v_storage.tier, SafetyTier::T0Refuse);

        let v_db = resolve_checked(Path::new("/home/user/.local/share/rinode/rinode.db"), &cfg).unwrap();
        assert_eq!(v_db.tier, SafetyTier::T0Refuse);
    }

    #[test]
    fn test_verify_safety_t0_ceremony() {
        let cfg = Config::default();
        let v_root = resolve_checked(Path::new("/"), &cfg).unwrap();
        let verdicts = vec![v_root];

        // Missing flags
        let f_default = SafetyFlags::default();
        assert!(verify_safety(&verdicts, &f_default, &cfg, None).is_err());

        let f_force = SafetyFlags { force: true, ..Default::default() };
        assert!(verify_safety(&verdicts, &f_force, &cfg, None).is_err());

        let f_no_root = SafetyFlags { no_preserve_root: true, ..Default::default() };
        assert!(verify_safety(&verdicts, &f_no_root, &cfg, None).is_err());

        // Permanent on T0 is strictly forbidden even with override flags
        let f_perm = SafetyFlags {
            no_preserve_root: true,
            allow_protected: true,
            force: true,
            permanent: true,
            ..Default::default()
        };
        assert!(verify_safety(&verdicts, &f_perm, &cfg, None).is_err());

        // All 3 required flags pass for vaulting
        let f_valid = SafetyFlags {
            no_preserve_root: true,
            allow_protected: true,
            force: true,
            permanent: false,
            ..Default::default()
        };
        assert!(verify_safety(&verdicts, &f_valid, &cfg, None).is_ok());
    }

    #[test]
    fn test_verify_safety_t1_ceremony() {
        let cfg = Config::default();
        let v_etc = resolve_checked(Path::new("/etc/test.conf"), &cfg).unwrap();
        let verdicts = vec![v_etc];

        // Non-interactive without --allow-protected fails
        let f_none = SafetyFlags::default();
        assert!(verify_safety(&verdicts, &f_none, &cfg, None).is_err());

        // Permanent on T1 fails
        let f_perm = SafetyFlags {
            allow_protected: true,
            permanent: true,
            ..Default::default()
        };
        assert!(verify_safety(&verdicts, &f_perm, &cfg, None).is_err());

        // With --allow-protected passes
        let f_ok = SafetyFlags {
            allow_protected: true,
            permanent: false,
            ..Default::default()
        };
        assert!(verify_safety(&verdicts, &f_ok, &cfg, None).is_ok());
    }

    #[test]
    fn test_verify_safety_t1_bulk() {
        let cfg = Config::default();
        let mut verdicts = Vec::new();
        for i in 0..12 {
            let v = resolve_checked(Path::new(&format!("/etc/conf_{}.conf", i)), &cfg).unwrap();
            verdicts.push(v);
        }

        // Bulk (>10) in non-interactive with only allow_protected fails (needs force too)
        let f_allow = SafetyFlags {
            allow_protected: true,
            ..Default::default()
        };
        assert!(verify_safety(&verdicts, &f_allow, &cfg, None).is_err());

        // Bulk with allow_protected AND force passes
        let f_bulk_ok = SafetyFlags {
            allow_protected: true,
            force: true,
            ..Default::default()
        };
        assert!(verify_safety(&verdicts, &f_bulk_ok, &cfg, None).is_ok());
    }

    #[test]
    fn test_verify_safety_t2_sensitive() {
        let cfg = Config::default();
        let v_key = resolve_checked(Path::new("/home/user/.ssh/id_ed25519"), &cfg).unwrap();
        let verdicts = vec![v_key];

        // Non-interactive without force fails
        let f_none = SafetyFlags::default();
        assert!(verify_safety(&verdicts, &f_none, &cfg, None).is_err());

        // With force passes
        let f_force = SafetyFlags { force: true, ..Default::default() };
        assert!(verify_safety(&verdicts, &f_force, &cfg, None).is_ok());

        // Permanent with force is allowed for T2
        let f_perm = SafetyFlags {
            force: true,
            permanent: true,
            ..Default::default()
        };
        assert!(verify_safety(&verdicts, &f_perm, &cfg, None).is_ok());
    }
}
