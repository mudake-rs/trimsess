//! PID-reuse-safe termination of verified Codex transcript holders.

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use rustix::fd::OwnedFd;
use rustix::io::Errno;
use rustix::process::{Pid, PidfdFlags, Signal, pidfd_open, pidfd_send_signal};

use super::{Activity, Holder, holder_still_matches, holder_still_matches_process};
use crate::codex::Fingerprint;
use crate::{Error, ErrorKind};

const TERM_GRACE: Duration = Duration::from_secs(5);
const KILL_GRACE: Duration = Duration::from_secs(5);
const WAIT_STEP: Duration = Duration::from_millis(50);

// Pin every verified writer with pidfd before signaling any process.
pub fn stop_verified_holders(
    activity: &Activity,
    path: &Path,
    fingerprint: &Fingerprint,
) -> Result<Vec<u32>, Error> {
    let signal_targets = pin_signal_targets(activity, path, fingerprint)?;
    let mut stopped = Vec::new();

    for (holder, pidfd) in signal_targets {
        match pidfd_send_signal(&pidfd, Signal::TERM) {
            Ok(()) => {}
            Err(Errno::SRCH) => continue,
            Err(error) => {
                return Err(Error::for_path(
                    ErrorKind::Active,
                    path,
                    format!(
                        "could not send SIGTERM to Codex pid {}: {error}",
                        holder.pid
                    ),
                )
                .with_process_stopped(!stopped.is_empty()));
            }
        }

        if !wait_for_exit(&holder, TERM_GRACE) {
            if !holder_still_matches_process(&holder) {
                stopped.push(holder.pid);
                continue;
            }
            match pidfd_send_signal(&pidfd, Signal::KILL) {
                Ok(()) => {}
                Err(Errno::SRCH) => {
                    stopped.push(holder.pid);
                    continue;
                }
                Err(error) => {
                    return Err(Error::for_path(
                        ErrorKind::Active,
                        path,
                        format!(
                            "Codex pid {} did not exit and SIGKILL failed: {error}",
                            holder.pid
                        ),
                    )
                    .with_process_stopped(!stopped.is_empty()));
                }
            }
            if !wait_for_exit(&holder, KILL_GRACE) {
                return Err(Error::for_path(
                    ErrorKind::Active,
                    path,
                    format!("Codex pid {} did not exit after SIGKILL", holder.pid),
                )
                .with_process_stopped(!stopped.is_empty()));
            }
        }
        stopped.push(holder.pid);
    }

    Ok(stopped)
}

// Resolve every target to a stable pidfd before the first process receives a
// signal. This prevents partial termination caused by a later validation error.
fn pin_signal_targets(
    activity: &Activity,
    path: &Path,
    fingerprint: &Fingerprint,
) -> Result<Vec<(Holder, OwnedFd)>, Error> {
    if let Some(holder) = activity.fd_holders.iter().find(|holder| !holder.is_codex) {
        return Err(Error::for_path(
            ErrorKind::Active,
            path,
            format!(
                "pid {} holds the target but is not a verified Codex process; source not replaced",
                holder.pid
            ),
        ));
    }
    if !activity.argument_only_pids.is_empty() {
        return Err(Error::for_path(
            ErrorKind::Active,
            path,
            format!(
                concat!(
                    "Codex process arguments reference the target but no matching fd proves ownership ",
                    "(pids {:?}); source not replaced"
                ),
                activity.argument_only_pids
            ),
        ));
    }

    let mut live_holders = Vec::new();
    for holder in &activity.fd_holders {
        if holder_still_matches(holder, fingerprint) {
            live_holders.push(holder.clone());
        }
    }

    let mut signal_targets = Vec::new();
    for holder in live_holders {
        if !holder_still_matches(&holder, fingerprint) {
            continue;
        }
        let pid = pid_for_signal(holder.pid, path)?;
        let pidfd = match pidfd_open(pid, PidfdFlags::empty()) {
            Ok(pidfd) => pidfd,
            Err(Errno::SRCH) => continue,
            Err(error) => {
                return Err(Error::for_path(
                    ErrorKind::Active,
                    path,
                    format!(
                        "could not open a stable pidfd for Codex pid {}: {error}",
                        holder.pid
                    ),
                ));
            }
        };
        if holder_still_matches(&holder, fingerprint) {
            signal_targets.push((holder, pidfd));
        }
    }

    Ok(signal_targets)
}

fn wait_for_exit(holder: &Holder, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !holder_still_matches_process(holder) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(WAIT_STEP);
    }
}

fn pid_for_signal(pid: u32, path: &Path) -> Result<Pid, Error> {
    let raw = i32::try_from(pid).map_err(|_| {
        Error::for_path(
            ErrorKind::Active,
            path,
            format!("pid {pid} cannot be represented safely"),
        )
    })?;
    Pid::from_raw(raw).ok_or_else(|| {
        Error::for_path(
            ErrorKind::Active,
            path,
            format!("pid {pid} is not a valid positive process id"),
        )
    })
}
