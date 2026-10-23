//! `adblocker` — manage hosts-file ad blocking on macOS, Windows, and Linux.
//!
//! Public API surface is re-exported here; each module owns one job. The
//! merge algorithm and command orchestration live in this file so `main.rs`
//! stays a thin wrapper.
//!
//! Commands that touch the hosts file come in two layers: `run_*` checks
//! privileges and flushes DNS; the `*_no_flush` / `*_with` function under it
//! does the work and is what tests call against temp files.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

pub mod cli;
pub mod config;
pub mod error;
pub mod fetch;
pub mod hosts;
pub mod validate;

pub use config::Paths;
pub use error::{AdBlockerError, Result};

use config::Config;

/// The merged, final blocklist plus provenance counts for reporting.
#[derive(Debug, Clone)]
pub struct Blocklist {
    pub domains: Vec<String>,
    pub total_sources: usize,
    pub total_rejected: usize,
}

/// A source that could not be refreshed, so its last good copy was used.
#[derive(Debug, Clone)]
pub struct StaleSource {
    pub url: String,
    pub error: String,
}

/// A report returned by [`run_update`].
#[derive(Debug, Clone)]
pub struct UpdateReport {
    pub blocklist: Blocklist,
    pub stale_sources: Vec<StaleSource>,
}

/// A report returned by [`run_apply`].
#[derive(Debug, Clone)]
pub struct ApplyReport {
    pub applied: usize,
    /// Cached domains dropped because they are protected (e.g. a cache built
    /// before a protected entry was added).
    pub skipped_protected: usize,
}

/// A report returned by [`run_status`].
#[derive(Debug, Clone)]
pub struct StatusReport {
    pub enabled: bool,
    /// Entries currently between the markers in the hosts file.
    pub hosts_domain_count: usize,
    /// `None` when there is no usable cache yet.
    pub cache_domain_count: Option<usize>,
    pub sources: Vec<String>,
    /// When a source was last downloaded successfully.
    pub last_update: Option<SystemTime>,
}

/// What happened to the cache after a config change (`allow`, `block`, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rebuild {
    /// Rebuilt offline from source snapshots; holds the new domain count.
    Rebuilt(usize),
    /// Some source has no snapshot yet, so `update` must run first.
    NeedsUpdate,
}

/// Everything known about one domain, returned by [`run_check`].
#[derive(Debug, Clone)]
pub struct CheckReport {
    pub domain: String,
    pub blocked_now: bool,
    pub in_cache: bool,
    pub in_sources: Vec<String>,
    pub custom_block: bool,
    /// The allowlist entry (the domain itself or a parent) that unblocks it.
    pub allowed_by: Option<String>,
    /// The configured source host it matches; those are never blocked.
    pub source_host: Option<String>,
    /// The protected entry (the domain itself or a parent) that covers it.
    pub protected_by: Option<&'static str>,
}

/// What `uninstall` removes (or would remove, without `--yes`).
#[derive(Debug, Clone, Default)]
pub struct UninstallReport {
    pub block_present: bool,
    pub backup: Option<PathBuf>,
    pub app_files: Vec<PathBuf>,
    pub done: bool,
}

