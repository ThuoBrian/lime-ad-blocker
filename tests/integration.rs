//! Integration tests: exercise the library against fixture files and temp
//! hosts files. These never touch the real hosts file, the network, or the
//! user's app directory, and never need sudo.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use adblocker::config;
use adblocker::hosts::{self, BEGIN_MARKER, END_MARKER, MarkerState};
use adblocker::validate::{self, parse_hosts_text};
use adblocker::{AdBlockerError, Paths, Rebuild, merge_blocklist};

/// Path to a fixture file.
fn fixture(name: &str) -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push(name);
    p
}

fn read_fixture(name: &str) -> String {
    fs::read_to_string(fixture(name)).unwrap_or_else(|e| panic!("read fixture {name}: {e}"))
}

/// Create a temp hosts file with the given content and return its path.
fn make_hosts(content: &str) -> (tempfile::TempPath, std::path::PathBuf) {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let f = f.into_temp_path();
    let path = f.to_path_buf();
    (f, path)
}

// --- parsing fixtures ---

#[test]
fn fixture_good_list_parses_cleanly() {
    let text = read_fixture("good_list.txt");
    let list = parse_hosts_text(&text).unwrap();
    assert_eq!(list.stats.accepted, 10);
    assert_eq!(list.stats.rejected_bad_ip, 0);
    assert_eq!(list.stats.rejected_bad_domain, 0);
}

#[test]
fn fixture_bad_ips_rejected() {
    let text = read_fixture("bad_ips.txt");
    let list = parse_hosts_text(&text).unwrap();
    // Only the one 0.0.0.0 line is accepted; the other four have bad IPs.
    assert_eq!(list.stats.accepted, 1);
    assert_eq!(list.stats.rejected_bad_ip, 4);
    assert!(list.domains.contains("good.example.com"));
}

#[test]
fn fixture_bad_domains_rejected() {
    let text = read_fixture("bad_domains.txt");
    let list = parse_hosts_text(&text).unwrap();
    assert_eq!(list.stats.accepted, 0);
    assert!(list.stats.rejected_bad_domain >= 6);
}

#[test]
fn fixture_comments_only_yields_nothing() {
    let text = read_fixture("comments_only.txt");
    let list = parse_hosts_text(&text).unwrap();
    assert_eq!(list.stats.accepted, 0);
    assert!(list.stats.rejected_comment >= 3);
    assert!(list.stats.rejected_blank >= 2);
}

#[test]
fn fixture_crlf_parses() {
    let text = read_fixture("crlf.txt");
    let list = parse_hosts_text(&text).unwrap();
    assert_eq!(list.stats.accepted, 3);
    assert!(list.domains.contains("crlf1.example.com"));
}

#[test]
fn fixture_mixed_counts() {
    let text = read_fixture("mixed.txt");
    let list = parse_hosts_text(&text).unwrap();
    // good1, good2 accepted; good1 duplicate counted; bad ip + bad domain +
    // junk (0.0.0.0) rejected; comments and blanks counted.
    assert_eq!(list.stats.accepted, 2);
    assert_eq!(list.stats.rejected_duplicate, 1);
    assert!(list.domains.contains("good1.example.com"));
    assert!(list.domains.contains("good2.example.com"));
}

#[test]
fn fixture_stevenblack_sample() {
    let text = read_fixture("stevenblack_sample.txt");
    let list = parse_hosts_text(&text).unwrap();
    // localhost.localdomain + the four real ad domains (one with an inline
    // comment) = 5 accepted.
    assert_eq!(list.stats.accepted, 5);
    assert!(list.domains.contains("ad-assets.futurecdn.net"));
    assert!(list.domains.contains("analytics.example.com"));
}

// --- marker / broken-marker fixtures ---

#[test]
fn fixture_broken_missing_end() {
    let text = read_fixture("broken_missing_end.txt");
    let (_f, path) = make_hosts(&text);
    let err = hosts::read_hosts(&path).unwrap_err();
    assert!(err.to_string().contains("no END marker"), "{err}");
}

