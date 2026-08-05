//! Backup-directory selection and fail-closed validation.

use std::env;
use std::fs::{self, Permissions};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::{Error, ErrorKind};

pub(super) fn ensure(directory: &Path, source_path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
                return Err(Error::for_path(
                    ErrorKind::Unchanged,
                    source_path,
                    format!(
                        "backup directory {} is not a regular directory; source not replaced",
                        directory.display()
                    ),
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(directory).map_err(|error| {
                Error::for_path(
                    ErrorKind::Unchanged,
                    source_path,
                    format!(
                        "cannot create backup directory {}: {error}; source not replaced",
                        directory.display()
                    ),
                )
            })?;
            fs::set_permissions(directory, Permissions::from_mode(0o700)).map_err(|error| {
                Error::for_path(
                    ErrorKind::Unchanged,
                    source_path,
                    format!(
                        "cannot secure backup directory {}: {error}; source not replaced",
                        directory.display()
                    ),
                )
            })?;
        }
        Err(error) => {
            return Err(Error::for_path(
                ErrorKind::Unchanged,
                source_path,
                format!(
                    "cannot inspect backup directory {}: {error}; source not replaced",
                    directory.display()
                ),
            ));
        }
    }
    Ok(())
}

pub(super) fn default_path() -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os("XDG_STATE_HOME").filter(|path| !path.is_empty()) {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(Error::new(
                ErrorKind::Unchanged,
                "XDG_STATE_HOME is relative; pass an explicit --backup-dir",
            ));
        }
        return Ok(path.join("trimsess/backups"));
    }
    if let Some(path) = env::var_os("HOME").filter(|path| !path.is_empty()) {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(Error::new(
                ErrorKind::Unchanged,
                "HOME is relative; pass an explicit --backup-dir",
            ));
        }
        return Ok(path.join(".local/state/trimsess/backups"));
    }
    Err(Error::new(
        ErrorKind::Unchanged,
        "cannot determine the default backup directory; pass --backup-dir",
    ))
}
