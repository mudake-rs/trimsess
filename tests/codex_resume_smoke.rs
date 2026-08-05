use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

const SESSION_ID: &str = "01900000-0000-7000-8000-000000000001";
const PARENT_SESSION_ID: &str = "01900000-0000-7000-8000-000000000002";
static NEXT_HOME: AtomicU64 = AtomicU64::new(0);

#[test]
#[ignore = "requires an installed Codex CLI 0.146.0; never uses the real CODEX_HOME"]
fn installed_codex_resumes_after_forced_trim() {
    assert_resume_after_forced_trim(None, RolloutShape::PostCompactionTurn);
}

#[test]
#[ignore = "requires an installed Codex CLI 0.146.0; never uses the real CODEX_HOME"]
fn installed_codex_resumes_copied_legacy_fork_after_forced_trim() {
    assert_resume_after_forced_trim(Some(PARENT_SESSION_ID), RolloutShape::PostCompactionTurn);
}

#[test]
#[ignore = "requires an installed Codex CLI 0.146.0; never uses the real CODEX_HOME"]
fn installed_codex_resumes_mid_turn_compaction_after_forced_trim() {
    assert_resume_after_forced_trim(None, RolloutShape::MidTurnCompaction);
}

#[derive(Clone, Copy)]
enum RolloutShape {
    PostCompactionTurn,
    MidTurnCompaction,
}

fn assert_resume_after_forced_trim(forked_from_id: Option<&str>, rollout_shape: RolloutShape) {
    let codex = std::env::var_os("TRIMSESS_CODEX_BIN")
        .map_or_else(|| PathBuf::from("codex"), PathBuf::from);
    assert_supported_codex(&codex);

    let home = SmokeHome::new();
    let rollout = home.write_rollout(forked_from_id, rollout_shape);
    let mut server = AppServer::start(&codex, home.path());
    let first_resume = server.resume();
    assert_eq!(first_resume["result"]["thread"]["id"], SESSION_ID);
    assert_eq!(
        first_resume["result"]["thread"]["path"],
        rollout.to_str().expect("rollout path should be UTF-8")
    );
    let first_model = first_resume["result"]["model"].clone();
    let first_cwd = first_resume["result"]["cwd"].clone();
    let first_approval_policy = first_resume["result"]["approvalPolicy"].clone();

    let trim = Command::new(env!("CARGO_BIN_EXE_trimsess"))
        .args(["--json", "trim", "--force", "--no-backup"])
        .arg(&rollout)
        .output()
        .expect("trimsess should start");
    assert!(
        trim.status.success(),
        "{}",
        String::from_utf8_lossy(&trim.stderr)
    );
    let report: Value = serde_json::from_slice(&trim.stdout).expect("trim report should be JSON");
    assert_eq!(report["status"], "trimmed");
    assert_eq!(report["source_replaced"], true);
    assert!(
        !report["stopped_pids"]
            .as_array()
            .expect("stopped_pids should be an array")
            .is_empty()
    );
    server.wait_for_forced_exit();
    drop(server);

    let mut resumed_server = AppServer::start(&codex, home.path());
    let second_resume = resumed_server.resume();
    assert_eq!(second_resume["result"]["thread"]["id"], SESSION_ID);
    assert_eq!(second_resume["result"]["thread"]["historyMode"], "legacy");
    assert_eq!(second_resume["result"]["model"], first_model);
    assert_eq!(second_resume["result"]["cwd"], first_cwd);
    assert_eq!(
        second_resume["result"]["approvalPolicy"],
        first_approval_policy
    );
    assert_eq!(
        second_resume["result"]["thread"]["turns"],
        Value::Array(Vec::new())
    );
}

fn assert_supported_codex(codex: &Path) {
    let output = Command::new(codex)
        .arg("--version")
        .output()
        .expect("installed Codex should start");
    assert!(output.status.success(), "codex --version should succeed");
    let version = String::from_utf8(output.stdout).expect("Codex version should be UTF-8");
    assert_eq!(version.trim(), "codex-cli 0.146.0");
}