/// Merge parsed source lists with custom blocks, then subtract the allowlist
/// and the built-in protected domains.
///
/// Order: union sources → add custom blocks → remove allowlist entries and
/// their subdomains → remove protected (always last, cannot be
/// un-protected) → reject if empty.
pub fn merge_blocklist(
    parsed: &[validate::ParsedList],
    custom_blocks: &[String],
    allowlist: &[String],
) -> Result<Blocklist> {
    let mut all: BTreeSet<String> = BTreeSet::new();
    let mut total_rejected = 0usize;

    for list in parsed {
        for d in &list.domains {
            all.insert(d.clone());
        }
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

    let allow = allowlist
        .iter()
        .map(|d| validate::normalize_domain(d))
        .collect::<Result<Vec<_>>>()?;
    all.retain(|d| !allow.iter().any(|a| validate::is_same_or_subdomain(d, a)));

    all.retain(|d| !validate::is_protected(d));

    if all.is_empty() {
        return Err(AdBlockerError::EmptyBlocklist);
    }

    Ok(Blocklist {
        domains: all.into_iter().collect(),
        total_sources: parsed.len(),
        total_rejected,
    })
}

/// The normalized hosts of every configured source.
fn source_hosts(cfg: &Config) -> Vec<String> {
    cfg.sources
        .iter()
        .filter_map(|url| config::source_host(url))
        .filter_map(|host| validate::normalize_domain(host).ok())
        .collect()
}

/// [`merge_blocklist`] with the config's lists, never blocking a host we
/// fetch from (or the next `update` would fail).
fn merge_with_config(cfg: &Config, parsed: &[validate::ParsedList]) -> Result<Blocklist> {
    let mut allowlist = cfg.allowlist.clone();
    allowlist.extend(source_hosts(cfg));
    merge_blocklist(parsed, &cfg.custom_blocks, &allowlist)
}

/// Load a source snapshot as a parsed list.
fn read_snapshot(path: &Path) -> Result<validate::ParsedList> {
    let domains: BTreeSet<String> = config::read_cache_from(path)?.into_iter().collect();
    let stats = validate::ParseStats {
        accepted: domains.len(),
        ..Default::default()
    };
    Ok(validate::ParsedList { domains, stats })
}

/// Run `update`: fetch every configured source, parse, merge, and cache.
/// Runs as the normal user — no sudo.
pub fn run_update(paths: &Paths, force: bool) -> Result<UpdateReport> {
    run_update_with(paths, force, fetch::fetch_url)
}

/// [`run_update`] with an injectable fetcher, so tests run offline.
///
/// A source that fails falls back to its last good snapshot (reported in
/// [`UpdateReport::stale_sources`]); if every source fails, the first error
/// is returned. Unless `force`, a result less than half the size of the
/// current cache is refused, since that usually means a broken upstream
/// list. Snapshots are only replaced once the new result is accepted.
pub fn run_update_with(
    paths: &Paths,
    force: bool,
    fetch: impl Fn(&str) -> Result<String>,
) -> Result<UpdateReport> {
    let cfg = Config::load_from(&paths.config())?;
    let mut parsed = Vec::with_capacity(cfg.sources.len());
    let mut fresh = Vec::new();
    let mut stale_sources = Vec::new();
    let mut failures = Vec::new();

    for url in &cfg.sources {
        let snapshot = paths.source_snapshot(url);
        let downloaded = fetch(url).and_then(|body| {
            let list = validate::parse_hosts_text(&body)?;
            if list.domains.is_empty() {
                return Err(AdBlockerError::DownloadOrValidationFailed { url: url.clone() });
            }
            Ok(list)
        });
        match downloaded {
            Ok(list) => {
                fresh.push((
                    snapshot,
                    url,
                    list.domains.iter().cloned().collect::<Vec<_>>(),
                ));
                parsed.push(list);
            }
            Err(e) => match read_snapshot(&snapshot) {
                Ok(list) => {
                    stale_sources.push(StaleSource {
                        url: url.clone(),
                        error: e.to_string(),
                    });
                    failures.push(e);
                    parsed.push(list);
                }
                Err(_) => return Err(e),
            },
        }
    }

    if fresh.is_empty() && !failures.is_empty() {
        // Nothing new was downloaded; surface the failure instead of
        // rewriting the same cache.
        return Err(failures.swap_remove(0));
    }

    let merged = merge_with_config(&cfg, &parsed)?;

    if !force && let Ok(previous) = config::read_cache_from(&paths.cache()) {
        let new = merged.domains.len();
        if new * 2 < previous.len() {
            return Err(AdBlockerError::SuspiciousShrink {
                previous: previous.len(),
                new,
            });
        }
    }

    for (snapshot, url, domains) in &fresh {
        config::write_snapshot(snapshot, url, domains)?;
    }
    config::write_cache_to(&paths.cache(), &merged.domains)?;
    Ok(UpdateReport {
        blocklist: merged,
        stale_sources,
    })
}

/// Rebuild the cache from source snapshots without downloading anything, so
/// allowlist and custom-block changes only need an `apply`.
pub fn rebuild_cache(paths: &Paths) -> Result<Rebuild> {
    let cfg = Config::load_from(&paths.config())?;
    let mut parsed = Vec::with_capacity(cfg.sources.len());
    for url in &cfg.sources {
        match read_snapshot(&paths.source_snapshot(url)) {
            Ok(list) => parsed.push(list),
            Err(_) => return Ok(Rebuild::NeedsUpdate),
        }
    }
    let merged = merge_with_config(&cfg, &parsed)?;
    config::write_cache_to(&paths.cache(), &merged.domains)?;
    Ok(Rebuild::Rebuilt(merged.domains.len()))
}

/// The hosts file split into sections; a file without markers becomes all
/// `before`.
fn current_sections(hosts_path: &Path) -> Result<hosts::HostsSections> {
    match hosts::read_hosts(hosts_path)? {
        hosts::MarkerState::Absent(s) | hosts::MarkerState::Present(s) => Ok(s),
    }
}

/// Run `apply`: re-read the cache and write it into the hosts file. Requires
/// root (macOS/Linux) or Administrator (Windows).
pub fn run_apply(paths: &Paths) -> Result<ApplyReport> {
    require_root("apply", &paths.hosts)?;
    let report = apply_no_flush(paths)?;
    hosts::flush_dns();
    Ok(report)
}

/// The work of `apply`, without the privilege check or DNS flush.
///
/// Re-validates every cached domain before writing it, and drops protected
/// ones, so a corrupted or outdated cache can never reach the hosts file.
pub fn apply_no_flush(paths: &Paths) -> Result<ApplyReport> {
    let cached = config::read_cache_from(&paths.cache())?;
    let mut set = BTreeSet::new();
    let mut skipped_protected = 0;
    for d in &cached {
        let n = validate::normalize_domain(d)?;
        if validate::is_protected(&n) {
            skipped_protected += 1;
        } else {
            set.insert(n);
        }
    }
    if set.is_empty() {
        return Err(AdBlockerError::EmptyBlocklist);
    }

    let sections = current_sections(&paths.hosts)?;
    hosts::backup(&paths.hosts)?;
    let domains: Vec<String> = set.into_iter().collect();
    let content = hosts::build_hosts_content(&sections, &domains);
    hosts::write_hosts_atomic(&paths.hosts, &content)?;

    Ok(ApplyReport {
        applied: domains.len(),
        skipped_protected,
    })
}

/// Run `disable`: remove the marked block from the hosts file. Requires root
/// (macOS/Linux) or Administrator (Windows).
pub fn run_disable(paths: &Paths) -> Result<()> {
    require_root("disable", &paths.hosts)?;
    disable_no_flush(paths)?;
    hosts::flush_dns();
    Ok(())
}

/// The work of `disable`, without the privilege check or DNS flush.
pub fn disable_no_flush(paths: &Paths) -> Result<()> {
    if matches!(
        hosts::read_hosts(&paths.hosts)?,
        hosts::MarkerState::Absent(_)
    ) {
        return Err(AdBlockerError::NotEnabled("disable"));
    }
    hosts::backup(&paths.hosts)?;
    hosts::remove_block(&paths.hosts)
}

/// Run `pause`: remove the block, wait `duration` (or until `stop` is set,
/// e.g. by Ctrl-C), then re-apply. Requires root or Administrator.
pub fn run_pause(paths: &Paths, duration: Duration, stop: &AtomicBool) -> Result<ApplyReport> {
    require_root("pause", &paths.hosts)?;
    pause_with(paths, duration, stop, hosts::flush_dns)
}

/// The work of `pause`, with the DNS flush injectable for tests. Blocking is
/// re-applied however the wait ends; the caller should only report the
/// result after this returns, since the terminal may already be gone.
pub fn pause_with(
    paths: &Paths,
    duration: Duration,
    stop: &AtomicBool,
    flush: impl Fn(),
) -> Result<ApplyReport> {
    if matches!(
        hosts::read_hosts(&paths.hosts)?,
        hosts::MarkerState::Absent(_)
    ) {
        return Err(AdBlockerError::NotEnabled("pause"));
    }
    // Make sure there is something to re-apply before turning blocking off.
    config::read_cache_from(&paths.cache())?;

    disable_no_flush(paths)?;
    flush();
    let deadline = Instant::now() + duration;
    while !stop.load(Ordering::SeqCst) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        std::thread::sleep(left.min(Duration::from_millis(200)));
    }
    let report = apply_no_flush(paths)?;
    flush();
    Ok(report)
}

