mod common;

use std::fs;
use std::thread;
use std::time::{Duration, Instant};

use common::*;
use serde_json::Value;

#[test]
fn force_stops_verified_codex_holder_and_trims() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let (_, expected) = valid_transcript(directory.session_id());
    let (input, _) = valid_transcript(directory.session_id());
    fs::write(&path, input).expect("fixture write should succeed");
    let mut holder = spawn_holder(&path, true);
    wait_until_active(&path);

    let output = run(&[
        "--json",
        "trim",
        "--force",
        "--no-backup",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json(&output)["status"], "trimmed");
    assert!(
        !json(&output)["stopped_pids"]
            .as_array()
            .expect("pids should be an array")
            .is_empty()
    );
    assert_eq!(fs::read(&path).expect("source should be trimmed"), expected);
    assert!(
        holder
            .wait()
            .expect("holder wait should work")
            .code()
            .is_none()
    );
}

#[test]
fn force_can_stop_its_parent_holder_and_complete() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let stdout_path = directory.0.join("parent-force.stdout");
    let stderr_path = directory.0.join("parent-force.stderr");
    let (input, expected) = valid_transcript(directory.session_id());
    fs::write(&path, input).expect("fixture write should succeed");
    let mut parent = spawn_parent_holder(&path, &stdout_path, &stderr_path);
    let parent_pid = parent.id();

    let _ = parent.wait().expect("parent holder should finish");
    let deadline = Instant::now() + Duration::from_secs(3);
    let report = loop {
        if let Ok(bytes) = fs::read(&stdout_path)
            && let Ok(report) = serde_json::from_slice::<Value>(&bytes)
        {
            break report;
        }
        assert!(
            Instant::now() < deadline,
            "orphaned trimsess did not report"
        );
        thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(report["status"], "trimmed");
    assert!(
        report["stopped_pids"]
            .as_array()
            .expect("stopped pids should be an array")
            .iter()
            .any(|pid| pid.as_u64() == Some(u64::from(parent_pid))),
        "{report}"
    );
    assert_eq!(fs::read(&path).expect("source should be trimmed"), expected);
    assert_eq!(
        fs::read(&stderr_path).expect("stderr should be readable"),
        b""
    );
}

#[test]
fn force_rescans_and_preserves_shutdown_append() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let (input, mut expected) = valid_transcript(directory.session_id());
    let shutdown_record = event(
        "task_complete",
        ",\"turn_id\":\"synthetic-shutdown\",\"last_agent_message\":null",
    );
    expected.extend_from_slice(&shutdown_record);
    fs::write(&path, input).expect("fixture write should succeed");
    let mut holder = spawn_shutdown_writer(&path, &shutdown_record);
    wait_until_active(&path);

    let output = run(&[
        "--json",
        "trim",
        "--force",
        "--no-backup",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&path).expect("source should be trimmed"), expected);
    let report = json(&output);
    assert_eq!(report["status"], "trimmed");
    assert_eq!(report["source_replaced"], true);
    let _ = holder.wait();
}

#[test]
fn post_stop_malformed_append_is_reported_without_replacement() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let (input, _) = valid_transcript(directory.session_id());
    let shutdown_bytes = b"{synthetic partial";
    fs::write(&path, &input).expect("fixture write should succeed");
    let mut holder = spawn_shutdown_writer(&path, shutdown_bytes);
    wait_until_active(&path);

    let output = run(&[
        "--json",
        "trim",
        "--force",
        "--no-backup",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert_eq!(output.status.code(), Some(4));
    let error: Value = serde_json::from_slice(&output.stderr).expect("stderr should be JSON");
    assert_eq!(error["process_stopped"], true);
    assert_eq!(error["source_replaced"], false);
    let mut expected = input;
    expected.extend_from_slice(shutdown_bytes);
    assert_eq!(
        fs::read(&path).expect("source should remain installed"),
        expected
    );
    let _ = holder.wait();
}
