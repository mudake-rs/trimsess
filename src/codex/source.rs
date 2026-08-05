//! Source path validation, identity capture, and stability checks.

use std::fs::{self, File, Metadata};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use rustix::process::geteuid;

use super::retention::Plan;
use super::scan;
use crate::{Error, ErrorKind};

// Filesystem identity and mutable state captured from the opened source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    pub device: u64,
    pub inode: u64,
    pub len: u64,
    pub modified_seconds: i64,
    pub modified_nanoseconds: i64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
}

impl Fingerprint {
    fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            len: metadata.len(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            mode: metadata.mode(),
            uid: metadata.uid(),
            gid: metadata.gid(),
        }
    }

    fn state_matches(&self, metadata: &Metadata) -> bool {
        self.device == metadata.dev()
            && self.inode == metadata.ino()
            && self.len == metadata.len()
            && self.modified_seconds == metadata.mtime()
            && self.modified_nanoseconds == metadata.mtime_nsec()
            && self.mode == metadata.mode()
            && self.uid == metadata.uid()
            && self.gid == metadata.gid()
    }
}

// A plan bound to the exact source inode and metadata from which it was built.
#[derive(Debug)]
pub struct Inspection {
    pub path: PathBuf,
    pub fingerprint: Fingerprint,
    pub plan: Plan,
}

pub fn inspect_path(input: &Path) -> Result<Inspection, Error> {
    let (path, mut file, fingerprint) = open_source(input, true)?;
    let plan = scan::scan(&mut file, &path, fingerprint.len, true)?;
    ensure_file_and_path_stable(&file, &path, &fingerprint)?;

    Ok(Inspection {
        path,
        fingerprint,
        plan,
    })
}

pub fn inspect_candidate(path: &Path) -> Result<Inspection, Error> {
    let (path, mut file, fingerprint) = open_source(path, false)?;
    let plan = scan::scan(&mut file, &path, fingerprint.len, false)?;
    ensure_file_and_path_stable(&file, &path, &fingerprint)?;

    Ok(Inspection {
        path,
        fingerprint,
        plan,
    })
}

pub fn ensure_path_stable(path: &Path, fingerprint: &Fingerprint) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            path,
            format!("source stability check failed: {error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(Error::for_path(
            ErrorKind::Unchanged,
            path,
            "source path changed type; source not replaced by trimsess",
        ));
    }
    if !fingerprint.state_matches(&metadata) {
        return Err(Error::for_path(
            ErrorKind::Unchanged,
            path,
            "source changed during the operation; source not replaced",
        ));
    }
    Ok(())
}

fn open_source(
    input: &Path,
    require_rollout_name: bool,
) -> Result<(PathBuf, File, Fingerprint), Error> {
    let link_metadata = fs::symlink_metadata(input).map_err(|error| {
        Error::for_path(
            ErrorKind::Target,
            input,
            format!("cannot inspect target: {error}"),
        )
    })?;
    if link_metadata.file_type().is_symlink() {
        return Err(Error::for_path(
            ErrorKind::Target,
            input,
            "target is a symlink; provide a regular rollout file",
        ));
    }
    if !link_metadata.file_type().is_file() {
        return Err(Error::for_path(
            ErrorKind::Target,
            input,
            "target is not a regular file",
        ));
    }

    let path = fs::canonicalize(input).map_err(|error| {
        Error::for_path(
            ErrorKind::Target,
            input,
            format!("cannot resolve target path: {error}"),
        )
    })?;
    if path.to_str().is_none() {
        return Err(Error::for_path(
            ErrorKind::Target,
            &path,
            "target path is not valid UTF-8",
        ));
    }
    if require_rollout_name {
        validate_rollout_name(&path)?;
    }

    let file = File::open(&path).map_err(|error| {
        Error::for_path(
            ErrorKind::Target,
            &path,
            format!("cannot open target for reading: {error}"),
        )
    })?;
    let metadata = file.metadata().map_err(|error| {
        Error::for_path(
            ErrorKind::Target,
            &path,
            format!("cannot read target metadata: {error}"),
        )
    })?;
    if !metadata.file_type().is_file() {
        return Err(Error::for_path(
            ErrorKind::Target,
            &path,
            "opened target is not a regular file",
        ));
    }
    let current_uid = geteuid().as_raw();
    if metadata.uid() != current_uid {
        return Err(Error::for_path(
            ErrorKind::Target,
            &path,
            format!(
                "target owner uid {} differs from current uid {current_uid}",
                metadata.uid()
            ),
        ));
    }

    Ok((path, file, Fingerprint::from_metadata(&metadata)))
}

