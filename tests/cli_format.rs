mod common;

use std::fs;

use common::*;

#[test]
fn reference_backed_fork_is_rejected_before_writes_or_signals() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let mut input = metadata_with(
        directory.session_id(),
        ",\"forked_from_id\":\"01900000-0000-7000-8000-000000000002\",\"history_base\":{}",
    );
    input.extend(compacted("synthetic", 1));
    append_records(&mut input, completed_user_turn("turn-1"));
    fs::write(&path, &input).expect("fixture write should succeed");
    let mut holder = spawn_holder(&path, true);
    wait_until_holds_fd(holder.id(), &path);

    let output = run(&[
        "--json",
        "trim",
        "--force",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert_eq!(output.status.code(), Some(4));
    assert_eq!(fs::read(&path).expect("source should remain"), input);
    assert!(
        holder
            .try_wait()
            .expect("holder status should be readable")
            .is_none()
    );
    let _ = holder.kill();
    let _ = holder.wait();
}

#[test]
fn copied_legacy_fork_trims_independently() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let parent_id = "01900000-0000-7000-8000-ffffffffffff";
    let canonical = metadata_with(
        directory.session_id(),
        &format!(",\"forked_from_id\":\"{parent_id}\""),
    );
    let inherited_metadata = metadata(parent_id);
    let checkpoint = compacted("synthetic fork checkpoint", 3);
    let updated_metadata = canonical.clone();

    let mut input = canonical.clone();
    input.extend_from_slice(&inherited_metadata);
    input.extend(event(
        "agent_message",
        ",\"message\":\"synthetic inherited data\"",
    ));
    input.extend_from_slice(&checkpoint);
    input.extend_from_slice(&updated_metadata);
    append_records(&mut input, completed_user_turn("fork-turn"));

    let mut expected = canonical;
    expected.extend(checkpoint);
    expected.extend(updated_metadata);
    append_records(&mut expected, completed_user_turn("fork-turn"));
    fs::write(&path, &input).expect("fixture write should succeed");

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
    assert_eq!(json(&output)["status"], "trimmed");
    assert_eq!(
        fs::read(&path).expect("trimmed fork should exist"),
        expected
    );
}

#[test]
fn copied_legacy_fork_rejects_reference_backed_ancestor() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let parent_id = "01900000-0000-7000-8000-ffffffffffff";
    let mut input = metadata_with(
        directory.session_id(),
        &format!(",\"forked_from_id\":\"{parent_id}\""),
    );
    input.extend(metadata_with(parent_id, ",\"history_base\":{}"));
    input.extend(compacted("synthetic fork checkpoint", 3));
    append_records(&mut input, completed_user_turn("fork-turn"));

    assert_unsupported_unchanged(&path, &input);
}

#[test]
fn paginated_child_and_reference_backed_metadata_are_rejected() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let subagent_source = String::from_utf8(metadata(directory.session_id()))
        .expect("metadata should be UTF-8")
        .replacen(
            "\"source\":\"cli\"",
            "\"source\":{\"subagent\":\"review\"}",
            1,
        )
        .into_bytes();
    let cases = [
        metadata_with_mode(directory.session_id(), "paginated", ""),
        subagent_source,
        metadata_with(directory.session_id(), ",\"forked_from_id\":null"),
        metadata_with(
            directory.session_id(),
            ",\"parent_thread_id\":\"01900000-0000-7000-8000-000000000002\"",
        ),
        metadata_with(directory.session_id(), ",\"parent_thread_id\":null"),
        metadata_with(directory.session_id(), ",\"history_base\":{}"),
        metadata_with(
            directory.session_id(),
            ",\"subagent_history_start_ordinal\":1",
        ),
        metadata_with(
            directory.session_id(),
            &format!(",\"forked_from_id\":\"{}\"", directory.session_id()),
        ),
    ];

    for mut input in cases {
        input.extend(compacted("synthetic", 1));
        append_records(&mut input, completed_user_turn("turn-1"));
        assert_unsupported_unchanged(&path, &input);
    }
}

