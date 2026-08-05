mod common;

use common::*;
use serde_json::Value;
use std::fs;

#[test]
fn open_descriptor_blocks_ordinary_trim() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let (input, _) = valid_transcript(directory.session_id());
    fs::write(&path, &input).expect("fixture write should succeed");
    let mut holder = spawn_holder(&path, false);
    wait_until_active(&path);

    let inspection = run(&[
        "--json",
        "inspect",
        path.to_str().expect("test path should be UTF-8"),
    ]);
    assert!(inspection.status.success());
    assert_eq!(json(&inspection)["status"], "active");
    assert!(
        json(&inspection)["fd_holder_pids"]
            .as_array()
            .expect("fd holder pids should be an array")
            .contains(&holder.id().into())
    );

    let output = run(&[
        "--json",
        "trim",
        "--no-backup",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert_eq!(output.status.code(), Some(5));
    assert_eq!(json(&output)["status"], "active");
    assert_eq!(fs::read(&path).expect("source should remain"), input);
    let _ = holder.kill();
    let _ = holder.wait();
}

#[test]
fn force_refuses_non_codex_holder() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let (input, _) = valid_transcript(directory.session_id());
    fs::write(&path, &input).expect("fixture write should succeed");
    let mut holder = spawn_holder(&path, false);
    wait_until_active(&path);

    let output = run(&[
        "trim",
        "--force",
        "--no-backup",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert_eq!(output.status.code(), Some(5));
    assert!(
        holder
            .try_wait()
            .expect("holder status should be readable")
            .is_none()
    );
    assert_eq!(fs::read(&path).expect("source should remain"), input);
    let _ = holder.kill();
    let _ = holder.wait();
}

#[test]
fn argument_only_codex_blocks_without_becoming_a_force_target() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let (input, _) = valid_transcript(directory.session_id());
    fs::write(&path, &input).expect("fixture write should succeed");
    let mut process = spawn_argument_only_codex(&directory.0, directory.session_id());
    wait_until_active(&path);

    let inspection = run(&[
        "--json",
        "inspect",
        path.to_str().expect("test path should be UTF-8"),
    ]);
    assert!(
        json(&inspection)["argument_only_pids"]
            .as_array()
            .expect("argument-only pids should be an array")
            .contains(&process.id().into())
    );

    let output = run(&[
        "--json",
        "trim",
        "--force",
        "--no-backup",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert_eq!(output.status.code(), Some(5));
    let error: Value = serde_json::from_slice(&output.stderr).expect("stderr should be JSON");
    assert_eq!(error["source_replaced"], false);
    assert_eq!(error["process_stopped"], false);
    assert!(
        process
            .try_wait()
            .expect("helper status should be readable")
            .is_none()
    );
    assert_eq!(fs::read(&path).expect("source should remain"), input);
    let _ = process.kill();
    let _ = process.wait();
}

#[test]
fn force_dry_run_reports_codex_without_stopping_it() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let (input, _) = valid_transcript(directory.session_id());
    fs::write(&path, &input).expect("fixture write should succeed");
    let mut holder = spawn_holder(&path, true);
    wait_until_active(&path);

    let output = run(&[
        "--json",
        "trim",
        "--dry-run",
        "--force",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert_eq!(output.status.code(), Some(5));
    let report = json(&output);
    assert_eq!(report["status"], "active");
    assert!(
        !report["would_stop_pids"]
            .as_array()
            .expect("pids should be an array")
            .is_empty()
    );
    assert!(
        holder
            .try_wait()
            .expect("holder status should be readable")
            .is_none()
    );
    assert_eq!(fs::read(&path).expect("source should remain"), input);
    let _ = holder.kill();
    let _ = holder.wait();
}

#[test]
fn force_dry_run_promises_no_signal_when_any_writer_is_unverified() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let (input, _) = valid_transcript(directory.session_id());
    fs::write(&path, &input).expect("fixture write should succeed");
    let mut holder = spawn_holder(&path, true);
    let mut argument_only = spawn_argument_only_codex(&directory.0, directory.session_id());
    wait_until_activity_pids(&path, holder.id(), argument_only.id());

    let output = run(&[
        "--json",
        "trim",
        "--dry-run",
        "--force",
        path.to_str().expect("test path should be UTF-8"),
    ]);

    assert_eq!(output.status.code(), Some(5));
    let report = json(&output);
    assert_eq!(report["status"], "active");
    assert_eq!(report["would_stop_pids"], serde_json::json!([]));
    assert!(
        holder
            .try_wait()
            .expect("holder status should work")
            .is_none()
    );
    assert!(
        argument_only
            .try_wait()
            .expect("argument-only status should work")
            .is_none()
    );
    assert_eq!(fs::read(&path).expect("source should remain"), input);
    let _ = holder.kill();
    let _ = holder.wait();
    let _ = argument_only.kill();
    let _ = argument_only.wait();
}
