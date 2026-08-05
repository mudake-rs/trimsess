//! Structural decoding of supported Codex records.

use std::borrow::Cow;
use std::path::Path;

use serde::Deserialize;
use serde_json::value::RawValue;

use crate::{Error, ErrorKind};

mod authority;
mod event;
mod metadata;
mod response;
mod turn_context;

pub(super) use authority::{
    MetadataLayout, validate_additional_metadata, validate_canonical_metadata,
    validate_inter_agent, validate_turn_context, verify_filename_session_id,
};
pub(super) use event::Event;
pub(super) use metadata::MetadataPayload;
use response::CheckpointHistory;
pub(super) use response::ResponseItem;
pub(super) use turn_context::TurnContextPayload;

// Serde borrows ordinary strings and allocates only when JSON unescaping is
// required. This keeps real multiline and non-ASCII transcript text valid.
pub(super) type WireString<'a> = Cow<'a, str>;

#[derive(Deserialize)]
struct WireEnvelope<'a> {
    timestamp: &'a str,
    #[serde(default)]
    ordinal: Present,
    #[serde(rename = "type")]
    record_type: &'a str,
    #[serde(borrow)]
    payload: &'a RawValue,
}

pub(super) struct Envelope<'a> {
    pub(super) record_type: RecordType,
    pub(super) payload: &'a RawValue,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RecordType {
    SessionMeta,
    ResponseItem,
    EventMessage,
    TurnContext,
    Compacted,
    WorldState,
    InterAgentCommunication,
    InterAgentCommunicationMetadata,
}

impl RecordType {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::SessionMeta => "session_meta",
            Self::ResponseItem => "response_item",
            Self::EventMessage => "event_msg",
            Self::TurnContext => "turn_context",
            Self::Compacted => "compacted",
            Self::WorldState => "world_state",
            Self::InterAgentCommunication => "inter_agent_communication",
            Self::InterAgentCommunicationMetadata => "inter_agent_communication_metadata",
        }
    }
}

#[derive(Deserialize)]
struct ObjectPayload {}

#[derive(Deserialize)]
pub(super) struct CompactedPayload<'a> {
    #[serde(borrow, rename = "message")]
    _message: WireString<'a>,
    #[serde(default)]
    replacement_history: Option<CheckpointHistory>,
    #[serde(default)]
    window_number: Option<u64>,
    #[serde(borrow, default, rename = "first_window_id")]
    _first_window_id: Option<WireString<'a>>,
    #[serde(borrow, default, rename = "previous_window_id")]
    _previous_window_id: Option<WireString<'a>>,
}

impl CompactedPayload<'_> {
    pub(super) const fn is_complete_base(&self) -> bool {
        self.replacement_history.is_some() && self.window_number.is_some()
    }
}

#[derive(Deserialize)]
pub(super) struct InterAgentPayload<'a> {
    #[serde(borrow, default, rename = "id")]
    _id: Option<WireString<'a>>,
    #[serde(borrow)]
    pub(super) author: WireString<'a>,
    #[serde(borrow)]
    pub(super) recipient: WireString<'a>,
    #[serde(borrow, default)]
    pub(super) other_recipients: Vec<WireString<'a>>,
    #[serde(borrow, rename = "content")]
    _content: WireString<'a>,
    #[serde(borrow, default, rename = "encrypted_content")]
    _encrypted_content: Option<WireString<'a>>,
    #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
    _passthrough: Option<InterAgentPassthrough<'a>>,
    #[serde(rename = "trigger_turn")]
    _trigger_turn: bool,
}

#[derive(Deserialize)]
struct InterAgentPassthrough<'a> {
    #[serde(borrow, default, rename = "turn_id")]
    _turn_id: Option<WireString<'a>>,
}

#[derive(Deserialize)]
pub(super) struct InterAgentMetadataPayload {
    #[serde(rename = "trigger_turn")]
    _trigger_turn: bool,
}

#[derive(Deserialize)]
pub(super) struct WorldStatePayload {
    #[serde(rename = "full")]
    _full: bool,
    #[serde(rename = "state")]
    _state: Present,
}

// Distinguishes an absent authority field from one present with any JSON value.
#[derive(Debug, Default)]
struct Present(bool);

pub(super) fn parse_envelope<'a>(
    record: &'a [u8],
    record_number: u64,
    path: &Path,
) -> Result<Envelope<'a>, Error> {
    let json = record.strip_suffix(b"\n").unwrap_or(record);
    let json = json.strip_suffix(b"\r").unwrap_or(json);
    let envelope: WireEnvelope<'_> = serde_json::from_slice(json).map_err(|error| {
        Error::for_path(
            ErrorKind::Unsupported,
            path,
            format!(
                "record {record_number} has malformed or unsupported top-level JSON at line {}, column {}",
                error.line(),
                error.column()
            ),
        )
    })?;
    if envelope.timestamp.is_empty() {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            format!("record {record_number} has an empty timestamp"),
        ));
    }
    if envelope.ordinal.0 {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            format!(
                "record {record_number} has a paginated ordinal; only legacy transcripts are supported"
            ),
        ));
    }
    let record_type = parse_record_type(envelope.record_type, record_number, path)?;
    let _: ObjectPayload = serde_json::from_str(envelope.payload.get()).map_err(|error| {
        Error::for_path(
            ErrorKind::Unsupported,
            path,
            format!(
                "record {record_number} type {} has an unsupported payload at line {}, column {}",
                record_type.as_str(),
                error.line(),
                error.column()
            ),
        )
    })?;
    Ok(Envelope {
        record_type,
        payload: envelope.payload,
    })
}

// Apply Codex retention policy without owning or copying record bytes.
pub(super) fn parse_payload<'a, T>(
    payload: &'a RawValue,
    record_type: RecordType,
    record_number: u64,
    path: &Path,
) -> Result<T, Error>
where
    T: Deserialize<'a>,
{
    serde_json::from_str(payload.get()).map_err(|error| {
        Error::for_path(
            ErrorKind::Unsupported,
            path,
            format!(
                "record {record_number} type {} has an unsupported payload at line {}, column {}",
                record_type.as_str(),
                error.line(),
                error.column()
            ),
        )
    })
}

fn parse_record_type(
    record_type: &str,
    record_number: u64,
    path: &Path,
) -> Result<RecordType, Error> {
    match record_type {
        "session_meta" => Ok(RecordType::SessionMeta),
        "response_item" => Ok(RecordType::ResponseItem),
        "event_msg" => Ok(RecordType::EventMessage),
        "turn_context" => Ok(RecordType::TurnContext),
        "compacted" => Ok(RecordType::Compacted),
        "world_state" => Ok(RecordType::WorldState),
        "inter_agent_communication" => Ok(RecordType::InterAgentCommunication),
        "inter_agent_communication_metadata" => Ok(RecordType::InterAgentCommunicationMetadata),
        _ => Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            format!("record {record_number} has an unsupported type"),
        )),
    }
}
