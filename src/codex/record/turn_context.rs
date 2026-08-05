//! Wire decoding for the resume settings retained after compaction.

use std::path::Path;

use serde::Deserialize;

use super::WireString;

#[derive(Deserialize)]
pub(in crate::codex) struct TurnContextPayload<'a> {
    #[serde(borrow, default)]
    pub(super) turn_id: Option<WireString<'a>>,
    #[serde(borrow)]
    cwd: WireString<'a>,
    #[serde(borrow, default)]
    workspace_roots: Option<Vec<WireString<'a>>>,
    #[serde(borrow, default, rename = "current_date")]
    _current_date: Option<WireString<'a>>,
    #[serde(borrow, default, rename = "timezone")]
    _timezone: Option<WireString<'a>>,
    #[serde(rename = "approval_policy")]
    _approval_policy: ApprovalPolicy,
    #[serde(default, rename = "approvals_reviewer")]
    _approvals_reviewer: Option<ApprovalsReviewer>,
    sandbox_policy: SandboxPolicy<'a>,
    #[serde(default, rename = "permission_profile")]
    _permission_profile: Option<JsonObject>,
    #[serde(borrow, default, rename = "network")]
    _network: Option<Network<'a>>,
    #[serde(default, rename = "file_system_sandbox_policy")]
    _file_system_sandbox_policy: Option<JsonObject>,
    #[serde(borrow)]
    model: WireString<'a>,
    #[serde(borrow, default, rename = "comp_hash")]
    _comp_hash: Option<WireString<'a>>,
    #[serde(default, rename = "personality")]
    _personality: Option<Personality>,
    #[serde(borrow, default)]
    collaboration_mode: Option<CollaborationMode<'a>>,
    #[serde(default, rename = "multi_agent_version")]
    _multi_agent_version: Option<MultiAgentVersion>,
    #[serde(borrow, default, rename = "multi_agent_mode")]
    _multi_agent_mode: Option<MultiAgentMode<'a>>,
    #[serde(default, rename = "realtime_active")]
    _realtime_active: Option<bool>,
    #[serde(borrow, default)]
    effort: Option<WireString<'a>>,
    #[serde(rename = "summary")]
    _summary: ReasoningSummary,
}

impl TurnContextPayload<'_> {
    pub(in crate::codex) fn turn_id(&self) -> Option<&str> {
        self.turn_id.as_deref()
    }

    pub(super) fn has_valid_resume_values(&self) -> bool {
        !self.model.is_empty()
            && self
                .effort
                .as_deref()
                .is_none_or(|effort| !effort.is_empty())
            && is_absolute(&self.cwd)
            && self
                .workspace_roots
                .as_ref()
                .is_none_or(|roots| roots.iter().all(|root| is_absolute(root)))
            && self.sandbox_policy.has_valid_paths()
            && self
                .collaboration_mode
                .as_ref()
                .is_none_or(CollaborationMode::has_valid_values)
    }
}

fn is_absolute(path: &str) -> bool {
    Path::new(path).is_absolute()
}

#[derive(Deserialize)]
struct JsonObject {}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
#[expect(
    dead_code,
    reason = "variant payloads exist only to validate wire data"
)]
enum ApprovalPolicy {
    #[serde(rename = "untrusted")]
    UnlessTrusted,
    #[serde(alias = "on-failure")]
    OnRequest,
    Granular(GranularApproval),
    Never,
}

#[derive(Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Codex defines five independent granular approval flags"
)]
struct GranularApproval {
    #[serde(rename = "sandbox_approval")]
    _sandbox_approval: bool,
    #[serde(rename = "rules")]
    _rules: bool,
    #[serde(default, rename = "skill_approval")]
    _skill_approval: bool,
    #[serde(default, rename = "request_permissions")]
    _request_permissions: bool,
    #[serde(rename = "mcp_elicitations")]
    _mcp_elicitations: bool,
}

#[derive(Deserialize)]
enum ApprovalsReviewer {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "auto_review", alias = "guardian_subagent")]
    AutoReview,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum SandboxPolicy<'a> {
    #[serde(rename = "danger-full-access")]
    DangerFullAccess,
    #[serde(rename = "read-only")]
    ReadOnly {
        #[serde(default, rename = "network_access")]
        _network_access: bool,
    },
    #[serde(rename = "external-sandbox")]
    ExternalSandbox {
        #[serde(default, rename = "network_access")]
        _network_access: NetworkAccess,
    },
    #[serde(rename = "workspace-write")]
    WorkspaceWrite {
        #[serde(borrow, default)]
        writable_roots: Vec<WireString<'a>>,
        #[serde(default, rename = "network_access")]
        _network_access: bool,
        #[serde(default, rename = "exclude_tmpdir_env_var")]
        _exclude_tmpdir_env_var: bool,
        #[serde(default, rename = "exclude_slash_tmp")]
        _exclude_slash_tmp: bool,
    },
}

impl SandboxPolicy<'_> {
    fn has_valid_paths(&self) -> bool {
        match self {
            Self::WorkspaceWrite { writable_roots, .. } => {
                writable_roots.iter().all(|root| is_absolute(root))
            }
            Self::DangerFullAccess | Self::ReadOnly { .. } | Self::ExternalSandbox { .. } => true,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum NetworkAccess {
    #[default]
    Restricted,
    Enabled,
}

#[derive(Deserialize)]
struct Network<'a> {
    #[serde(borrow, rename = "allowed_domains")]
    _allowed_domains: Vec<WireString<'a>>,
    #[serde(borrow, rename = "denied_domains")]
    _denied_domains: Vec<WireString<'a>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Personality {
    None,
    Friendly,
    Pragmatic,
}

#[derive(Deserialize)]
struct CollaborationMode<'a> {
    #[serde(rename = "mode")]
    _mode: ModeKind,
    #[serde(borrow)]
    settings: CollaborationSettings<'a>,
}

impl CollaborationMode<'_> {
    fn has_valid_values(&self) -> bool {
        !self.settings.model.is_empty()
            && self
                .settings
                .reasoning_effort
                .as_deref()
                .is_none_or(|effort| !effort.is_empty())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ModeKind {
    Plan,
    #[serde(
        alias = "code",
        alias = "pair_programming",
        alias = "execute",
        alias = "custom"
    )]
    Default,
}

#[derive(Deserialize)]
struct CollaborationSettings<'a> {
    #[serde(borrow)]
    model: WireString<'a>,
    #[serde(borrow, default)]
    reasoning_effort: Option<WireString<'a>>,
    #[serde(borrow, default, rename = "developer_instructions")]
    _developer_instructions: Option<WireString<'a>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum MultiAgentVersion {
    Disabled,
    V1,
    V2,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    dead_code,
    reason = "variant payloads exist only to validate wire data"
)]
enum MultiAgentMode<'a> {
    None,
    Custom(#[serde(borrow)] WireString<'a>),
    ExplicitRequestOnly,
    Proactive,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum ReasoningSummary {
    Auto,
    Concise,
    Detailed,
    None,
}
