//! Fail-closed discovery of processes related to the target transcript.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use rustix::process::geteuid;

use super::procfs::{
    arguments_match, is_codex_process, parse_pid, process_holds_inode, read_process_state,
};
use super::{Activity, Holder};
use crate::codex::Fingerprint;
use crate::{Error, ErrorKind};

// An open descriptor for the captured inode is authoritative. Matching argv is
// only a fail-closed hint because it cannot prove that a process owns the file.
pub fn detect(
    path: &Path,
    fingerprint: &Fingerprint,
    session_id: Option<&str>,
) -> Result<Activity, Error> {
    let user_id = geteuid().as_raw();
    let self_pid = std::process::id();
    let proc_entries = fs::read_dir("/proc").map_err(|error| {
        Error::for_path(
            ErrorKind::Active,
            path,
            format!("cannot inspect Linux process state: {error}"),
        )
    })?;
    let mut fd_holders = Vec::new();
    let mut argument_only = BTreeSet::new();
    let mut uninspectable = BTreeSet::new();

    for entry in proc_entries {
        let Ok(entry) = entry else {
            continue;
        };
        let Some(pid) = parse_pid(&entry.file_name()) else {
            continue;
        };
        if pid == self_pid {
            continue;
        }
        let proc_path = entry.path();
        let Ok(metadata) = fs::metadata(&proc_path) else {
            continue;
        };
        if metadata.uid() != user_id {
            continue;
        }

        let Some((state, start_time)) = read_process_state(&proc_path) else {
            continue;
        };
        if state == b'Z' {
            continue;
        }

        let is_codex = is_codex_process(&proc_path);
        let holds_fd = match process_holds_inode(&proc_path, fingerprint) {
            Ok(holds_fd) => holds_fd,
            Err(error) => {
                let process_unchanged =
                    read_process_state(&proc_path).is_some_and(|(current_state, current_start)| {
                        current_state != b'Z' && current_start == start_time
                    });
                match descriptor_error_policy(error.kind(), process_unchanged) {
                    DescriptorErrorPolicy::ProcessGone => false,
                    DescriptorErrorPolicy::Uninspectable => {
                        uninspectable.insert(pid);
                        false
                    }
                    DescriptorErrorPolicy::Fatal => {
                        return Err(Error::for_path(
                            ErrorKind::Active,
                            path,
                            format!(
                                concat!(
                                    "cannot inspect open descriptors of same-user pid ",
                                    "{pid}: {error}; stop that process or run where its ",
                                    "descriptors are visible"
                                ),
                                pid = pid,
                                error = error
                            ),
                        ));
                    }
                }
            }
        };
        if holds_fd {
            fd_holders.push(Holder {
                pid,
                start_time,
                is_codex,
            });
            continue;
        }

        if is_codex && arguments_match(&proc_path, path, session_id) {
            argument_only.insert(pid);
        }
    }

    fd_holders.sort_by_key(|holder| holder.pid);
    Ok(Activity {
        fd_holders,
        argument_only_pids: argument_only.into_iter().collect(),
        uninspectable_pids: uninspectable.into_iter().collect(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DescriptorErrorPolicy {
    ProcessGone,
    Uninspectable,
    Fatal,
}

// Permission denial is common for unrelated user services. It cannot prove a
// writer, so report the stable process without authorizing signals.
fn descriptor_error_policy(
    error_kind: io::ErrorKind,
    process_unchanged: bool,
) -> DescriptorErrorPolicy {
    if error_kind == io::ErrorKind::NotFound || !process_unchanged {
        DescriptorErrorPolicy::ProcessGone
    } else if error_kind == io::ErrorKind::PermissionDenied {
        DescriptorErrorPolicy::Uninspectable
    } else {
        DescriptorErrorPolicy::Fatal
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{DescriptorErrorPolicy, descriptor_error_policy};

    #[test]
    fn stable_permission_denial_is_reported_without_blocking() {
        assert_eq!(
            descriptor_error_policy(io::ErrorKind::PermissionDenied, true),
            DescriptorErrorPolicy::Uninspectable
        );
        assert_eq!(
            descriptor_error_policy(io::ErrorKind::PermissionDenied, false),
            DescriptorErrorPolicy::ProcessGone
        );
    }

    #[test]
    fn unexpected_descriptor_errors_still_fail_closed() {
        assert_eq!(
            descriptor_error_policy(io::ErrorKind::InvalidData, true),
            DescriptorErrorPolicy::Fatal
        );
    }
}
