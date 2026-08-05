//! Privacy-safe operation reports and CLI-compatible rendering.

use std::io;
use std::path::PathBuf;

use serde::Serialize;

use crate::activity::Activity;
use crate::codex::Inspection;
use crate::{Error, ErrorKind};

mod output;

pub use output::{write_error, write_report};

const SCHEMA_VERSION: u32 = 1;

/// Select human-readable or stable JSON rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum OutputMode {
    /// Concise text intended for a terminal or log.
    Human,
    /// One stable JSON object intended for automation.
    Json,
}

/// Stable operation state used by Rust callers and JSON output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[must_use]
pub enum Status {
    /// Command-line usage was invalid.
    InvalidUsage,
    /// The transcript is supported, inactive, and can be trimmed.
    Ready,
    /// A writer is active or cannot be checked safely.
    Active,
    /// The transcript has no compaction boundary.
    NotTrimmable,
    /// The transcript already contains only the minimal retained records.
    AlreadyMinimal,
    /// Full validation completed without writing or signaling.
    DryRun,
    /// Replacement and post-write validation completed.
    Trimmed,
    /// The explicit target path is invalid or unavailable.
    InvalidTarget,
    /// The transcript shape is malformed or unsupported.
    Unsupported,
    /// An operation failed before source replacement.
    FailedSourceNotReplaced,
    /// An operation failed after source replacement.
    FailedSourceReplaced,
}

impl Status {
    /// Return the stable machine-readable spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidUsage => "invalid_usage",
            Self::Ready => "ready",
            Self::Active => "active",
            Self::NotTrimmable => "not_trimmable",
            Self::AlreadyMinimal => "already_minimal",
            Self::DryRun => "dry_run",
            Self::Trimmed => "trimmed",
            Self::InvalidTarget => "invalid_target",
            Self::Unsupported => "unsupported",
            Self::FailedSourceNotReplaced => "failed_source_not_replaced",
            Self::FailedSourceReplaced => "failed_source_replaced",
        }
    }
}

/// Complete privacy-safe result of an inspection or trim operation.
///
/// Counts describe the validated plan for `inspect` and dry runs, and the
/// exact candidate installed at the trim commit point. A concurrent append
/// after that point is intentionally not included. Serializing this value
/// emits the stable versioned JSON schema documented by the CLI.
#[derive(Debug, Serialize)]
#[must_use]
pub struct OperationReport {
    schema_version: u32,
    agent: &'static str,
    format: &'static str,
    pub(crate) status: Status,
    session_id: String,
    path: PathBuf,
    active: bool,
    before_bytes: u64,
    after_bytes: u64,
    saved_bytes: u64,
    before_records: u64,
    after_records: u64,
    removed_records: u64,
    compaction_count: u64,
    newest_compaction_record: Option<u64>,
    backup_path: Option<PathBuf>,
    fd_holder_pids: Vec<u32>,
    argument_only_pids: Vec<u32>,
    uninspectable_pids: Vec<u32>,
    stopped_pids: Vec<u32>,
    would_stop_pids: Vec<u32>,
    source_replaced: bool,
    resume_command: Option<String>,
}

