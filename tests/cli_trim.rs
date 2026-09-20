mod common;

use std::fs::{self, File};
use std::io::Read;

use common::*;
use serde_json::Value;

#[test]
fn trims_to_newest_boundary_and_preserves_retained_bytes() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let (input, expected) = valid_transcript(directory.session_id());
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
    assert_eq!(
        fs::read(&path).expect("trimmed source should exist"),
        expected
    );
    let report = json(&output);
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["agent"], "codex");
    assert_eq!(report["format"], "codex_jsonl");
    assert_eq!(report["session_id"], directory.session_id());
    assert_eq!(report["status"], "trimmed");
    assert_eq!(report["compaction_count"], 2);
    assert_eq!(report["removed_records"], 7);
    assert_eq!(report["backup_path"], Value::Null);
}

#[test]
fn token_usage_records_are_accepted_and_follow_normal_retention() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let meta = metadata(directory.session_id());
    let old_usage = record("token_usage_record", "{\"usage\":{\"total_tokens\":1}}");
    let checkpoint = compacted("synthetic", 1);
    let retained_usage = record(
        "token_usage_record",
        "{\"turn_id\":\"turn-1\",\"usage\":{\"total_tokens\":2}}",
    );

    let mut input = meta.clone();
    input.extend(old_usage);
    input.extend_from_slice(&checkpoint);
    append_records(&mut input, completed_user_turn("turn-1"));
    input.extend_from_slice(&retained_usage);

    let mut expected = meta;
    expected.extend(checkpoint);
    append_records(&mut expected, completed_user_turn("turn-1"));
    expected.extend(retained_usage);
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
    assert_eq!(json(&output)["status"], "trimmed");
    assert_eq!(fs::read(&path).expect("source should be trimmed"), expected);
}

