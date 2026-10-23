//! Parse and validate hosts-file blocklist lines.
//!
//! Only lines mapping a domain to `0.0.0.0` or `127.0.0.1` are accepted;
//! everything else is rejected and counted. A built-in protected list can
//! never be blocked.

use std::collections::BTreeSet;

use crate::error::{AdBlockerError, Result};

/// Domains that must never be blocked, regardless of source or user config.
/// Each entry also protects all of its subdomains.
///
/// Includes the Apple and Microsoft update / activation / sign-in domains
/// that would break the system if blocked, and the default blocklist source
/// host (so a future `update` can always fetch the list). The hosts of any
/// other configured sources are protected at merge time.
pub const PROTECTED_DOMAINS: &[&str] = &[
    // Apple software update & activation
    "swscan.apple.com",
    "swcdn.apple.com",
    "swdist.apple.com",
    "swdownload.apple.com",
    "swquery.apple.com",
    "appldnld.apple.com",
    "mesu.apple.com",
    "updates.cdn-apple.com",
    "updates-http.cdn-apple.com",
    "oscdn.apple.com",
    "osrecovery.apple.com",
    "gdmf.apple.com",
    // Activation & signature validation
    "albert.apple.com",
    "gs.apple.com",
    "skl.apple.com",
    "ppq.apple.com",
    "ocsp.apple.com",
    // App Store & iTunes
    "itunes.apple.com",
    "apps.apple.com",
    "buy.itunes.apple.com",
    // iCloud
    "icloud.com",
    "setup.icloud.com",
    // Windows Update & delivery
    "windowsupdate.com",
    "update.microsoft.com",
    "delivery.mp.microsoft.com",
    "download.microsoft.com",
    // Windows activation & Microsoft sign-in
    "sls.microsoft.com",
    "login.live.com",
    "login.microsoftonline.com",
    // Microsoft Defender definitions & cloud protection
    "definitionupdates.microsoft.com",
    "wdcp.microsoft.com",
    // Blocklist source host
    "raw.githubusercontent.com",
];

/// Normalize a domain: trim, lowercase ASCII, strip trailing dots, validate.
///
/// Returns the canonical form, or [`AdBlockerError::InvalidDomain`] if the
/// domain is malformed.
pub fn normalize_domain(input: &str) -> Result<String> {
    let mut s = input.trim().to_lowercase();
    while s.ends_with('.') {
        s.pop();
    }
    if s.is_empty() || s.len() > 253 {
        return Err(AdBlockerError::InvalidDomain(input.trim().to_string()));
    }
    let labels: Vec<&str> = s.split('.').collect();
    // Require at least one dot — reject bare hosts like "localhost".
    if labels.len() < 2 {
        return Err(AdBlockerError::InvalidDomain(s));
    }
    for label in &labels {
        if label.is_empty() || label.len() > 63 {
            return Err(AdBlockerError::InvalidDomain(s));
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(AdBlockerError::InvalidDomain(s));
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(AdBlockerError::InvalidDomain(s));
        }
    }
    // Reject all-numeric labels: that is an IP literal, not a domain
    // (catches junk lines like `0.0.0.0 0.0.0.0`).
    if labels
        .iter()
        .all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(AdBlockerError::InvalidDomain(s));
    }
    Ok(s)
}

/// Returns true if `domain` (already normalized) is on the protected list or
/// is a subdomain of a protected entry.
pub fn is_protected(domain: &str) -> bool {
    PROTECTED_DOMAINS
        .iter()
        .any(|&p| is_same_or_subdomain(domain, p))
}

/// `domain == parent` or `domain` ends with `.parent`.
pub fn is_same_or_subdomain(domain: &str, parent: &str) -> bool {
    domain
        .strip_suffix(parent)
        .is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))
}

/// Per-category rejection counts from parsing a single hosts file.
#[derive(Debug, Default, Clone)]
pub struct ParseStats {
    pub accepted: usize,
    pub rejected_blank: usize,
    pub rejected_comment: usize,
    pub rejected_bad_ip: usize,
    pub rejected_bad_domain: usize,
    pub rejected_junk: usize,
    pub rejected_duplicate: usize,
}

/// A parsed hosts file: the unique accepted domains plus parse statistics.
#[derive(Debug, Default)]
pub struct ParsedList {
    pub domains: BTreeSet<String>,
    pub stats: ParseStats,
}