impl OperationReport {
    pub(crate) fn from_inspection(
        inspection: &Inspection,
        activity: &Activity,
        status: Status,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            agent: "codex",
            format: "codex_jsonl",
            status,
            session_id: inspection.plan.session_id.clone(),
            path: inspection.path.clone(),
            active: activity.is_active(),
            before_bytes: inspection.plan.before_bytes,
            after_bytes: inspection.plan.after_bytes,
            saved_bytes: inspection.plan.saved_bytes,
            before_records: inspection.plan.before_records,
            after_records: inspection.plan.after_records,
            removed_records: inspection.plan.removed_records,
            compaction_count: inspection.plan.compaction_count,
            newest_compaction_record: inspection.plan.newest_compaction_record,
            backup_path: None,
            fd_holder_pids: activity.fd_holder_pids(),
            argument_only_pids: activity.argument_only_pids().to_vec(),
            uninspectable_pids: activity.uninspectable_pids().to_vec(),
            stopped_pids: Vec::new(),
            would_stop_pids: Vec::new(),
            source_replaced: false,
            resume_command: None,
        }
    }

    pub(crate) fn with_backup(mut self, backup_path: Option<PathBuf>) -> Self {
        self.backup_path = backup_path;
        self
    }

    pub(crate) fn with_stopped_pids(mut self, stopped_pids: Vec<u32>) -> Self {
        self.stopped_pids = stopped_pids;
        self
    }

    pub(crate) fn with_would_stop_pids(mut self, would_stop_pids: Vec<u32>) -> Self {
        self.would_stop_pids = would_stop_pids;
        self
    }

    pub(crate) fn with_resume_command(mut self) -> Self {
        self.resume_command = Some(format!("codex resume {}", self.session_id));
        self
    }

    pub(crate) const fn with_source_replaced(mut self) -> Self {
        self.source_replaced = true;
        self
    }

    /// Convert a final-output failure while preserving the recovery state.
    #[must_use]
    pub fn output_error(&self, error: &io::Error) -> crate::Error {
        let replaced = self.source_replaced || matches!(self.status, Status::Trimmed);
        Error {
            kind: if replaced {
                ErrorKind::Replaced
            } else {
                ErrorKind::Unchanged
            },
            message: format!("could not write the final result: {error}"),
            path: Some(self.path.clone()),
            source_replaced: replaced,
            process_stopped: !self.stopped_pids.is_empty(),
            backup_path: self.backup_path.clone(),
        }
    }

    /// Return the stable JSON schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Return the detected agent name.
    #[must_use]
    pub const fn agent(&self) -> &'static str {
        self.agent
    }

    /// Return the detected transcript format name.
    #[must_use]
    pub const fn format(&self) -> &'static str {
        self.format
    }

    /// Return the operation status.
    pub const fn status(&self) -> Status {
        self.status
    }

    /// Return the verified session UUID.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Return the resolved transcript path.
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Report whether a writer was verified by fd or matching Codex arguments.
    ///
    /// See [`Self::uninspectable_pids`] for privilege-transitioned processes
    /// whose hidden descriptors cannot contribute to this boolean.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Return the source size before trimming.
    #[must_use]
    pub const fn before_bytes(&self) -> u64 {
        self.before_bytes
    }

    /// Return the projected or installed size after trimming.
    #[must_use]
    pub const fn after_bytes(&self) -> u64 {
        self.after_bytes
    }

    /// Return the number of bytes removed by the plan.
    #[must_use]
    pub const fn saved_bytes(&self) -> u64 {
        self.saved_bytes
    }

    /// Return the source record count before trimming.
    #[must_use]
    pub const fn before_records(&self) -> u64 {
        self.before_records
    }

    /// Return the projected or installed record count after trimming.
    #[must_use]
    pub const fn after_records(&self) -> u64 {
        self.after_records
    }

    /// Return the number of records removed by the plan.
    #[must_use]
    pub const fn removed_records(&self) -> u64 {
        self.removed_records
    }

    /// Return the number of compaction records found in the source.
    #[must_use]
    pub const fn compaction_count(&self) -> u64 {
        self.compaction_count
    }

    /// Return the one-based record number of the newest compaction.
    #[must_use]
    pub const fn newest_compaction_record(&self) -> Option<u64> {
        self.newest_compaction_record
    }

    /// Return the validated compressed backup path when one was created.
    #[must_use]
    pub fn backup_path(&self) -> Option<&std::path::Path> {
        self.backup_path.as_deref()
    }

    /// Return process IDs with an open descriptor for the target inode.
    #[must_use]
    pub fn fd_holder_pids(&self) -> &[u32] {
        &self.fd_holder_pids
    }

    /// Return Codex process IDs whose arguments name the target without an fd.
    #[must_use]
    pub fn argument_only_pids(&self) -> &[u32] {
        &self.argument_only_pids
    }

    /// Return privilege-transitioned process IDs whose identity and fds are hidden.
    #[must_use]
    pub fn uninspectable_pids(&self) -> &[u32] {
        &self.uninspectable_pids
    }

    /// Return verified Codex process IDs stopped by this operation.
    #[must_use]
    pub fn stopped_pids(&self) -> &[u32] {
        &self.stopped_pids
    }

    /// Return process IDs that a forced non-dry-run operation would stop.
    #[must_use]
    pub fn would_stop_pids(&self) -> &[u32] {
        &self.would_stop_pids
    }

    /// Report whether trimsess atomically replaced the source.
    #[must_use]
    pub const fn source_was_replaced(&self) -> bool {
        self.source_replaced
    }

    /// Return the resume command after a process stop or successful trim.
    #[must_use]
    pub fn resume_command(&self) -> Option<&str> {
        self.resume_command.as_deref()
    }
}
