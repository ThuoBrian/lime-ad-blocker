//! Read and write `/etc/hosts` between ad-blocker markers.
//!
//! Only content between `# BEGIN AD-BLOCKER` and `# END AD-BLOCKER` is
//! touched; the macOS defaults and any other entries are preserved. Writes
//! are atomic (temp file + rename) with root:wheel 644 ownership preserved.

use std::path::Path;

use crate::error::{AdBlockerError, Result};

pub const BEGIN_MARKER: &str = "# BEGIN AD-BLOCKER";
pub const END_MARKER: &str = "# END AD-BLOCKER";

/// The three sections of a hosts file split by the markers.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HostsSections {
    pub before: String,
    pub block: String,
    pub after: String,
}

/// Marker state discovered while reading a hosts file.
#[derive(Debug)]
pub enum MarkerState {
    /// No markers present (fresh install, or already disabled).
    Absent,
    /// Exactly one BEGIN and one END marker, in the right order.
    Present(HostsSections),
}

/// Read and analyze the hosts file's marker state.
///
/// Refuses symlinks. Returns an error explaining the exact problem if the
/// markers are broken (missing END, duplicate markers, END before BEGIN)
/// rather than guessing.
pub fn read_hosts(path: &Path) -> Result<MarkerState> {
    // Refuse symlinks: use symlink_metadata so we inspect the link itself,
    // not its target.
    let meta = std::fs::symlink_metadata(path).map_err(|e| AdBlockerError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    if meta.file_type().is_symlink() {
        return Err(AdBlockerError::HostsIsSymlink);
    }

    let content = std::fs::read_to_string(path).map_err(|e| AdBlockerError::Io {
        path: path.display().to_string(),
        source: e,
    })?;

    let lines: Vec<&str> = content.lines().collect();

    let mut begin_lines: Vec<usize> = Vec::new();
    let mut end_lines: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed == BEGIN_MARKER {
            begin_lines.push(i);
        } else if trimmed == END_MARKER {
            end_lines.push(i);
        }
    }

    match (begin_lines.len(), end_lines.len()) {
        (0, 0) => Ok(MarkerState::Absent),
        (1, 1) => {
            let begin = begin_lines[0];
            let end = end_lines[0];
            if end < begin {
                return Err(AdBlockerError::Marker(
                    "END marker appears before BEGIN marker".to_string(),
                ));
            }
            let before = lines[..begin].join("\n");
            let block = lines[begin + 1..end].join("\n");
            let after = lines[end + 1..].join("\n");
            Ok(MarkerState::Present(HostsSections {
                before,
                block,
                after,
            }))
        }
        (b, e) => {
            let detail = match (b, e) {
                (n, 0) => format!("found {n} BEGIN markers but no END marker"),
                (0, n) => format!("found {n} END markers but no BEGIN marker"),
                _ => format!(
                    "found {b} BEGIN markers and {e} END markers (expected 1 each or 0 each)"
                ),
            };
            Err(AdBlockerError::Marker(detail))
        }
    }
}

