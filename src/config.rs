//! User configuration (sources, allowlist, custom blocks) and the cached
//! merged blocklist.
//!
//! Config lives at `~/Library/Application Support/adblocker/config.toml`;
//! the cache lives next to it as `blocklist.cache`. The config is created
//! with defaults on first run so the tool "just works". Path-taking
//! variants (`load_from`, `save_to`, `write_cache_to`, `read_cache_from`)
//! exist for tests so they never touch the real `HOME`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AdBlockerError, Result};
use crate::validate;

/// The default StevenBlack unified hosts source.
pub const DEFAULT_SOURCE: &str = "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_sources")]
    pub sources: Vec<String>,
    #[serde(default)]
    pub allowlist: Vec<String>,
    #[serde(default)]
    pub custom_blocks: Vec<String>,
}

fn default_sources() -> Vec<String> {
    vec![DEFAULT_SOURCE.to_string()]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sources: default_sources(),
            allowlist: Vec::new(),
            custom_blocks: Vec::new(),
        }
    }
}

impl Config {
    /// Load config from the default path, creating it with defaults if the
    /// file does not yet exist.
    pub fn load() -> Result<Self> {
        Self::load_from(&config_path())
    }

    /// Load config from `path`, creating it with defaults if missing.
    pub fn load_from(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let cfg: Config =
                    toml::from_str(&content).map_err(|e| AdBlockerError::Config(e.to_string()))?;
                Ok(cfg)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let cfg = Config::default();
                cfg.save_to(path)?;
                Ok(cfg)
            }
            Err(e) => Err(AdBlockerError::Io {
                path: path.display().to_string(),
                source: e,
            }),
        }
    }

    /// Save config to the default path.
    pub fn save(&self) -> Result<()> {
        self.save_to(&config_path())
    }

    /// Save config to `path`, creating parent directories as needed.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AdBlockerError::Io {
                path: parent.display().to_string(),
                source: e,
            })?;
        }
        let content =
            toml::to_string_pretty(self).map_err(|e| AdBlockerError::Config(e.to_string()))?;
        std::fs::write(path, content).map_err(|e| AdBlockerError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        Ok(())
    }

    /// Add a domain to the allowlist (normalized, deduplicated).
    pub fn add_allow(&mut self, domain: &str) -> Result<()> {
        let normalized = validate::normalize_domain(domain)?;
        if !self.allowlist.iter().any(|d| d == &normalized) {
            self.allowlist.push(normalized);
        }
        Ok(())
    }

    /// Add a domain to the custom block list (normalized, deduplicated).
    pub fn add_block(&mut self, domain: &str) -> Result<()> {
        let normalized = validate::normalize_domain(domain)?;
        if !self.custom_blocks.iter().any(|d| d == &normalized) {
            self.custom_blocks.push(normalized);
        }
        Ok(())
    }
}

/// `~/Library/Application Support/adblocker/config.toml`
pub fn config_path() -> PathBuf {
    app_support_dir().join("config.toml")
}

/// `~/Library/Application Support/adblocker/blocklist.cache`
pub fn cache_path() -> PathBuf {
    app_support_dir().join("blocklist.cache")
}

/// The application support directory for this tool, under the user's home.
fn app_support_dir() -> PathBuf {
    home_dir()
        .join("Library")
        .join("Application Support")
        .join("adblocker")
}

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".to_string()))
}

/// Write the merged domain list to `path`.
///
/// Format: a header comment line followed by one normalized domain per line,
/// sorted. Human-inspectable and trivial to re-read.
pub fn write_cache_to(path: &Path, domains: &[String]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AdBlockerError::Io {
            path: parent.display().to_string(),
            source: e,
        })?;
    }
    let mut content = String::new();
    content.push_str(&format!("# adblocker cache v1 count={}\n", domains.len()));
    for d in domains {
        content.push_str(d);
        content.push('\n');
    }
    std::fs::write(path, content).map_err(|e| AdBlockerError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    Ok(())
}

/// Write the merged domain list to the default cache path.
pub fn write_cache(domains: &[String]) -> Result<()> {
    write_cache_to(&cache_path(), domains)
}

