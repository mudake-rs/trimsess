//! Chunk-boundary-independent comparison of source and decoded backup bytes.

use std::io::Read;
use std::path::Path;

use crate::{Error, ErrorKind};

const BUFFER_BYTES: usize = 64 * 1024;

pub(super) fn readers<A: Read, B: Read>(
    mut source: A,
    mut backup: B,
    path: &Path,
) -> Result<(), Error> {
    let mut source_buffer = vec![0_u8; BUFFER_BYTES];
    let mut backup_buffer = vec![0_u8; BUFFER_BYTES];
    let (mut source_start, mut source_end) = (0_usize, 0_usize);
    let (mut backup_start, mut backup_end) = (0_usize, 0_usize);

    loop {
        if source_start == source_end {
            source_end = source.read(&mut source_buffer).map_err(|error| {
                Error::for_path(
                    ErrorKind::Unchanged,
                    path,
                    format!("source read during backup validation failed: {error}"),
                )
            })?;
            source_start = 0;
        }
        if backup_start == backup_end {
            backup_end = backup.read(&mut backup_buffer).map_err(|error| {
                Error::for_path(
                    ErrorKind::Unchanged,
                    path,
                    format!("backup decode during validation failed: {error}"),
                )
            })?;
            backup_start = 0;
        }

        if source_end == 0 || backup_end == 0 {
            return if source_end == backup_end {
                Ok(())
            } else {
                Err(Error::for_path(
                    ErrorKind::Unchanged,
                    path,
                    "backup validation length mismatch; source not replaced",
                ))
            };
        }

        let compared = (source_end - source_start).min(backup_end - backup_start);
        if source_buffer[source_start..source_start + compared]
            != backup_buffer[backup_start..backup_start + compared]
        {
            return Err(Error::for_path(
                ErrorKind::Unchanged,
                path,
                "backup validation content mismatch; source not replaced",
            ));
        }
        source_start += compared;
        backup_start += compared;
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::path::Path;

    use super::readers;

    #[test]
    fn comparison_accepts_different_read_chunk_sizes() {
        let bytes = b"synthetic backup bytes across uneven chunks";
        let source = Chunked::new(bytes, 3);
        let backup = Chunked::new(bytes, 11);
        readers(source, backup, Path::new("synthetic.jsonl"))
            .expect("equal readers should compare exactly");
    }

    struct Chunked<'a> {
        bytes: &'a [u8],
        offset: usize,
        chunk: usize,
    }

    impl<'a> Chunked<'a> {
        const fn new(bytes: &'a [u8], chunk: usize) -> Self {
            Self {
                bytes,
                offset: 0,
                chunk,
            }
        }
    }

    impl Read for Chunked<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let remaining = self.bytes.len().saturating_sub(self.offset);
            let count = remaining.min(self.chunk).min(buffer.len());
            buffer[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
            self.offset += count;
            Ok(count)
        }
    }
}
