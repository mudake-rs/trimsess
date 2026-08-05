//! Creation and byte-for-byte validation of compressed recovery backups.

use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::ensure_open_file_matches;
use super::exclusive;
use crate::codex::{self, Inspection};
use crate::{Error, ErrorKind};

mod compare;
mod directory;

// A backup becomes reportable only after sync and exact decompression validation.
pub(super) fn create(
    inspection: &Inspection,
    requested_backup_dir: Option<&Path>,
) -> Result<Backup, Error> {
    let directory = match requested_backup_dir {
        Some(directory) => directory.to_path_buf(),
        None => directory::default_path()?,
    };
    directory::ensure(&directory, &inspection.path)?;
    let file_name = inspection
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            Error::for_path(
                ErrorKind::Target,
                &inspection.path,
                "source file name is not valid UTF-8",
            )
        })?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            Error::for_path(
                ErrorKind::Unchanged,
                &inspection.path,
                format!("system clock cannot name backup: {error}"),
            )
        })?
        .as_millis();
    let prefix = format!("{file_name}.{stamp}.{}", std::process::id());
    let (backup_path, backup_file) =
        exclusive::create(&directory, &prefix, "zst", 0o600, &inspection.path)?;
    let backup = Backup::new(backup_path);

    let mut source = File::open(&inspection.path).map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("cannot reopen source for backup: {error}"),
        )
    })?;
    ensure_open_file_matches(&source, &inspection.fingerprint, &inspection.path)?;
    let mut encoder = zstd::stream::write::Encoder::new(backup_file, 3).map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("cannot initialize zstd backup: {error}"),
        )
    })?;
    io::copy(&mut source, &mut encoder).map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("backup write failed: {error}; source not replaced"),
        )
    })?;
    encoder.flush().map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("backup flush failed: {error}; source not replaced"),
        )
    })?;
    let backup_file = encoder.finish().map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("backup finalization failed: {error}; source not replaced"),
        )
    })?;
    backup_file.sync_all().map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("backup sync failed: {error}; source not replaced"),
        )
    })?;
    validate(inspection, backup.path())?;
    codex::ensure_path_stable(&inspection.path, &inspection.fingerprint)?;
    let backup_directory = File::open(&directory).map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("cannot open backup directory for sync: {error}; source not replaced"),
        )
    })?;
    backup_directory.sync_all().map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("cannot sync backup directory: {error}; source not replaced"),
        )
    })?;
    Ok(backup)
}

fn validate(inspection: &Inspection, backup_path: &Path) -> Result<(), Error> {
    let backup = File::open(backup_path).map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("cannot reopen backup for validation: {error}"),
        )
    })?;
    let decoded = zstd::stream::read::Decoder::new(BufReader::new(backup)).map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("cannot decode backup for validation: {error}"),
        )
    })?;
    let source = File::open(&inspection.path).map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("cannot reopen source for backup validation: {error}"),
        )
    })?;
    ensure_open_file_matches(&source, &inspection.fingerprint, &inspection.path)?;
    compare::readers(
        BufReader::new(source),
        BufReader::new(decoded),
        &inspection.path,
    )
}

// A validated backup remains provisional until the transcript commit point.
pub(super) struct Backup {
    path: PathBuf,
    preserve: bool,
}

impl Backup {
    const fn new(path: PathBuf) -> Self {
        Self {
            path,
            preserve: false,
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) const fn preserve(&mut self) {
        self.preserve = true;
    }
}

impl Drop for Backup {
    fn drop(&mut self) {
        if !self.preserve {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::Backup;

    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn provisional_backup_is_removed_until_commit_preserves_it() {
        let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "trimsess-backup-test-{}-{sequence}",
            std::process::id()
        ));
        fs::write(&path, b"synthetic backup").expect("fixture should be written");
        drop(Backup::new(path.clone()));
        assert!(!path.exists());

        fs::write(&path, b"synthetic backup").expect("fixture should be written");
        let mut backup = Backup::new(path.clone());
        backup.preserve();
        drop(backup);
        assert!(path.exists());
        fs::remove_file(path).expect("preserved fixture should be removed");
    }
}