/// Read the cached domain list from `path`. Returns
/// [`AdBlockerError::EmptyBlocklist`] if the cache holds no domains.
pub fn read_cache_from(path: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path).map_err(|e| AdBlockerError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    let domains: Vec<String> = content
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| l.to_string())
        .collect();
    if domains.is_empty() {
        return Err(AdBlockerError::EmptyBlocklist);
    }
    Ok(domains)
}

/// Read the cached domain list from the default cache path.
pub fn read_cache() -> Result<Vec<String>> {
    read_cache_from(&cache_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_has_stevenblack_source() {
        let cfg = Config::default();
        assert_eq!(cfg.sources, vec![DEFAULT_SOURCE.to_string()]);
        assert!(cfg.allowlist.is_empty());
        assert!(cfg.custom_blocks.is_empty());
    }

    #[test]
    fn round_trip_toml() {
        let cfg = Config {
            sources: vec!["https://example.com/list".to_string()],
            allowlist: vec!["good.com".to_string()],
            custom_blocks: vec!["bad.com".to_string()],
        };
        let s = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.sources, cfg.sources);
        assert_eq!(back.allowlist, cfg.allowlist);
        assert_eq!(back.custom_blocks, cfg.custom_blocks);
    }

    #[test]
    fn partial_config_uses_defaults() {
        // A hand-edited config missing `sources` should still load, falling
        // back to the default source.
        let toml = "allowlist = [\"good.com\"]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.sources, vec![DEFAULT_SOURCE.to_string()]);
        assert_eq!(cfg.allowlist, vec!["good.com"]);
    }

    #[test]
    fn load_creates_default_when_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert!(!path.exists());
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.sources, vec![DEFAULT_SOURCE.to_string()]);
        assert!(path.exists(), "config file should have been created");
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");
        let cfg = Config {
            sources: vec!["https://example.com/list".to_string()],
            allowlist: vec!["good.com".to_string()],
            custom_blocks: vec!["bad.com".to_string()],
        };
        cfg.save_to(&path).unwrap();
        let back = Config::load_from(&path).unwrap();
        assert_eq!(back.sources, cfg.sources);
        assert_eq!(back.allowlist, cfg.allowlist);
        assert_eq!(back.custom_blocks, cfg.custom_blocks);
    }

    #[test]
    fn add_allow_normalizes_and_dedups() {
        let mut cfg = Config::default();
        cfg.add_allow("Good.COM").unwrap();
        cfg.add_allow("good.com").unwrap();
        assert_eq!(cfg.allowlist, vec!["good.com"]);
    }

    #[test]
    fn add_block_normalizes_and_dedups() {
        let mut cfg = Config::default();
        cfg.add_block("Bad.COM.").unwrap();
        cfg.add_block("bad.com").unwrap();
        assert_eq!(cfg.custom_blocks, vec!["bad.com"]);
    }

    #[test]
    fn add_allow_rejects_invalid() {
        let mut cfg = Config::default();
        assert!(cfg.add_allow("not a domain").is_err());
    }

    #[test]
    fn cache_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("blocklist.cache");
        let domains = vec!["a.com".to_string(), "b.com".to_string()];
        write_cache_to(&path, &domains).unwrap();
        let back = read_cache_from(&path).unwrap();
        assert_eq!(back, domains);
    }

    #[test]
    fn cache_empty_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("blocklist.cache");
        write_cache_to(&path, &[]).unwrap();
        assert!(matches!(
            read_cache_from(&path).unwrap_err(),
            AdBlockerError::EmptyBlocklist
        ));
    }

    #[test]
    fn cache_missing_comment_lines_skipped() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("blocklist.cache");
        let domains = vec!["a.com".to_string()];
        write_cache_to(&path, &domains).unwrap();
        // Append a stray comment; it must be ignored on read.
        let existing = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("{existing}# stray\n")).unwrap();
        let back = read_cache_from(&path).unwrap();
        assert_eq!(back, vec!["a.com"]);
    }
}
