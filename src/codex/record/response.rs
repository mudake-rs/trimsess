//! Codex wire types required to validate a compaction checkpoint.

use serde::Deserialize;
use serde::de::{IgnoredAny, SeqAccess, Visitor};
use std::fmt;

use super::WireString;

mod types;

use types::{
    AgentMessageContent, ContentItem, FunctionOutput, LocalShellAction, LocalShellStatus,
    MessagePhase, MetadataPassthrough, ReasoningContent, ReasoningSummary, WebSearchAction,
};

/// Validates the array while deliberately retaining none of its private data.
pub(super) struct CheckpointHistory;

impl<'de> Deserialize<'de> for CheckpointHistory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(CheckpointHistoryVisitor)
    }
}

struct CheckpointHistoryVisitor;

impl<'de> Visitor<'de> for CheckpointHistoryVisitor {
    type Value = CheckpointHistory;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of Codex response items")
    }

    fn visit_seq<A>(self, mut items: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while items
            .next_element::<CheckpointResponseItem<'de>>()?
            .is_some()
        {}
        Ok(CheckpointHistory)
    }
}

/// Classification retained after a standalone response item is fully decoded.
pub(in crate::codex) struct ResponseItem {
    is_agent_message: bool,
}

impl ResponseItem {
    pub(in crate::codex) const fn is_agent_message(&self) -> bool {
        self.is_agent_message
    }
}

impl<'de> Deserialize<'de> for ResponseItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let item = CheckpointResponseItem::deserialize(deserializer)?;
        Ok(Self {
            is_agent_message: matches!(item, CheckpointResponseItem::AgentMessage { .. }),
        })
    }
}

/// A checkpoint is authoritative only if Codex can decode every replacement
/// item. Unknown item types remain valid because Codex maps them to `Other`.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CheckpointResponseItem<'a> {
    AdditionalTools {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, rename = "role")]
        _role: WireString<'a>,
        #[serde(rename = "tools")]
        _tools: Vec<IgnoredAny>,
    },
    Message {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, rename = "role")]
        _role: WireString<'a>,
        #[serde(borrow, rename = "content")]
        _content: Vec<ContentItem<'a>>,
        #[serde(default, rename = "phase")]
        _phase: Option<MessagePhase>,
        #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
        _passthrough: Option<MetadataPassthrough<'a>>,
    },
    AgentMessage {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, rename = "author")]
        _author: WireString<'a>,
        #[serde(borrow, rename = "recipient")]
        _recipient: WireString<'a>,
        #[serde(borrow, rename = "content")]
        _content: Vec<AgentMessageContent<'a>>,
        #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
        _passthrough: Option<MetadataPassthrough<'a>>,
    },
    Reasoning {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, rename = "summary")]
        _summary: Vec<ReasoningSummary<'a>>,
        #[serde(borrow, default, rename = "content")]
        _content: Option<Vec<ReasoningContent<'a>>>,
        #[serde(borrow, default, rename = "encrypted_content")]
        _encrypted_content: Option<WireString<'a>>,
        #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
        _passthrough: Option<MetadataPassthrough<'a>>,
    },
    LocalShellCall {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, default, rename = "call_id")]
        _call_id: Option<WireString<'a>>,
        #[serde(rename = "status")]
        _status: LocalShellStatus,
        #[serde(borrow, rename = "action")]
        _action: LocalShellAction<'a>,
        #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
        _passthrough: Option<MetadataPassthrough<'a>>,
    },
    FunctionCall {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, rename = "name")]
        _name: WireString<'a>,
        #[serde(borrow, default, rename = "namespace")]
        _namespace: Option<WireString<'a>>,
        #[serde(borrow, rename = "arguments")]
        _arguments: WireString<'a>,
        #[serde(borrow, rename = "call_id")]
        _call_id: WireString<'a>,
        #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
        _passthrough: Option<MetadataPassthrough<'a>>,
    },
    ToolSearchCall {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, default, rename = "call_id")]
        _call_id: Option<WireString<'a>>,
        #[serde(borrow, default, rename = "status")]
        _status: Option<WireString<'a>>,
        #[serde(borrow, rename = "execution")]
        _execution: WireString<'a>,
        #[serde(rename = "arguments")]
        _arguments: IgnoredAny,
        #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
        _passthrough: Option<MetadataPassthrough<'a>>,
    },
    FunctionCallOutput {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, rename = "call_id")]
        _call_id: WireString<'a>,
        #[serde(borrow, rename = "output")]
        _output: FunctionOutput<'a>,
        #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
        _passthrough: Option<MetadataPassthrough<'a>>,
    },
    CustomToolCall {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, default, rename = "status")]
        _status: Option<WireString<'a>>,
        #[serde(borrow, rename = "call_id")]
        _call_id: WireString<'a>,
        #[serde(borrow, rename = "name")]
        _name: WireString<'a>,
        #[serde(borrow, default, rename = "namespace")]
        _namespace: Option<WireString<'a>>,
        #[serde(borrow, rename = "input")]
        _input: WireString<'a>,
        #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
        _passthrough: Option<MetadataPassthrough<'a>>,
    },
    CustomToolCallOutput {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, rename = "call_id")]
        _call_id: WireString<'a>,
        #[serde(borrow, default, rename = "name")]
        _name: Option<WireString<'a>>,
        #[serde(borrow, rename = "output")]
        _output: FunctionOutput<'a>,
        #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
        _passthrough: Option<MetadataPassthrough<'a>>,
    },
    ToolSearchOutput {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, default, rename = "call_id")]
        _call_id: Option<WireString<'a>>,
        #[serde(borrow, rename = "status")]
        _status: WireString<'a>,
        #[serde(borrow, rename = "execution")]
        _execution: WireString<'a>,
        #[serde(rename = "tools")]
        _tools: Vec<IgnoredAny>,
        #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
        _passthrough: Option<MetadataPassthrough<'a>>,
    },
    WebSearchCall {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, default, rename = "status")]
        _status: Option<WireString<'a>>,
        #[serde(borrow, default, rename = "action")]
        _action: Option<WebSearchAction<'a>>,
        #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
        _passthrough: Option<MetadataPassthrough<'a>>,
    },
    ImageGenerationCall {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, rename = "status")]
        _status: WireString<'a>,
        #[serde(borrow, default, rename = "revised_prompt")]
        _revised_prompt: Option<WireString<'a>>,
        #[serde(borrow, rename = "result")]
        _result: WireString<'a>,
        #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
        _passthrough: Option<MetadataPassthrough<'a>>,
    },
    #[serde(alias = "compaction_summary")]
    Compaction {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, rename = "encrypted_content")]
        _encrypted_content: WireString<'a>,
        #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
        _passthrough: Option<MetadataPassthrough<'a>>,
    },
    CompactionTrigger {},
    ContextCompaction {
        #[serde(borrow, default, rename = "id")]
        _id: Option<WireString<'a>>,
        #[serde(borrow, default, rename = "encrypted_content")]
        _encrypted_content: Option<WireString<'a>>,
        #[serde(borrow, default, rename = "internal_chat_message_metadata_passthrough")]
        _passthrough: Option<MetadataPassthrough<'a>>,
    },
    #[serde(other)]
    Other,
}
