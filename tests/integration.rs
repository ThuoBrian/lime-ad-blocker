//! Integration tests: exercise the library against fixture files and temp
//! hosts files. These never touch the real `/etc/hosts` and never need sudo.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;

use adblocker::config;
use adblocker::hosts::{self, BEGIN_MARKER, END_MARKER, MarkerState};
use adblocker::validate::{self, parse_hosts_text};
use adblocker::{AdBlockerError, merge_blocklist};

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
fn make_hosts(content: &str) -> (tempfile::NamedTempFile, std::path::PathBuf) {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    let path = f.path().to_path_buf();
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
        _ => panic!("expected Present"),
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
