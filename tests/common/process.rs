//! Disposable process helpers for active-writer CLI tests.

use std::fs::{self, File};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::{json, run};

pub fn copy_executable(directory: &Path, name: &str, source: &Path) -> PathBuf {
    let helper = directory.join(name);
    fs::copy(source, &helper).expect("test executable should copy");
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755))
        .expect("test executable should be executable");
    File::open(&helper)
        .expect("test executable should reopen")
        .sync_all()
        .expect("test executable should sync");
    helper
}

pub fn spawn_with_retry(command: &mut Command, context: &str) -> Child {
    for attempt in 0..25 {
        match command.spawn() {
            Ok(child) => return child,
            Err(error)
                if error.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < 24 =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("{context}: {error}"),
        }
    }
    unreachable!("spawn retry loop must return or panic")
}
pub fn spawn_holder(path: &Path, codex_name: bool) -> Child {
    let executable = if codex_name {
        copy_executable(
            path.parent().expect("test rollout should have a parent"),
            "codex",
            Path::new("/bin/bash"),
        )
    } else {
        PathBuf::from("/bin/bash")
    };
    spawn_with_retry(
        Command::new(executable)
            .arg("-c")
            .arg("exec 3>>\"$1\"; while :; do sleep 1 3>&-; done")
            .arg("holder")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
        "holder should start",
    )
}

pub fn spawn_argument_only_codex(directory: &Path, session_id: &str) -> Child {
    let executable_directory = directory.join("argument-only");
    fs::create_dir(&executable_directory)
        .expect("argument-only executable directory should be created");
    let helper = copy_executable(&executable_directory, "codex", Path::new("/bin/bash"));
    spawn_with_retry(
        Command::new(helper)
            .arg("-c")
            .arg("while :; do sleep 1; done")
            .arg("argument-only")
            .arg(session_id)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
        "argument-only Codex helper should start",
    )
}

pub fn spawn_shutdown_writer(path: &Path, shutdown_bytes: &[u8]) -> Child {
    let helper = copy_executable(
        path.parent().expect("test rollout should have a parent"),
        "codex",
        Path::new("/bin/bash"),
    );
    spawn_with_retry(
        Command::new(helper)
            .arg("-c")
            .arg(
                "exec 3>>\"$1\"; trap 'printf \"%s\" \"$2\" >&3; exit 0' TERM; while :; do sleep 1 3>&-; done",
            )
            .arg("holder")
            .arg(path)
            .arg(std::ffi::OsStr::from_bytes(shutdown_bytes))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
        "shutdown writer should start",
    )
}

pub fn spawn_parent_holder(path: &Path, stdout_path: &Path, stderr_path: &Path) -> Child {
    let helper = copy_executable(
        path.parent().expect("test rollout should have a parent"),
        "codex",
        Path::new("/bin/bash"),
    );
    let stdout = File::create(stdout_path).expect("parent stdout should be created");
    let stderr = File::create(stderr_path).expect("parent stderr should be created");
    spawn_with_retry(
        Command::new(helper)
            .arg("-c")
            .arg(
                "exec 3>>\"$1\"; \"$2\" --json trim --force --no-backup \"$1\"; status=$?; :; exit \"$status\"",
            )
            .arg("holder")
            .arg(path)
            .arg(env!("CARGO_BIN_EXE_trimsess"))
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr)),
        "parent holder should start",
    )
}

pub fn wait_until_active(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut last_output = None;
    while Instant::now() < deadline {
        let output = run(&[
            "--json",
            "inspect",
            path.to_str().expect("test path should be UTF-8"),
        ]);
        if matches!(output.status.code(), Some(0 | 5))
            && !output.stdout.is_empty()
            && json(&output)["active"] == true
        {
            return;
        }
        last_output = Some(output);
        thread::sleep(Duration::from_millis(25));
    }
    let output = last_output.expect("inspect should run before its deadline");
    panic!(
        "holder did not become visible: status={:?}, stdout={}, stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn wait_until_holds_fd(pid: u32, path: &Path) {
    let target = fs::metadata(path).expect("target metadata should exist");
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if fs::read_dir(format!("/proc/{pid}/fd"))
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .any(|entry| {
                fs::metadata(entry.path()).is_ok_and(|metadata| {
                    metadata.dev() == target.dev() && metadata.ino() == target.ino()
                })
            })
        {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("holder did not open the target fd");
}

pub fn wait_until_activity_pids(path: &Path, fd_pid: u32, argument_pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        let output = run(&[
            "--json",
            "inspect",
            path.to_str().expect("test path should be UTF-8"),
        ]);
        if output.status.success() {
            let report = json(&output);
            let fd_holders = report["fd_holder_pids"]
                .as_array()
                .expect("fd holder pids should be an array");
            let argument_only = report["argument_only_pids"]
                .as_array()
                .expect("argument-only pids should be an array");
            if fd_holders.contains(&fd_pid.into()) && argument_only.contains(&argument_pid.into()) {
                return;
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("activity report did not include both expected pids");
}