fn validate_rollout_name(path: &Path) -> Result<(), Error> {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(Error::for_path(
            ErrorKind::Target,
            path,
            "target has no UTF-8 file name",
        ));
    };
    let matches_contract = name
        .strip_prefix("rollout-")
        .and_then(|stem| stem.strip_suffix(".jsonl"))
        .is_some_and(|stem| !stem.is_empty());
    if !matches_contract {
        return Err(Error::for_path(
            ErrorKind::Target,
            path,
            "target name must match rollout-*.jsonl",
        ));
    }
    Ok(())
}

fn ensure_file_and_path_stable(
    file: &File,
    path: &Path,
    fingerprint: &Fingerprint,
) -> Result<(), Error> {
    let metadata = file.metadata().map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            path,
            format!("cannot re-read source metadata: {error}"),
        )
    })?;
    if !fingerprint.state_matches(&metadata) {
        return Err(Error::for_path(
            ErrorKind::Unchanged,
            path,
            "source changed while scanning; source not replaced",
        ));
    }
    ensure_path_stable(path, fingerprint)
}

#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{Fingerprint, ensure_path_stable};

    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn appended_source_fails_fingerprint_check() {
        let path = temporary_file_path();
        fs::write(&path, b"synthetic").expect("fixture write should succeed");
        let metadata = fs::metadata(&path).expect("metadata should exist");
        let fingerprint = Fingerprint::from_metadata(&metadata);
        let mut file = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("fixture should reopen");
        file.write_all(b" append").expect("append should succeed");
        file.sync_all().expect("append should sync");
        assert!(ensure_path_stable(&path, &fingerprint).is_err());
        fs::remove_file(path).expect("fixture should be removable");
    }

    #[test]
    fn replaced_source_path_fails_fingerprint_check() {
        let path = temporary_file_path();
        let moved = path.with_extension("moved");
        fs::write(&path, b"original").expect("fixture write should succeed");
        let metadata = fs::metadata(&path).expect("metadata should exist");
        let fingerprint = Fingerprint::from_metadata(&metadata);
        fs::rename(&path, &moved).expect("original should move");
        fs::write(&path, b"replacement").expect("replacement should be written");
        assert!(ensure_path_stable(&path, &fingerprint).is_err());
        fs::remove_file(path).expect("replacement should be removable");
        fs::remove_file(moved).expect("original should be removable");
    }

    #[test]
    fn changed_source_mode_fails_fingerprint_check() {
        let path = temporary_file_path();
        fs::write(&path, b"synthetic").expect("fixture write should succeed");
        let metadata = fs::metadata(&path).expect("metadata should exist");
        let fingerprint = Fingerprint::from_metadata(&metadata);
        let changed_mode = (metadata.mode() & 0o7777) ^ 0o100;
        fs::set_permissions(&path, fs::Permissions::from_mode(changed_mode))
            .expect("mode change should succeed");
        assert!(ensure_path_stable(&path, &fingerprint).is_err());
        fs::remove_file(path).expect("fixture should be removable");
    }

    fn temporary_file_path() -> PathBuf {
        let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "trimsess-codex-test-{}-{sequence}",
            std::process::id()
        ))
    }
}
