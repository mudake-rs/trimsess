//! Command-line grammar and complete operator-facing help.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

const ROOT_HELP: &str = "trimsess preserves the first session_meta record, the newest compacted\n\
record, and every later record. It removes only older transcript windows.\n\
\n\
The target must be an explicit path to one Codex rollout-*.jsonl file.\n\
It must be a non-symlink regular file owned by the current user.\n\
Copied legacy forks are supported; paginated and reference-backed forks fail closed.\n\
The newest checkpoint must carry replacement history and a window number,\n\
followed by a completed user turn with turn context. Tail rollbacks fail closed.\n\
Unsupported or changed formats fail closed. Stop Codex first, or use\n\
trim --force to stop only verified Codex processes holding the target inode.\n\
Verification requires the exact /proc/<pid>/exe basename codex.\n\
Processes whose descriptors are hidden by /proc permissions are reported but\n\
never signaled; stop remote file transfer before trimming.\n\
If a hidden process holds the target, post-rename appends are lost from the installed file.\n\
Each JSONL record is limited to 128 MiB including its line ending.\n\
Trim writes a zstd backup by default to $XDG_STATE_HOME/trimsess/backups,\n\
falling back to ~/.local/state/trimsess/backups.\n\
--force can discard unflushed Codex work.\n\
It can also stop the Codex session invoking trimsess; use an external supervisor\n\
if that session must observe the final report.\n\
\n\
Examples:\n\
  trimsess inspect /path/to/rollout-2026-08-04T12-00-00-01900000-0000-7000-8000-000000000001.jsonl\n\
  trimsess trim --dry-run /path/to/rollout-2026-08-04T12-00-00-01900000-0000-7000-8000-000000000001.jsonl\n\
  trimsess trim --force /path/to/rollout-2026-08-04T12-00-00-01900000-0000-7000-8000-000000000001.jsonl\n\
  codex resume 01900000-0000-7000-8000-000000000001\n\
\n\
Exit codes:\n\
  0  success or safe no-op\n\
  2  invalid command-line usage\n\
  3  invalid or unavailable target\n\
  4  unsupported or malformed transcript\n\
  5  active or unsafe writer\n\
  6  failure; source not replaced\n\
  7  failure after replacement; use the reported backup when present\n\
\n\
Repository: https://github.com/mudake-rs/trimsess";

const INSPECT_HELP: &str = "Reads and validates one explicit Codex transcript path without writing or\n\
signaling processes. Reports the newest compaction boundary, projected trim,\n\
and active-writer state. Copied legacy forks are supported; paginated,\n\
reference-backed, rollback-tail, and changed formats fail closed.\n\
Processes whose descriptors are hidden by /proc permissions are reported but\n\
never treated as verified writers.\n\
The target must be a non-symlink regular file owned by the current user.\n\
Each JSONL record is limited to 128 MiB including its line ending.\n\
\n\
Examples:\n\
  trimsess inspect /path/to/rollout-2026-08-04T12-00-00-01900000-0000-7000-8000-000000000001.jsonl\n\
  trimsess trim --dry-run /path/to/rollout-2026-08-04T12-00-00-01900000-0000-7000-8000-000000000001.jsonl\n\
  trimsess trim --force /path/to/rollout-2026-08-04T12-00-00-01900000-0000-7000-8000-000000000001.jsonl\n\
  codex resume 01900000-0000-7000-8000-000000000001\n\
\n\
Exit codes: 0 inspected (including active), 2 usage, 3 target, 4 format,\n\
5 activity-check failure,\n\
6 read/stability failure.\n\
Repository: https://github.com/mudake-rs/trimsess";

const TRIM_HELP: &str = "Preserves the first session_meta, newest compacted record, and later records;\n\
removes only older windows. Writes a validated compressed backup by default,\n\
builds and validates a same-directory candidate, then atomically replaces the\n\
explicit transcript.\n\
The target must be a non-symlink regular file owned by the current user.\n\
Copied legacy forks are supported; paginated and reference-backed forks fail closed.\n\
The newest checkpoint must carry replacement history and a window number,\n\
followed by a completed user turn with turn context. Tail rollbacks fail closed.\n\
The default backup directory is $XDG_STATE_HOME/trimsess/backups, falling back\n\
to ~/.local/state/trimsess/backups. Unsupported formats fail closed.\n\
Each JSONL record is limited to 128 MiB including its line ending.\n\
--force sends SIGTERM to verified Codex fd holders and escalates to SIGKILL\n\
after five seconds; unflushed Codex work can be lost.\n\
Verification requires the exact /proc/<pid>/exe basename codex.\n\
It may stop the Codex session invoking trimsess; use an external supervisor if\n\
that session must observe the final report.\n\
Processes whose descriptors are hidden by /proc permissions are reported but\n\
never signaled; stop remote file transfer before trimming.\n\
If a hidden process holds the target, post-rename appends are lost from the installed file.\n\
After exit 7, stop Codex and restore the reported zstd backup through a\n\
separate validated temporary file. --no-backup removes that recovery path.\n\
\n\
Examples:\n\
  trimsess trim --dry-run /path/to/rollout-2026-08-04T12-00-00-01900000-0000-7000-8000-000000000001.jsonl\n\
  trimsess trim --force /path/to/rollout-2026-08-04T12-00-00-01900000-0000-7000-8000-000000000001.jsonl\n\
  codex resume 01900000-0000-7000-8000-000000000001\n\
\n\
Exit codes: 0 success/no-op, 2 usage, 3 target, 4 format, 5 active,\n\
6 source not replaced, 7 source replaced.\n\
Repository: https://github.com/mudake-rs/trimsess";

#[derive(Debug, Parser)]
#[command(
    name = "trimsess",
    version,
    about = "Safely trim oversized Codex session transcripts",
    after_help = ROOT_HELP,
    after_long_help = ROOT_HELP,
    arg_required_else_help = true
)]
pub struct Cli {
    /// Emit stable machine-readable output.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Validate a transcript and report the projected trim without side effects.
    #[command(after_help = INSPECT_HELP, after_long_help = INSPECT_HELP)]
    Inspect {
        /// Exact path to one Codex rollout-*.jsonl transcript.
        transcript_path: PathBuf,
    },

    /// Back up and atomically trim a transcript.
    #[command(after_help = TRIM_HELP, after_long_help = TRIM_HELP)]
    Trim {
        /// Exact path to one Codex rollout-*.jsonl transcript.
        transcript_path: PathBuf,

        /// Report the plan without files or process signals.
        #[arg(long)]
        dry_run: bool,

        /// Skip the compressed backup. Recovery after replacement is unavailable.
        #[arg(long)]
        no_backup: bool,

        /// Directory for the compressed backup.
        #[arg(long, value_name = "PATH", conflicts_with = "no_backup")]
        backup_dir: Option<PathBuf>,

        /// Stop verified same-user Codex processes holding the target inode.
        #[arg(long)]
        force: bool,
    },
}