struct SmokeHome(PathBuf);

impl SmokeHome {
    fn new() -> Self {
        let sequence = NEXT_HOME.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "trimsess-codex-smoke-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("smoke CODEX_HOME should be created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write_rollout(&self, forked_from_id: Option<&str>, rollout_shape: RolloutShape) -> PathBuf {
        let directory = self.0.join("sessions/2026/08/04");
        fs::create_dir_all(&directory).expect("session directory should be created");
        let path = directory.join(format!("rollout-2026-08-04T12-00-00-{SESSION_ID}.jsonl"));
        fs::write(&path, synthetic_rollout(forked_from_id, rollout_shape))
            .expect("synthetic rollout should be written");
        path
    }
}

impl Drop for SmokeHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct AppServer {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl AppServer {
    fn start(codex: &Path, home: &Path) -> Self {
        let mut child = Command::new(codex)
            .args(["app-server", "--stdio", "-c", "analytics.enabled=false"])
            .env("CODEX_HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Codex app-server should start");
        let input = child.stdin.take().expect("app-server stdin should exist");
        let output = child.stdout.take().expect("app-server stdout should exist");
        Self {
            child,
            input,
            output: BufReader::new(output),
        }
    }

    fn resume(&mut self) -> Value {
        writeln!(
            self.input,
            "{{\"method\":\"initialize\",\"id\":0,\"params\":{{\"clientInfo\":{{\"name\":\"trimsess_smoke\",\"title\":\"trimsess smoke\",\"version\":\"0.1.0\"}},\"capabilities\":{{\"experimentalApi\":true}}}}}}"
        )
        .expect("initialize request should be sent");
        writeln!(self.input, "{{\"method\":\"initialized\",\"params\":{{}}}}")
            .expect("initialized notification should be sent");
        writeln!(
            self.input,
            "{{\"method\":\"thread/resume\",\"id\":1,\"params\":{{\"threadId\":\"{SESSION_ID}\",\"excludeTurns\":true}}}}"
        )
        .expect("resume request should be sent");
        self.input
            .flush()
            .expect("app-server requests should flush");

        loop {
            let mut line = String::new();
            let bytes = self
                .output
                .read_line(&mut line)
                .expect("app-server response should be readable");
            assert_ne!(bytes, 0, "app-server exited before resume response");
            let message: Value =
                serde_json::from_str(&line).expect("app-server line should be JSON");
            if message["id"] == 1 {
                assert!(message.get("error").is_none(), "resume failed: {message}");
                return message;
            }
        }
    }