#[test]
fn fixture_broken_duplicate_begin() {
    let text = read_fixture("broken_duplicate_begin.txt");
    let (_f, path) = make_hosts(&text);
    let err = hosts::read_hosts(&path).unwrap_err();
    assert!(err.to_string().contains("2 BEGIN markers"), "{err}");
}

#[test]
fn fixture_broken_duplicate_end() {
    let text = read_fixture("broken_duplicate_end.txt");
    let (_f, path) = make_hosts(&text);
    let err = hosts::read_hosts(&path).unwrap_err();
    assert!(err.to_string().contains("2 END markers"), "{err}");
}

#[test]
fn fixture_broken_end_before_begin() {
    let text = read_fixture("broken_end_before_begin.txt");
    let (_f, path) = make_hosts(&text);
    let err = hosts::read_hosts(&path).unwrap_err();
    assert!(
        err.to_string().contains("END marker appears before BEGIN"),
        "{err}"
    );
}

#[test]
fn fixture_valid_with_after_preserves_after_section() {
    let text = read_fixture("valid_with_after.txt");
    let (_f, path) = make_hosts(&text);
    match hosts::read_hosts(&path).unwrap() {
        MarkerState::Present(s) => {
            assert!(s.before.contains("localhost"));
            assert!(s.block.contains("evil.example.com"));
            assert!(s.after.contains("myhost.local"));
        }
        MarkerState::Absent => panic!("expected Present"),
    }
}

// --- full apply / disable cycle on a temp hosts file ---

#[test]
fn full_apply_then_disable_cycle() {
    let text = read_fixture("macos_default_hosts.txt");
    let dir = tempfile::tempdir().unwrap();
    let hosts_path = dir.path().join("hosts");
    fs::write(&hosts_path, &text).unwrap();

    // Build a blocklist from the good fixture.
    let parsed = parse_hosts_text(&read_fixture("good_list.txt")).unwrap();
    let merged = merge_blocklist(&[parsed], &[], &[]).unwrap();

    // Apply: read sections, write block, no flush (tests never flush DNS).
    let sections = match hosts::read_hosts(&hosts_path).unwrap() {
        MarkerState::Absent => hosts::sections_from_unmarked(&text),
        MarkerState::Present(s) => s,
    };
    let content = hosts::build_hosts_content(&sections, &merged.domains);
    hosts::write_hosts_atomic_no_flush(&hosts_path, &content).unwrap();

    let written = fs::read_to_string(&hosts_path).unwrap();
    assert!(written.contains(BEGIN_MARKER));
    assert!(written.contains(END_MARKER));
    assert!(written.contains("0.0.0.0 ads.example.com"));
    // macOS defaults preserved.
    assert!(written.contains("127.0.0.1\tlocalhost"));
    assert!(written.contains("broadcasthost"));
    assert!(written.contains("::1"));

    // Now disable: remove the block, defaults remain.
    hosts::remove_block_no_flush(&hosts_path).unwrap();
    let after = fs::read_to_string(&hosts_path).unwrap();
    assert!(!after.contains(BEGIN_MARKER));
    assert!(!after.contains("ads.example.com"));
    assert!(after.contains("127.0.0.1\tlocalhost"));
    assert!(after.contains("broadcasthost"));
}

#[test]
fn apply_preserves_macos_defaults() {
    let text = read_fixture("macos_default_hosts.txt");
    let dir = tempfile::tempdir().unwrap();
    let hosts_path = dir.path().join("hosts");
    fs::write(&hosts_path, &text).unwrap();

    let parsed = parse_hosts_text(&read_fixture("good_list.txt")).unwrap();
    let merged = merge_blocklist(&[parsed], &[], &[]).unwrap();
    let sections = hosts::sections_from_unmarked(&text);
    let content = hosts::build_hosts_content(&sections, &merged.domains);
    hosts::write_hosts_atomic_no_flush(&hosts_path, &content).unwrap();

    let written = fs::read_to_string(&hosts_path).unwrap();
    // The five macOS default lines must all survive intact and above the
    // BEGIN marker.
    let begin_pos = written.find(BEGIN_MARKER).unwrap();
    let defaults = [
        "127.0.0.1\tlocalhost",
        "255.255.255.255\tbroadcasthost",
        "::1             localhost",
    ];
    for d in defaults {
        let pos = written
            .find(d)
            .unwrap_or_else(|| panic!("missing default: {d}"));
        assert!(pos < begin_pos, "default {d} ended up below the marker");
    }
}

