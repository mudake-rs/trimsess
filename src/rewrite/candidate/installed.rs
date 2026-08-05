//! Byte-exact validation of the installed transcript prefix.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;

use super::ensure_open_file_matches;
use crate::codex::Inspection;
use crate::{Error, ErrorKind};

// Keep the inspected source inode readable after the candidate replaces its
// path. Post-commit validation uses this descriptor as byte authority.
pub(in crate::rewrite) fn pin_source(inspection: &Inspection) -> Result<File, Error> {
    let source = File::open(&inspection.path).map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            &inspection.path,
            format!("cannot pin source for post-write validation: {error}"),
        )
    })?;
    ensure_open_file_matches(&source, &inspection.fingerprint, &inspection.path)?;
    Ok(source)
}

pub(in crate::rewrite) fn validate_installed(
    original: &Inspection,
    source: &mut File,
    candidate: &mut File,
) -> Result<(), Error> {
    let candidate_metadata = candidate.metadata().map_err(|error| {
        Error::for_path(
            ErrorKind::Replaced,
            &original.path,
            format!("cannot inspect installed candidate: {error}"),
        )
    })?;
    let path_metadata = fs::symlink_metadata(&original.path).map_err(|error| {
        Error::for_path(
            ErrorKind::Replaced,
            &original.path,
            format!("cannot inspect installed transcript path: {error}"),
        )
    })?;
    let same_installed_inode = path_metadata.file_type().is_file()
        && !path_metadata.file_type().is_symlink()
        && path_metadata.dev() == candidate_metadata.dev()
        && path_metadata.ino() == candidate_metadata.ino();
    let content_shape_matches = candidate_metadata.len() >= original.plan.after_bytes;
    let metadata_matches = candidate_metadata.uid() == original.fingerprint.uid
        && candidate_metadata.gid() == original.fingerprint.gid
        && (candidate_metadata.mode() & 0o7777) == (original.fingerprint.mode & 0o7777);
    let prefix_matches = retained_prefix_matches(original, source, candidate)?;

    if !same_installed_inode || !content_shape_matches || !metadata_matches || !prefix_matches {
        return Err(Error::for_path(
            ErrorKind::Replaced,
            &original.path,
            "installed transcript failed post-replacement validation",
        ));
    }
    Ok(())
}

// Compare the installed prefix with the two authoritative ranges on the old
// inode. Appended complete records are allowed after the commit point.
fn retained_prefix_matches(
    original: &Inspection,
    source: &mut File,
    installed: &mut File,
) -> Result<bool, Error> {
    let tail_start = original.plan.retained_tail_start.ok_or_else(|| {
        Error::for_path(
            ErrorKind::Replaced,
            &original.path,
            "installed validation has no retained compaction boundary",
        )
    })?;
    source.seek(SeekFrom::Start(0)).map_err(|error| {
        validation_error(original, "cannot seek pinned source metadata", &error)
    })?;
    installed
        .seek(SeekFrom::Start(0))
        .map_err(|error| validation_error(original, "cannot seek installed metadata", &error))?;
    if !compare_exact(source, installed, original.plan.metadata_end, original)? {
        return Ok(false);
    }

    source
        .seek(SeekFrom::Start(tail_start))
        .map_err(|error| validation_error(original, "cannot seek pinned source tail", &error))?;
    installed
        .seek(SeekFrom::Start(original.plan.metadata_end))
        .map_err(|error| validation_error(original, "cannot seek installed tail", &error))?;
    let tail_bytes = original
        .fingerprint
        .len
        .checked_sub(tail_start)
        .ok_or_else(|| {
            Error::for_path(
                ErrorKind::Replaced,
                &original.path,
                "installed validation has an invalid retained tail range",
            )
        })?;
    compare_exact(source, installed, tail_bytes, original)
}

