//! `adblocker` — manage `/etc/hosts` ad blocking on macOS.
//!
//! Public API surface is re-exported here; each module owns one job. The
//! merge algorithm and command orchestration live in this file so `main.rs`
//! stays a thin wrapper.

use std::collections::BTreeSet;
use std::path::Path;

pub mod cli;
pub mod config;
pub mod error;
pub mod fetch;
pub mod hosts;
pub mod validate;

pub use error::{AdBlockerError, Result};

/// The merged, final blocklist plus provenance counts for reporting.
#[derive(Debug, Clone)]
pub struct Blocklist {
    pub domains: Vec<String>,
    pub total_sources: usize,
    pub total_accepted: usize,
    pub total_rejected: usize,
    pub total_after_merge: usize,
}

/// A report returned by [`run_status`].
#[derive(Debug, Clone)]
pub struct StatusReport {
    pub enabled: bool,
    pub domains_blocked: usize,
    pub cache_present: bool,
    pub cache_domain_count: Option<usize>,
    pub sources: Vec<String>,
}

/// Merge parsed source lists with custom blocks, then subtract the allowlist
/// and the built-in protected domains.
///
/// Order: union sources → add custom blocks → remove allowlist → remove
/// protected (always last, cannot be un-protected) → reject if empty.
pub fn merge_blocklist(
    parsed: &[validate::ParsedList],
    custom_blocks: &[String],
    allowlist: &[String],
) -> Result<Blocklist> {
    let mut all: BTreeSet<String> = BTreeSet::new();
    let mut total_accepted = 0usize;
    let mut total_rejected = 0usize;

    for list in parsed {
        for d in &list.domains {
            all.insert(d.clone());
        }
        total_accepted += list.stats.accepted;
        total_rejected += list.stats.rejected_blank
            + list.stats.rejected_comment
            + list.stats.rejected_bad_ip
            + list.stats.rejected_bad_domain
            + list.stats.rejected_junk
            + list.stats.rejected_duplicate;
    }

    for d in custom_blocks {
        let n = validate::normalize_domain(d)?;
        all.insert(n);
    }

    for d in allowlist {
        let n = validate::normalize_domain(d)?;
        all.remove(&n);
    }

    for &p in validate::PROTECTED_DOMAINS {
        all.remove(p);
    }

    if all.is_empty() {
        return Err(AdBlockerError::EmptyBlocklist);
    }

    let total_after_merge = all.len();
    Ok(Blocklist {
        domains: all.into_iter().collect(),
        total_sources: parsed.len(),
        total_accepted,
        total_rejected,
        total_after_merge,
    })
}

/// Run `update`: fetch every configured source, parse, merge, and cache.
/// Runs as the normal user — no sudo.
pub fn run_update() -> Result<Blocklist> {
    let cfg = config::Config::load()?;
    let mut parsed: Vec<validate::ParsedList> = Vec::with_capacity(cfg.sources.len());

    for url in &cfg.sources {
        let body = fetch::fetch_url(url)?;
        let list = validate::parse_hosts_text(&body)?;
        if list.domains.is_empty() {
            return Err(AdBlockerError::DownloadOrValidationFailed);
        }
        parsed.push(list);
    }

    let merged = merge_blocklist(&parsed, &cfg.custom_blocks, &cfg.allowlist)?;
    config::write_cache(&merged.domains)?;
    Ok(merged)
}

/// Run `apply`: re-read the cache and write it into the hosts file at
/// `hosts_path`. Requires root.
pub fn run_apply(hosts_path: &Path) -> Result<Blocklist> {
    require_root("apply")?;
    let domains = config::read_cache()?;

    // Re-validate the cached domains before writing them.
    let mut set = BTreeSet::new();
    for d in &domains {
        let n = validate::normalize_domain(d)?;
        if validate::is_protected(&n) {
            return Err(AdBlockerError::ProtectedDomain { domain: n });
        }
        set.insert(n);
    }
    if set.is_empty() {
        return Err(AdBlockerError::EmptyBlocklist);
    }

    let sections =
        match hosts::read_hosts(hosts_path)? {
            hosts::MarkerState::Absent => hosts::sections_from_unmarked(
                &std::fs::read_to_string(hosts_path).map_err(|e| AdBlockerError::Io {
                    path: hosts_path.display().to_string(),
                    source: e,
                })?,
            ),
            hosts::MarkerState::Present(s) => s,
        };

    hosts::ensure_backup(hosts_path)?;
    let content = hosts::build_hosts_content(&sections, &set.into_iter().collect::<Vec<_>>());
    hosts::write_hosts_atomic(hosts_path, &content)?;

    Ok(Blocklist {
        domains: domains.clone(),
        total_sources: 0,
        total_accepted: 0,
        total_rejected: 0,
        total_after_merge: domains.len(),
    })
}

/// Run `disable`: remove the marked block from the hosts file. Requires root.
pub fn run_disable(hosts_path: &Path) -> Result<()> {
    require_root("disable")?;
    hosts::remove_block(hosts_path)
}

