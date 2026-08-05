//! Exact retained-range copying and candidate validation.

use std::fs::{self, File, Permissions};
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use rustix::fs::fchown;
use rustix::process::Gid;

use super::ensure_open_file_matches;
use super::exclusive;
use crate::codex::{self, Inspection};
use crate::{Error, ErrorKind};

mod installed;

pub(super) use installed::{pin_source, validate_installed};

pub(super) fn create(inspection: &Inspection) -> Result<Candidate, Error> {
    let directory = inspection.path.parent().ok_or_else(|| {
        Error::for_path(
            ErrorKind::Target,
            &inspection.path,
            "source has no parent directory",
        )
    })?;
    let prefix = format!(".trimsess-{}", std::process::id());
    let (path, file) = exclusive::create(directory, &prefix, "tmp", 0o600, &inspection.path)?;
    Ok(Candidate {
        path,
        file,
        installed: false,
    })
}

// Copy the two retained ranges without decoding or reserializing their bytes.
pub(super) fn write(inspection: &Inspection, candidate: &mut File) -> Result<(), Error> {
    let tail_start = inspection.plan.retained_tail_start.ok_or_else(|| {
        Error::for_path(
            ErrorKind::Unsupported,
            &inspection.path,
            "trim plan has no compaction boundary",
        )
    })?;
    let mut source = File::open(&inspection.path).map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("cannot reopen source for candidate: {error}"),
        )
    })?;
    ensure_open_file_matches(&source, &inspection.fingerprint, &inspection.path)?;

    copy_exact(
        &mut source,
        candidate,
        inspection.plan.metadata_end,
        &inspection.path,
    )?;
    source.seek(SeekFrom::Start(tail_start)).map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("cannot seek to retained boundary: {error}"),
        )
    })?;
    let tail_bytes = inspection
        .fingerprint
        .len
        .checked_sub(tail_start)
        .ok_or_else(|| {
            Error::for_path(
                ErrorKind::Unsupported,
                &inspection.path,
                "retained tail range is invalid",
            )
        })?;
    copy_exact(&mut source, candidate, tail_bytes, &inspection.path)
}

fn copy_exact(
    source: &mut File,
    destination: &mut File,
    bytes: u64,
    path: &Path,
) -> Result<(), Error> {
    let mut limited = source.take(bytes);
    let copied = io::copy(&mut limited, destination).map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            path,
            format!("candidate write failed: {error}; source not replaced"),
        )
    })?;
    if copied != bytes {
        return Err(Error::for_path(
            ErrorKind::Unchanged,
            path,
            format!("candidate source ended after {copied} of {bytes} bytes; source not replaced"),
        ));
    }
    Ok(())
}

pub(super) fn apply_source_metadata(
    inspection: &Inspection,
    candidate: &File,
) -> Result<(), Error> {
    let candidate_metadata = candidate.metadata().map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("cannot inspect candidate ownership: {error}; source not replaced"),
        )
    })?;
    if candidate_metadata.gid() != inspection.fingerprint.gid {
        fchown(
            candidate,
            None,
            Some(Gid::from_raw(inspection.fingerprint.gid)),
        )
        .map_err(|error| {
            Error::for_path(
                ErrorKind::Unchanged,
                &inspection.path,
                format!("cannot preserve source group: {error}; source not replaced"),
            )
        })?;
    }
    // chown can clear set-id bits, so mode is applied after ownership.
    candidate
        .set_permissions(Permissions::from_mode(inspection.fingerprint.mode & 0o7777))
        .map_err(|error| {
            Error::for_path(
                ErrorKind::Unchanged,
                &inspection.path,
                format!("cannot preserve source mode: {error}; source not replaced"),
            )
        })?;
    Ok(())
}

pub(super) fn validate(inspection: &Inspection, candidate_path: &Path) -> Result<(), Error> {
    let candidate = codex::inspect_candidate(candidate_path)?;
    if !matches_retention_plan(inspection, &candidate) {
        return Err(Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            "candidate validation did not match the retention plan; source not replaced",
        ));
    }
    Ok(())
}

// Candidate validation is bound to the plan computed from the unchanged
// source immediately before the rewrite.
fn matches_retention_plan(source: &Inspection, output: &Inspection) -> bool {
    let expected_bytes = source.plan.after_bytes;
    let expected_records = source.plan.after_records;
    let content_matches = output.plan.session_id == source.plan.session_id
        && output.plan.before_bytes == expected_bytes
        && output.plan.before_records == expected_records
        && output.plan.removed_records == 0;
    let metadata_matches = output.fingerprint.uid == source.fingerprint.uid
        && output.fingerprint.gid == source.fingerprint.gid
        && (output.fingerprint.mode & 0o7777) == (source.fingerprint.mode & 0o7777);
    content_matches && metadata_matches
}

// Same-directory candidate cleanup remains armed until atomic installation.
pub(super) struct Candidate {
    pub(super) path: PathBuf,
    pub(super) file: File,
    pub(super) installed: bool,
}

impl Drop for Candidate {
    fn drop(&mut self) {
        if !self.installed {
            let _ = fs::remove_file(&self.path);
        }
    }
}