fn compare_exact(
    left: &mut File,
    right: &mut File,
    mut remaining: u64,
    original: &Inspection,
) -> Result<bool, Error> {
    const BUFFER_BYTES: usize = 64 * 1024;
    let mut left_buffer = vec![0_u8; BUFFER_BYTES];
    let mut right_buffer = vec![0_u8; BUFFER_BYTES];
    while remaining > 0 {
        let buffer_bytes = u64::try_from(BUFFER_BYTES).map_err(|_| {
            Error::for_path(
                ErrorKind::Replaced,
                &original.path,
                "installed validation buffer size cannot be represented safely",
            )
        })?;
        let count = usize::try_from(remaining.min(buffer_bytes)).map_err(|_| {
            Error::for_path(
                ErrorKind::Replaced,
                &original.path,
                "installed validation range cannot be represented safely",
            )
        })?;
        left.read_exact(&mut left_buffer[..count])
            .map_err(|error| validation_error(original, "cannot read pinned source", &error))?;
        right
            .read_exact(&mut right_buffer[..count])
            .map_err(|error| {
                validation_error(original, "cannot read installed transcript", &error)
            })?;
        if left_buffer[..count] != right_buffer[..count] {
            return Ok(false);
        }
        let compared = u64::try_from(count).map_err(|_| {
            Error::for_path(
                ErrorKind::Replaced,
                &original.path,
                "installed validation byte count cannot be represented safely",
            )
        })?;
        remaining = remaining.checked_sub(compared).ok_or_else(|| {
            Error::for_path(
                ErrorKind::Replaced,
                &original.path,
                "installed validation byte counter underflowed",
            )
        })?;
    }
    Ok(true)
}

fn validation_error(original: &Inspection, operation: &str, error: &io::Error) -> Error {
    Error::for_path(
        ErrorKind::Replaced,
        &original.path,
        format!("{operation}: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File, OpenOptions};
    use std::io::{Seek, SeekFrom, Write};
    use std::os::unix::fs::MetadataExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::validate_installed;
    use crate::codex::{Fingerprint, Inspection, Plan};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn installed_validation_allows_append_but_rejects_prefix_change() {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "trimsess-installed-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("test directory should be created");
        let old_path = directory.join("old-inode");
        let installed_path = directory.join("rollout-synthetic.jsonl");
        let metadata = b"metadata\n";
        let discarded = b"discarded\n";
        let tail = b"tail\n";
        let appended = b"partial append without a newline";
        let mut old_bytes = metadata.to_vec();
        old_bytes.extend_from_slice(discarded);
        old_bytes.extend_from_slice(tail);
        let mut installed_bytes = metadata.to_vec();
        installed_bytes.extend_from_slice(tail);
        installed_bytes.extend_from_slice(appended);
        fs::write(&old_path, &old_bytes).expect("old source should be written");
        fs::write(&installed_path, &installed_bytes).expect("installed source should be written");

        let old_metadata = fs::metadata(&old_path).expect("old metadata should exist");
        let after_bytes =
            u64::try_from(metadata.len() + tail.len()).expect("fixture size should fit in u64");
        let original = Inspection {
            path: installed_path.clone(),
            fingerprint: fingerprint(&old_metadata),
            plan: Plan {
                session_id: "01900000-0000-7000-8000-000000000001".to_owned(),
                before_bytes: old_metadata.len(),
                after_bytes,
                saved_bytes: u64::try_from(discarded.len())
                    .expect("fixture size should fit in u64"),
                before_records: 3,
                after_records: 2,
                removed_records: 1,
                compaction_count: 1,
                newest_compaction_record: Some(2),
                metadata_end: u64::try_from(metadata.len())
                    .expect("fixture size should fit in u64"),
                retained_tail_start: Some(
                    u64::try_from(metadata.len() + discarded.len())
                        .expect("fixture size should fit in u64"),
                ),
            },
        };
        let mut source = File::open(&old_path).expect("old source should open");
        let mut installed = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&installed_path)
            .expect("installed source should open");

        validate_installed(&original, &mut source, &mut installed)
            .expect("a later append must not invalidate the installed prefix");

        installed
            .seek(SeekFrom::Start(0))
            .expect("installed source should seek");
        installed
            .write_all(b"X")
            .expect("installed prefix should change");
        installed.sync_all().expect("prefix change should sync");
        assert!(validate_installed(&original, &mut source, &mut installed).is_err());

        fs::remove_dir_all(directory).expect("test directory should be removed");
    }

    fn fingerprint(metadata: &fs::Metadata) -> Fingerprint {
        Fingerprint {
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
}
