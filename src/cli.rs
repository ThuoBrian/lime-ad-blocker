//! Command-line interface definitions (clap derive).

use std::time::Duration;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "adblocker",
    version,
    about = "Manage hosts-file ad blocking on macOS, Windows, and Linux"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Download, validate, and cache the blocklist (runs as your normal user)
    Update {
        /// Accept the result even if it is less than half the previous size
        #[arg(long)]
        force: bool,
    },
    /// Write the cached blocklist into the hosts file (requires admin/sudo)
    Apply,
    /// Remove the ad-blocker block from the hosts file (requires admin/sudo)
    Disable,
    /// Turn blocking off for a while, e.g. `pause 15m` (requires admin/sudo)
    Pause {
        /// How long: a number with s, m, or h (e.g. 90s, 15m, 1h); max 24h
        #[arg(value_parser = parse_duration)]
        duration: Duration,
    },
    /// Undo the last change to the hosts file; run again to redo (requires admin/sudo)
    Restore,
    /// Show whether blocking is on, domain counts, last update, and sources
    Status,
    /// Explain whether and why a domain is blocked
    Check { domain: String },
    /// Show your sources, allowlist, and custom blocks
    List,
    /// Never block a domain or its subdomains
    Allow { domain: String },
    /// Remove a domain from your allowlist
    Unallow { domain: String },
    /// Block a domain that isn't in the sources
    Block { domain: String },
    /// Remove a domain from your custom blocks
    Unblock { domain: String },
    /// Remove the block, the backup, and all adblocker data
    Uninstall {
        /// Actually remove everything (without it, only shows what would go)
        #[arg(long)]
        yes: bool,
    },
}

/// Parse `90s`, `15m`, or `1h` into a duration of at most 24 hours.
fn parse_duration(s: &str) -> Result<Duration, String> {
    let usage = || format!("{s:?} is not a duration; use a number with s, m, or h (e.g. 15m)");
    let s = s.trim();
    let (split, _) = s.char_indices().last().ok_or_else(usage)?;
    let (number, unit) = s.split_at(split);
    let n: u64 = number.parse().map_err(|_| usage())?;
    let secs = match unit {
        "s" => n,
        "m" => n.saturating_mul(60),
        "h" => n.saturating_mul(3600),
        _ => return Err(usage()),
    };
    if secs == 0 || secs > 24 * 3600 {
        return Err("pause must be between 1s and 24h".to_string());
    }
    Ok(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_duration_units() {
        assert_eq!(parse_duration("90s"), Ok(Duration::from_secs(90)));
        assert_eq!(parse_duration("15m"), Ok(Duration::from_secs(900)));
        assert_eq!(parse_duration("2h"), Ok(Duration::from_secs(7200)));
    }

    #[test]
    fn parse_duration_rejects_bad_input() {
        for bad in ["", "15", "m", "15x", "-5m", "0m", "25h", "1.5h", "15é"] {
            assert!(parse_duration(bad).is_err(), "{bad:?} should be rejected");
        }
    }
}
