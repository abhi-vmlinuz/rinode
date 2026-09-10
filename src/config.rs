use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct StorageConfig {
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
    #[serde(default = "default_max_vault_size_gb")]
    pub max_vault_size_gb: u64,
    #[serde(default = "default_max_hash_file_size_mb")]
    pub max_hash_file_size_mb: u64,
}

fn default_retention_days() -> u32 {
    14
}
fn default_max_vault_size_gb() -> u64 {
    20
}
fn default_max_hash_file_size_mb() -> u64 {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExclusionsConfig {
    #[serde(default = "default_system_paths")]
    pub system_paths: Vec<String>,
    #[serde(default = "default_path_regex")]
    pub path_regex: Vec<String>,
    #[serde(default = "default_filename_regex")]
    pub filename_regex: Vec<String>,
}

fn default_system_paths() -> Vec<String> {
    vec![
        "/proc".into(),
        "/sys".into(),
        "/dev".into(),
        "/run".into(),
        "/tmp".into(),
    ]
}

fn default_path_regex() -> Vec<String> {
    vec![
        r".*/node_modules/.*".into(),
        r".*/\.git/(?!config|HEAD).*".into(),
        r".*/target/(debug|release)/.*".into(),
        r".*/build/.*".into(),
        r".*/\.cache/.*".into(),
        r".*/__pycache__/.*".into(),
    ]
}

fn default_filename_regex() -> Vec<String> {
    vec![
        r"^\..*\.swp$".into(),
        r".*~$".into(),
        r".*\.tmp$".into(),
        r"^core(\.\d+)?$".into(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigRaw {
    #[serde(default)]
    pub storage: Option<StorageConfig>,
    #[serde(default)]
    pub exclusions: Option<ExclusionsConfig>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExclusionKind {
    SystemPath,
    PathRegex,
    FilenameRegex,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub storage: StorageConfig,
    pub exclusions: ExclusionsConfig,
    compiled_path_regex: Vec<Regex>,
    compiled_filename_regex: Vec<Regex>,
}

impl Default for Config {
    fn default() -> Self {
        let storage = StorageConfig {
            retention_days: default_retention_days(),
            max_vault_size_gb: default_max_vault_size_gb(),
            max_hash_file_size_mb: default_max_hash_file_size_mb(),
        };
        let exclusions = ExclusionsConfig {
            system_paths: default_system_paths(),
            path_regex: default_path_regex(),
            filename_regex: default_filename_regex(),
        };

        let compiled_path_regex = exclusions
            .path_regex
            .iter()
            .filter_map(|r| Regex::new(r).ok())
            .collect();
        let compiled_filename_regex = exclusions
            .filename_regex
            .iter()
            .filter_map(|r| Regex::new(r).ok())
            .collect();

        Config {
            storage,
            exclusions,
            compiled_path_regex,
            compiled_filename_regex,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let candidates = [
            PathBuf::from("rinode.toml"),
            directories::ProjectDirs::from("com", "recent-inode", "rinode")
                .map(|p| p.config_dir().join("config.toml"))
                .unwrap_or_else(|| PathBuf::from("/nonexistent")),
            PathBuf::from("/etc/rinode/config.toml"),
        ];

        for candidate in &candidates {
            if candidate.exists() {
                if let Ok(content) = fs::read_to_string(candidate) {
                    if let Ok(raw) = toml::from_str::<ConfigRaw>(&content) {
                        return Self::from_raw(raw);
                    }
                }
            }
        }

        Config::default()
    }

    fn from_raw(raw: ConfigRaw) -> Self {
        let mut cfg = Config::default();
        if let Some(s) = raw.storage {
            cfg.storage = s;
        }
        if let Some(e) = raw.exclusions {
            cfg.compiled_path_regex = e
                .path_regex
                .iter()
                .filter_map(|r| Regex::new(r).ok())
                .collect();
            cfg.compiled_filename_regex = e
                .filename_regex
                .iter()
                .filter_map(|r| Regex::new(r).ok())
                .collect();
            cfg.exclusions = e;
        }
        cfg
    }

    /// Convert a shell glob pattern (e.g. *.log, test??.tmp) into a regex pattern
    pub fn glob_to_regex(glob: &str) -> String {
        let mut regex_str = String::from("^");
        for c in glob.chars() {
            match c {
                '*' => regex_str.push_str(".*"),
                '?' => regex_str.push('.'),
                '.' => regex_str.push_str("\\."),
                '+' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|' | '\\' => {
                    regex_str.push('\\');
                    regex_str.push(c);
                }
                _ => regex_str.push(c),
            }
        }
        regex_str.push('$');
        regex_str
    }

    /// Intelligently classify user input into the appropriate exclusion type
    pub fn classify_input(input: &str) -> (ExclusionKind, String) {
        let trimmed = input.trim();

        // 1. Absolute path -> System path prefix
        if trimmed.starts_with('/') && !trimmed.contains('*') && !trimmed.contains('?') {
            return (ExclusionKind::SystemPath, trimmed.trim_end_matches('/').to_string());
        }

        // 2. Globs with directory separator -> Path regex
        if trimmed.contains('/') && (trimmed.contains('*') || trimmed.contains('?')) {
            let mut pattern = Self::glob_to_regex(trimmed);
            // Relax leading/trailing anchors if user typed something like node_modules/*
            if pattern.starts_with("^.*") {
                // already starts with wildcard
            } else if pattern.starts_with('^') {
                pattern.insert_str(1, ".*");
            }
            return (ExclusionKind::PathRegex, pattern);
        }

        // 3. Filename glob -> Filename regex
        if trimmed.contains('*') || trimmed.contains('?') {
            return (ExclusionKind::FilenameRegex, Self::glob_to_regex(trimmed));
        }

        // 4. Looks like a directory name (e.g. node_modules, build, target)
        if !trimmed.contains('.') && !trimmed.contains('/') {
            return (
                ExclusionKind::PathRegex,
                format!(r".*/{}/.*", regex::escape(trimmed)),
            );
        }

        // 5. Default exact filename match
        (
            ExclusionKind::FilenameRegex,
            format!(r"^{}$", regex::escape(trimmed)),
        )
    }

    /// Check if a path or filename is excluded
    pub fn is_excluded(&self, path: &Path, filename: &str) -> bool {
        self.test_path(path, filename).is_some()
    }

    /// Test a path and return a detailed diagnostic of what matched (if any)
    pub fn test_path(&self, path: &Path, filename: &str) -> Option<String> {
        let path_str = path.to_string_lossy();

        // 1. Check system prefix paths
        for sys_path in &self.exclusions.system_paths {
            if path_str.starts_with(sys_path) {
                return Some(format!("system_paths prefix '{}'", sys_path));
            }
        }

        // 2. Check path regex
        for (i, re) in self.compiled_path_regex.iter().enumerate() {
            if re.is_match(&path_str) {
                let raw_rule = self.exclusions.path_regex.get(i).map(|s| s.as_str()).unwrap_or("unknown");
                return Some(format!("path_regex rule '{}'", raw_rule));
            }
        }

        // 3. Check filename regex
        for (i, re) in self.compiled_filename_regex.iter().enumerate() {
            if re.is_match(filename) {
                let raw_rule = self.exclusions.filename_regex.get(i).map(|s| s.as_str()).unwrap_or("unknown");
                return Some(format!("filename_regex rule '{}'", raw_rule));
            }
        }

        None
    }

    /// Recompile regexes after adding or removing rules
    fn recompile(&mut self) {
        self.compiled_path_regex = self
            .exclusions
            .path_regex
            .iter()
            .filter_map(|r| Regex::new(r).ok())
            .collect();
        self.compiled_filename_regex = self
            .exclusions
            .filename_regex
            .iter()
            .filter_map(|r| Regex::new(r).ok())
            .collect();
    }

    /// Add a new exclusion rule and persist it to user configuration
    pub fn add_rule(&mut self, kind: ExclusionKind, value: String) -> Result<PathBuf, String> {
        match kind {
            ExclusionKind::SystemPath => {
                if !self.exclusions.system_paths.contains(&value) {
                    self.exclusions.system_paths.push(value);
                }
            }
            ExclusionKind::PathRegex => {
                Regex::new(&value).map_err(|e| format!("Invalid regex '{}': {}", value, e))?;
                if !self.exclusions.path_regex.contains(&value) {
                    self.exclusions.path_regex.push(value);
                }
            }
            ExclusionKind::FilenameRegex => {
                Regex::new(&value).map_err(|e| format!("Invalid regex '{}': {}", value, e))?;
                if !self.exclusions.filename_regex.contains(&value) {
                    self.exclusions.filename_regex.push(value);
                }
            }
        }

        self.recompile();
        self.save_to_user_config()
    }

    /// Remove an existing rule by exact match, classified pattern, or substring
    pub fn remove_rule(&mut self, target: &str) -> Result<bool, String> {
        let mut removed = false;

        // 1. Try exact match
        if let Some(pos) = self.exclusions.system_paths.iter().position(|r| r == target) {
            self.exclusions.system_paths.remove(pos);
            removed = true;
        } else if let Some(pos) = self.exclusions.path_regex.iter().position(|r| r == target) {
            self.exclusions.path_regex.remove(pos);
            removed = true;
        } else if let Some(pos) = self.exclusions.filename_regex.iter().position(|r| r == target) {
            self.exclusions.filename_regex.remove(pos);
            removed = true;
        } else {
            // 2. Try classified pattern
            let (_kind, classified) = Self::classify_input(target);
            if let Some(pos) = self.exclusions.system_paths.iter().position(|r| r == &classified) {
                self.exclusions.system_paths.remove(pos);
                removed = true;
            } else if let Some(pos) = self.exclusions.path_regex.iter().position(|r| r == &classified) {
                self.exclusions.path_regex.remove(pos);
                removed = true;
            } else if let Some(pos) = self.exclusions.filename_regex.iter().position(|r| r == &classified) {
                self.exclusions.filename_regex.remove(pos);
                removed = true;
            } else {
                // 3. Try substring match
                if let Some(pos) = self.exclusions.path_regex.iter().position(|r| r.contains(target)) {
                    self.exclusions.path_regex.remove(pos);
                    removed = true;
                } else if let Some(pos) = self.exclusions.filename_regex.iter().position(|r| r.contains(target)) {
                    self.exclusions.filename_regex.remove(pos);
                    removed = true;
                } else if let Some(pos) = self.exclusions.system_paths.iter().position(|r| r.contains(target)) {
                    self.exclusions.system_paths.remove(pos);
                    removed = true;
                }
            }
        }

        if removed {
            self.recompile();
            self.save_to_user_config()?;
        }

        Ok(removed)
    }

    /// Save current configuration to user config (~/.config/rinode/config.toml)
    pub fn save_to_user_config(&self) -> Result<PathBuf, String> {
        let config_dir = directories::ProjectDirs::from("com", "recent-inode", "rinode")
            .map(|p| p.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config/rinode"));

        fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
        let config_file = config_dir.join("config.toml");

        let raw = ConfigRaw {
            storage: Some(self.storage.clone()),
            exclusions: Some(self.exclusions.clone()),
        };

        let toml_str = toml::to_string_pretty(&raw).map_err(|e| e.to_string())?;
        fs::write(&config_file, toml_str).map_err(|e| e.to_string())?;

        Ok(config_file)
    }
}
