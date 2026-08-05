//! Synthetic Codex JSONL records with no private transcript content.

pub fn record(record_type: &str, payload: &str) -> Vec<u8> {
    format!(
        "{{\"timestamp\":\"2026-08-04T12:00:00.000Z\",\"unknown\":17,\"type\":\"{record_type}\",\"payload\":{payload}}}\n"
    )
    .into_bytes()
}

pub fn metadata(session_id: &str) -> Vec<u8> {
    metadata_with_mode(session_id, "legacy", "")
}

pub fn metadata_with(session_id: &str, extra_fields: &str) -> Vec<u8> {
    metadata_with_mode(session_id, "legacy", extra_fields)
}

pub fn metadata_with_mode(session_id: &str, history_mode: &str, extra_fields: &str) -> Vec<u8> {
    record(
        "session_meta",
        &format!(
            "{{\"session_id\":\"{session_id}\",\"id\":\"{session_id}\",\"timestamp\":\"2026-08-04T12:00:00.000Z\",\"cwd\":\"/synthetic\",\"originator\":\"trimsess-test\",\"cli_version\":\"0.146.0\",\"source\":\"cli\",\"history_mode\":\"{history_mode}\",\"model_provider\":null,\"base_instructions\":null{extra_fields},\"opaque\":true}}"
        ),
    )
}

pub fn compacted(message: &str, window_number: u64) -> Vec<u8> {
    record(
        "compacted",
        &format!(
            "{{\"message\":\"{message}\",\"replacement_history\":[{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"synthetic checkpoint\"}}]}}],\"window_number\":{window_number}}}"
        ),
    )
}

pub fn event(event_type: &str, fields: &str) -> Vec<u8> {
    record(
        "event_msg",
        &format!("{{\"type\":\"{event_type}\"{fields}}}"),
    )
}

pub fn turn_context(turn_id: &str) -> Vec<u8> {
    record(
        "turn_context",
        &format!(
            "{{\"turn_id\":\"{turn_id}\",\"cwd\":\"/synthetic\",\"workspace_roots\":[\"/synthetic\"],\"current_date\":\"2026-08-04\",\"timezone\":\"Etc/UTC\",\"approval_policy\":\"never\",\"approvals_reviewer\":\"user\",\"sandbox_policy\":{{\"type\":\"danger-full-access\"}},\"permission_profile\":{{\"type\":\"disabled\"}},\"network\":{{\"allowed_domains\":[],\"denied_domains\":[]}},\"file_system_sandbox_policy\":{{\"kind\":\"unrestricted\",\"entries\":[]}},\"model\":\"gpt-5\",\"comp_hash\":\"synthetic\",\"personality\":\"pragmatic\",\"collaboration_mode\":{{\"mode\":\"default\",\"settings\":{{\"model\":\"gpt-5\",\"reasoning_effort\":\"medium\",\"developer_instructions\":null}}}},\"multi_agent_version\":\"v2\",\"realtime_active\":false,\"effort\":\"medium\",\"summary\":\"auto\"}}"
        ),
    )
}

pub fn response_message(role: &str, text: &str) -> Vec<u8> {
    record(
        "response_item",
        &format!(
            "{{\"type\":\"message\",\"role\":\"{role}\",\"content\":[{{\"type\":\"output_text\",\"text\":\"{text}\"}}]}}"
        ),
    )
}

pub fn completed_user_turn(turn_id: &str) -> [Vec<u8>; 4] {
    [
        event("task_started", &format!(",\"turn_id\":\"{turn_id}\"")),
        event("user_message", ",\"message\":\"synthetic user\""),
        turn_context(turn_id),
        event("task_complete", &format!(",\"turn_id\":\"{turn_id}\"")),
    ]
}

pub fn append_records(output: &mut Vec<u8>, records: impl IntoIterator<Item = Vec<u8>>) {
    for record in records {
        output.extend(record);
    }
}

pub fn valid_transcript(session_id: &str) -> (Vec<u8>, Vec<u8>) {
    let meta = metadata(session_id);
    let mut records = vec![
        event("agent_message", ",\"message\":\"synthetic old data\""),
        record("compacted", "{\"message\":\"synthetic legacy checkpoint\"}"),
    ];
    records.extend(completed_user_turn("turn-old"));
    records.push(response_message("assistant", "synthetic old response"));
    records.push(compacted("synthetic newest checkpoint", 2));
    records.push(record(
        "world_state",
        "{\"full\":true,\"state\":{\"synthetic\":true}}",
    ));
    records.extend(completed_user_turn("turn-new"));
    records.push(response_message("assistant", "synthetic new response"));
    let mut input = meta.clone();
    for line in &records {
        input.extend_from_slice(line);
    }
    let mut expected = meta;
    for line in &records[7..] {
        expected.extend_from_slice(line);
    }
    (input, expected)
}
