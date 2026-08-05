//! Durable backup creation and crash-safe transcript replacement.

use std::fs::{self, File};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::activity;
use crate::codex::{self, Fingerprint, Inspection, TrimOptions};
use crate::report::{OperationReport, Status};
use crate::{Error, ErrorKind};

mod backup;
mod candidate;
mod exclusive;

// Stop a verified writer only after a supported provisional scan, then rescan.
pub fn trim(options: &TrimOptions) -> Result<OperationReport, Error> {
    let mut inspection = codex::inspect_path(&options.path)?;
    let mut activity = activity::detect(
        &inspection.path,
        &inspection.fingerprint,
        Some(&inspection.plan.session_id),
    )?;

    if inspection.plan.compaction_count == 0 || inspection.plan.removed_records == 0 {
        let status = if inspection.plan.compaction_count == 0 {
            Status::NotTrimmable
        } else {
            Status::AlreadyMinimal
        };
        return Ok(OperationReport::from_inspection(
            &inspection,
            &activity,
            status,
        ));
    }

    if options.dry_run {
        let status = if activity.is_active() {
            Status::Active
        } else {
            Status::DryRun
        };
        let would_stop = if options.force {
            activity.force_target_pids()
        } else {
            Vec::new()
        };
        return Ok(
            OperationReport::from_inspection(&inspection, &activity, status)
                .with_would_stop_pids(would_stop),
        );
    }

    if activity.is_active() && !options.force {
        return Ok(OperationReport::from_inspection(
            &inspection,
            &activity,
            Status::Active,
        ));
    }

    let mut stopped_pids = Vec::new();
    if activity.is_active() {
        stopped_pids =
            activity::stop_verified_holders(&activity, &inspection.path, &inspection.fingerprint)?;
        let stopped = !stopped_pids.is_empty();
        inspection = codex::inspect_path(&inspection.path)
            .map_err(|error| attach_recovery_context(error, stopped, None))?;
        activity = activity::detect(
            &inspection.path,
            &inspection.fingerprint,
            Some(&inspection.plan.session_id),
        )
        .map_err(|error| attach_recovery_context(error, stopped, None))?;
        if activity.is_active() {
            return Err(Error::for_path(
                ErrorKind::Active,
                &inspection.path,
                "a writer is still active after forced termination; source not replaced",
            )
            .with_process_stopped(stopped));
        }
        if inspection.plan.compaction_count == 0 || inspection.plan.removed_records == 0 {
            let status = if inspection.plan.compaction_count == 0 {
                Status::NotTrimmable
            } else {
                Status::AlreadyMinimal
            };
            return Ok(
                OperationReport::from_inspection(&inspection, &activity, status)
                    .with_stopped_pids(stopped_pids)
                    .with_resume_command(),
            );
        }
    }

    perform_rewrite(
        &inspection,
        stopped_pids,
        options.backup,
        options.backup_dir.as_deref(),
    )
}