/// Run `restore`: swap the hosts file with its backup, undoing the most
/// recent change (running it again redoes it). Requires root or
/// Administrator.
pub fn run_restore(paths: &Paths) -> Result<()> {
    require_root("restore", &paths.hosts)?;
    restore_no_flush(paths)?;
    hosts::flush_dns();
    Ok(())
}

/// The work of `restore`, without the privilege check or DNS flush.
pub fn restore_no_flush(paths: &Paths) -> Result<()> {
    let backup = paths.backup();
    let previous = match std::fs::read_to_string(&backup) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(AdBlockerError::NoBackup {
                path: backup.display().to_string(),
            });
        }
        Err(e) => {
            return Err(AdBlockerError::Io {
                path: backup.display().to_string(),
                source: e,
            });
        }
    };
    // Refuse a symlinked or unreadable hosts file like every other write
    // path. Broken markers are fine: repairing them is what restore is for.
    if let Err(e @ (AdBlockerError::HostsIsSymlink | AdBlockerError::Io { .. })) =
        hosts::read_hosts(&paths.hosts)
    {
        return Err(e);
    }
    hosts::backup(&paths.hosts)?;
    hosts::write_hosts_atomic(&paths.hosts, &previous)
}

/// Run `status`: report on/off, domain counts, cache state, last update, and
/// sources.
pub fn run_status(paths: &Paths) -> Result<StatusReport> {
    let cfg = Config::load_from(&paths.config())?;
    let enabled = matches!(
        hosts::read_hosts(&paths.hosts)?,
        hosts::MarkerState::Present(_)
    );

    let cache_domain_count = match config::read_cache_from(&paths.cache()) {
        Ok(d) => Some(d.len()),
        Err(AdBlockerError::Io { .. } | AdBlockerError::EmptyBlocklist) => None,
        Err(e) => return Err(e),
    };

    // Snapshots are only written by a successful download. Caches from
    // before snapshots existed fall back to the cache's own timestamp.
    let modified = |p: PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let last_update = cfg
        .sources
        .iter()
        .filter_map(|url| modified(paths.source_snapshot(url)))
        .max()
        .or_else(|| modified(paths.cache()));

    Ok(StatusReport {
        enabled,
        hosts_domain_count: hosts::count_blocked_in_hosts(&paths.hosts),
        cache_domain_count,
        sources: cfg.sources,
        last_update,
    })
}