#[test]
fn apply_is_idempotent() {
    let text = read_fixture("macos_default_hosts.txt");
    let dir = tempfile::tempdir().unwrap();
    let hosts_path = dir.path().join("hosts");
    fs::write(&hosts_path, &text).unwrap();

    let parsed = parse_hosts_text(&read_fixture("good_list.txt")).unwrap();
    let merged = merge_blocklist(&[parsed], &[], &[]).unwrap();

    let apply_once = |path: &Path| {
        let sections = match hosts::read_hosts(path).unwrap() {
            MarkerState::Absent => hosts::sections_from_unmarked(&text),
            MarkerState::Present(s) => s,
        };
        let content = hosts::build_hosts_content(&sections, &merged.domains);
        hosts::write_hosts_atomic_no_flush(path, &content).unwrap();
    };

    apply_once(&hosts_path);
    let first = fs::read_to_string(&hosts_path).unwrap();
    apply_once(&hosts_path);
    let second = fs::read_to_string(&hosts_path).unwrap();
    assert_eq!(first, second, "second apply changed the file");
}

// --- merge: allowlist / protected / custom blocks ---

#[test]
fn allowlist_removes_domain() {
    let parsed = parse_hosts_text(&read_fixture("good_list.txt")).unwrap();
    let merged = merge_blocklist(&[parsed], &[], &["ads.example.com".to_string()]).unwrap();
    assert!(!merged.domains.contains(&"ads.example.com".to_string()));
    assert_eq!(merged.total_after_merge, 9);
}

#[test]
fn protected_domain_never_blocked() {
    let parsed = parse_hosts_text(&read_fixture("protected_domains.txt")).unwrap();
    let merged = merge_blocklist(&[parsed], &[], &[]).unwrap();
    // Every protected domain in the fixture must be absent; only the real
    // ad domain survives.
    for p in [
        "swscan.apple.com",
        "mesu.apple.com",
        "albert.apple.com",
        "icloud.com",
        "raw.githubusercontent.com",
    ] {
        assert!(
            !merged.domains.contains(&p.to_string()),
            "protected {p} was not stripped"
        );
    }
    assert!(merged.domains.contains(&"evil.example.com".to_string()));
}

#[test]
fn custom_blocks_added() {
    let parsed = parse_hosts_text(&read_fixture("good_list.txt")).unwrap();
    let merged = merge_blocklist(&[parsed], &["extra.example.com".to_string()], &[]).unwrap();
    assert!(merged.domains.contains(&"extra.example.com".to_string()));
}

#[test]
fn empty_blocklist_rejected() {
    let parsed = parse_hosts_text(&read_fixture("comments_only.txt")).unwrap();
    assert!(matches!(
        merge_blocklist(&[parsed], &[], &[]).unwrap_err(),
        AdBlockerError::EmptyBlocklist
    ));
}

// --- cache round-trip on a temp dir ---

#[test]
fn cache_write_and_read_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("blocklist.cache");
    let domains = vec!["a.example.com".to_string(), "b.example.com".to_string()];
    config::write_cache_to(&cache, &domains).unwrap();
    let back = config::read_cache_from(&cache).unwrap();
    assert_eq!(back, domains);
}

#[test]
fn cache_round_trip_through_merge() {
    // End-to-end: parse fixture -> merge -> cache -> read cache -> re-validate.
    let parsed = parse_hosts_text(&read_fixture("good_list.txt")).unwrap();
    let merged = merge_blocklist(&[parsed], &[], &[]).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("blocklist.cache");
    config::write_cache_to(&cache, &merged.domains).unwrap();
    let back = config::read_cache_from(&cache).unwrap();

    let mut set = BTreeSet::new();
    for d in &back {
        set.insert(validate::normalize_domain(d).unwrap());
    }
    assert_eq!(set.len(), merged.domains.len());
}

