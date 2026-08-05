//! Forward-only transcript scanning and retention-policy application.

use std::fs::File;
use std::io::{BufReader, Seek, SeekFrom};
use std::path::Path;

use super::jsonl;
use super::record::{
    self, CompactedPayload, Envelope, Event, InterAgentMetadataPayload, InterAgentPayload,
    MetadataPayload, RecordType, ResponseItem, TurnContextPayload, WorldStatePayload,
};
use super::retention::{Plan, ScanState};
use crate::{Error, ErrorKind};

pub(super) fn scan(
    file: &mut File,
    path: &Path,
    source_bytes: u64,
    verify_filename: bool,
) -> Result<Plan, Error> {
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        Error::for_path(
            ErrorKind::Unchanged,
            path,
            format!("cannot seek source: {error}"),
        )
    })?;
    let mut reader = BufReader::new(file);
    let mut record = Vec::new();
    let mut state = ScanState::default();

    loop {
        let next_record = state.next_record_number(path)?;
        let record_bytes = jsonl::read_record(&mut reader, &mut record, path, next_record)?;
        let Some(record_bytes) = record_bytes else {
            break;
        };
        let start = state.accept_record(next_record, record_bytes, path)?;
        let envelope = record::parse_envelope(&record, next_record, path)?;
        apply_record_policy(
            &mut state,
            &envelope,
            start,
            next_record,
            path,
            verify_filename,
        )?;
    }
    state.finish(path, source_bytes)
}

// Parse and validate the common top-level authority before format-specific
// policy sees a record.
fn apply_record_policy(
    state: &mut ScanState,
    envelope: &Envelope<'_>,
    record_start: u64,
    record_number: u64,
    path: &Path,
    verify_filename: bool,
) -> Result<(), Error> {
    if record_number == 1 {
        if envelope.record_type != RecordType::SessionMeta {
            return Err(Error::for_path(
                ErrorKind::Unsupported,
                path,
                "record 1 is not session_meta",
            ));
        }
        let metadata: MetadataPayload<'_> =
            record::parse_payload(envelope.payload, envelope.record_type, record_number, path)?;
        let metadata_layout = record::validate_canonical_metadata(&metadata, path)?;
        if verify_filename {
            record::verify_filename_session_id(path, metadata.id())?;
        }
        state.session_id = Some(metadata.id().to_owned());
        state.metadata_end = state.offset;
        state.note_canonical_metadata(metadata_layout);
        return Ok(());
    }

    match envelope.record_type {
        RecordType::SessionMeta => {
            if !state.accepts_copied_metadata() {
                return Err(Error::for_path(
                    ErrorKind::Unsupported,
                    path,
                    format!(
                        "record {record_number} is an additional session_meta outside a copied legacy fork"
                    ),
                ));
            }
            let metadata: MetadataPayload<'_> =
                record::parse_payload(envelope.payload, envelope.record_type, record_number, path)?;
            record::validate_additional_metadata(&metadata, path)?;
        }
        RecordType::Compacted => {
            let compacted: CompactedPayload<'_> =
                record::parse_payload(envelope.payload, envelope.record_type, record_number, path)?;
            state.compaction_count = state.compaction_count.checked_add(1).ok_or_else(|| {
                Error::for_path(
                    ErrorKind::Unsupported,
                    path,
                    "compaction counter overflowed",
                )
            })?;
            state.newest_compaction_record = Some(record_number);
            state.retained_tail_start = Some(record_start);
            state.note_compaction(compacted.is_complete_base());
        }
        RecordType::TurnContext => {
            let context: TurnContextPayload<'_> =
                record::parse_payload(envelope.payload, envelope.record_type, record_number, path)?;
            record::validate_turn_context(&context, record_number, path)?;
            state.note_turn_context(context.turn_id());
        }
        RecordType::EventMessage => {
            let event: Event<'_> =
                record::parse_payload(envelope.payload, envelope.record_type, record_number, path)?;
            note_event(state, &event, record_number);
        }
        RecordType::ResponseItem => {
            let response: ResponseItem =
                record::parse_payload(envelope.payload, envelope.record_type, record_number, path)?;
            // A role=user message can be a contextual fragment rather than a
            // real user turn. EventMsg::UserMessage is the legacy authority.
            if response.is_agent_message() {
                state.note_user_boundary();
            }
        }
        RecordType::InterAgentCommunication => {
            let communication: InterAgentPayload<'_> =
                record::parse_payload(envelope.payload, envelope.record_type, record_number, path)?;
            record::validate_inter_agent(&communication, record_number, path)?;
            state.note_user_boundary();
        }
        RecordType::InterAgentCommunicationMetadata => {
            let _: InterAgentMetadataPayload =
                record::parse_payload(envelope.payload, envelope.record_type, record_number, path)?;
        }
        RecordType::WorldState => {
            let _: WorldStatePayload =
                record::parse_payload(envelope.payload, envelope.record_type, record_number, path)?;
        }
    }
    Ok(())
}

fn note_event(state: &mut ScanState, event: &Event<'_>, record_number: u64) {
    if let Some(turn_id) = event.turn_started() {
        state.note_turn_started(turn_id);
    } else if let Some(turn_id) = event.turn_completed() {
        state.note_turn_complete(turn_id);
    } else if event.is_turn_aborted() {
        state.note_turn_aborted();
    } else if event.is_user_message() {
        state.note_user_boundary();
    } else if event.is_rollback() {
        state.note_rollback(record_number);
    }
}
