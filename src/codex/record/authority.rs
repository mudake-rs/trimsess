//! Validation of fields that define transcript identity and resume safety.

use std::path::Path;

use super::{InterAgentPayload, MetadataPayload, TurnContextPayload};
use crate::{Error, ErrorKind};

// The canonical first metadata record is the sole authority for whether later
// legacy metadata records are structurally valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::codex) enum MetadataLayout {
    Standalone,
    CopiedFork,
}

pub(in crate::codex) fn validate_turn_context(
    context: &TurnContextPayload<'_>,
    record_number: u64,
    path: &Path,
) -> Result<(), Error> {
    if !context.has_valid_resume_values() {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            format!("record {record_number} turn_context has invalid resume settings"),
        ));
    }
    Ok(())
}

pub(in crate::codex) fn validate_inter_agent(
    communication: &InterAgentPayload<'_>,
    record_number: u64,
    path: &Path,
) -> Result<(), Error> {
    if !is_agent_path(&communication.author)
        || !is_agent_path(&communication.recipient)
        || communication
            .other_recipients
            .iter()
            .any(|recipient| !is_agent_path(recipient))
    {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            format!("record {record_number} has an invalid inter-agent endpoint"),
        ));
    }
    Ok(())
}

pub(in crate::codex) fn validate_canonical_metadata(
    metadata: &MetadataPayload<'_>,
    path: &Path,
) -> Result<MetadataLayout, Error> {
    validate_metadata_fields(metadata, path)?;
    validate_legacy_layout(metadata, path)?;

    Ok(if metadata.forked_from_id.0.is_some() {
        MetadataLayout::CopiedFork
    } else {
        MetadataLayout::Standalone
    })
}

pub(in crate::codex) fn validate_additional_metadata(
    metadata: &MetadataPayload<'_>,
    path: &Path,
) -> Result<(), Error> {
    validate_metadata_fields(metadata, path)?;
    validate_legacy_layout(metadata, path)
}

fn validate_metadata_fields(metadata: &MetadataPayload<'_>, path: &Path) -> Result<(), Error> {
    if !is_uuid(metadata.id()) {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            "session_meta id is not a supported UUID",
        ));
    }
    if metadata
        .session_id
        .0
        .as_deref()
        .is_some_and(|id| !is_uuid(id))
    {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            "session_meta session_id is not a supported UUID",
        ));
    }
    if metadata
        .session_id
        .0
        .as_deref()
        .is_some_and(|id| id != metadata.id())
    {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            "session_meta session_id does not match id",
        ));
    }
    if metadata.timestamp.is_empty()
        || metadata.cwd.is_empty()
        || metadata.originator.is_empty()
        || metadata.cli_version.is_empty()
    {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            "session_meta has an empty timestamp, cwd, originator, or cli_version",
        ));
    }
    if metadata
        .forked_from_id
        .0
        .as_deref()
        .is_some_and(|parent_id| !is_uuid(parent_id) || parent_id == metadata.id())
    {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            "session_meta forked_from_id is invalid or self-referential",
        ));
    }
    Ok(())
}

// Copied legacy forks may contain ancestor metadata and later metadata updates
// after their canonical first record. Reference-backed and child layouts
// remain unsupported in every metadata record.
fn validate_legacy_layout(metadata: &MetadataPayload<'_>, path: &Path) -> Result<(), Error> {
    if metadata.parent_thread_id.0.is_some() {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            "child transcript is unsupported in version 1",
        ));
    }
    if metadata.has_child_source() {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            "sub-agent transcript source is unsupported in version 1",
        ));
    }
    if metadata.history_base.0 {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            "reference-backed transcript history is unsupported in version 1",
        ));
    }
    if metadata.subagent_history_start_ordinal.0 {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            "subagent transcript history is unsupported in version 1",
        ));
    }
    if let Some(history_mode) = metadata.history_mode.0.as_deref()
        && history_mode != "legacy"
    {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            "non-legacy transcript history is unsupported in version 1",
        ));
    }
    Ok(())
}

pub(in crate::codex) fn verify_filename_session_id(
    path: &Path,
    session_id: &str,
) -> Result<(), Error> {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(Error::for_path(
            ErrorKind::Target,
            path,
            "target has no UTF-8 file name",
        ));
    };
    let Some(stem) = name.strip_suffix(".jsonl") else {
        return Err(Error::for_path(
            ErrorKind::Target,
            path,
            "target does not end in .jsonl",
        ));
    };
    if !stem.ends_with(session_id) {
        return Err(Error::for_path(
            ErrorKind::Unsupported,
            path,
            format!("session_meta id {session_id} does not match the rollout file name"),
        ));
    }
    Ok(())
}

fn is_uuid(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    value.bytes().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => byte == b'-',
        _ => byte.is_ascii_hexdigit(),
    })
}

// Mirrors protocol/src/agent_path.rs in Codex CLI 0.146.0.
fn is_agent_path(value: &str) -> bool {
    if value == "/morpheus" {
        return true;
    }
    let Some(path) = value.strip_prefix("/root") else {
        return false;
    };
    if path.is_empty() {
        return true;
    }
    if !path.starts_with('/') || path.ends_with('/') {
        return false;
    }
    path[1..].split('/').all(|segment| {
        !segment.is_empty()
            && segment != "root"
            && segment != "."
            && segment != ".."
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    })
}

#[cfg(test)]
mod tests {
    use super::{is_agent_path, is_uuid};

    #[test]
    fn uuid_shape_is_strict() {
        assert!(is_uuid("01900000-0000-7000-8000-000000000001"));
        assert!(!is_uuid("01900000_0000_7000_8000_000000000001"));
        assert!(!is_uuid("not-a-session"));
    }

    #[test]
    fn agent_path_shape_matches_codex() {
        assert!(is_agent_path("/root"));
        assert!(is_agent_path("/root/researcher_2"));
        assert!(is_agent_path("/morpheus"));
        assert!(!is_agent_path("root/researcher"));
        assert!(!is_agent_path("/root/BadName"));
        assert!(!is_agent_path("/root/root"));
        assert!(!is_agent_path("/root/"));
    }
}
