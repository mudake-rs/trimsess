//! Safe, bounded inspection and trimming of supported agent transcripts.
//!
//! Version 1 exposes only the independently verified Codex format. Future
//! runtimes can live beside [`codex`] once their retention contracts are proven.
//!
//! The library never discovers an implicit target. Callers must provide one
//! exact rollout path, then inspect or trim it through [`codex`]. All reports
//! and errors obey the same transcript-content privacy boundary as the CLI.
//!
//! # Example
//!
//! ```no_run
//! use trimsess::codex::{self, TrimOptions};
//!
//! # fn run() -> Result<(), trimsess::Error> {
//! let path = "/path/to/rollout-2026-08-04T12-00-00-01900000-0000-7000-8000-000000000001.jsonl";
//! let inspection = codex::inspect(path)?;
//! if inspection.saved_bytes() > 0 && !inspection.is_active() {
//!     let result = codex::trim(TrimOptions::new(path).dry_run(true))?;
//!     assert!(!result.source_was_replaced());
//! }
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod activity;
pub mod codex;
pub mod report;
mod rewrite;

use std::fmt;
use std::path::{Path, PathBuf};

use report::{OperationReport, Status};

/// Stable category for an operation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum ErrorKind {
    /// Invalid command-line or caller-supplied operation usage.
    Usage,
    /// Invalid, unavailable, or unsafe transcript path.
    Target,
    /// Malformed or unsupported transcript structure.
    Unsupported,
    /// Active or unverifiable transcript writer.
    Active,
    /// Operation failed before trimsess replaced the source.
    Unchanged,
    /// Operation failed after trimsess replaced the source.
    Replaced,
}

impl ErrorKind {
    pub(crate) const fn status(self) -> Status {
        match self {
            Self::Usage => Status::InvalidUsage,
            Self::Target => Status::InvalidTarget,
            Self::Unsupported => Status::Unsupported,
            Self::Active => Status::Active,
            Self::Unchanged => Status::FailedSourceNotReplaced,
            Self::Replaced => Status::FailedSourceReplaced,
        }
    }

    /// Stable machine-readable error code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Target => "target",
            Self::Unsupported => "unsupported",
            Self::Active => "active",
            Self::Unchanged => "source_not_replaced",
            Self::Replaced => "source_replaced",
        }
    }
}

/// Privacy-safe failure from transcript inspection or trimming.
///
/// Errors returned by [`codex::inspect`] and [`codex::trim`] may contain paths,
/// record numbers, versions, session IDs, and operating-system errors. They
/// never contain transcript record content.
#[derive(Debug)]
pub struct Error {
    pub(crate) kind: ErrorKind,
    pub(crate) message: String,
    pub(crate) path: Option<PathBuf>,
    pub(crate) source_replaced: bool,
    pub(crate) process_stopped: bool,
    pub(crate) backup_path: Option<PathBuf>,
}

impl Error {
    /// Constructs an error for a caller-owned boundary such as CLI parsing.
    ///
    /// Callers are responsible for keeping `message` within their own privacy
    /// policy. Errors produced by transcript operations already satisfy the
    /// trimsess privacy contract.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            path: None,
            source_replaced: false,
            process_stopped: false,
            backup_path: None,
        }
    }

    pub(crate) fn for_path(kind: ErrorKind, path: &Path, message: impl Into<String>) -> Self {
        let mut error = Self::new(kind, message);
        error.path = Some(path.to_path_buf());
        error
    }

    pub(crate) const fn with_process_stopped(mut self, stopped: bool) -> Self {
        self.process_stopped = stopped;
        self
    }

    pub(crate) const fn source_replaced(mut self) -> Self {
        self.source_replaced = true;
        self.kind = ErrorKind::Replaced;
        self
    }

    /// Return the stable failure category.
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Return the privacy-safe diagnostic message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Return the affected transcript path when one was resolved.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Report whether trimsess crossed the atomic replacement point.
    #[must_use]
    pub const fn source_was_replaced(&self) -> bool {
        self.source_replaced
    }

    /// Report whether a verified Codex writer was stopped before the failure.
    #[must_use]
    pub const fn process_was_stopped(&self) -> bool {
        self.process_stopped
    }

    /// Return the validated recovery backup path when one was created.
    #[must_use]
    pub fn backup_path(&self) -> Option<&Path> {
        self.backup_path.as_deref()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub(crate) fn inspect_codex(path: &Path) -> Result<OperationReport, Error> {
    let inspection = codex::inspect_path(path)?;
    let activity = activity::detect(
        &inspection.path,
        &inspection.fingerprint,
        Some(&inspection.plan.session_id),
    )?;
    let status = if inspection.plan.compaction_count == 0 {
        Status::NotTrimmable
    } else if inspection.plan.removed_records == 0 {
        Status::AlreadyMinimal
    } else if activity.is_active() {
        Status::Active
    } else {
        Status::Ready
    };

    Ok(OperationReport::from_inspection(
        &inspection,
        &activity,
        status,
    ))
}
