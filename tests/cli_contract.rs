mod common;

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::symlink;

use common::*;
use serde_json::Value;

#[test]
fn symlink_target_is_rejected() {
    let directory = TestDir::new();
    let real_path = directory.rollout();
    let link_path = directory
        .0
        .join(format!("rollout-link-{}.jsonl", directory.session_id()));
    let (input, _) = valid_transcript(directory.session_id());
    fs::write(&real_path, input).expect("fixture write should succeed");
    symlink(&real_path, &link_path).expect("symlink should be created");

    let output = run(&[
        "inspect",
        link_path.to_str().expect("test path should be UTF-8"),
    ]);

    assert_eq!(output.status.code(), Some(3));
}

#[test]
fn backup_failure_leaves_source_unchanged() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let backup_file = directory.0.join("not-a-directory");
    let (input, _) = valid_transcript(directory.session_id());
    fs::write(&path, &input).expect("fixture write should succeed");
    fs::write(&backup_file, b"synthetic").expect("blocker write should succeed");

    let output = run(&[
        "trim",
        "--backup-dir",
        backup_file.to_str().expect("test path should be UTF-8"),
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert_eq!(output.status.code(), Some(6));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("\nsource: not replaced by trimsess\n")
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("\nnext: "));
    assert_eq!(fs::read(&path).expect("source should remain"), input);
}

#[test]
fn help_is_complete_and_links_the_repository() {
    for args in [
        &["--help"][..],
        &["-h"],
        &["inspect", "--help"],
        &["inspect", "-h"],
        &["trim", "--help"],
        &["trim", "-h"],
    ] {
        let output = run(args);
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).expect("help should be UTF-8");
        assert!(help.contains("https://github.com/mudake-rs/trimsess"));
        assert!(help.contains("128 MiB"));
        assert!(help.contains("Copied legacy forks"));
    }
    let trim = run(&["trim", "--help"]);
    let help = String::from_utf8(trim.stdout).expect("help should be UTF-8");
    assert!(help.contains("--force"));
    assert!(help.contains("SIGKILL"));
    assert!(help.contains("codex resume"));

    let root = run(&["--help"]);
    let help = String::from_utf8(root.stdout).expect("help should be UTF-8");
    assert!(help.contains("--dry-run"));
    assert!(help.contains("codex resume"));
}

#[test]
fn json_usage_errors_are_machine_readable() {
    let output = run(&["--json", "trim"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).expect("stderr should be JSON");
    assert_eq!(error["status"], "invalid_usage");
    assert_eq!(error["error_code"], "usage");
    assert_eq!(error["source_replaced"], false);
}

#[test]
fn many_records_are_streamed_without_an_in_memory_transcript_model() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let mut file = File::create(&path).expect("fixture should be creatable");
    file.write_all(&metadata(directory.session_id()))
        .expect("metadata write should succeed");
    for index in 0..50_000_u64 {
        file.write_all(&event(
            "agent_message",
            &format!(",\"message\":\"synthetic\",\"index\":{index}"),
        ))
        .expect("record write should succeed");
    }
    file.write_all(&compacted("synthetic", 1))
        .expect("compaction write should succeed");
    for record in completed_user_turn("turn-large") {
        file.write_all(&record)
            .expect("resume-turn write should succeed");
    }
    file.sync_all().expect("fixture sync should succeed");
    drop(file);

    let output = run(&[
        "--json",
        "inspect",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json(&output)["before_records"], 50_006);
}