// --- nothing written when the parsed list is empty (simulated failed fetch) ---

#[test]
fn nothing_written_when_source_yields_nothing() {
    // An empty parsed list (e.g. a download that returned garbage) must not
    // produce a blocklist. The merge rejects it.
    let empty = parse_hosts_text("# nothing\n\n").unwrap();
    assert!(empty.domains.is_empty());
    let err = merge_blocklist(&[empty], &[], &[]).unwrap_err();
    assert!(matches!(err, AdBlockerError::EmptyBlocklist));
}

// --- command flows against a temp app dir + temp hosts file ---

const SRC_A: &str = "https://lists-a.example.net/hosts";
const SRC_B: &str = "https://lists-b.example.net/hosts";

/// A temp dir holding an app dir (configured with `sources`) and a hosts
/// file with the macOS defaults.
fn temp_paths(sources: &[&str]) -> (tempfile::TempDir, Paths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path().join("app"), dir.path().join("hosts"));
    fs::write(&paths.hosts, read_fixture("macos_default_hosts.txt")).unwrap();
    let cfg = config::Config {
        sources: sources.iter().map(ToString::to_string).collect(),
        ..config::Config::default()
    };
    cfg.save_to(&paths.config()).unwrap();
    (dir, paths)
}

fn hosts_list(domains: &[&str]) -> String {
    domains.iter().flat_map(|d| ["0.0.0.0 ", d, "\n"]).collect()
}

fn offline(_url: &str) -> adblocker::Result<String> {
    Err(AdBlockerError::Config("offline".to_string()))
}

#[expect(clippy::unnecessary_wraps, reason = "must match the fetcher signature")]
fn good(_url: &str) -> adblocker::Result<String> {
    Ok(read_fixture("good_list.txt"))
}

fn cache(paths: &Paths) -> Vec<String> {
    config::read_cache_from(&paths.cache()).unwrap()
}

#[test]
fn update_apply_disable_restore_cycle() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    adblocker::run_update_with(&paths, false, good).unwrap();

    let report = adblocker::apply_no_flush(&paths).unwrap();
    assert_eq!(report.applied, 10);
    assert!(hosts::is_blocked_in_hosts(&paths.hosts, "ads.example.com"));

    adblocker::disable_no_flush(&paths).unwrap();
    assert!(!hosts::is_blocked_in_hosts(&paths.hosts, "ads.example.com"));

    // Restore undoes the disable; restoring again redoes it.
    adblocker::restore_no_flush(&paths).unwrap();
    assert!(hosts::is_blocked_in_hosts(&paths.hosts, "ads.example.com"));
    adblocker::restore_no_flush(&paths).unwrap();
    assert!(!hosts::is_blocked_in_hosts(&paths.hosts, "ads.example.com"));
    assert!(
        fs::read_to_string(&paths.hosts)
            .unwrap()
            .contains("broadcasthost")
    );
}

#[test]
fn restore_without_backup_errors() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    assert!(matches!(
        adblocker::restore_no_flush(&paths).unwrap_err(),
        AdBlockerError::NoBackup { .. }
    ));
}

#[test]
fn restore_repairs_broken_markers() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    adblocker::run_update_with(&paths, false, good).unwrap();
    adblocker::apply_no_flush(&paths).unwrap();
    // Something mangles the file after our last change.
    let mangled = fs::read_to_string(&paths.hosts)
        .unwrap()
        .replace(END_MARKER, "");
    fs::write(&paths.hosts, mangled).unwrap();
    assert!(hosts::read_hosts(&paths.hosts).is_err());

    adblocker::restore_no_flush(&paths).unwrap();
    assert!(hosts::read_hosts(&paths.hosts).is_ok());
}

