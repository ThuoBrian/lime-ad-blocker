//! Library error type. All modules return [`Result`]; `main.rs` prints the
//! error and maps it to an exit code.

use thiserror::Error;

/// How to re-run a command with the privileges needed to edit the hosts file.
#[cfg(not(windows))]
const ELEVATE_HINT: &str = "re-run with sudo (e.g. `just apply`)";
#[cfg(windows)]
const ELEVATE_HINT: &str =
    "re-run from an Administrator terminal (or prefix with `sudo` if Sudo for Windows is enabled)";

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

    #[error("the hosts file is a symlink — refusing to modify for safety")]
    HostsIsSymlink,

    #[error("`{command}` needs administrator/root privileges; {}", ELEVATE_HINT)]
    NotRoot { command: String },

    #[error("{url} returned no valid hosts entries; refusing to use it")]
    DownloadOrValidationFailed { url: String },

    #[error("blocklist is empty after processing — refusing to write empty hosts block")]
    EmptyBlocklist,

    #[error(
        "new blocklist has {new} domains, down from {previous}; refusing in case a source is \
         broken. If this is expected (e.g. you removed a source), run `adblocker update --force`"
    )]
    SuspiciousShrink { previous: usize, new: usize },

    #[error("blocking is not on — nothing to {0}")]
    NotEnabled(&'static str),

    #[error("no backup at {path} yet — one is made before the first change")]
    NoBackup { path: String },

    #[error("could not install the Ctrl-C handler, so `pause` could not guarantee resuming: {0}")]
    Signal(String),

    #[error("invalid domain: {0}")]
    InvalidDomain(String),

    #[error("DNS flush failed: {0}")]
    DnsFlush(String),
}

pub type Result<T> = std::result::Result<T, AdBlockerError>;