/// Run `check <domain>`: explain whether and why a domain is blocked.
pub fn run_check(paths: &Paths, domain: &str) -> Result<CheckReport> {
    let domain = validate::normalize_domain(domain)?;
    let cfg = Config::load_from(&paths.config())?;
    let contains = |path: PathBuf| {
        config::read_cache_from(&path).is_ok_and(|list| list.iter().any(|d| d == &domain))
    };
    let covering = |entries: &[String]| {
        entries
            .iter()
            .find(|e| validate::is_same_or_subdomain(&domain, e))
            .cloned()
    };

    Ok(CheckReport {
        blocked_now: hosts::is_blocked_in_hosts(&paths.hosts, &domain),
        in_cache: contains(paths.cache()),
        in_sources: cfg
            .sources
            .iter()
            .filter(|url| contains(paths.source_snapshot(url)))
            .cloned()
            .collect(),
        custom_block: cfg.custom_blocks.contains(&domain),
        allowed_by: covering(&cfg.allowlist),
        source_host: covering(&source_hosts(&cfg)),
        protected_by: validate::PROTECTED_DOMAINS
            .iter()
            .find(|&&p| validate::is_same_or_subdomain(&domain, p))
            .copied(),
        domain,
    })
}

/// Run `list`: the current configuration.
pub fn run_list(paths: &Paths) -> Result<Config> {
    Config::load_from(&paths.config())
}

/// Edit the config with `change`, save it, and rebuild the cache offline.
fn edit_config(paths: &Paths, change: impl FnOnce(&mut Config) -> Result<()>) -> Result<Rebuild> {
    let mut cfg = Config::load_from(&paths.config())?;
    change(&mut cfg)?;
    cfg.save_to(&paths.config())?;
    rebuild_cache(paths)
}

/// Run `allow <domain>`: allowlist a domain (and its subdomains).
pub fn run_allow(paths: &Paths, domain: &str) -> Result<Rebuild> {
    edit_config(paths, |cfg| cfg.add_allow(domain))
}

/// Run `block <domain>`: add a custom block.
pub fn run_block(paths: &Paths, domain: &str) -> Result<Rebuild> {
    edit_config(paths, |cfg| cfg.add_block(domain))
}

/// Run `unallow <domain>`: remove an allowlist entry.
pub fn run_unallow(paths: &Paths, domain: &str) -> Result<Rebuild> {
    edit_config(paths, |cfg| cfg.remove_allow(domain))
}

/// Run `unblock <domain>`: remove a custom block.
pub fn run_unblock(paths: &Paths, domain: &str) -> Result<Rebuild> {
    edit_config(paths, |cfg| cfg.remove_block(domain))
}

