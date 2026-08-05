//! Retention state and the byte ranges produced by one transcript scan.

use std::path::Path;

use super::record::MetadataLayout;
use crate::{Error, ErrorKind};

/// Complete byte-range and count authority produced by one validated scan.
#[derive(Debug, Clone)]
pub struct Plan {
    pub session_id: String,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub saved_bytes: u64,
    pub before_records: u64,
    pub after_records: u64,
    pub removed_records: u64,
    pub compaction_count: u64,
    pub newest_compaction_record: Option<u64>,
    pub metadata_end: u64,
    pub retained_tail_start: Option<u64>,
}

// Owns every counter and retained boundary produced by one forward-only scan.
#[derive(Default)]
pub(super) struct ScanState {
    pub(super) record_number: u64,
    pub(super) offset: u64,
    pub(super) metadata_end: u64,
    pub(super) session_id: Option<String>,
    pub(super) compaction_count: u64,
    pub(super) newest_compaction_record: Option<u64>,
    pub(super) retained_tail_start: Option<u64>,
    metadata_layout: Option<MetadataLayout>,
    newest_compaction_is_complete_base: bool,
    post_compaction_turn: Option<TailTurn>,
    has_post_compaction_resume_turn: bool,
    post_compaction_rollback: Option<u64>,
}

// Minimal surviving turn state needed to prove resume settings remain in the
// retained tail.
#[derive(Default)]
struct TailTurn {
    turn_id: Option<String>,
    counts_as_user_turn: bool,
    has_turn_context: bool,
}

impl ScanState {
    pub(super) const fn note_canonical_metadata(&mut self, layout: MetadataLayout) {
        self.metadata_layout = Some(layout);
    }

    pub(super) fn accepts_copied_metadata(&self) -> bool {
        self.metadata_layout == Some(MetadataLayout::CopiedFork)
    }

    pub(super) fn next_record_number(&self, path: &Path) -> Result<u64, Error> {
        self.record_number.checked_add(1).ok_or_else(|| {
            Error::for_path(ErrorKind::Unsupported, path, "record counter overflowed")
        })
    }

    pub(super) fn accept_record(
        &mut self,
        record_number: u64,
        record_bytes: u64,
        path: &Path,
    ) -> Result<u64, Error> {
        let start = self.offset;
        self.offset = self.offset.checked_add(record_bytes).ok_or_else(|| {
            Error::for_path(ErrorKind::Unsupported, path, "byte counter overflowed")
        })?;
        self.record_number = record_number;
        Ok(start)
    }

    pub(super) fn finish(self, path: &Path, source_bytes: u64) -> Result<Plan, Error> {
        if self.offset != source_bytes {
            return Err(Error::for_path(
                ErrorKind::Unchanged,
                path,
                format!(
                    "source length changed while scanning: expected {source_bytes} bytes, read {}",
                    self.offset
                ),
            ));
        }
        self.validate_retention_shape(path)?;
        let (after_bytes, after_records) = self.projected_counts(path, source_bytes)?;
        let removed_records = self
            .record_number
            .checked_sub(after_records)
            .ok_or_else(|| {
                Error::for_path(
                    ErrorKind::Unsupported,
                    path,
                    "projected record count exceeds source count",
                )
            })?;
        let saved_bytes = source_bytes.checked_sub(after_bytes).ok_or_else(|| {
            Error::for_path(
                ErrorKind::Unsupported,
                path,
                "projected byte count exceeds source size",
            )
        })?;
        let session_id = self.session_id.ok_or_else(|| {
            Error::for_path(
                ErrorKind::Unsupported,
                path,
                "transcript is empty or missing session_meta",
            )
        })?;

        Ok(Plan {
            session_id,
            before_bytes: source_bytes,
            after_bytes,
            saved_bytes,
            before_records: self.record_number,
            after_records,
            removed_records,
            compaction_count: self.compaction_count,
            newest_compaction_record: self.newest_compaction_record,
            metadata_end: self.metadata_end,
            retained_tail_start: self.retained_tail_start,
        })
    }