#[test]
fn failed_source_falls_back_to_last_good_copy() {
    let (_dir, paths) = temp_paths(&[SRC_A, SRC_B]);
    let both = |url: &str| {
        Ok(if url == SRC_A {
            hosts_list(&["a1.example.com", "a2.example.com"])
        } else {
            hosts_list(&["b1.example.com", "b2.example.com"])
        })
    };
    adblocker::run_update_with(&paths, false, both).unwrap();

    // A is down now; B still works.
    let report = adblocker::run_update_with(&paths, false, |url: &str| {
        if url == SRC_A {
            offline(url)
        } else {
            both(url)
        }
    })
    .unwrap();
    assert_eq!(report.stale_sources.len(), 1);
    assert_eq!(report.stale_sources[0].url, SRC_A);
    assert!(cache(&paths).contains(&"a1.example.com".to_string()));
    assert!(cache(&paths).contains(&"b1.example.com".to_string()));
}

#[test]
fn update_fails_when_every_source_fails() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    adblocker::run_update_with(&paths, false, good).unwrap();
    assert!(adblocker::run_update_with(&paths, false, offline).is_err());
    assert_eq!(cache(&paths).len(), 10, "cache must be left alone");
}

#[test]
fn update_fails_without_snapshot_to_fall_back_on() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    assert!(adblocker::run_update_with(&paths, false, offline).is_err());
    assert!(!paths.cache().exists());
}

#[test]
fn sudden_shrink_is_refused_unless_forced() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    adblocker::run_update_with(&paths, false, good).unwrap();

    let tiny = |_: &str| Ok(hosts_list(&["ads.example.com"]));
    let err = adblocker::run_update_with(&paths, false, tiny).unwrap_err();
    assert!(
        matches!(
            err,
            AdBlockerError::SuspiciousShrink {
                previous: 10,
                new: 1
            }
        ),
        "{err}"
    );
    // The broken list must not replace the last good snapshot either.
    assert_eq!(
        adblocker::rebuild_cache(&paths).unwrap(),
        Rebuild::Rebuilt(10)
    );

    adblocker::run_update_with(&paths, true, tiny).unwrap();
    assert_eq!(cache(&paths), vec!["ads.example.com"]);
}

#[test]
fn allow_rebuilds_offline_and_covers_subdomains() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    let list = |_: &str| {
        Ok(hosts_list(&[
            "ads.example.com",
            "cdn.ads.example.com",
            "other.example.org",
        ]))
    };
    adblocker::run_update_with(&paths, false, list).unwrap();

    let rebuild = adblocker::run_allow(&paths, "ads.example.com").unwrap();
    assert_eq!(rebuild, Rebuild::Rebuilt(1));
    assert_eq!(cache(&paths), vec!["other.example.org"]);

    adblocker::run_unallow(&paths, "ads.example.com").unwrap();
    assert_eq!(cache(&paths).len(), 3);
}

#[test]
fn block_and_unblock_rebuild_offline() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    adblocker::run_update_with(&paths, false, good).unwrap();
    adblocker::run_block(&paths, "extra.example.com").unwrap();
    assert!(cache(&paths).contains(&"extra.example.com".to_string()));
    adblocker::run_unblock(&paths, "extra.example.com").unwrap();
    assert!(!cache(&paths).contains(&"extra.example.com".to_string()));
}

#[test]
fn config_change_before_first_update_needs_update() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    assert_eq!(
        adblocker::run_allow(&paths, "ads.example.com").unwrap(),
        Rebuild::NeedsUpdate
    );
}

#[test]
fn source_host_is_never_blocked() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    let list = |_: &str| Ok(hosts_list(&["lists-a.example.net", "ads.example.com"]));
    adblocker::run_update_with(&paths, false, list).unwrap();
    assert_eq!(cache(&paths), vec!["ads.example.com"]);
}

