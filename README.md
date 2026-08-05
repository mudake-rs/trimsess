# trimsess

`trimsess` safely removes obsolete history windows from oversized local Codex
JSONL transcripts while preserving the state required to resume the same
session.

Version 1 is Linux-only and Codex-only. It never discovers a session
implicitly: every command requires the exact path to one
`rollout-*.jsonl` file.

> `trim --force` stops verified Codex processes holding the target file and
> can discard their unflushed work. Prefer stopping Codex normally.

## Supported input

Supported:

- standalone Codex legacy transcripts;
- copied legacy forks with their own rollout file.

Rejected without modifying the source:

- paginated or reference-backed forks using `history_base`;
- child and sub-agent transcripts;
- non-legacy history, rollback tails, malformed JSON, and unknown record
  layouts;
- Claude and other agent formats.

The format contract is verified against Codex CLI `0.146.0`, source tag
[`rust-v0.146.0`](https://github.com/openai/codex/tree/rust-v0.146.0).
Codex transcripts are internal state; a changed shape is unsupported rather
than guessed.

## Install

Install for the current user:

```text
cargo install --locked trimsess
trimsess --version
```

Ensure `~/.cargo/bin` is in `PATH`.

Build or install from a local checkout with stable Rust and a C toolchain:

```text
cargo build --release --locked
cargo install --locked --path .
target/release/trimsess --version
```

Install the already-built binary system-wide:

```text
sudo install -Dm0755 target/release/trimsess /usr/local/bin/trimsess
trimsess --version
```

The Linux binary requires only glibc and `libgcc_s` at runtime. It installs no
service or configuration. Run it as the normal user who owns the transcript,
not with `sudo`.

## Use

Use this sequence:

```text
trimsess inspect /exact/path/to/rollout-...-<session-id>.jsonl
trimsess trim --dry-run /exact/path/to/rollout-...-<session-id>.jsonl
trimsess trim /exact/path/to/rollout-...-<session-id>.jsonl
codex resume <session-id>
```

`inspect` validates the transcript, reports active processes, and projects the
trim without side effects. `trim --dry-run` validates the source and reports
the planned operation without writing or signaling. If Codex still owns the
file, stop it normally or replace the trim command with:

```text
trimsess trim --force /exact/path/to/rollout-...-<session-id>.jsonl
```

Important options:

| Option | Effect |
|---|---|
| `--json` | Emit the stable JSON schema instead of human-readable output |
| `--dry-run` | Validate and report without writing or sending signals |
| `--force` | Stop verified Codex processes holding the target inode |
| `--backup-dir <PATH>` | Write the compressed backup to this directory |
| `--no-backup` | Disable backup and the associated recovery path |

Run `trimsess <COMMAND> --help` for the complete contract.

## Retention

The installed transcript contains exactly:

1. the first canonical `session_meta` record;
2. the newest complete `compacted` record;
3. every record after that compaction.

Retained records and unknown fields remain byte-for-byte unchanged. The newest
compaction must contain a decodable `replacement_history` and
`window_number`, followed by a completed user turn with valid `turn_context`.
A later rollback is unsupported.

A valid transcript without compaction is a safe no-op. A transcript already in
the minimal form is also a no-op.

The transcript is streamed rather than loaded into memory. One JSONL record,
including its line ending, may be at most 128 MiB.

## Process and filesystem safety

The target must be a non-symlink regular file owned by the current user.
`trimsess` verifies the session UUID against the file name and transcript
metadata.

Without `--force`, an open writer or matching Codex process blocks `trim`.
`--force` may signal only a same-user process that:

- holds the captured target device and inode; and
- has the exact `/proc/<pid>/exe` basename `codex`.

It sends `SIGTERM`, waits up to five seconds, then uses `SIGKILL` if necessary.
After the process exits, trimsess reopens and rescans the complete transcript
so Codex shutdown records are included. If the invoking Codex session owns the
file, `--force` can stop the caller before it receives the final report.

A privilege-transitioned process can hide both its identity and file
descriptors from `/proc`. Such a PID is reported in `uninspectable_pids` and is
never signaled. Stop remote transfers or other hidden processes that could
write the transcript before trimming; a post-rename append through the old
inode will not appear in the installed file.

## Backup and replacement

By default, trimsess creates and validates an exact zstd-compressed backup in
`$XDG_STATE_HOME/trimsess/backups`, or
`~/.local/state/trimsess/backups` when `XDG_STATE_HOME` is unset.

The candidate transcript is written beside the source, validated completely,
given the source ownership and mode, flushed, and installed with one atomic
rename. Activity and source identity are checked again immediately before that
rename.

Any failure before the rename leaves the source byte-for-byte unchanged. A
failure after replacement returns exit code `7` and reports the recovery
backup when one exists.

Transcript content is never included in normal output, JSON, logs, or errors.

## Output contract

`--json` emits schema version `1` with session identity, before/after counts,
compaction boundary, backup path, process PID sets, replacement state, and
resume command.

Successful statuses are `ready`, `active`, `not_trimmable`,
`already_minimal`, `dry_run`, and `trimmed`. Error statuses are
`invalid_usage`, `invalid_target`, `unsupported`, `active`,
`failed_source_not_replaced`, and `failed_source_replaced`.

`inspect` returns `0` after successfully reporting an active session; inspect
the JSON `status` or `active` field. Automation should also check
`source_replaced` and retain any reported `backup_path`.

| Exit | Meaning |
|---:|---|
| `0` | Success or safe no-op |
| `2` | Invalid CLI usage |
| `3` | Invalid or unavailable target |
| `4` | Malformed or unsupported transcript |
| `5` | Active or unsafe writer |
| `6` | Failure before source replacement |
| `7` | Failure after source replacement |

## Recovery

There is no implicit restore command. After exit code `7`:

1. stop Codex and every possible writer;
2. preserve the current transcript for diagnosis;
3. decompress the reported backup into a new file in the transcript directory;
4. name that file so it ends with the same session UUID and `.jsonl`;
5. validate it with `trimsess inspect`;
6. atomically rename the validated file over the original path.

Never decompress directly over the source. A decompression failure would
truncate it. `--no-backup` removes this recovery path.

## Rust library

The CLI is a thin wrapper around the concrete Codex API:

```rust,no_run
use trimsess::codex::{self, TrimOptions};

let path = "/path/to/rollout-...-<session-id>.jsonl";
let inspection = codex::inspect(path)?;
println!("{} bytes can be removed", inspection.saved_bytes());

let result = codex::trim(
    TrimOptions::new(path)
        .dry_run(true)
        .force(false),
)?;
assert!(!result.source_was_replaced());
# Ok::<(), trimsess::Error>(())
```

## License

Licensed under either the [Apache License, Version 2.0](./LICENSE-APACHE) or the
[MIT License](./LICENSE-MIT), at your option.

## Operator checklist

- [ ] Use the exact transcript path.
- [ ] Run `inspect` and verify the session ID and projected counts.
- [ ] Run `trim --dry-run`.
- [ ] Stop Codex normally, or explicitly accept the risk of `--force`.
- [ ] Keep the reported backup path until resume is verified.
- [ ] Resume the original session and verify its state.