/// Build the new hosts file content from preserved sections and a domain list.
pub fn build_hosts_content(sections: &HostsSections, domains: &[String]) -> String {
    let mut out = String::new();
    if !sections.before.is_empty() {
        out.push_str(&sections.before);
        out.push('\n');
    }
    out.push_str(BEGIN_MARKER);
    out.push('\n');
    for domain in domains {
        out.push_str("0.0.0.0 ");
        out.push_str(domain);
        out.push('\n');
    }
    out.push_str(END_MARKER);
    out.push('\n');
    if !sections.after.is_empty() {
        out.push_str(&sections.after);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// Sections for a file with no markers: all existing content becomes `before`.
pub fn sections_from_unmarked(content: &str) -> HostsSections {
    HostsSections {
        before: content.to_string(),
        block: String::new(),
        after: String::new(),
    }
}

/// Write the hosts file atomically (temp file in the same directory, then
/// rename) with mode 0644. Does not flush DNS — use [`write_hosts_atomic`]
/// in production or this in tests.
pub fn write_hosts_atomic_no_flush(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().ok_or_else(|| AdBlockerError::Io {
        path: path.display().to_string(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent directory"),
    })?;

    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(|e| AdBlockerError::Io {
        path: dir.display().to_string(),
        source: e,
    })?;

    std::io::Write::write_all(&mut temp, content.as_bytes()).map_err(|e| AdBlockerError::Io {
        path: temp.path().display().to_string(),
        source: e,
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o644)).map_err(
            |e| AdBlockerError::Io {
                path: temp.path().display().to_string(),
                source: e,
            },
        )?;
    }

    temp.persist(path).map_err(|e| AdBlockerError::Io {
        path: path.display().to_string(),
        source: e.error,
    })?;

    Ok(())
}

/// Write the hosts file atomically and flush the macOS DNS cache.
pub fn write_hosts_atomic(path: &Path, content: &str) -> Result<()> {
    write_hosts_atomic_no_flush(path, content)?;
    flush_dns();
    Ok(())
}

/// Back up the hosts file to `<path>.adblocker-backup` if no backup exists.
pub fn ensure_backup(path: &Path) -> Result<()> {
    let backup = backup_path(path);
    if !backup.exists() {
        std::fs::copy(path, &backup).map_err(|e| AdBlockerError::Io {
            path: backup.display().to_string(),
            source: e,
        })?;
    }
    Ok(())
}

/// Path used for the one-time backup of the original hosts file.
pub fn backup_path(path: &Path) -> std::path::PathBuf {
    // For "/etc/hosts" this yields "/etc/hosts.adblocker-backup".
    let mut buf = path.as_os_str().to_os_string();
    buf.push(".adblocker-backup");
    std::path::PathBuf::from(buf)
}

/// Remove the ad-blocker block, preserving everything outside the markers.
/// Does not flush DNS.
pub fn remove_block_no_flush(path: &Path) -> Result<()> {
    match read_hosts(path)? {
        MarkerState::Absent => Err(AdBlockerError::Marker(
            "no ad-blocker markers found — nothing to disable".to_string(),
        )),
        MarkerState::Present(sections) => {
            let mut content = sections.before;
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            if !sections.after.is_empty() {
                content.push_str(&sections.after);
                if !content.ends_with('\n') {
                    content.push('\n');
                }
            }
            write_hosts_atomic_no_flush(path, &content)
        }
    }
}

/// Remove the ad-blocker block and flush DNS.
pub fn remove_block(path: &Path) -> Result<()> {
    remove_block_no_flush(path)?;
    flush_dns();
    Ok(())
}

/// Flush the macOS DNS cache. Failure is a warning on stderr, not a hard
/// error: the hosts file is already written at the call site, and a failed
/// flush does not undo it.
pub fn flush_dns() {
    let r1 = std::process::Command::new("dscacheutil")
        .arg("-flushcache")
        .status();
    let r2 = std::process::Command::new("killall")
        .args(["-HUP", "mDNSResponder"])
        .status();
    match (r1, r2) {
        (Ok(s1), Ok(s2)) if s1.success() && s2.success() => {}
        _ => {
            eprintln!("warning: hosts file updated but DNS cache flush failed;");
            eprintln!("         run: dscacheutil -flushcache; killall -HUP mDNSResponder");
        }
    }
}

/// Count the blocked entries (lines between markers) in the hosts file.
/// Returns 0 if the markers are absent or the file cannot be read.
pub fn count_blocked_in_hosts(path: &Path) -> usize {
    let Ok(content) = std::fs::read_to_string(path) else {
        return 0;
    };
    let mut count = 0;
    let mut inside = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == BEGIN_MARKER {
            inside = true;
            continue;
        }
        if trimmed == END_MARKER {
            inside = false;
            continue;
        }
        if inside && trimmed.starts_with("0.0.0.0 ") {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_hosts(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    // --- read_hosts marker handling ---

    #[test]
    fn read_no_markers() {
        let f = make_hosts("127.0.0.1 localhost\n");
        let state = read_hosts(f.path()).unwrap();
        assert!(matches!(state, MarkerState::Absent));
    }

    #[test]
    fn read_markers_present() {
        let f = make_hosts(
            "127.0.0.1 localhost\n\
             # BEGIN AD-BLOCKER\n\
             0.0.0.0 evil.com\n\
             # END AD-BLOCKER\n",
        );
        let state = read_hosts(f.path()).unwrap();
        match state {
            MarkerState::Present(s) => {
                assert!(s.before.contains("localhost"));
                assert!(s.block.contains("evil.com"));
                assert!(s.after.is_empty());
            }
            _ => panic!("expected Present"),
        }
    }

    #[test]
    fn read_markers_with_after() {
        let f = make_hosts(
            "127.0.0.1 localhost\n\
             # BEGIN AD-BLOCKER\n\
             0.0.0.0 evil.com\n\
             # END AD-BLOCKER\n\
             # my custom entry\n",
        );
        let state = read_hosts(f.path()).unwrap();
        match state {
            MarkerState::Present(s) => assert!(s.after.contains("# my custom entry")),
            _ => panic!("expected Present"),
        }
    }

    #[test]
    fn read_missing_end() {
        let f = make_hosts(
            "127.0.0.1 localhost\n\
             # BEGIN AD-BLOCKER\n\
             0.0.0.0 evil.com\n",
        );
        let err = read_hosts(f.path()).unwrap_err();
        assert!(err.to_string().contains("no END marker"), "{err}");
    }

    #[test]
    fn read_missing_begin() {
        let f = make_hosts("0.0.0.0 evil.com\n# END AD-BLOCKER\n");
        let err = read_hosts(f.path()).unwrap_err();
        assert!(err.to_string().contains("no BEGIN marker"), "{err}");
    }

    #[test]
    fn read_duplicate_begin() {
        let f = make_hosts(
            "# BEGIN AD-BLOCKER\n\
             # BEGIN AD-BLOCKER\n\
             # END AD-BLOCKER\n",
        );
        let err = read_hosts(f.path()).unwrap_err();
        assert!(err.to_string().contains("2 BEGIN markers"), "{err}");
    }

    #[test]
    fn read_duplicate_end() {
        let f = make_hosts(
            "# BEGIN AD-BLOCKER\n\
             # END AD-BLOCKER\n\
             # END AD-BLOCKER\n",
        );
        let err = read_hosts(f.path()).unwrap_err();
        assert!(err.to_string().contains("2 END markers"), "{err}");
    }

    #[test]
    fn read_end_before_begin() {
        let f = make_hosts("# END AD-BLOCKER\n# BEGIN AD-BLOCKER\n");
        let err = read_hosts(f.path()).unwrap_err();
        assert!(
            err.to_string().contains("END marker appears before BEGIN"),
            "{err}"
        );
    }

    // --- build_hosts_content ---

    #[test]
    fn build_content_with_domains() {
        let sections = HostsSections {
            before: "127.0.0.1 localhost".to_string(),
            block: String::new(),
            after: String::new(),
        };
        let out = build_hosts_content(&sections, &["evil.com".to_string(), "ads.com".to_string()]);
        assert!(out.contains("127.0.0.1 localhost"));
        assert!(out.contains("# BEGIN AD-BLOCKER"));
        assert!(out.contains("0.0.0.0 evil.com"));
        assert!(out.contains("0.0.0.0 ads.com"));
        assert!(out.contains("# END AD-BLOCKER"));
    }

    #[test]
    fn build_content_preserves_after() {
        let sections = HostsSections {
            before: "127.0.0.1 localhost".to_string(),
            block: String::new(),
            after: "# my entry".to_string(),
        };
        let out = build_hosts_content(&sections, &["evil.com".to_string()]);
        assert!(out.contains("# my entry"));
    }

    // --- atomic write ---

    #[test]
    fn write_atomic_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        write_hosts_atomic_no_flush(&path, "test\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "test\n");
    }

    #[test]
    fn write_atomic_sets_0644_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        write_hosts_atomic_no_flush(&path, "test\n").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o644);
    }

    #[test]
    fn write_atomic_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        std::fs::write(&path, "old\n").unwrap();
        write_hosts_atomic_no_flush(&path, "new\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
    }

    // --- remove_block ---

    #[test]
    fn remove_block_no_markers_errors() {
        let f = make_hosts("127.0.0.1 localhost\n");
        let err = remove_block_no_flush(f.path()).unwrap_err();
        assert!(err.to_string().contains("nothing to disable"), "{err}");
    }

    #[test]
    fn remove_block_preserves_outside_content() {
        let f = make_hosts(
            "127.0.0.1 localhost\n\
             # BEGIN AD-BLOCKER\n\
             0.0.0.0 evil.com\n\
             # END AD-BLOCKER\n",
        );
        remove_block_no_flush(f.path()).unwrap();
        let after = std::fs::read_to_string(f.path()).unwrap();
        assert!(after.contains("localhost"));
        assert!(!after.contains("BEGIN AD-BLOCKER"));
        assert!(!after.contains("evil.com"));
    }

    #[test]
    fn remove_block_preserves_after_section() {
        let f = make_hosts(
            "127.0.0.1 localhost\n\
             # BEGIN AD-BLOCKER\n\
             0.0.0.0 evil.com\n\
             # END AD-BLOCKER\n\
             10.0.0.1 myhost\n",
        );
        remove_block_no_flush(f.path()).unwrap();
        let after = std::fs::read_to_string(f.path()).unwrap();
        assert!(after.contains("myhost"));
        assert!(after.contains("localhost"));
        assert!(!after.contains("evil.com"));
    }

    // --- symlink refusal ---

    #[test]
    fn refuse_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::write(&real, "test\n").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let err = read_hosts(&link).unwrap_err();
        assert!(matches!(err, AdBlockerError::HostsIsSymlink), "{err}");
    }

    // --- backup ---

    #[test]
    fn backup_path_is_hosts_with_suffix() {
        let p = backup_path(Path::new("/etc/hosts"));
        assert_eq!(p, std::path::PathBuf::from("/etc/hosts.adblocker-backup"));
    }

    #[test]
    fn ensure_backup_creates_copy_once() {
        let dir = tempfile::tempdir().unwrap();
        let hosts = dir.path().join("hosts");
        std::fs::write(&hosts, "127.0.0.1 localhost\n").unwrap();
        ensure_backup(&hosts).unwrap();
        let backup = backup_path(&hosts);
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "127.0.0.1 localhost\n"
        );
        // Second call must not overwrite.
        std::fs::write(&hosts, "changed\n").unwrap();
        ensure_backup(&hosts).unwrap();
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "127.0.0.1 localhost\n"
        );
    }
}
