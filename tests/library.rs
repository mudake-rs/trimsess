use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use trimsess::codex::{self, TrimOptions};
use trimsess::report::Status;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf, String);

impl TestDir {
    fn new() -> Self {
        let base = std::env::temp_dir();
        for _ in 0..1000 {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!(
                "trimsess-library-test-{}-{sequence}",
                std::process::id()
            ));
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

    fn rollout(&self) -> PathBuf {
        self.0.join(format!("rollout-test-{}.jsonl", self.1))
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_minimal_transcript(path: &Path, session_id: &str) -> Vec<u8> {
    let transcript = format!(
        "{{\"timestamp\":\"2026-08-04T12:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"session_id\":\"{session_id}\",\"id\":\"{session_id}\",\"timestamp\":\"2026-08-04T12:00:00.000Z\",\"cwd\":\"/synthetic\",\"originator\":\"trimsess-test\",\"cli_version\":\"0.146.0\",\"history_mode\":\"legacy\"}}}}\n"
    )
    .into_bytes();
    fs::write(path, &transcript).expect("fixture write should succeed");
    transcript
}

#[test]
fn codex_library_api_is_usable_without_the_cli() {
    let directory = TestDir::new();
    let path = directory.rollout();
    let source = write_minimal_transcript(&path, &directory.1);

    let inspection = codex::inspect(&path).expect("library inspection should succeed");
    assert_eq!(inspection.status(), Status::NotTrimmable);
    assert_eq!(inspection.agent(), "codex");
    assert_eq!(inspection.session_id(), directory.1);
    assert_eq!(inspection.before_records(), 1);
    assert_eq!(inspection.after_records(), 1);

    let trim = codex::trim(
        TrimOptions::new(&path)
            .dry_run(true)
            .force(false)
            .without_backup(),
    )
    .expect("library trim should safely no-op");
    assert_eq!(trim.status(), Status::NotTrimmable);
    assert_eq!(
        fs::read(&path).expect("source should remain readable"),
        source
    );
}
