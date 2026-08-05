//! Collision-safe creation of private rewrite and backup files.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::{Error, ErrorKind};

pub(super) fn create(
    directory: &Path,
    prefix: &str,
    extension: &str,
    mode: u32,
    error_path: &Path,
) -> Result<(PathBuf, File), Error> {
    for counter in 0_u16..1000 {
        let path = directory.join(format!("{prefix}.{counter}.{extension}"));
        match OpenOptions::new()
            .write(true)
            .read(true)
            .create_new(true)
            .mode(mode)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(Error::for_path(
                    ErrorKind::Unchanged,
                    error_path,
                    format!("cannot create exclusive file: {error}; source not replaced"),
                ));
            }
        }
    }
    Err(Error::for_path(
        ErrorKind::Unchanged,
        error_path,
        "could not allocate a collision-free file name; source not replaced",
    ))
}