/// Parse hosts-file text into a [`ParsedList`].
///
/// Accepts lines of the form `0.0.0.0 <domain>` or `127.0.0.1 <domain>`,
/// case-insensitive, with arbitrary whitespace separation and optional inline
/// `# comment`. Everything else is rejected and counted.
pub fn parse_hosts_text(text: &str) -> Result<ParsedList> {
    let mut list = ParsedList::default();

    for raw in text.lines() {
        // `.lines()` strips the line terminator; trim handles CRLF residue.
        let line = raw.trim();

        if line.is_empty() {
            list.stats.rejected_blank += 1;
            continue;
        }
        // Strip inline comments: take text before the first `#`.
        let before_hash = line.split('#').next().unwrap_or("").trim();
        if before_hash.is_empty() {
            // Line was a comment (or only whitespace + comment).
            list.stats.rejected_comment += 1;
            continue;
        }
        let mut parts = before_hash.split_whitespace();
        let ip = parts.next().unwrap_or("");
        let domain = parts.next();

        if !is_block_ip(ip) {
            list.stats.rejected_bad_ip += 1;
            continue;
        }
        let Some(domain) = domain else {
            // Valid IP but no domain field.
            list.stats.rejected_junk += 1;
            continue;
        };

        match normalize_domain(domain) {
            Ok(normalized) => {
                if list.domains.insert(normalized) {
                    list.stats.accepted += 1;
                } else {
                    list.stats.rejected_duplicate += 1;
                }
            }
            Err(_) => {
                list.stats.rejected_bad_domain += 1;
            }
        }
    }

    Ok(list)
}