/// Run `status`: report on/off, domain count, cache state, and sources.
pub fn run_status(hosts_path: &Path) -> Result<StatusReport> {
    let cfg = config::Config::load()?;
    let enabled = matches!(
        hosts::read_hosts(hosts_path)?,
        hosts::MarkerState::Present(_)
    );

    let cache = config::read_cache();
    let (cache_present, cache_domain_count) = match cache {
        Ok(d) => (true, Some(d.len())),
        Err(AdBlockerError::Io { .. }) | Err(AdBlockerError::EmptyBlocklist) => (false, None),
        Err(e) => return Err(e),
    };

    Ok(StatusReport {
        enabled,
        domains_blocked: cache_domain_count.unwrap_or(0),
        cache_present,
        cache_domain_count,
        sources: cfg.sources,
    })
}

/// Run `allow <domain>`: add to the allowlist and save config.
pub fn run_allow(domain: &str) -> Result<()> {
    let mut cfg = config::Config::load()?;
    cfg.add_allow(domain)?;
    cfg.save()
}

/// Run `block <domain>`: add to custom blocks and save config.
pub fn run_block(domain: &str) -> Result<()> {
    let mut cfg = config::Config::load()?;
    cfg.add_block(domain)?;
    cfg.save()
}

/// Refuse to continue unless the process is running as root (euid 0).
/// Uses `id -u` rather than pulling in a `libc` dependency.
fn require_root(command: &str) -> Result<()> {
    let out = std::process::Command::new("id")
        .arg("-u")
        .output()
        .map_err(|_| AdBlockerError::NotRoot {
            command: command.to_string(),
        })?;
    if std::str::from_utf8(&out.stdout).unwrap_or("").trim() != "0" {
        return Err(AdBlockerError::NotRoot {
            command: command.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use validate::ParsedList;

    fn parsed(domains: &[&str]) -> ParsedList {
        let mut list = ParsedList::default();
        for d in domains {
            list.domains.insert(d.to_string());
            list.stats.accepted += 1;
        }
        list
    }

    #[test]
    fn merge_unions_sources() {
        let lists = vec![parsed(&["a.com", "b.com"]), parsed(&["b.com", "c.com"])];
        let m = merge_blocklist(&lists, &[], &[]).unwrap();
        assert_eq!(m.domains, vec!["a.com", "b.com", "c.com"]);
        assert_eq!(m.total_after_merge, 3);
    }

    #[test]
    fn merge_adds_custom_blocks() {
        let lists = vec![parsed(&["a.com"])];
        let m = merge_blocklist(&lists, &["custom.com".to_string()], &[]).unwrap();
        assert!(m.domains.contains(&"custom.com".to_string()));
    }

    #[test]
    fn merge_removes_allowlist() {
        let lists = vec![parsed(&["a.com", "b.com"])];
        let m = merge_blocklist(&lists, &[], &["a.com".to_string()]).unwrap();
        assert!(!m.domains.contains(&"a.com".to_string()));
        assert!(m.domains.contains(&"b.com".to_string()));
    }

    #[test]
    fn merge_removes_protected_unconditionally() {
        // Even though swscan.apple.com appears in the source and is NOT in
        // the allowlist, it must be stripped.
        let lists = vec![parsed(&["swscan.apple.com", "evil.com"])];
        let m = merge_blocklist(&lists, &[], &[]).unwrap();
        assert!(!m.domains.contains(&"swscan.apple.com".to_string()));
        assert!(m.domains.contains(&"evil.com".to_string()));
    }

    #[test]
    fn merge_protected_not_unprotectable_via_custom_block() {
        let lists = vec![parsed(&["evil.com"])];
        // A user trying to block a protected domain via custom_blocks fails:
        // protected removal runs last.
        let m = merge_blocklist(&lists, &["albert.apple.com".to_string()], &[]).unwrap();
        assert!(!m.domains.contains(&"albert.apple.com".to_string()));
    }

    #[test]
    fn merge_rejects_empty() {
        let lists = vec![parsed(&[])];
        assert!(matches!(
            merge_blocklist(&lists, &[], &[]).unwrap_err(),
            AdBlockerError::EmptyBlocklist
        ));
    }

    #[test]
    fn merge_output_is_sorted() {
        let lists = vec![parsed(&["z.com", "a.com", "m.com"])];
        let m = merge_blocklist(&lists, &[], &[]).unwrap();
        assert_eq!(m.domains, vec!["a.com", "m.com", "z.com"]);
    }

    #[test]
    fn merge_rejects_invalid_custom_block() {
        let lists = vec![parsed(&["evil.com"])];
        assert!(merge_blocklist(&lists, &["bad domain".to_string()], &[]).is_err());
    }

    #[test]
    fn btree_set_dedups_and_sorts() {
        // Sanity check the underlying structure behaviour.
        let mut s = BTreeSet::new();
        for d in ["c.com", "a.com", "a.com", "b.com"] {
            s.insert(d.to_string());
        }
        assert_eq!(
            s.into_iter().collect::<Vec<_>>(),
            vec!["a.com", "b.com", "c.com"]
        );
    }
}
