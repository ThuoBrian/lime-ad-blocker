//! Command-line interface definitions (clap derive).

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "adblocker",
    version,
    about = "Manage /etc/hosts ad blocking on macOS"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Download, validate, and cache the blocklist (runs as your normal user)
    Update,
    /// Write the cached blocklist into /etc/hosts (requires sudo)
    Apply,
    /// Remove the ad-blocker block from /etc/hosts (requires sudo)
    Disable,
    /// Show whether blocking is on, the domain count, last update, and sources
    Status,
    /// Add a domain to your allowlist (so it is never blocked)
    Allow { domain: String },
    /// Add a custom domain to block
    Block { domain: String },
}