#[test]
fn check_explains_each_reason() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    let list = |_: &str| {
        Ok(hosts_list(&[
            "ads.example.com",
            "fine.example.org",
            "swscan.apple.com",
        ]))
    };
    adblocker::run_update_with(&paths, false, list).unwrap();
    adblocker::run_allow(&paths, "example.org").unwrap();
    adblocker::apply_no_flush(&paths).unwrap();

    let blocked = adblocker::run_check(&paths, "ADS.example.com").unwrap();
    assert_eq!(blocked.domain, "ads.example.com");
    assert!(blocked.blocked_now && blocked.in_cache);
    assert_eq!(blocked.in_sources, vec![SRC_A]);

    let allowed = adblocker::run_check(&paths, "fine.example.org").unwrap();
    assert!(!allowed.blocked_now);
    assert_eq!(allowed.allowed_by.as_deref(), Some("example.org"));

    let protected = adblocker::run_check(&paths, "swscan.apple.com").unwrap();
    assert_eq!(protected.protected_by, Some("swscan.apple.com"));
    assert!(!protected.blocked_now);

    let source = adblocker::run_check(&paths, "lists-a.example.net").unwrap();
    assert_eq!(source.source_host.as_deref(), Some("lists-a.example.net"));
}

#[test]
fn apply_skips_protected_domains_in_an_old_cache() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    config::write_cache_to(
        &paths.cache(),
        &["evil.example.com".to_string(), "p01.icloud.com".to_string()],
    )
    .unwrap();
    let report = adblocker::apply_no_flush(&paths).unwrap();
    assert_eq!(report.applied, 1);
    assert_eq!(report.skipped_protected, 1);
    assert!(!hosts::is_blocked_in_hosts(&paths.hosts, "p01.icloud.com"));
}

#[test]
fn pause_turns_blocking_back_on() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    adblocker::run_update_with(&paths, false, good).unwrap();
    adblocker::apply_no_flush(&paths).unwrap();

    let flushes = AtomicUsize::new(0);
    // A short pause that runs out on its own.
    let report = adblocker::pause_with(
        &paths,
        Duration::from_millis(300),
        &AtomicBool::new(false),
        || {
            flushes.fetch_add(1, Ordering::SeqCst);
        },
    )
    .unwrap();
    assert_eq!(report.applied, 10);
    assert!(hosts::is_blocked_in_hosts(&paths.hosts, "ads.example.com"));
    assert_eq!(
        flushes.load(Ordering::SeqCst),
        2,
        "flush on pause and resume"
    );

    // A long pause ended early (as Ctrl-C does) still re-applies.
    adblocker::pause_with(
        &paths,
        Duration::from_secs(3600),
        &AtomicBool::new(true),
        || {},
    )
    .unwrap();
    assert!(hosts::is_blocked_in_hosts(&paths.hosts, "ads.example.com"));
}

#[test]
fn pause_requires_blocking_to_be_on() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    let err = adblocker::pause_with(
        &paths,
        Duration::from_secs(1),
        &AtomicBool::new(true),
        || {},
    )
    .unwrap_err();
    assert!(matches!(err, AdBlockerError::NotEnabled("pause")), "{err}");
}

#[test]
fn status_reports_last_update() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    assert!(adblocker::run_status(&paths).unwrap().last_update.is_none());
    adblocker::run_update_with(&paths, false, good).unwrap();
    let status = adblocker::run_status(&paths).unwrap();
    assert!(status.last_update.is_some());
    assert_eq!(status.cache_domain_count, Some(10));
}

#[test]
fn uninstall_previews_then_removes_only_its_own_files() {
    let (_dir, paths) = temp_paths(&[SRC_A]);
    adblocker::run_update_with(&paths, false, good).unwrap();
    adblocker::apply_no_flush(&paths).unwrap();
    let keep = paths.app_dir.join("not-ours.txt");
    fs::write(&keep, "keep me").unwrap();

    let preview = adblocker::uninstall_no_flush(&paths, false).unwrap();
    assert!(preview.block_present && preview.backup.is_some());
    assert!(!preview.done);
    assert!(paths.config().exists(), "preview must not delete anything");

    let done = adblocker::uninstall_no_flush(&paths, true).unwrap();
    assert!(done.done);
    assert!(
        !fs::read_to_string(&paths.hosts)
            .unwrap()
            .contains(BEGIN_MARKER)
    );
    assert!(!paths.backup().exists());
    assert!(!paths.config().exists() && !paths.cache().exists());
    assert!(!paths.sources_dir().exists());
    assert!(keep.exists(), "unknown files must survive");
}