// The rename below is the sole transcript commit point.
fn perform_rewrite(
    inspection: &Inspection,
    stopped_pids: Vec<u32>,
    backup_enabled: bool,
    requested_backup_dir: Option<&Path>,
) -> Result<OperationReport, Error> {
    let process_stopped = !stopped_pids.is_empty();
    let mut backup = if backup_enabled {
        Some(
            backup::create(inspection, requested_backup_dir)
                .map_err(|error| attach_recovery_context(error, process_stopped, None))?,
        )
    } else {
        None
    };

    let mut candidate = candidate::create(inspection)
        .map_err(|error| attach_recovery_context(error, process_stopped, None))?;
    candidate::write(inspection, &mut candidate.file)
        .map_err(|error| attach_recovery_context(error, process_stopped, None))?;
    candidate::apply_source_metadata(inspection, &candidate.file)
        .map_err(|error| attach_recovery_context(error, process_stopped, None))?;
    candidate.file.sync_all().map_err(|error| {
        attach_recovery_context(
            Error::for_path(
                ErrorKind::Unchanged,
                &inspection.path,
                format!("could not sync candidate file: {error}; source not replaced"),
            ),
            process_stopped,
            None,
        )
    })?;

    candidate::validate(inspection, &candidate.path)
        .map_err(|error| attach_recovery_context(error, process_stopped, None))?;
    let mut pinned_source = candidate::pin_source(inspection)
        .map_err(|error| attach_recovery_context(error, process_stopped, None))?;
    codex::ensure_path_stable(&inspection.path, &inspection.fingerprint)
        .map_err(|error| attach_recovery_context(error, process_stopped, None))?;
    let final_activity = activity::detect(
        &inspection.path,
        &inspection.fingerprint,
        Some(&inspection.plan.session_id),
    )
    .map_err(|error| attach_recovery_context(error, process_stopped, None))?;
    if final_activity.is_active() {
        return Err(attach_recovery_context(
            Error::for_path(
                ErrorKind::Active,
                &inspection.path,
                "a writer became active before replacement; source not replaced",
            ),
            process_stopped,
            None,
        ));
    }
    // Process discovery can take long enough for an existing writer to append.
    // Pin source identity once more after that scan and immediately before the
    // atomic commit.
    codex::ensure_path_stable(&inspection.path, &inspection.fingerprint)
        .map_err(|error| attach_recovery_context(error, process_stopped, None))?;

    atomic_replace(&candidate.path, &inspection.path).map_err(|error| {
        attach_recovery_context(
            Error::for_path(
                ErrorKind::Unchanged,
                &inspection.path,
                format!("atomic replacement failed: {error}; source not replaced"),
            ),
            process_stopped,
            None,
        )
    })?;
    if let Some(backup) = &mut backup {
        backup.preserve();
    }
    candidate.installed = true;
    let backup_path = backup.as_ref().map(|backup| backup.path().to_path_buf());

    sync_and_validate_installed(inspection, &mut pinned_source, &mut candidate.file)
        .map_err(|error| attach_recovery_context(error, process_stopped, backup_path.clone()))?;

    Ok(
        OperationReport::from_inspection(inspection, &final_activity, Status::Trimmed)
            .with_backup(backup_path)
            .with_stopped_pids(stopped_pids)
            .with_resume_command()
            .with_source_replaced(),
    )
}

// Everything in this phase runs after the sole commit point. Every failure is
// therefore marked as source-replaced before recovery context is attached.
fn sync_and_validate_installed(
    original: &Inspection,
    pinned_source: &mut File,
    installed: &mut File,
) -> Result<(), Error> {
    let parent = original.path.parent().ok_or_else(|| {
        Error::for_path(
            ErrorKind::Replaced,
            &original.path,
            "installed source has no parent directory",
        )
        .source_replaced()
    })?;
    let directory = File::open(parent).map_err(|error| {
        Error::for_path(
            ErrorKind::Replaced,
            &original.path,
            format!("source was replaced but parent directory could not be opened: {error}"),
        )
        .source_replaced()
    })?;
    directory.sync_all().map_err(|error| {
        Error::for_path(
            ErrorKind::Replaced,
            &original.path,
            format!("source was replaced but directory sync failed: {error}"),
        )
        .source_replaced()
    })?;

    candidate::validate_installed(original, pinned_source, installed)
        .map_err(Error::source_replaced)
}

fn ensure_open_file_matches(
    file: &File,
    fingerprint: &Fingerprint,
    path: &Path,
) -> Result<(), Error> {
    let metadata = file.metadata().map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            path,
            format!("cannot inspect reopened source: {error}"),
        )
    })?;
    if metadata.dev() != fingerprint.device
        || metadata.ino() != fingerprint.inode
        || metadata.len() != fingerprint.len
        || metadata.mtime() != fingerprint.modified_seconds
        || metadata.mtime_nsec() != fingerprint.modified_nanoseconds
    {
        return Err(Error::for_path(
            ErrorKind::Unchanged,
            path,
            "reopened source does not match the inspected source; source not replaced",
        ));
    }
    Ok(())
}

fn attach_recovery_context(
    mut error: Error,
    process_stopped: bool,
    backup_path: Option<PathBuf>,
) -> Error {
    error.process_stopped |= process_stopped;
    if error.backup_path.is_none() {
        error.backup_path = backup_path;
    }
    error
}

fn atomic_replace(candidate: &Path, source: &Path) -> io::Result<()> {
    fs::rename(candidate, source)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::atomic_replace;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn failed_atomic_replace_leaves_both_targets_present() {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "trimsess-rewrite-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("test directory should be created");
        let candidate = directory.join("candidate");
        let source = directory.join("source-directory");
        fs::write(&candidate, b"candidate").expect("candidate should be written");
        fs::create_dir(&source).expect("source directory should be created");

        assert!(atomic_replace(&candidate, &source).is_err());
        assert_eq!(
            fs::read(&candidate).expect("candidate should remain"),
            b"candidate"
        );
        assert!(source.is_dir());
        fs::remove_dir_all(directory).expect("test directory should be removed");
    }
}
