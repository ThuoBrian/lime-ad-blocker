//! Thin entry point: parse args, call the library, print results.
//! The only place that references the real `/etc/hosts`.

use std::path::Path;
use std::process::ExitCode;

use adblocker::cli::{Cli, Command};
use adblocker::{StatusReport, hosts};
use clap::Parser;

const HOSTS_PATH: &str = "/etc/hosts";

fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("adblocker: {e}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: Cli) -> adblocker::Result<()> {
    match cli.command {
        Command::Update => {
            let report = adblocker::run_update()?;
            println!(
                "Updated: {} domains from {} source(s) ({} rejected).",
                report.total_after_merge, report.total_sources, report.total_rejected
            );
            println!("Run `just apply` (sudo) to write them to /etc/hosts.");
            Ok(())
        }
        Command::Apply => {
            let report = adblocker::run_apply(Path::new(HOSTS_PATH))?;
            println!(
                "Applied: {} domains blocked in {}.",
                report.total_after_merge, HOSTS_PATH
            );
            Ok(())
        }
        Command::Disable => {
            adblocker::run_disable(Path::new(HOSTS_PATH))?;
            println!("Disabled: ad-blocker block removed from {HOSTS_PATH}.");
            Ok(())
        }
        Command::Status => {
            let s = adblocker::run_status(Path::new(HOSTS_PATH))?;
            print_status(&s);
            Ok(())
        }
        Command::Allow { domain } => {
            adblocker::run_allow(&domain)?;
            println!("Added {domain} to your allowlist.");
            Ok(())
        }
        Command::Block { domain } => {
            adblocker::run_block(&domain)?;
            println!("Added {domain} to your custom blocks.");
            Ok(())
        }
    }
}

fn print_status(s: &StatusReport) {
    let state = if s.enabled { "on" } else { "off" };
    println!("Blocking:   {state}");
    println!("Domains:    {}", s.domains_blocked);
    let cached = match s.cache_domain_count {
        Some(n) => format!("yes ({n} domains)"),
        None => "no".to_string(),
    };
    println!("Cache:      {cached}");
    let sources = if s.sources.is_empty() {
        "(none configured)".to_string()
    } else {
        s.sources.join(", ")
    };
    println!("Sources:    {sources}");
    if s.enabled {
        let count = hosts::count_blocked_in_hosts(Path::new(HOSTS_PATH));
        println!("In /etc/hosts: {count} entries between markers.");
    }
}
