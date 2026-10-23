//! User configuration (sources, allowlist, custom blocks), the cached merged
//! blocklist, per-source snapshots, and where all of them live.
//!
//! Everything lives in one per-user app directory (see [`default_app_dir`]):
//! `config.toml`, the merged `blocklist.cache`, and `sources/*.list` — the
//! last good copy of each source, used when a download fails and to rebuild
//! the cache offline after an allowlist change. The config is created with
//! defaults on first run so the tool "just works". [`Paths`] carries every
//! location, so tests point it at a temp dir and never touch the real files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AdBlockerError, Result};
use crate::validate;

/// The default `StevenBlack` unified hosts source.
pub const DEFAULT_SOURCE: &str = "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts";

/// Every file the tool reads or writes. `main` builds the real one; tests
/// point it at a temp dir.
#[derive(Debug, Clone)]
pub struct Paths {
    pub app_dir: PathBuf,
    pub hosts: PathBuf,
}

impl Paths {
    pub fn new(app_dir: PathBuf, hosts: PathBuf) -> Self {
        Self { app_dir, hosts }
    }

    pub fn config(&self) -> PathBuf {
        self.app_dir.join("config.toml")
    }

    pub fn cache(&self) -> PathBuf {
        self.app_dir.join("blocklist.cache")
    }

    pub fn sources_dir(&self) -> PathBuf {
        self.app_dir.join("sources")
    }

    /// The last good copy of the source at `url`. Named by a hash of the URL
    /// so reordering `sources` keeps each snapshot attached to its list.
    pub fn source_snapshot(&self, url: &str) -> PathBuf {
        self.sources_dir()
            .join(format!("{:016x}.list", fnv1a64(url.as_bytes())))
    }

    /// The rolling backup of the hosts file, taken before every change.
    pub fn backup(&self) -> PathBuf {
        crate::hosts::backup_path(&self.hosts)
    }
}

/// FNV-1a: a stable, dependency-free hash. Only used for file names, so
/// collision resistance is irrelevant; stability across Rust versions is not
/// (which rules out `DefaultHasher`).
fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

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
    /// Load config from `path`, creating it with defaults if missing.
    pub fn load_from(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let cfg: Config =
                    toml::from_str(&content).map_err(|e| AdBlockerError::Config(e.to_string()))?;
                cfg.validate_sources()?;
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

    /// Reject any source that is not an `https://` URL, so a blocklist can
    /// never be fetched over plain HTTP and tampered with in transit.
    fn validate_sources(&self) -> Result<()> {
        for url in &self.sources {
            if source_host(url).is_none() {
                return Err(AdBlockerError::Config(format!(
                    "source {url:?} must be an https:// URL"
                )));
            }
        }
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

    /// Remove a domain from the allowlist; errors if it was not there.
    pub fn remove_allow(&mut self, domain: &str) -> Result<()> {
        remove_entry(&mut self.allowlist, domain, "allowlist")
    }

    /// Remove a domain from the custom block list; errors if it was not there.
    pub fn remove_block(&mut self, domain: &str) -> Result<()> {
        remove_entry(&mut self.custom_blocks, domain, "custom blocks")
    }
}

fn remove_entry(list: &mut Vec<String>, domain: &str, name: &str) -> Result<()> {
    let normalized = validate::normalize_domain(domain)?;
    let before = list.len();
    list.retain(|d| d != &normalized);
    if list.len() == before {
        return Err(AdBlockerError::Config(format!(
            "{normalized} is not in your {name}"
        )));
    }
    Ok(())
}

/// The host of an `https://` source URL (no port or credentials), or `None`
/// if the URL is not HTTPS or has no host.
pub fn source_host(url: &str) -> Option<&str> {
    let scheme = url.get(..8)?;
    if !scheme.eq_ignore_ascii_case("https://") {
        return None;
    }
    let authority = url[8..].split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?.split(':').next()?;
    (!host.is_empty()).then_some(host)
}

/// The per-user app directory. `ADBLOCKER_DIR` overrides it — useful when
/// elevating as a different account than the one that ran `update`.
pub fn default_app_dir() -> PathBuf {
    match std::env::var_os("ADBLOCKER_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => platform_app_dir(),
    }
}

/// `~/Library/Application Support/adblocker` on macOS.
#[cfg(target_os = "macos")]
fn platform_app_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".to_string()))
        .join("Library")
        .join("Application Support")
        .join("adblocker")
}

/// `%APPDATA%\adblocker` on Windows, falling back to
/// `%USERPROFILE%\AppData\Roaming`.
#[cfg(windows)]
fn platform_app_dir() -> PathBuf {
    let roaming = std::env::var("APPDATA").map_or_else(
        |_| {
            PathBuf::from(std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string()))
                .join("AppData")
                .join("Roaming")
        },
        PathBuf::from,
    );
    roaming.join("adblocker")
}

