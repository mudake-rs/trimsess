//! Structural decoding for events that define retained turn boundaries.

use serde::Deserialize;

use super::WireString;

pub(in crate::codex) struct Event<'a> {
    boundary: Boundary<'a>,
}

#[derive(Clone)]
enum Boundary<'a> {
    TurnStarted(WireString<'a>),
    TurnComplete(WireString<'a>),
    UserMessage,
    TurnAborted,
    Rollback,
    Other,
}

impl<'de: 'a, 'a> Deserialize<'de> for Event<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let event = WireEvent::deserialize(deserializer)?;
        let boundary = match event {
            WireEvent::TurnStarted { turn_id, .. } => Boundary::TurnStarted(turn_id),
            WireEvent::TurnComplete { turn_id, .. } => Boundary::TurnComplete(turn_id),
            WireEvent::UserMessage { .. } => Boundary::UserMessage,
            WireEvent::ThreadRolledBack { .. } => Boundary::Rollback,
            WireEvent::TurnAborted { .. } => Boundary::TurnAborted,
            WireEvent::Other => Boundary::Other,
        };
        Ok(Self { boundary })
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireEvent<'a> {
    #[serde(rename = "task_started", alias = "turn_started")]
    TurnStarted {
        #[serde(borrow)]
        turn_id: WireString<'a>,
        #[serde(borrow, default, rename = "trace_id")]
        _trace_id: Option<WireString<'a>>,
        #[serde(default, rename = "started_at")]
        _started_at: Option<i64>,
        #[serde(default, rename = "model_context_window")]
        _model_context_window: Option<i64>,
        #[serde(default, rename = "collaboration_mode_kind")]
        _collaboration_mode_kind: ModeKind,
    },
    #[serde(rename = "task_complete", alias = "turn_complete")]
    TurnComplete {
        #[serde(borrow)]
        turn_id: WireString<'a>,
        #[serde(borrow, default, rename = "last_agent_message")]
        _last_agent_message: Option<WireString<'a>>,
        #[serde(default, rename = "error")]
        _error: Option<JsonObject>,
        #[serde(default, rename = "started_at")]
        _started_at: Option<i64>,
        #[serde(default, rename = "completed_at")]
        _completed_at: Option<i64>,
        #[serde(default, rename = "duration_ms")]
        _duration_ms: Option<i64>,
        #[serde(default, rename = "time_to_first_token_ms")]
        _time_to_first_token_ms: Option<i64>,
    },
    UserMessage {
        #[serde(borrow, default, rename = "client_id")]
        _client_id: Option<WireString<'a>>,
        #[serde(borrow, rename = "message")]
        _message: WireString<'a>,
        #[serde(borrow, default, rename = "images")]
        _images: Option<Vec<WireString<'a>>>,
        #[serde(default, rename = "image_details")]
        _image_details: Vec<Option<ImageDetail>>,
        #[serde(borrow, default, rename = "local_images")]
        _local_images: Vec<WireString<'a>>,
        #[serde(default, rename = "local_image_details")]
        _local_image_details: Vec<Option<ImageDetail>>,
        #[serde(borrow, default, rename = "audio")]
        _audio: Option<Vec<WireString<'a>>>,
        #[serde(borrow, default, rename = "local_audio")]
        _local_audio: Vec<WireString<'a>>,
        #[serde(default, rename = "text_elements")]
        _text_elements: Vec<JsonObject>,
    },
    ThreadRolledBack {
        #[serde(rename = "num_turns")]
        _num_turns: u32,
    },
    TurnAborted {
        #[serde(borrow, default)]
        _turn_id: Option<WireString<'a>>,
        #[serde(rename = "reason")]
        _reason: TurnAbortReason,
        #[serde(default, rename = "started_at")]
        _started_at: Option<i64>,
        #[serde(default, rename = "completed_at")]
        _completed_at: Option<i64>,
        #[serde(default, rename = "duration_ms")]
        _duration_ms: Option<i64>,
    },
    #[serde(other)]
    Other,
}

impl Event<'_> {
    pub(in crate::codex) fn turn_started(&self) -> Option<&str> {
        match &self.boundary {
            Boundary::TurnStarted(turn_id) => Some(turn_id.as_ref()),
            _ => None,
        }
    }

    pub(in crate::codex) fn turn_completed(&self) -> Option<&str> {
        match &self.boundary {
            Boundary::TurnComplete(turn_id) => Some(turn_id.as_ref()),
            _ => None,
        }
    }

    pub(in crate::codex) const fn is_user_message(&self) -> bool {
        matches!(&self.boundary, Boundary::UserMessage)
    }

    pub(in crate::codex) const fn is_turn_aborted(&self) -> bool {
        matches!(&self.boundary, Boundary::TurnAborted)
    }

    pub(in crate::codex) const fn is_rollback(&self) -> bool {
        matches!(&self.boundary, Boundary::Rollback)
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ModeKind {
    Plan,
    #[default]
    #[serde(
        alias = "code",
        alias = "pair_programming",
        alias = "execute",
        alias = "custom"
    )]
    Default,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum ImageDetail {
    Auto,
    Low,
    High,
    Original,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum TurnAbortReason {
    Interrupted,
    Replaced,
    ReviewEnded,
    BudgetLimited,
}

#[derive(Deserialize)]
struct JsonObject {}
