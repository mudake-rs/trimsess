//! Fail-closed discovery of processes related to the target transcript.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use rustix::process::geteuid;

use super::procfs::{
    arguments_match, fd_directory_has_different_owner, is_codex_process, parse_pid,
    process_holds_inode, read_process_state,
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
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(_)
                if read_process_state(&proc_path).is_none_or(
                    |(current_state, current_start)| {
                        current_state == b'Z' || current_start != start_time
                    },
                ) =>
            {
                false
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied
                    && fd_directory_has_different_owner(&proc_path, user_id) =>
            {
                uninspectable.insert(pid);
                false
            }
            Err(error) => {
                return Err(Error::for_path(
                    ErrorKind::Active,
                    path,
                    format!(
                        concat!(
                            "cannot inspect open descriptors of same-user pid {pid}: {error}; ",
                            "stop that process or run where its descriptors are visible"
                        ),
                        pid = pid,
                        error = error
                    ),
                ));
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
