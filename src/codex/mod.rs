//! Concrete inspection and trimming for verified Codex JSONL rollouts.

use std::path::{Path, PathBuf};

use crate::report::OperationReport;

mod jsonl;
mod record;
mod retention;
mod scan;
mod source;

#[cfg(test)]
pub(crate) use retention::Plan;
pub(crate) use source::{
    Fingerprint, Inspection, ensure_path_stable, inspect_candidate, inspect_path,
};

/// Options for one Codex transcript trim.
///
/// Backups are enabled by default. Dry-run and forced writer termination are
/// disabled until explicitly requested.
#[derive(Debug, Clone)]
#[must_use]
pub struct TrimOptions {
    pub(crate) path: PathBuf,
    pub(crate) dry_run: bool,
    pub(crate) backup: bool,
    pub(crate) backup_dir: Option<PathBuf>,
    pub(crate) force: bool,
}

impl TrimOptions {
    /// Creates options for one exact rollout path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            dry_run: false,
            backup: true,
            backup_dir: None,
            force: false,
        }
    }

    /// Selects validation-only mode.
    ///
    /// A dry run performs every validation except writes and process signals.
    pub const fn dry_run(mut self, enabled: bool) -> Self {
        self.dry_run = enabled;
        self
    }

    /// Authorizes stopping verified Codex processes that hold the target inode.
    ///
    /// This option has no signaling side effect during a dry run.
    pub const fn force(mut self, enabled: bool) -> Self {
        self.force = enabled;
        self
    }

    /// Disables the pre-replacement compressed backup.
    ///
    /// This removes the recovery path for failures after atomic replacement.
    pub fn without_backup(mut self) -> Self {
        self.backup = false;
        self.backup_dir = None;
        self
    }

    /// Stores the backup in an explicit directory and enables backups.
    pub fn backup_dir(mut self, directory: impl Into<PathBuf>) -> Self {
        self.backup = true;
        self.backup_dir = Some(directory.into());
        self
    }
}

/// Inspects one explicit Codex rollout path without modifying it.
///
/// The scan is forward-only. Memory is bounded by one limited JSONL record and
/// the decoded validation state for that record, never by transcript length.
///
/// # Errors
///
/// Returns an error when the path is unsafe or unavailable, the transcript is
/// malformed or unsupported, source stability cannot be proven, or same-user
/// process state cannot be inspected safely.
pub fn inspect(path: impl AsRef<Path>) -> Result<OperationReport, crate::Error> {
    crate::inspect_codex(path.as_ref())
}

/// Validates, optionally backs up, and atomically trims one Codex rollout.
///
/// A non-dry-run operation may stop verified writers only when authorized by
/// [`TrimOptions::force`]. The source is never rewritten in place; the atomic
/// rename is the only commit point.
///
/// # Errors
///
/// Returns an error when validation, writer handling, backup creation,
/// candidate construction, replacement, durability, or post-write validation
/// fails. Inspect [`crate::Error::source_was_replaced`] and
/// [`crate::Error::backup_path`] before deciding how to recover.
#[expect(
    clippy::needless_pass_by_value,
    reason = "TrimOptions is a one-shot operation builder"
)]
pub fn trim(options: TrimOptions) -> Result<OperationReport, crate::Error> {
    crate::rewrite::trim(&options)
}