#[test]
fn paginated_ordinal_is_rejected_even_when_metadata_says_legacy() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let metadata = String::from_utf8(metadata(directory.session_id()))
        .expect("metadata should be UTF-8")
        .replacen("\"unknown\":17", "\"ordinal\":0,\"unknown\":17", 1)
        .into_bytes();

    assert_unsupported_unchanged(&path, &metadata);
}

#[test]
fn malformed_and_missing_metadata_are_rejected_unchanged() {
    for input in [
        b"{not json}\n".to_vec(),
        event("agent_message", ",\"message\":\"synthetic\""),
    ] {
        let directory = TestDir::new();
        let path = directory.rollout();
        fs::write(&path, &input).expect("fixture write should succeed");
        let output = run(&[
            "trim",
            "--no-backup",
            path.to_str().expect("test path should be UTF-8"),
        ]);
        assert_eq!(output.status.code(), Some(4));
        assert_eq!(fs::read(&path).expect("source should remain"), input);
    }
}

#[test]
fn conflicting_metadata_session_ids_are_rejected_unchanged() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let expected = format!("\"session_id\":\"{}\"", directory.session_id());
    let conflicting = "\"session_id\":\"01900000-0000-7000-8000-ffffffffffff\"";
    let input = String::from_utf8(metadata(directory.session_id()))
        .expect("metadata should be UTF-8")
        .replacen(&expected, conflicting, 1)
        .into_bytes();

    assert_unsupported_unchanged(&path, &input);
}

#[test]
fn duplicate_metadata_unknown_types_and_unsafe_boundary_are_rejected() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let meta = metadata(directory.session_id());
    let private_type = "AKIAIOSFODNN7EXAMPLE-private-synthetic-type";
    let mut duplicate_meta = meta.clone();
    duplicate_meta.extend_from_slice(&meta);
    duplicate_meta.extend(compacted("synthetic", 1));
    append_records(&mut duplicate_meta, completed_user_turn("turn-1"));

    let mut unknown_type = meta.clone();
    unknown_type.extend(record(private_type, "{\"value\":1}"));
    unknown_type.extend(compacted("synthetic", 1));
    append_records(&mut unknown_type, completed_user_turn("turn-1"));

    let mut missing_tail_context = meta;
    missing_tail_context.extend(turn_context("turn-before"));
    missing_tail_context.extend(compacted("synthetic", 1));
    missing_tail_context.extend(event("task_started", ",\"turn_id\":\"turn-after\""));
    missing_tail_context.extend(event("user_message", ",\"message\":\"synthetic\""));
    missing_tail_context.extend(event("task_complete", ",\"turn_id\":\"turn-after\""));

    let mut non_object_payload = metadata(directory.session_id());
    non_object_payload.extend(record("event_msg", "\"private synthetic value\""));
    non_object_payload.extend(compacted("synthetic", 1));
    append_records(&mut non_object_payload, completed_user_turn("turn-1"));

    for input in [
        duplicate_meta,
        unknown_type,
        missing_tail_context,
        non_object_payload,
    ] {
        fs::write(&path, &input).expect("fixture write should succeed");
        for json_mode in [false, true] {
            let mut arguments = Vec::new();
            if json_mode {
                arguments.push("--json");
            }
            arguments.extend([
                "trim",
                "--no-backup",
                path.to_str().expect("test path should be UTF-8"),
            ]);
            let output = run(&arguments);
            assert_eq!(output.status.code(), Some(4));
            assert!(!String::from_utf8_lossy(&output.stderr).contains("private synthetic value"));
            assert!(!String::from_utf8_lossy(&output.stderr).contains(private_type));
            assert_eq!(fs::read(&path).expect("source should remain"), input);
        }
    }
}