    // A retained checkpoint is safe only when it is a complete history base
    // and the surviving tail can reconstruct resume settings.
    fn validate_retention_shape(&self, path: &Path) -> Result<(), Error> {
        if let Some(boundary) = self.newest_compaction_record {
            if !self.newest_compaction_is_complete_base {
                return Err(Error::for_path(
                    ErrorKind::Unsupported,
                    path,
                    format!(
                        concat!(
                            "newest compacted record {boundary} is not a complete ",
                            "replacement-history checkpoint"
                        ),
                        boundary = boundary
                    ),
                ));
            }
            if let Some(record) = self.post_compaction_rollback {
                return Err(Error::for_path(
                    ErrorKind::Unsupported,
                    path,
                    format!(
                        concat!(
                            "thread rollback record {record} follows newest compacted record ",
                            "{boundary}; unsupported retention shape"
                        ),
                        record = record,
                        boundary = boundary
                    ),
                ));
            }
            if !self.has_post_compaction_resume_turn {
                return Err(Error::for_path(
                    ErrorKind::Unsupported,
                    path,
                    format!(
                        concat!(
                            "no completed user turn with turn_context follows newest compacted record ",
                            "{boundary}; unsupported retention shape"
                        ),
                        boundary = boundary
                    ),
                ));
            }
        }
        Ok(())
    }

    // Project the two byte ranges copied by the rewrite: session metadata and
    // the newest checkpoint plus its complete tail.
    fn projected_counts(&self, path: &Path, source_bytes: u64) -> Result<(u64, u64), Error> {
        match (self.retained_tail_start, self.newest_compaction_record) {
            (Some(tail_start), Some(boundary)) => {
                let tail_bytes = source_bytes.checked_sub(tail_start).ok_or_else(|| {
                    Error::for_path(
                        ErrorKind::Unsupported,
                        path,
                        "retained byte range is invalid",
                    )
                })?;
                let after_bytes = self.metadata_end.checked_add(tail_bytes).ok_or_else(|| {
                    Error::for_path(
                        ErrorKind::Unsupported,
                        path,
                        "projected byte count overflowed",
                    )
                })?;
                let tail_records = self
                    .record_number
                    .checked_sub(boundary)
                    .and_then(|count| count.checked_add(1))
                    .ok_or_else(|| {
                        Error::for_path(
                            ErrorKind::Unsupported,
                            path,
                            "retained record range is invalid",
                        )
                    })?;
                let after_records = tail_records.checked_add(1).ok_or_else(|| {
                    Error::for_path(
                        ErrorKind::Unsupported,
                        path,
                        "projected record count overflowed",
                    )
                })?;
                Ok((after_bytes, after_records))
            }
            (None, None) => Ok((source_bytes, self.record_number)),
            _ => Err(Error::for_path(
                ErrorKind::Unsupported,
                path,
                "compaction boundary state is inconsistent",
            )),
        }
    }

    pub(super) fn note_compaction(&mut self, is_complete_base: bool) {
        self.newest_compaction_is_complete_base = is_complete_base;
        self.post_compaction_turn = None;
        self.has_post_compaction_resume_turn = false;
        self.post_compaction_rollback = None;
    }

    pub(super) fn note_turn_started(&mut self, turn_id: &str) {
        if self.newest_compaction_record.is_some() {
            self.post_compaction_turn = Some(TailTurn {
                turn_id: Some(turn_id.to_owned()),
                ..TailTurn::default()
            });
        }
    }

    pub(super) fn note_user_boundary(&mut self) {
        if self.newest_compaction_record.is_some() {
            self.post_compaction_turn
                .get_or_insert_with(TailTurn::default)
                .counts_as_user_turn = true;
        }
    }

    pub(super) fn note_turn_context(&mut self, turn_id: Option<&str>) {
        if self.newest_compaction_record.is_none() {
            return;
        }
        let turn = self
            .post_compaction_turn
            .get_or_insert_with(TailTurn::default);
        if turn_ids_are_compatible(turn.turn_id.as_deref(), turn_id) {
            if turn.turn_id.is_none() {
                turn.turn_id = turn_id.map(str::to_owned);
            }
            turn.has_turn_context = true;
        }
    }

    pub(super) fn note_turn_complete(&mut self, turn_id: &str) {
        if self.newest_compaction_record.is_none() {
            return;
        }
        if self.post_compaction_turn.as_ref().is_some_and(|turn| {
            turn_ids_are_compatible(turn.turn_id.as_deref(), Some(turn_id))
                && turn.counts_as_user_turn
                && turn.has_turn_context
        }) {
            self.has_post_compaction_resume_turn = true;
        }
        self.post_compaction_turn = None;
    }

    pub(super) fn note_turn_aborted(&mut self) {
        if self.newest_compaction_record.is_some() {
            self.post_compaction_turn = None;
        }
    }

    pub(super) const fn note_rollback(&mut self, record_number: u64) {
        if self.newest_compaction_record.is_some() {
            self.post_compaction_rollback = Some(record_number);
        }
    }
}

fn turn_ids_are_compatible(left: Option<&str>, right: Option<&str>) -> bool {
    left.is_none_or(|left| right.is_none_or(|right| left == right))
}
