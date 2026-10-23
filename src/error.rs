//! Library error type. All modules return [`Result`]; only `main.rs` uses
//! `anyhow` for top-level reporting and exit codes.

use thiserror::Error;

/// Every error produced by the `adblocker` library.
#[derive(Debug, Error)]
pub enum AdBlockerError {
    #[error("fetch failed for {url}: {source}")]
    Fetch {
        url: String,
        #[source]
        source: ureq::Error,
    },

    #[error("response from {url} exceeded size limit ({limit} bytes)")]
    ResponseTooLarge { url: String, limit: u64 },

    #[error("I/O error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("config error: {0}")]
    Config(String),

    #[error("hosts file marker error: {0}")]
    Marker(String),

    #[error("/etc/hosts is a symlink — refusing to modify for safety")]
    HostsIsSymlink,

    #[error("not running as root — `{command}` requires sudo; run `just {command}` instead")]
    NotRoot { command: String },

    #[error("download or validation failed; refusing to write hosts file")]
    DownloadOrValidationFailed,

    #[error("blocklist is empty after processing — refusing to write empty hosts block")]
    EmptyBlocklist,

    #[error("domain {domain} is protected and cannot be blocked")]
    ProtectedDomain { domain: String },

    #[error("invalid domain: {0}")]
    InvalidDomain(String),

    #[error("DNS flush failed: {0}")]
    DnsFlush(String),
}

pub type Result<T> = std::result::Result<T, AdBlockerError>;