/// Files in the app directory that belong to this tool. `uninstall` deletes
/// only these (never the whole directory), so a mistaken `ADBLOCKER_DIR`
/// can't take anything else with it.
const APP_FILES: &[&str] = &[
    "config.toml",
    "blocklist.cache",
    "refresh.log",
    "refresh-and-apply.sh",
    "refresh-and-apply.ps1",
];

/// Run `uninstall`: without `yes`, report what would be removed; with it,
/// remove the block, the backup, and the tool's files. Requires root or
/// Administrator when anything in the hosts directory must go.
pub fn run_uninstall(paths: &Paths, yes: bool) -> Result<UninstallReport> {
    let report = uninstall_no_flush(paths, false)?;
    if !yes {
        return Ok(report);
    }
    if report.block_present || report.backup.is_some() {
        require_root("uninstall", &paths.hosts)?;
    }
    let report = uninstall_no_flush(paths, true)?;
    if report.block_present {
        hosts::flush_dns();
    }
    Ok(report)
}

/// The work of `uninstall`, without the privilege check or DNS flush.
pub fn uninstall_no_flush(paths: &Paths, yes: bool) -> Result<UninstallReport> {
    let block_present = matches!(
        hosts::read_hosts(&paths.hosts)?,
        hosts::MarkerState::Present(_)
    );
    let backup = Some(paths.backup()).filter(|p| p.exists());
    let mut app_files: Vec<PathBuf> = APP_FILES
        .iter()
        .map(|f| paths.app_dir.join(f))
        .filter(|p| p.exists())
        .collect();
    if let Ok(entries) = std::fs::read_dir(paths.sources_dir()) {
        app_files.extend(
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "list")),
        );
    }

    if yes {
        if block_present {
            hosts::remove_block(&paths.hosts)?;
        }
        for file in backup.iter().chain(&app_files) {
            std::fs::remove_file(file).map_err(|e| AdBlockerError::Io {
                path: file.display().to_string(),
                source: e,
            })?;
        }
        // Only succeeds if nothing else is in there, which is the point.
        let _ = std::fs::remove_dir(paths.sources_dir());
        let _ = std::fs::remove_dir(&paths.app_dir);
    }

    Ok(UninstallReport {
        block_present,
        backup,
        app_files,
        done: yes,
    })
}

/// Refuse to continue unless the process is running as root (euid 0).
/// Uses `id -u` rather than pulling in a `libc` dependency.
#[cfg(not(windows))]
pub fn require_root(command: &str, _hosts_path: &Path) -> Result<()> {
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

/// Refuse to continue unless the process can write the hosts file, which on
/// Windows means running elevated. Opening for append without writing tests
/// exactly the permission we need, with no `unsafe` Win32 token calls.
#[cfg(windows)]
pub fn require_root(command: &str, hosts_path: &Path) -> Result<()> {
    match std::fs::OpenOptions::new().append(true).open(hosts_path) {
        Ok(_) => Ok(()),
        // A read-only attribute also denies access even when elevated; report
        // that as the I/O error it is rather than as missing privileges.
        Err(e)
            if e.kind() == std::io::ErrorKind::PermissionDenied
                && !std::fs::metadata(hosts_path).is_ok_and(|m| m.permissions().readonly()) =>
        {
            Err(AdBlockerError::NotRoot {
                command: command.to_string(),
            })
        }
        Err(e) => Err(AdBlockerError::Io {
            path: hosts_path.display().to_string(),
            source: e,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use validate::ParsedList;

    fn parsed(domains: &[&str]) -> ParsedList {
        let mut list = ParsedList::default();
        for d in domains {
            list.domains.insert((*d).to_string());
            list.stats.accepted += 1;
        }
        list
    }

    #[test]
    fn merge_unions_sources() {
        let lists = vec![parsed(&["a.com", "b.com"]), parsed(&["b.com", "c.com"])];
        let m = merge_blocklist(&lists, &[], &[]).unwrap();
        assert_eq!(m.domains, vec!["a.com", "b.com", "c.com"]);
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
    fn merge_allowlist_covers_subdomains_only() {
        let lists = vec![parsed(&[
            "cdn.a.com",
            "a.com",
            "nota.com",
            "a.com.evil.net",
        ])];
        let m = merge_blocklist(&lists, &[], &["a.com".to_string()]).unwrap();
        assert_eq!(m.domains, vec!["a.com.evil.net", "nota.com"]);
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
}