/// A line's first field must be exactly one of these to be accepted.
fn is_block_ip(s: &str) -> bool {
    s == "0.0.0.0" || s == "127.0.0.1"
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- normalize_domain ---

    #[test]
    fn normalize_lowercases() {
        assert_eq!(normalize_domain("Example.COM").unwrap(), "example.com");
    }

    #[test]
    fn normalize_strips_trailing_dot() {
        assert_eq!(normalize_domain("evil.com.").unwrap(), "evil.com");
        assert_eq!(normalize_domain("evil.com..").unwrap(), "evil.com");
    }

    #[test]
    fn normalize_rejects_empty() {
        assert!(normalize_domain("").is_err());
    }

    #[test]
    fn normalize_rejects_no_dot() {
        assert!(normalize_domain("localhost").is_err());
    }

    #[test]
    fn normalize_rejects_too_long() {
        let label = "a".repeat(254);
        assert!(normalize_domain(&format!("{label}.com")).is_err());
    }

    #[test]
    fn normalize_rejects_label_too_long() {
        let label = "a".repeat(64);
        assert!(normalize_domain(&format!("{label}.com")).is_err());
    }

    #[test]
    fn normalize_rejects_leading_hyphen() {
        assert!(normalize_domain("-evil.com").is_err());
    }

    #[test]
    fn normalize_rejects_trailing_hyphen() {
        assert!(normalize_domain("evil-.com").is_err());
    }

    #[test]
    fn normalize_rejects_underscore() {
        assert!(normalize_domain("ev_il.com").is_err());
    }

    #[test]
    fn normalize_rejects_wildcard() {
        assert!(normalize_domain("*.evil.com").is_err());
    }

    #[test]
    fn normalize_rejects_non_ascii() {
        assert!(normalize_domain("évïl.com").is_err());
    }

    #[test]
    fn normalize_accepts_subdomain() {
        assert_eq!(
            normalize_domain("a.b.c.example.com").unwrap(),
            "a.b.c.example.com"
        );
    }

    #[test]
    fn normalize_rejects_all_numeric() {
        // `0.0.0.0` is an IP literal, not a domain.
        assert!(normalize_domain("0.0.0.0").is_err());
        assert!(normalize_domain("10.0.0.1").is_err());
    }

    // --- is_protected ---

    #[test]
    fn protected_matches_exact_and_subdomains() {
        assert!(is_protected("icloud.com"));
        assert!(is_protected("p01-caldav.icloud.com"));
        assert!(is_protected("ctldl.windowsupdate.com"));
        assert!(is_protected("activation-v2.sls.microsoft.com"));
    }

    #[test]
    fn protected_does_not_match_lookalikes() {
        assert!(!is_protected("evilicloud.com"));
        assert!(!is_protected("icloud.com.evil.net"));
        assert!(!is_protected("microsoft.com"));
    }

    // --- parse_hosts_text ---

    #[test]
    fn parse_accepts_0_0_0_0() {
        let l = parse_hosts_text("0.0.0.0 evil.com\n").unwrap();
        assert_eq!(l.stats.accepted, 1);
        assert!(l.domains.contains("evil.com"));
    }

    #[test]
    fn parse_accepts_127_0_0_1() {
        let l = parse_hosts_text("127.0.0.1 evil.com\n").unwrap();
        assert_eq!(l.stats.accepted, 1);
        assert!(l.domains.contains("evil.com"));
    }

    #[test]
    fn parse_lowercases_domain() {
        let l = parse_hosts_text("0.0.0.0 EVIL.COM\n").unwrap();
        assert_eq!(l.stats.accepted, 1);
        assert!(l.domains.contains("evil.com"));
    }

    #[test]
    fn parse_counts_blanks() {
        let l = parse_hosts_text("\n\n0.0.0.0 evil.com\n").unwrap();
        assert_eq!(l.stats.rejected_blank, 2);
        assert_eq!(l.stats.accepted, 1);
    }

    #[test]
    fn parse_counts_comments() {
        let l = parse_hosts_text("# hello\n0.0.0.0 evil.com\n").unwrap();
        assert_eq!(l.stats.rejected_comment, 1);
        assert_eq!(l.stats.accepted, 1);
    }

    #[test]
    fn parse_handles_inline_comment() {
        let l = parse_hosts_text("0.0.0.0 evil.com # note\n").unwrap();
        assert_eq!(l.stats.accepted, 1);
        assert!(l.domains.contains("evil.com"));
    }

    #[test]
    fn parse_rejects_bad_ip() {
        let l = parse_hosts_text("8.8.8.8 evil.com\n").unwrap();
        assert_eq!(l.stats.rejected_bad_ip, 1);
        assert_eq!(l.stats.accepted, 0);
    }

    #[test]
    fn parse_rejects_ipv6_ip() {
        let l = parse_hosts_text("::1 evil.com\n").unwrap();
        assert_eq!(l.stats.rejected_bad_ip, 1);
    }

    #[test]
    fn parse_rejects_junk_ip_as_domain() {
        let l = parse_hosts_text("0.0.0.0 0.0.0.0\n").unwrap();
        assert_eq!(l.stats.rejected_bad_domain, 1);
        assert_eq!(l.stats.accepted, 0);
    }

    #[test]
    fn parse_rejects_single_field() {
        let l = parse_hosts_text("0.0.0.0\n").unwrap();
        assert_eq!(l.stats.rejected_junk, 1);
    }

    #[test]
    fn parse_handles_crlf() {
        let l = parse_hosts_text("0.0.0.0 evil.com\r\n").unwrap();
        assert_eq!(l.stats.accepted, 1);
        assert!(l.domains.contains("evil.com"));
    }

    #[test]
    fn parse_counts_duplicates() {
        let l = parse_hosts_text("0.0.0.0 evil.com\n0.0.0.0 evil.com\n").unwrap();
        assert_eq!(l.stats.accepted, 1);
        assert_eq!(l.stats.rejected_duplicate, 1);
    }

    #[test]
    fn parse_handles_tab_separator() {
        let l = parse_hosts_text("0.0.0.0\tevil.com\n").unwrap();
        assert_eq!(l.stats.accepted, 1);
    }

    #[test]
    fn parse_handles_multiple_spaces() {
        let l = parse_hosts_text("0.0.0.0    evil.com\n").unwrap();
        assert_eq!(l.stats.accepted, 1);
    }

    #[test]
    fn parse_rejects_bad_domain() {
        let l = parse_hosts_text("0.0.0.0 -evil.com\n").unwrap();
        assert_eq!(l.stats.rejected_bad_domain, 1);
    }

    #[test]
    fn parse_stevenblack_header_sample() {
        // A representative slice of the StevenBlack hosts header.
        let sample = "\
# Title: StevenBlack/hosts
#
# This hosts file is a merged collection of hosts from reputable sources
#
# Number of unique domains: 71,492
#
127.0.0.1 localhost
127.0.0.1 localhost.localdomain
255.255.255.255 broadcasthost
::1 localhost
0.0.0.0 0.0.0.0

# Start StevenBlack

0.0.0.0 ad-assets.futurecdn.net
0.0.0.0 ads.google.com
";
        let l = parse_hosts_text(sample).unwrap();
        // The two real ad domains plus `localhost.localdomain` (a valid
        // 127.0.0.1 line with a dotted domain) are accepted. `localhost`
        // itself is rejected (no dot), and `0.0.0.0 0.0.0.0` is rejected
        // (all-numeric labels).
        assert_eq!(l.stats.accepted, 3);
        assert!(l.domains.contains("ad-assets.futurecdn.net"));
        assert!(l.domains.contains("ads.google.com"));
        assert!(l.domains.contains("localhost.localdomain"));
        // bad ip: 255.255.255.255 and ::1
        assert_eq!(l.stats.rejected_bad_ip, 2);
        // bad domain: `localhost` (no dot) and `0.0.0.0` (all-numeric)
        assert_eq!(l.stats.rejected_bad_domain, 2);
        assert!(l.stats.rejected_comment >= 5);
        assert!(l.stats.rejected_blank >= 1);
    }

    // --- protected list ---

    #[test]
    fn protected_domains_are_flagged() {
        assert!(is_protected("swscan.apple.com"));
        assert!(is_protected("raw.githubusercontent.com"));
        assert!(!is_protected("evil.com"));
    }

    #[test]
    fn protected_list_includes_apple_and_source() {
        // Spot-check that critical domains are present.
        assert!(PROTECTED_DOMAINS.contains(&"swscan.apple.com"));
        assert!(PROTECTED_DOMAINS.contains(&"albert.apple.com"));
        assert!(PROTECTED_DOMAINS.contains(&"icloud.com"));
        assert!(PROTECTED_DOMAINS.contains(&"raw.githubusercontent.com"));
    }
}