/// `$XDG_CONFIG_HOME/adblocker` (default `~/.config/adblocker`) on Linux and
/// other Unix systems. Most Linux `sudo` setups reset `HOME` to root's, so
/// under sudo we look up the invoking user's home in `/etc/passwd` to make
/// `apply` read the same cache `update` wrote.
#[cfg(not(any(windows, target_os = "macos")))]
fn platform_app_dir() -> PathBuf {
    if let Some(home) = std::env::var("SUDO_USER").ok().and_then(|user| {
        let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
        home_from_passwd(&passwd, &user)
    }) {
        return home.join(".config").join("adblocker");
    }
    let config_home = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".to_string())).join(".config")
        }
    };
    config_home.join("adblocker")
}

/// The home directory of `user` in `/etc/passwd`-format text.
#[cfg(not(any(windows, target_os = "macos")))]
fn home_from_passwd(passwd: &str, user: &str) -> Option<PathBuf> {
    passwd.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        (fields.len() >= 6 && fields[0] == user).then(|| PathBuf::from(fields[5]))
    })
}

/// Write a domain list to `path`: one header comment line, then one domain
/// per line. Used for the merged cache and for source snapshots.
fn write_domain_list(path: &Path, header: &str, domains: &[String]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AdBlockerError::Io {
            path: parent.display().to_string(),
            source: e,
        })?;
    }
    let mut content = format!("# {header} count={}\n", domains.len());
    for d in domains {
        content.push_str(d);
        content.push('\n');
    }
    std::fs::write(path, content).map_err(|e| AdBlockerError::Io {
        path: path.display().to_string(),
        source: e,
    })
}

/// Write the merged domain list to `path`.
///
/// Format: a header comment line followed by one normalized domain per line,
/// sorted. Human-inspectable and trivial to re-read.
pub fn write_cache_to(path: &Path, domains: &[String]) -> Result<()> {
    write_domain_list(path, "adblocker cache v1", domains)
}

/// Save the last good copy of the source at `url`.
pub fn write_snapshot(path: &Path, url: &str, domains: &[String]) -> Result<()> {
    write_domain_list(path, &format!("adblocker source v1 url={url}"), domains)
}

/// Read a domain list (cache or snapshot) from `path`. Returns
/// [`AdBlockerError::EmptyBlocklist`] if it holds no domains.
pub fn read_cache_from(path: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path).map_err(|e| AdBlockerError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    let domains: Vec<String> = content
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(ToString::to_string)
        .collect();
    if domains.is_empty() {
        return Err(AdBlockerError::EmptyBlocklist);
    }
    Ok(domains)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_has_stevenblack_source() {
        let cfg = Config::default();
        assert_eq!(cfg.sources, vec![DEFAULT_SOURCE.to_string()]);
        assert_eq!(cfg.allowlist, Vec::<String>::new());
        assert_eq!(cfg.custom_blocks, Vec::<String>::new());
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
    fn load_rejects_non_https_source() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "sources = [\"http://example.com/hosts\"]\n").unwrap();
        let err = Config::load_from(&path).unwrap_err();
        assert!(err.to_string().contains("https://"), "{err}");
    }

    #[test]
    fn source_host_extracts_host() {
        assert_eq!(
            source_host(DEFAULT_SOURCE),
            Some("raw.githubusercontent.com")
        );
        assert_eq!(
            source_host("HTTPS://user@lists.example.com:8443/h?x=1"),
            Some("lists.example.com")
        );
        assert_eq!(source_host("http://example.com/hosts"), None);
        assert_eq!(source_host("https:///hosts"), None);
        assert_eq!(source_host("ftp"), None);
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
    fn remove_allow_and_block() {
        let mut cfg = Config::default();
        cfg.add_allow("good.com").unwrap();
        cfg.add_block("bad.com").unwrap();
        cfg.remove_allow("GOOD.com").unwrap();
        cfg.remove_block("bad.com.").unwrap();
        assert_eq!(cfg.allowlist, Vec::<String>::new());
        assert_eq!(cfg.custom_blocks, Vec::<String>::new());
    }

    #[test]
    fn remove_missing_entry_errors() {
        let mut cfg = Config::default();
        let err = cfg.remove_allow("good.com").unwrap_err();
        assert!(err.to_string().contains("not in your allowlist"), "{err}");
    }

    #[test]
    fn snapshot_name_is_stable_per_url() {
        let paths = Paths::new(PathBuf::from("app"), PathBuf::from("hosts"));
        let a = paths.source_snapshot(DEFAULT_SOURCE);
        assert_eq!(a, paths.source_snapshot(DEFAULT_SOURCE));
        assert_ne!(a, paths.source_snapshot("https://example.com/hosts"));
        // Pinned so a toolchain or refactor change can't silently orphan
        // existing snapshots.
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn snapshot_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sources").join("x.list");
        let domains = vec!["a.com".to_string()];
        write_snapshot(&path, DEFAULT_SOURCE, &domains).unwrap();
        assert_eq!(read_cache_from(&path).unwrap(), domains);
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn home_from_passwd_finds_user() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\nbrian:x:1000:1000::/home/brian:/bin/zsh\n";
        assert_eq!(
            home_from_passwd(passwd, "brian"),
            Some(PathBuf::from("/home/brian"))
        );
        assert_eq!(home_from_passwd(passwd, "nobody"), None);
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
