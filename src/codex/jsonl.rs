//! Bounded JSONL record streaming with oversized-record draining.

use std::io::{BufRead, Read};
use std::path::Path;

use crate::{Error, ErrorKind};

const MAX_RECORD_BYTES: usize = 128 * 1024 * 1024;

pub(super) fn read_record<R: BufRead>(
    reader: &mut R,
    record: &mut Vec<u8>,
    path: &Path,
    record_number: u64,
) -> Result<Option<u64>, Error> {
    record.clear();
    let limit = u64::try_from(MAX_RECORD_BYTES)
        .map_err(|_| Error::new(ErrorKind::Unsupported, "record limit is invalid"))?
        .checked_add(1)
        .ok_or_else(|| Error::new(ErrorKind::Unsupported, "record limit overflowed"))?;
    let mut limited = reader.take(limit);
    let bytes = limited.read_until(b'\n', record).map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            path,
            format!("cannot read record {record_number}: {error}"),
        )
    })?;
    if bytes == 0 {
        return Ok(None);
    }
    if bytes <= MAX_RECORD_BYTES {
        return u64::try_from(bytes).map(Some).map_err(|_| {
            Error::for_path(
                ErrorKind::Unsupported,
                path,
                format!("record {record_number} byte count overflowed"),
            )
        });
    }

    let mut actual = u64::try_from(bytes).map_err(|_| {
        Error::for_path(
            ErrorKind::Unsupported,
            path,
            format!("record {record_number} byte count overflowed"),
        )
    })?;
    if record.last() != Some(&b'\n') {
        actual = consume_record_remainder(reader, actual, path, record_number)?;
    }
    Err(Error::for_path(
        ErrorKind::Unsupported,
        path,
        format!(
            "record {record_number} is {actual} bytes, exceeding the {MAX_RECORD_BYTES}-byte limit"
        ),
    ))
}

fn consume_record_remainder<R: BufRead>(
    reader: &mut R,
    mut total: u64,
    path: &Path,
    record_number: u64,
) -> Result<u64, Error> {
    loop {
        let available = reader.fill_buf().map_err(|error| {
            Error::for_path(
                ErrorKind::Unchanged,
                path,
                format!("cannot finish reading oversized record {record_number}: {error}"),
            )
        })?;
        if available.is_empty() {
            return Ok(total);
        }
        let consumed = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        let ended = available.get(consumed.saturating_sub(1)) == Some(&b'\n');
        total = total
            .checked_add(u64::try_from(consumed).map_err(|_| {
                Error::for_path(
                    ErrorKind::Unsupported,
                    path,
                    format!("record {record_number} byte count overflowed"),
                )
            })?)
            .ok_or_else(|| {
                Error::for_path(
                    ErrorKind::Unsupported,
                    path,
                    format!("record {record_number} byte count overflowed"),
                )
            })?;
        reader.consume(consumed);
        if ended {
            return Ok(total);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Read};
    use std::path::Path;

    use super::{MAX_RECORD_BYTES, read_record};

    #[test]
    fn record_size_limit_accepts_boundary_and_rejects_next_byte() {
        let mut record = Vec::new();
        let mut at_limit = BufReader::new(RepeatedLine::new(MAX_RECORD_BYTES));
        let bytes = read_record(&mut at_limit, &mut record, Path::new("synthetic.jsonl"), 1)
            .expect("record at limit should be readable");
        assert_eq!(bytes, Some(MAX_RECORD_BYTES as u64));

        let mut over_limit = BufReader::new(RepeatedLine::new(MAX_RECORD_BYTES + 1));
        let error = read_record(
            &mut over_limit,
            &mut record,
            Path::new("synthetic.jsonl"),
            1,
        )
        .expect_err("record over limit should fail");
        let oversized = format!("{} bytes", MAX_RECORD_BYTES + 1);
        assert!(error.message.contains(&oversized));
    }

    struct RepeatedLine {
        remaining: usize,
    }

    impl RepeatedLine {
        const fn new(bytes: usize) -> Self {
            Self { remaining: bytes }
        }
    }

    impl Read for RepeatedLine {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.remaining == 0 || buffer.is_empty() {
                return Ok(0);
            }
            let count = buffer.len().min(self.remaining);
            for (index, byte) in buffer[..count].iter_mut().enumerate() {
                *byte = if self.remaining - index == 1 {
                    b'\n'
                } else {
                    b'x'
                };
            }
            self.remaining -= count;
            Ok(count)
        }
    }
}
