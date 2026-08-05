//! Human and stable JSON rendering across the transcript privacy boundary.

use std::io::{self, Write};
use std::path::PathBuf;

use serde::Serialize;

use super::{OperationReport, OutputMode, SCHEMA_VERSION, Status};
use crate::ErrorKind;

#[derive(Serialize)]
struct ErrorReport<'a> {
    schema_version: u32,
    status: Status,
    error_code: &'static str,
    path: Option<&'a PathBuf>,
    message: &'a str,
    source_replaced: bool,
    process_stopped: bool,
    backup_path: Option<&'a PathBuf>,
}

/// Write a successful operation report to standard output.
///
/// # Errors
///
/// Returns the first serialization or standard-output write error. The caller
/// must combine that error with [`OperationReport::output_error`] so recovery
/// state is not lost.
pub fn write_report(report: &OperationReport, mode: OutputMode) -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    if mode == OutputMode::Json {
        serde_json::to_writer(&mut output, report).map_err(io::Error::other)?;
        writeln!(output)?;
        return Ok(());
    }

    writeln!(
        output,
        "{}: {}/{} session {}",
        report.status.as_str(),
        report.agent,
        report.format,
        report.session_id
    )?;
    writeln!(output, "path: {}", report.path.display())?;
    let activity = if report.active { "active" } else { "inactive" };
    let newest = report
        .newest_compaction_record
        .map_or_else(|| "none".to_owned(), |record| record.to_string());
    writeln!(
        output,
        "state: {activity}; compactions: {} (newest record: {newest})",
        report.compaction_count
    )?;
    let count_label = if report.status == Status::Trimmed {
        "installed at commit"
    } else {
        "projected"
    };
    writeln!(
        output,
        "{count_label}: {} records / {} bytes -> {} records / {} bytes",
        report.before_records, report.before_bytes, report.after_records, report.after_bytes
    )?;
    if let Some(backup_path) = &report.backup_path {
        writeln!(output, "backup: {}", backup_path.display())?;
    }
    if !report.fd_holder_pids.is_empty() {
        writeln!(output, "open-fd holder pids: {:?}", report.fd_holder_pids)?;
    }
    if !report.argument_only_pids.is_empty() {
        writeln!(
            output,
            "argument-only Codex pids: {:?}",
            report.argument_only_pids
        )?;
    }
    if !report.uninspectable_pids.is_empty() {
        writeln!(
            output,
            "pids with hidden descriptors: {} (full list in --json)",
            report.uninspectable_pids.len()
        )?;
    }
    if !report.stopped_pids.is_empty() {
        writeln!(output, "stopped Codex pids: {:?}", report.stopped_pids)?;
    }
    if !report.would_stop_pids.is_empty() {
        writeln!(
            output,
            "would stop Codex pids: {:?}",
            report.would_stop_pids
        )?;
    }
    if let Some(command) = &report.resume_command {
        writeln!(output, "resume: {command}")?;
    }
    if report.status == Status::Active {
        writeln!(
            output,
            "next: stop the reported writer, or use trim --force only for verified Codex fd holders"
        )?;
    }
    Ok(())
}

/// Best-effort write a privacy-safe failure report to standard error.
pub fn write_error(error: &crate::Error, mode: OutputMode) {
    let stderr = io::stderr();
    let mut output = stderr.lock();
    if mode == OutputMode::Json {
        let report = ErrorReport {
            schema_version: SCHEMA_VERSION,
            status: error.kind.status(),
            error_code: error.kind.as_str(),
            path: error.path.as_ref(),
            message: &error.message,
            source_replaced: error.source_replaced,
            process_stopped: error.process_stopped,
            backup_path: error.backup_path.as_ref(),
        };
        if serde_json::to_writer(&mut output, &report).is_ok() {
            let _ = writeln!(output);
        }
        return;
    }

    let path = error
        .path
        .as_ref()
        .map(|path| format!(" {}", path.display()))
        .unwrap_or_default();
    let _ = writeln!(output, "trimsess:{path}: {}", error.message);
    let source_state = if error.source_replaced {
        "replaced by trimsess"
    } else {
        "not replaced by trimsess"
    };
    let _ = writeln!(output, "source: {source_state}");
    if error.process_stopped {
        let _ = writeln!(output, "Codex was stopped");
    }
    if let Some(backup_path) = &error.backup_path {
        let _ = writeln!(output, "backup: {}", backup_path.display());
    }
    let next = match error.kind {
        ErrorKind::Usage => "correct the command line and retry",
        ErrorKind::Target => "correct the explicit transcript path and retry",
        ErrorKind::Unsupported => {
            "leave the transcript unchanged and verify the Codex format and version"
        }
        ErrorKind::Active => "stop the reported writer or resolve the activity check, then retry",
        ErrorKind::Unchanged => "fix the reported pre-commit failure and retry",
        ErrorKind::Replaced if error.backup_path.is_some() => {
            "keep Codex stopped and use the reported backup if resume validation fails"
        }
        ErrorKind::Replaced => {
            "keep Codex stopped and validate the installed transcript before resuming"
        }
    };
    let _ = writeln!(output, "next: {next}");
}
