//! Thin entry point: parse args, call the library, print results.
//! The only place that references the real system hosts file.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use adblocker::cli::{Cli, Command};
use adblocker::{AdBlockerError, CheckReport, Paths, Rebuild, StatusReport, UninstallReport};
use clap::Parser;

/// How to run `apply` with the privileges it needs, for hints.
#[cfg(not(windows))]
const APPLY_HINT: &str = "`sudo adblocker apply`";
#[cfg(windows)]
const APPLY_HINT: &str = "`adblocker apply` from an Administrator terminal";

/// What `uninstall` leaves for the user to remove.
#[cfg(target_os = "macos")]
const LEFTOVERS: &str = "the daily refresh (`sh scripts/uninstall-schedule.sh`, then \
    `sudo rm /etc/sudoers.d/adblocker`) and the binary (`cargo uninstall adblocker`)";
#[cfg(windows)]
const LEFTOVERS: &str = "the daily refresh (`scripts\\windows\\uninstall-schedule.ps1` as \
    Administrator) and the binary (`cargo uninstall adblocker`)";
#[cfg(not(any(windows, target_os = "macos")))]
const LEFTOVERS: &str = "any cron entry you added and the binary (`cargo uninstall adblocker`)";

/// Set by the Ctrl-C / terminal-close handler to end a `pause` early.
static STOP: AtomicBool = AtomicBool::new(false);

/// The system hosts file: `/etc/hosts`.
#[cfg(not(windows))]
fn hosts_path() -> PathBuf {
    PathBuf::from("/etc/hosts")
}

/// The system hosts file: `%SystemRoot%\System32\drivers\etc\hosts`.
#[cfg(windows)]
fn hosts_path() -> PathBuf {
    PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string()))
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts")
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let paths = Paths::new(adblocker::config::default_app_dir(), hosts_path());
    match dispatch(cli, &paths) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("adblocker: {e}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: Cli, paths: &Paths) -> adblocker::Result<()> {
    let hosts = paths.hosts.display();
    match cli.command {
        Command::Update { force } => {
            let report = adblocker::run_update(paths, force)?;
            for s in &report.stale_sources {
                eprintln!(
                    "warning: could not refresh {}; using its last good copy ({})",
                    s.url, s.error
                );
            }
            let b = &report.blocklist;
            println!(
                "Updated: {} domains from {} source(s) ({} rejected).",
                b.total_after_merge, b.total_sources, b.total_rejected
            );
            println!("Run {APPLY_HINT} to write them to {hosts}.");
        }
        Command::Apply => {
            let report = adblocker::run_apply(paths)?;
            println!("Applied: {} domains blocked in {hosts}.", report.applied);
            if report.skipped_protected > 0 {
                println!(
                    "Skipped {} protected domain(s) left in an older cache; \
                     the next `adblocker update` drops them for good.",
                    report.skipped_protected
                );
            }
        }
        Command::Disable => {
            adblocker::run_disable(paths)?;
            println!("Disabled: ad-blocker block removed from {hosts}.");
        }
        Command::Pause { duration } => {
            adblocker::require_root("pause", &paths.hosts)?;
            ctrlc::set_handler(|| STOP.store(true, Ordering::SeqCst))
                .map_err(|e| AdBlockerError::Signal(e.to_string()))?;
            println!(
                "Pausing blocking for {}. Press Ctrl-C to turn it back on sooner.",
                human_duration(duration)
            );
            let report = adblocker::run_pause(paths, duration, &STOP)?;
            // The terminal may be gone by now (that's one way the pause
            // ends), so don't let a failed print turn into a panic.
            let _ = writeln!(
                std::io::stdout(),
                "Blocking is back on: {} domains.",
                report.applied
            );
        }
        Command::Restore => {
            adblocker::run_restore(paths)?;
            println!(
                "Restored {hosts} to its state before the last change. \
                 Run `restore` again to redo it."
            );
        }
        Command::Status => print_status(&adblocker::run_status(paths)?),
        Command::Check { domain } => print_check(&adblocker::run_check(paths, &domain)?),
        Command::List => {
            let cfg = adblocker::run_list(paths)?;
            print_list("Sources", &cfg.sources);
            print_list("Allowlist", &cfg.allowlist);
            print_list("Custom blocks", &cfg.custom_blocks);
            println!("\nConfig file: {}", paths.config().display());
        }
        Command::Allow { domain } => {
            let rebuild = adblocker::run_allow(paths, &domain)?;
            println!("Added {domain} (and its subdomains) to your allowlist.");
            print_rebuild(&rebuild);
        }
        Command::Unallow { domain } => {
            let rebuild = adblocker::run_unallow(paths, &domain)?;
            println!("Removed {domain} from your allowlist.");
            print_rebuild(&rebuild);
        }
        Command::Block { domain } => {
            let rebuild = adblocker::run_block(paths, &domain)?;
            println!("Added {domain} to your custom blocks.");
            print_rebuild(&rebuild);
        }
        Command::Unblock { domain } => {
            let rebuild = adblocker::run_unblock(paths, &domain)?;
            println!("Removed {domain} from your custom blocks.");
            print_rebuild(&rebuild);
        }
        Command::Uninstall { yes } => print_uninstall(&adblocker::run_uninstall(paths, yes)?),
    }
    Ok(())
}