    fn wait_for_forced_exit(&mut self) {
        let status = self.child.wait().expect("forced app-server should exit");
        assert!(
            !status.success(),
            "forced app-server should not report success"
        );
    }
}

impl Drop for AppServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn synthetic_rollout(forked_from_id: Option<&str>, rollout_shape: RolloutShape) -> String {
    let fork_field = forked_from_id.map_or_else(String::new, |parent_id| {
        format!(",\"forked_from_id\":\"{parent_id}\"")
    });
    let mut records = vec![format!(
        r#"{{"timestamp":"2026-08-04T12:00:00.000Z","type":"session_meta","payload":{{"session_id":"{SESSION_ID}","id":"{SESSION_ID}","timestamp":"2026-08-04T12:00:00.000Z","cwd":"/tmp","originator":"trimsess-smoke","cli_version":"0.146.0","history_mode":"legacy","model_provider":null,"base_instructions":null{fork_field}}}}}"#
    )];
    if let Some(parent_id) = forked_from_id {
        records.push(format!(r#"{{"timestamp":"2026-08-04T11:00:00.000Z","type":"session_meta","payload":{{"session_id":"{parent_id}","id":"{parent_id}","timestamp":"2026-08-04T11:00:00.000Z","cwd":"/tmp","originator":"trimsess-smoke-parent","cli_version":"0.146.0","history_mode":"legacy","model_provider":null,"base_instructions":null}}}}"#));
    }
    records.extend(match rollout_shape {
        RolloutShape::PostCompactionTurn => post_compaction_turn_records(),
        RolloutShape::MidTurnCompaction => mid_turn_compaction_records(),
    });
    if let Some(parent_id) = forked_from_id {
        records.insert(
            4,
            format!(
                r#"{{"timestamp":"2026-08-04T12:00:02.500Z","type":"session_meta","payload":{{"session_id":"{SESSION_ID}","id":"{SESSION_ID}","timestamp":"2026-08-04T12:00:02.500Z","cwd":"/tmp","originator":"trimsess-smoke-update","cli_version":"0.146.0","history_mode":"legacy","forked_from_id":"{parent_id}","model_provider":null,"base_instructions":null}}}}"#
            ),
        );
    }
    records.join("\n") + "\n"
}

fn post_compaction_turn_records() -> Vec<String> {
    [
        r#"{"timestamp":"2026-08-04T12:00:01.000Z","type":"event_msg","payload":{"type":"agent_message","message":"synthetic old data"}}"#,
        r#"{"timestamp":"2026-08-04T12:00:02.000Z","type":"compacted","payload":{"message":"synthetic checkpoint\nRésumé \"quoted\"","replacement_history":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"synthetic checkpoint"}]},{"type":"function_call","name":"shell","arguments":"{\"cmd\":\"printf résumé\\n\"}","call_id":"call-1"}],"window_number":1}}"#,
        r#"{"timestamp":"2026-08-04T12:00:03.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}"#,
        r#"{"timestamp":"2026-08-04T12:00:04.000Z","type":"event_msg","payload":{"type":"user_message","message":"synthetic user"}}"#,
        r#"{"timestamp":"2026-08-04T12:00:05.000Z","type":"turn_context","payload":{"turn_id":"turn-1","cwd":"/tmp","approval_policy":"never","sandbox_policy":{"type":"danger-full-access"},"model":"gpt-5","summary":"auto"}}"#,
        r#"{"timestamp":"2026-08-04T12:00:06.000Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1","last_agent_message":"synthetic response"}}"#,
        r#"{"timestamp":"2026-08-04T12:00:07.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"synthetic response"}]}}"#,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn mid_turn_compaction_records() -> Vec<String> {
    [
        r#"{"timestamp":"2026-08-04T12:00:01.000Z","type":"event_msg","payload":{"type":"agent_message","message":"synthetic old data"}}"#,
        r#"{"timestamp":"2026-08-04T12:00:02.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}"#,
        r#"{"timestamp":"2026-08-04T12:00:03.000Z","type":"event_msg","payload":{"type":"user_message","message":"synthetic user"}}"#,
        r#"{"timestamp":"2026-08-04T12:00:04.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"synthetic pre-compaction response"}]}}"#,
        r#"{"timestamp":"2026-08-04T12:00:05.000Z","type":"compacted","payload":{"message":"synthetic mid-turn checkpoint","replacement_history":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"synthetic checkpoint"}]}],"window_number":1}}"#,
        r#"{"timestamp":"2026-08-04T12:00:06.000Z","type":"world_state","payload":{"full":true,"state":{}}}"#,
        r#"{"timestamp":"2026-08-04T12:00:07.000Z","type":"turn_context","payload":{"turn_id":"turn-1","cwd":"/tmp","approval_policy":"never","sandbox_policy":{"type":"danger-full-access"},"model":"gpt-5","summary":"auto"}}"#,
        r#"{"timestamp":"2026-08-04T12:00:08.000Z","type":"event_msg","payload":{"type":"token_count","info":null}}"#,
        r#"{"timestamp":"2026-08-04T12:00:09.000Z","type":"event_msg","payload":{"type":"context_compacted"}}"#,
        r#"{"timestamp":"2026-08-04T12:00:10.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"synthetic post-compaction response"}]}}"#,
        r#"{"timestamp":"2026-08-04T12:00:11.000Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1","last_agent_message":"synthetic response"}}"#,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}
