//! Nested wire types used by checkpoint response items.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::super::WireString;

#[derive(Deserialize)]
pub(super) struct MetadataPassthrough<'a> {
    #[serde(borrow, default, rename = "turn_id")]
    _turn_id: Option<WireString<'a>>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ContentItem<'a> {
    InputText {
        #[serde(borrow, rename = "text")]
        _text: WireString<'a>,
    },
    InputImage {
        #[serde(borrow, rename = "image_url")]
        _image_url: WireString<'a>,
        #[serde(default, rename = "detail")]
        _detail: Option<ImageDetail>,
    },
    InputAudio {
        #[serde(borrow, rename = "audio_url")]
        _audio_url: WireString<'a>,
    },
    OutputText {
        #[serde(borrow, rename = "text")]
        _text: WireString<'a>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum AgentMessageContent<'a> {
    InputText {
        #[serde(borrow, rename = "text")]
        _text: WireString<'a>,
    },
    EncryptedContent {
        #[serde(borrow, rename = "encrypted_content")]
        _encrypted_content: WireString<'a>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MessagePhase {
    Commentary,
    FinalAnswer,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ReasoningSummary<'a> {
    SummaryText {
        #[serde(borrow, rename = "text")]
        _text: WireString<'a>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ReasoningContent<'a> {
    ReasoningText {
        #[serde(borrow, rename = "text")]
        _text: WireString<'a>,
    },
    Text {
        #[serde(borrow, rename = "text")]
        _text: WireString<'a>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum LocalShellStatus {
    Completed,
    InProgress,
    Incomplete,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum LocalShellAction<'a> {
    Exec {
        #[serde(borrow, rename = "command")]
        _command: Vec<WireString<'a>>,
        #[serde(default, rename = "timeout_ms")]
        _timeout_ms: Option<u64>,
        #[serde(borrow, default, rename = "working_directory")]
        _working_directory: Option<WireString<'a>>,
        #[serde(borrow, default, rename = "env")]
        _env: Option<BTreeMap<WireString<'a>, WireString<'a>>>,
        #[serde(borrow, default, rename = "user")]
        _user: Option<WireString<'a>>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum WebSearchAction<'a> {
    Search {
        #[serde(borrow, default, rename = "query")]
        _query: Option<WireString<'a>>,
        #[serde(borrow, default, rename = "queries")]
        _queries: Option<Vec<WireString<'a>>>,
    },
    OpenPage {
        #[serde(borrow, default, rename = "url")]
        _url: Option<WireString<'a>>,
    },
    FindInPage {
        #[serde(borrow, default, rename = "url")]
        _url: Option<WireString<'a>>,
        #[serde(borrow, default, rename = "pattern")]
        _pattern: Option<WireString<'a>>,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum ImageDetail {
    Auto,
    Low,
    High,
    Original,
}

#[derive(Deserialize)]
#[serde(untagged)]
#[expect(dead_code, reason = "fields exist only to validate Codex wire values")]
pub(super) enum FunctionOutput<'a> {
    Text(#[serde(borrow)] WireString<'a>),
    ContentItems(#[serde(borrow)] Vec<FunctionOutputContent<'a>>),
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum FunctionOutputContent<'a> {
    InputText {
        #[serde(borrow, rename = "text")]
        _text: WireString<'a>,
    },
    InputImage {
        #[serde(borrow, rename = "image_url")]
        _image_url: WireString<'a>,
        #[serde(default, rename = "detail")]
        _detail: Option<ImageDetail>,
    },
    InputAudio {
        #[serde(borrow, rename = "audio_url")]
        _audio_url: WireString<'a>,
    },
    EncryptedContent {
        #[serde(borrow, rename = "encrypted_content")]
        _encrypted_content: WireString<'a>,
    },
}
