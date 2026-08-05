mod common;

use std::fs;

use common::*;

#[test]
fn incomplete_checkpoint_and_rollback_tail_are_rejected() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let weak_checkpoints = [
        record(
            "compacted",
            "{\"message\":\"synthetic\",\"window_number\":1}",
        ),
        record(
            "compacted",
            "{\"message\":\"synthetic\",\"replacement_history\":[]}",
        ),
        record(
            "compacted",
            "{\"message\":\"synthetic\",\"replacement_history\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":17}]}],\"window_number\":1}",
        ),
        record(
            "compacted",
            "{\"message\":\"synthetic\",\"replacement_history\":[],\"window_id\":3}",
        ),
    ];

    for checkpoint in weak_checkpoints {
        let mut input = metadata(directory.session_id());
        input.extend(checkpoint);
        append_records(&mut input, completed_user_turn("turn-1"));
        assert_unsupported_unchanged(&path, &input);
    }

    let mut mid_turn_compaction = metadata(directory.session_id());
    mid_turn_compaction.extend(event("task_started", ",\"turn_id\":\"turn-1\""));
    mid_turn_compaction.extend(event("user_message", ",\"message\":\"synthetic\""));
    mid_turn_compaction.extend(compacted("synthetic", 1));
    mid_turn_compaction.extend(turn_context("turn-1"));
    mid_turn_compaction.extend(event("task_complete", ",\"turn_id\":\"turn-1\""));
    assert_unsupported_unchanged(&path, &mid_turn_compaction);

    let mut rollback = metadata(directory.session_id());
    rollback.extend(compacted("synthetic", 1));
    append_records(&mut rollback, completed_user_turn("turn-1"));
    rollback.extend(event("thread_rolled_back", ",\"num_turns\":1"));
    assert_unsupported_unchanged(&path, &rollback);
}

#[test]
fn window_number_and_inter_agent_turn_are_supported() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let meta = metadata(directory.session_id());
    let checkpoint = compacted("synthetic", 3);
    let tail = [
        event("task_started", ",\"turn_id\":\"turn-agent\""),
        record(
            "inter_agent_communication",
            "{\"author\":\"/root/worker\",\"recipient\":\"/root\",\"other_recipients\":[],\"content\":\"synthetic\",\"trigger_turn\":true}",
        ),
        turn_context("turn-agent"),
        record(
            "inter_agent_communication_metadata",
            "{\"trigger_turn\":true}",
        ),
        event("task_complete", ",\"turn_id\":\"turn-agent\""),
    ];
    let mut input = meta.clone();
    input.extend(event("agent_message", ",\"message\":\"synthetic old\""));
    input.extend_from_slice(&checkpoint);
    for record in &tail {
        input.extend_from_slice(record);
    }
    let mut expected = meta;
    expected.extend(checkpoint);
    for record in &tail {
        expected.extend_from_slice(record);
    }
    fs::write(&path, input).expect("fixture write should succeed");

    let output = run(&[
        "--json",
        "trim",
        "--no-backup",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&path).expect("source should be trimmed"), expected);
}

#[test]
fn escaped_text_and_function_call_checkpoint_are_supported() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let meta = String::from_utf8(metadata(directory.session_id()))
        .expect("metadata should be UTF-8")
        .replacen("/synthetic", r"/synth\u0065tic", 1)
        .into_bytes();
    let checkpoint = record(
        "compacted",
        r#"{"message":"Line one.\nHe said \"hi\". Résumé C:\\temp","replacement_history":[{"type":"function_call","name":"shell","arguments":"{\"cmd\":\"printf résumé\\n\"}","call_id":"call-1"}],"window_number":7}"#,
    );
    let escaped_event = record(
        "event_msg",
        r#"{"type":"user_message","message":"Line one.\nRésumé \"quoted\""}"#,
    );
    let escaped_context = String::from_utf8(turn_context("turn-escaped"))
        .expect("turn context should be UTF-8")
        .replacen("/synthetic", r"/synth\u0065tic", 1)
        .into_bytes();
    let tail = [
        event("task_started", ",\"turn_id\":\"turn-escaped\""),
        escaped_event,
        escaped_context,
        event("task_complete", ",\"turn_id\":\"turn-escaped\""),
    ];
    let mut input = meta.clone();
    input.extend(event("agent_message", ",\"message\":\"synthetic old\""));
    input.extend_from_slice(&checkpoint);
    append_records(&mut input, tail.clone());
    let mut expected = meta;
    expected.extend(checkpoint);
    append_records(&mut expected, tail);
    fs::write(&path, input).expect("fixture write should succeed");

    let output = run(&[
        "--json",
        "trim",
        "--no-backup",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&path).expect("source should be trimmed"), expected);
}

#[test]
fn contextual_or_malformed_response_does_not_prove_a_resume_turn() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let responses = [
        "{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"<user_instructions>synthetic</user_instructions>\"}]}",
        "{\"type\":\"agent_message\",\"author\":\"/root/worker\",\"recipient\":\"/root\",\"content\":[{\"type\":\"input_text\",\"text\":17}]}",
    ];

    for response in responses {
        let mut input = metadata(directory.session_id());
        input.extend(compacted("synthetic", 1));
        input.extend(event("task_started", ",\"turn_id\":\"turn-1\""));
        input.extend(record("response_item", response));
        input.extend(turn_context("turn-1"));
        input.extend(event("task_complete", ",\"turn_id\":\"turn-1\""));
        assert_unsupported_unchanged(&path, &input);
    }
}

#[test]
fn invalid_turn_context_cannot_supply_resume_settings() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let mut input = metadata(directory.session_id());
    input.extend(compacted("synthetic", 1));
    input.extend(event("task_started", ",\"turn_id\":\"turn-1\""));
    input.extend(event("user_message", ",\"message\":\"synthetic\""));
    let invalid_context = String::from_utf8(turn_context("turn-1"))
        .expect("turn context should be UTF-8")
        .replacen("\"cwd\":\"/synthetic\"", "\"cwd\":\"relative\"", 1)
        .into_bytes();
    input.extend(invalid_context);
    input.extend(event("task_complete", ",\"turn_id\":\"turn-1\""));

    assert_unsupported_unchanged(&path, &input);
}

#[test]
fn structured_agent_response_proves_an_inter_agent_turn() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let meta = metadata(directory.session_id());
    let checkpoint = compacted("synthetic", 1);
    let tail = [
        event("task_started", ",\"turn_id\":\"turn-agent\""),
        record(
            "response_item",
            "{\"type\":\"agent_message\",\"author\":\"/root/worker\",\"recipient\":\"/root\",\"content\":[{\"type\":\"input_text\",\"text\":\"synthetic\"}]}",
        ),
        turn_context("turn-agent"),
        event("task_complete", ",\"turn_id\":\"turn-agent\""),
    ];
    let mut input = meta.clone();
    input.extend(event("agent_message", ",\"message\":\"synthetic old\""));
    input.extend_from_slice(&checkpoint);
    append_records(&mut input, tail.clone());
    let mut expected = meta;
    expected.extend(checkpoint);
    append_records(&mut expected, tail);
    fs::write(&path, input).expect("fixture write should succeed");

    let output = run(&[
        "trim",
        "--no-backup",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(path).expect("source should be readable"), expected);
}
