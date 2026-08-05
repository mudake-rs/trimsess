//! Shared test workspace and CLI-output helpers.
#![allow(
    dead_code,
    unused_imports,
    reason = "each integration-test binary uses a different shared helper subset"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

mod fixture;
mod process;

pub use fixture::*;
pub use process::*;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

pub struct TestDir(pub PathBuf, String);

impl TestDir {
    pub fn new() -> Self {
        let base = std::env::temp_dir();
        for _ in 0..1000 {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("trimsess-test-{}-{sequence}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => {
                    let session_id = format!("01900000-0000-7000-8000-{sequence:012x}");
                    return Self(path, session_id);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("could not create test directory: {error}"),
            }
        }
        panic!("could not allocate test directory");
    }

    pub fn rollout(&self) -> PathBuf {
        self.0.join(format!("rollout-test-{}.jsonl", self.1))
    }

    pub fn session_id(&self) -> &str {
        &self.1
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_trimsess"))
        .args(args)
        .output()
        .expect("trimsess should start")
}

pub fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout should be JSON")
}

pub fn assert_unsupported_unchanged(path: &Path, input: &[u8]) {
    fs::write(path, input).expect("fixture write should succeed");
    let output = run(&[
        "--json",
        "trim",
        "--no-backup",
        path.to_str().expect("test path should be UTF-8"),
    ]);
    assert_eq!(output.status.code(), Some(4));
    let error: Value = serde_json::from_slice(&output.stderr).expect("stderr should be JSON");
    assert_eq!(error["status"], "unsupported");
    assert_eq!(error["source_replaced"], false);
    assert_eq!(fs::read(path).expect("source should remain"), input);
}
