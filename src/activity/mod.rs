//! Linux process discovery and verified Codex writer termination.

use crate::codex::Fingerprint;

mod detect;
mod procfs;
mod terminate;

pub use detect::detect;
pub use terminate::stop_verified_holders;

/// Process identity captured with its `/proc` start time to prevent PID reuse.
#[derive(Debug, Clone)]
pub struct Holder {
    pub pid: u32,
    pub start_time: u64,
    pub is_codex: bool,
}

/// One fail-closed snapshot of same-user processes related to a transcript.
#[derive(Debug, Default)]
pub struct Activity {
    pub(super) fd_holders: Vec<Holder>,
    pub(super) argument_only_pids: Vec<u32>,
    pub(super) uninspectable_pids: Vec<u32>,
}

impl Activity {
    // Processes with kernel-hidden descriptors are reported separately and
    // never become force targets.
    pub const fn is_active(&self) -> bool {
        !self.fd_holders.is_empty() || !self.argument_only_pids.is_empty()
    }

    pub fn fd_holder_pids(&self) -> Vec<u32> {
        self.fd_holders.iter().map(|holder| holder.pid).collect()
    }

    pub fn argument_only_pids(&self) -> &[u32] {
        &self.argument_only_pids
    }

    pub fn uninspectable_pids(&self) -> &[u32] {
        &self.uninspectable_pids
    }

    // A dry run may promise termination only when the real force path could
    // signal every active process in this snapshot.
    pub fn force_target_pids(&self) -> Vec<u32> {
        if !self.argument_only_pids.is_empty()
            || self.fd_holders.iter().any(|holder| !holder.is_codex)
        {
            return Vec::new();
        }
        self.fd_holders.iter().map(|holder| holder.pid).collect()
    }
}

fn holder_still_matches(holder: &Holder, fingerprint: &Fingerprint) -> bool {
    procfs::holder_still_matches(holder, fingerprint)
}

fn holder_still_matches_process(holder: &Holder) -> bool {
    procfs::holder_still_matches_process(holder)
}
