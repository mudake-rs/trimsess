//! Minimal `/proc` queries used to establish process and file identity.

use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::Holder;
use crate::codex::Fingerprint;

pub(super) fn parse_pid(name: &OsStr) -> Option<u32> {
    name.to_str()?.parse().ok()
}

pub(super) fn process_holds_inode(
    proc_path: &Path,
    fingerprint: &Fingerprint,
) -> Result<bool, std::io::Error> {
    let entries = fs::read_dir(proc_path.join("fd"))?;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let metadata = match fs::metadata(entry.path()) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if metadata.dev() == fingerprint.device && metadata.ino() == fingerprint.inode {
            return Ok(true);
        }
    }
    Ok(false)
}

// `/proc/<pid>/comm` is process-controlled; only the executable identity is
// strong enough to authorize `--force` signaling.
pub(super) fn is_codex_process(proc_path: &Path) -> bool {
    fs::read_link(proc_path.join("exe"))
        .ok()
        .and_then(|path| path.file_name().map(OsStr::as_bytes).map(ToOwned::to_owned))
        .is_some_and(|name| codex_name(&name))
}

// A root-owned fd directory on a same-user process is Linux's observable sign
// of a privilege transition, not a transient procfs read failure.
pub(super) fn fd_directory_has_different_owner(proc_path: &Path, user_id: u32) -> bool {
    fs::metadata(proc_path.join("fd")).is_ok_and(|metadata| metadata.uid() != user_id)
}

fn codex_name(name: &[u8]) -> bool {
    name == b"codex"
}

pub(super) fn arguments_match(proc_path: &Path, path: &Path, session_id: Option<&str>) -> bool {
    let Ok(cmdline) = fs::read(proc_path.join("cmdline")) else {
        return false;
    };
    let path_bytes = path.as_os_str().as_bytes();
    cmdline.split(|byte| *byte == 0).any(|argument| {
        contains(argument, path_bytes)
            || session_id.is_some_and(|id| contains(argument, id.as_bytes()))
    })
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

pub(super) fn read_process_state(proc_path: &Path) -> Option<(u8, u64)> {
    let stat = fs::read_to_string(proc_path.join("stat")).ok()?;
    let close = stat.rfind(')')?;
    let mut fields = stat.get(close + 1..)?.split_whitespace();
    let state = fields.next()?.as_bytes().first().copied()?;
    let start_time = fields.nth(18)?.parse().ok()?;
    Some((state, start_time))
}

pub(super) fn holder_still_matches(holder: &Holder, fingerprint: &Fingerprint) -> bool {
    let proc_path = PathBuf::from(format!("/proc/{}", holder.pid));
    read_process_state(&proc_path).map(|(_, start_time)| start_time) == Some(holder.start_time)
        && is_codex_process(&proc_path)
        && process_holds_inode(&proc_path, fingerprint).unwrap_or(false)
}

pub(super) fn holder_still_matches_process(holder: &Holder) -> bool {
    let proc_path = PathBuf::from(format!("/proc/{}", holder.pid));
    read_process_state(&proc_path)
        .is_some_and(|(state, start_time)| state != b'Z' && start_time == holder.start_time)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{MetadataExt, symlink};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{codex_name, contains, fd_directory_has_different_owner, is_codex_process};

    static NEXT_PROC: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn byte_substring_match_is_exact() {
        assert!(contains(b"prefix-session-suffix", b"session"));
        assert!(!contains(b"prefix-session-suffix", b"other"));
        assert!(!contains(b"anything", b""));
    }

    #[test]
    fn only_the_codex_executable_name_authorizes_signaling() {
        assert!(codex_name(b"codex"));
        assert!(!codex_name(b"codex-helper"));
        assert!(!codex_name(b"codex-code-mode-host"));
    }

    #[test]
    fn fd_owner_difference_identifies_privilege_transition() {
        let sequence = NEXT_PROC.fetch_add(1, Ordering::Relaxed);
        let proc_path = std::env::temp_dir().join(format!(
            "trimsess-procfs-owner-test-{}-{sequence}",
            std::process::id(),
        ));
        fs::create_dir(&proc_path).expect("fake proc directory should be created");
        fs::create_dir(proc_path.join("fd")).expect("fake fd directory should be created");
        let owner = fs::metadata(proc_path.join("fd"))
            .expect("fake fd metadata should exist")
            .uid();

        assert!(!fd_directory_has_different_owner(&proc_path, owner));
        assert!(fd_directory_has_different_owner(
            &proc_path,
            owner.wrapping_add(1)
        ));

        fs::remove_dir_all(proc_path).expect("fake proc directory should be removed");
    }

    #[test]
    fn mutable_process_name_does_not_authorize_signaling() {
        let sequence = NEXT_PROC.fetch_add(1, Ordering::Relaxed);
        let proc_path = std::env::temp_dir().join(format!(
            "trimsess-procfs-test-{}-{sequence}",
            std::process::id(),
        ));
        fs::create_dir(&proc_path).expect("fake proc directory should be created");
        fs::write(proc_path.join("comm"), b"codex\n").expect("comm should be written");
        symlink("/bin/bash", proc_path.join("exe")).expect("exe link should be created");

        assert!(!is_codex_process(&proc_path));

        fs::remove_dir_all(proc_path).expect("fake proc directory should be removed");
    }
}