#[test]
fn dry_run_reports_plan_without_files_or_signals() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let backup_dir = directory.0.join("dry-run-backups");
    let (input, _) = valid_transcript(directory.session_id());
    fs::write(&path, &input).expect("fixture write should succeed");
    let before_entries = fs::read_dir(&directory.0)
        .expect("directory should be readable")
        .count();

    let output = run(&[
        "--json",
        "trim",
        "--dry-run",
        "--force",
        "--backup-dir",
        backup_dir.to_str().expect("test path should be UTF-8"),
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert!(output.status.success());
    assert_eq!(json(&output)["status"], "dry_run");
    assert_eq!(fs::read(&path).expect("source should remain"), input);
    assert_eq!(
        fs::read_dir(&directory.0)
            .expect("directory should be readable")
            .count(),
        before_entries
    );
    assert!(!backup_dir.exists());
}

#[test]
fn compressed_backup_is_checksummed_and_restorable() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let backup_dir = directory.0.join("backups");
    let (input, _) = valid_transcript(directory.session_id());
    fs::write(&path, &input).expect("fixture write should succeed");

    let output = run(&[
        "--json",
        "trim",
        "--backup-dir",
        backup_dir.to_str().expect("test path should be UTF-8"),
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    let backup_path = report["backup_path"]
        .as_str()
        .expect("backup path should exist");
    assert_checksumming_zstd_frame(backup_path);
    let backup = File::open(backup_path).expect("backup should be readable");
    let mut decoder = zstd::stream::read::Decoder::new(backup).expect("backup should decode");
    let mut restored = Vec::new();
    decoder
        .read_to_end(&mut restored)
        .expect("backup should decode fully");
    assert_eq!(restored, input);
}

#[test]
fn mid_turn_compaction_retains_turn_boundary_and_creates_restorable_backup() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let backup_dir = directory.0.join("backups");
    let (input, expected) = mid_turn_compaction_transcript(directory.session_id());
    fs::write(&path, &input).expect("fixture write should succeed");

    let inspection = run(&[
        "--json",
        "inspect",
        path.to_str().expect("test path should be UTF-8"),
    ]);
    assert!(
        inspection.status.success(),
        "{}",
        String::from_utf8_lossy(&inspection.stderr)
    );
    let inspection = json(&inspection);
    assert_eq!(inspection["status"], "ready");
    assert_eq!(inspection["after_bytes"], expected.len());
    assert_eq!(inspection["after_records"], 11);

    let output = run(&[
        "--json",
        "trim",
        "--backup-dir",
        backup_dir.to_str().expect("test path should be UTF-8"),
        path.to_str().expect("test path should be UTF-8"),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&path).expect("source should be trimmed"), expected);

    let report = json(&output);
    let backup_path = report["backup_path"]
        .as_str()
        .expect("backup path should exist");
    assert_checksumming_zstd_frame(backup_path);
    let backup = File::open(backup_path).expect("backup should be readable");
    let mut decoder = zstd::stream::read::Decoder::new(backup).expect("backup should decode");
    let mut restored = Vec::new();
    decoder
        .read_to_end(&mut restored)
        .expect("backup should decode fully");
    assert_eq!(restored, input);
}

fn assert_checksumming_zstd_frame(path: &str) {
    const ZSTD_MAGIC: [u8; 4] = 0xFD2F_B528_u32.to_le_bytes();
    const CHECKSUM_FLAG: u8 = 1 << 2;

    let mut frame = fs::read(path).expect("backup frame should be readable");
    assert!(frame.len() >= 5, "backup should contain a zstd header");
    assert_eq!(&frame[..4], &ZSTD_MAGIC);
    assert_ne!(frame[4] & CHECKSUM_FLAG, 0, "checksum flag should be set");

    let checksum_trailer_byte = frame
        .last_mut()
        .expect("checksummed zstd frame should not be empty");
    *checksum_trailer_byte ^= 1;
    let mut decoder =
        zstd::stream::read::Decoder::new(frame.as_slice()).expect("frame header should decode");
    let mut discarded = Vec::new();
    assert!(
        decoder.read_to_end(&mut discarded).is_err(),
        "checksum trailer corruption should be rejected"
    );
}

#[test]
fn newest_of_multiple_mid_turn_compactions_keeps_one_continuous_turn() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let meta = metadata(directory.session_id());
    let retained_turn = [
        event("task_started", ",\"turn_id\":\"turn-mid\""),
        event("user_message", ",\"message\":\"synthetic user\""),
        compacted("synthetic first checkpoint", 3),
        turn_context("turn-mid"),
        event("context_compacted", ""),
        response_message("assistant", "synthetic middle response"),
        compacted("synthetic newest checkpoint", 4),
        record("world_state", "{\"full\":true,\"state\":{}}"),
        turn_context("turn-mid"),
        event("context_compacted", ""),
        event("task_complete", ",\"turn_id\":\"turn-mid\""),
    ];
    let mut input = meta.clone();
    input.extend(event(
        "agent_message",
        ",\"message\":\"synthetic old data\"",
    ));
    append_records(&mut input, retained_turn.clone());
    let mut expected = meta;
    append_records(&mut expected, retained_turn);
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
    assert_eq!(json(&output)["status"], "trimmed");
    assert_eq!(fs::read(&path).expect("source should be trimmed"), expected);
}

#[test]
fn mid_turn_suffix_at_record_two_is_already_minimal() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let (_, expected) = mid_turn_compaction_transcript(directory.session_id());
    fs::write(&path, &expected).expect("fixture write should succeed");

    let output = run(&[
        "--json",
        "trim",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert!(output.status.success());
    assert_eq!(json(&output)["status"], "already_minimal");
    assert_eq!(fs::read(&path).expect("source should remain"), expected);
    assert!(!directory.0.join("backups").exists());
}

#[test]
fn no_compaction_is_a_no_op() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let mut input = metadata(directory.session_id());
    input.extend(turn_context("turn-1"));
    input.extend(response_message("assistant", "synthetic"));
    fs::write(&path, &input).expect("fixture write should succeed");

    let output = run(&[
        "--json",
        "trim",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert!(output.status.success());
    assert_eq!(json(&output)["status"], "not_trimmable");
    assert_eq!(fs::read(&path).expect("source should remain"), input);
    assert!(!directory.0.join("backups").exists());
}

#[test]
fn second_trim_is_an_already_minimal_no_op() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let (_, expected) = valid_transcript(directory.session_id());
    fs::write(&path, &expected).expect("fixture write should succeed");

    let output = run(&[
        "--json",
        "trim",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert!(output.status.success());
    assert_eq!(json(&output)["status"], "already_minimal");
    assert_eq!(fs::read(&path).expect("source should remain"), expected);
}
