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
fn compressed_backup_is_exact() {
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
    let backup = File::open(backup_path).expect("backup should be readable");
    let mut decoder = zstd::stream::read::Decoder::new(backup).expect("backup should decode");
    let mut restored = Vec::new();
    decoder
        .read_to_end(&mut restored)
        .expect("backup should decode fully");
    assert_eq!(restored, input);
}

#[test]
fn mid_turn_compaction_retains_turn_boundary_and_creates_exact_backup() {
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
    let backup = File::open(backup_path).expect("backup should be readable");
    let mut decoder = zstd::stream::read::Decoder::new(backup).expect("backup should decode");
    let mut restored = Vec::new();
    decoder
        .read_to_end(&mut restored)
        .expect("backup should decode fully");
    assert_eq!(restored, input);
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
