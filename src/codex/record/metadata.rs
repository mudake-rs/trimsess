//! Wire decoding for session identity and history authority.

use serde::Deserialize;
use serde::de::{Deserializer, IgnoredAny};

use super::{Present, WireString};

#[derive(Deserialize)]
pub(in crate::codex) struct MetadataPayload<'a> {
    #[serde(borrow, default)]
    pub(super) session_id: OptionalString<'a>,
    #[serde(borrow)]
    pub(super) id: WireString<'a>,
    #[serde(borrow)]
    pub(super) timestamp: WireString<'a>,
    #[serde(borrow)]
    pub(super) cwd: WireString<'a>,
    #[serde(borrow)]
    pub(super) originator: WireString<'a>,
    #[serde(borrow)]
    pub(super) cli_version: WireString<'a>,
    #[serde(borrow, default)]
    pub(super) forked_from_id: OptionalString<'a>,
    #[serde(borrow, default)]
    pub(super) parent_thread_id: OptionalString<'a>,
    #[serde(default)]
    source: SessionSource<'a>,
    #[serde(borrow, default, rename = "thread_source")]
    _thread_source: Option<WireString<'a>>,
    #[serde(borrow, default, rename = "agent_nickname")]
    _agent_nickname: Option<WireString<'a>>,
    #[serde(borrow, default, alias = "agent_type", rename = "agent_role")]
    _agent_role: Option<WireString<'a>>,
    #[serde(borrow, default, rename = "agent_path")]
    _agent_path: Option<WireString<'a>>,
    #[serde(borrow, default, rename = "model_provider")]
    _model_provider: Option<WireString<'a>>,
    #[serde(borrow, default, rename = "base_instructions")]
    _base_instructions: Option<BaseInstructions<'a>>,
    #[serde(default, rename = "dynamic_tools")]
    _dynamic_tools: Option<Vec<JsonObject>>,
    #[serde(borrow, default, rename = "selected_capability_roots")]
    _selected_capability_roots: Vec<CapabilityRoot<'a>>,
    #[serde(borrow, default, rename = "memory_mode")]
    _memory_mode: Option<WireString<'a>>,
    #[serde(borrow, default)]
    pub(super) history_mode: OptionalString<'a>,
    #[serde(default)]
    pub(super) history_base: Present,
    #[serde(default)]
    pub(super) subagent_history_start_ordinal: Present,
    #[serde(default, rename = "multi_agent_version")]
    _multi_agent_version: Option<MultiAgentVersion>,
    #[serde(borrow, default, rename = "context_window")]
    _context_window: Option<SessionContextWindow<'a>>,
    #[serde(borrow, default, rename = "git")]
    _git: Option<GitInfo<'a>>,
}

impl MetadataPayload<'_> {
    pub(in crate::codex) fn id(&self) -> &str {
        self.id.as_ref()
    }

    pub(super) const fn has_child_source(&self) -> bool {
        matches!(&self.source, SessionSource::SubAgent(_))
    }
}

// Missing `session_id` is accepted because Codex migrates it from `id`.
// Explicit null is not a valid `SessionId` and must remain distinguishable.
#[derive(Default)]
pub(super) struct OptionalString<'a>(pub(super) Option<WireString<'a>>);

impl<'de: 'a, 'a> Deserialize<'de> for OptionalString<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        WireString::deserialize(deserializer).map(|value| Self(Some(value)))
    }
}

#[derive(Deserialize)]
struct JsonObject {}

#[derive(Deserialize)]
struct BaseInstructions<'a> {
    #[serde(borrow, rename = "text")]
    _text: WireString<'a>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapabilityRoot<'a> {
    #[serde(borrow, rename = "id")]
    _id: WireString<'a>,
    #[serde(borrow, rename = "location")]
    _location: CapabilityLocation<'a>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum CapabilityLocation<'a> {
    Environment {
        #[serde(borrow, rename = "environmentId")]
        _environment_id: WireString<'a>,
        #[serde(borrow, rename = "path")]
        _path: WireString<'a>,
    },
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "lowercase")]
#[expect(
    dead_code,
    reason = "variant payloads exist only to validate wire data"
)]
enum SessionSource<'a> {
    Cli,
    #[default]
    #[serde(rename = "vscode")]
    VsCode,
    Exec,
    Mcp,
    Custom(#[serde(borrow)] WireString<'a>),
    Internal(InternalSessionSource),
    SubAgent(SubAgentSource<'a>),
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum InternalSessionSource {
    MemoryConsolidation,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
#[expect(
    dead_code,
    reason = "variant payloads exist only to validate wire data"
)]
enum SubAgentSource<'a> {
    Review,
    Compact,
    ThreadSpawn {
        #[serde(borrow, rename = "parent_thread_id")]
        _parent_thread_id: WireString<'a>,
        #[serde(rename = "depth")]
        _depth: i32,
        #[serde(borrow, default, rename = "agent_path")]
        _agent_path: Option<WireString<'a>>,
        #[serde(borrow, default, rename = "agent_nickname")]
        _agent_nickname: Option<WireString<'a>>,
        #[serde(borrow, default, alias = "agent_type", rename = "agent_role")]
        _agent_role: Option<WireString<'a>>,
    },
    MemoryConsolidation,
    Other(#[serde(borrow)] WireString<'a>),
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum MultiAgentVersion {
    Disabled,
    V1,
    V2,
}

#[derive(Deserialize)]
struct SessionContextWindow<'a> {
    #[serde(borrow, rename = "window_id")]
    _window_id: WireString<'a>,
}

#[derive(Deserialize)]
struct GitInfo<'a> {
    #[serde(borrow, default, rename = "commit_hash")]
    _commit_hash: Option<WireString<'a>>,
    #[serde(borrow, default, rename = "branch")]
    _branch: Option<WireString<'a>>,
    #[serde(borrow, default, rename = "repository_url")]
    _repository_url: Option<WireString<'a>>,
}

impl<'de> Deserialize<'de> for Present {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IgnoredAny::deserialize(deserializer).map(|_| Self(true))
    }
}