fn print_rebuild(rebuild: &Rebuild) {
    match rebuild {
        Rebuild::Rebuilt(n) => println!("Cache rebuilt ({n} domains). Run {APPLY_HINT} to use it."),
        Rebuild::NeedsUpdate => {
            println!("Run `adblocker update`, then {APPLY_HINT}, for it to take effect.");
        }
    }
}

fn print_status(s: &StatusReport) {
    let state = if s.enabled { "on" } else { "off" };
    println!("Blocking:     {state}");
    println!("In hosts:     {} domains", s.hosts_domain_count);
    let cached = match s.cache_domain_count {
        Some(n) => format!("yes ({n} domains)"),
        None => "no".to_string(),
    };
    println!("Cache:        {cached}");
    let updated = match s
        .last_update
        .and_then(|t| SystemTime::now().duration_since(t).ok())
    {
        Some(age) if age > Duration::from_secs(7 * 86_400) => {
            format!("{} — stale, run `adblocker update`", ago(age))
        }
        Some(age) => ago(age),
        None => "never".to_string(),
    };
    println!("Last update:  {updated}");
    let sources = if s.sources.is_empty() {
        "(none configured)".to_string()
    } else {
        s.sources.join(", ")
    };
    println!("Sources:      {sources}");
}

fn print_check(r: &CheckReport) {
    let state = if r.blocked_now {
        "BLOCKED"
    } else {
        "not blocked"
    };
    println!("{}: {state}", r.domain);
    let sources = if r.in_sources.is_empty() {
        "none".to_string()
    } else {
        r.in_sources.join(", ")
    };
    println!("  Listed by sources: {sources}");
    if r.custom_block {
        println!(
            "  Custom block:      yes (undo: `adblocker unblock {}`)",
            r.domain
        );
    }
    if let Some(a) = &r.allowed_by {
        println!("  Allowlisted:       yes, by {a} (undo: `adblocker unallow {a}`)");
    }
    if let Some(h) = &r.source_host {
        println!("  Source host:       yes, {h} is never blocked so updates keep working");
    }
    if let Some(p) = r.protected_by {
        println!("  Protected:         yes, by {p}; Lime never blocks it");
    }
    println!(
        "  In cache:          {}",
        if r.in_cache { "yes" } else { "no" }
    );

    if r.in_cache != r.blocked_now {
        println!("The hosts file is behind the cache; run {APPLY_HINT} to sync it.");
    } else if r.blocked_now {
        println!(
            "To unblock: `adblocker allow {}`, then {APPLY_HINT}.",
            r.domain
        );
    }
}

fn print_list(title: &str, items: &[String]) {
    println!("{title}:");
    if items.is_empty() {
        println!("  (none)");
    }
    for item in items {
        println!("  {item}");
    }
}

fn print_uninstall(r: &UninstallReport) {
    let mut items = Vec::new();
    if r.block_present {
        items.push("the ad-blocker block in the hosts file".to_string());
    }
    items.extend(
        r.backup
            .iter()
            .chain(&r.app_files)
            .map(|p| p.display().to_string()),
    );

    if items.is_empty() {
        println!("Nothing to remove.");
    } else {
        println!(
            "{}",
            if r.done {
                "Removed:"
            } else {
                "This would remove:"
            }
        );
        for item in &items {
            println!("  {item}");
        }
    }
    if r.done {
        println!("Still yours to remove: {LEFTOVERS}.");
    } else if !items.is_empty() {
        println!("Run `adblocker uninstall --yes` (with admin/sudo) to do it.");
    }
}

/// `900s` → `15m`, for echoing a pause length back.
fn human_duration(d: Duration) -> String {
    let s = d.as_secs();
    match (s % 3600, s % 60) {
        (0, _) => format!("{}h", s / 3600),
        (_, 0) => format!("{}m", s / 60),
        _ => format!("{s}s"),
    }
}

/// A coarse "how long ago" for `status`.
fn ago(age: Duration) -> String {
    let s = age.as_secs();
    let (n, unit) = match s {
        0..60 => return "just now".to_string(),
        60..3600 => (s / 60, "minute"),
        3600..86_400 => (s / 3600, "hour"),
        _ => (s / 86_400, "day"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}
