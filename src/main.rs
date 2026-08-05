#![forbid(unsafe_code)]

mod cli;

use std::env;
use std::process::ExitCode;

use clap::Parser;
use cli::{Cli, Command};
use trimsess::codex::{self, TrimOptions};
use trimsess::report::{self, OperationReport, OutputMode, Status};
use trimsess::{Error, ErrorKind};

const EXIT_OK: u8 = 0;
const EXIT_USAGE: u8 = 2;
const EXIT_TARGET: u8 = 3;
const EXIT_UNSUPPORTED: u8 = 4;
const EXIT_ACTIVE: u8 = 5;
const EXIT_UNCHANGED: u8 = 6;
const EXIT_REPLACED: u8 = 7;

struct CliOutcome {
    report: OperationReport,
    exit_code: u8,
}

fn main() -> ExitCode {
    let arguments: Vec<_> = env::args_os().collect();
    let requested_json = arguments
        .iter()
        .skip(1)
        .any(|argument| argument == "--json");
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error) if error.exit_code() == i32::from(EXIT_OK) => {
            let _ = error.print();
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            if requested_json {
                let error = Error::new(ErrorKind::Usage, error.to_string());
                report::write_error(&error, OutputMode::Json);
            } else {
                let _ = error.print();
            }
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let mode = if cli.json {
        OutputMode::Json
    } else {
        OutputMode::Human
    };

    match run(cli) {
        Ok(outcome) => match report::write_report(&outcome.report, mode) {
            Ok(()) => ExitCode::from(outcome.exit_code),
            Err(error) => {
                let fallback = outcome.report.output_error(&error);
                report::write_error(&fallback, mode);
                ExitCode::from(error_exit_code(fallback.kind()))
            }
        },
        Err(error) => {
            report::write_error(&error, mode);
            ExitCode::from(error_exit_code(error.kind()))
        }
    }
}

fn run(cli: Cli) -> Result<CliOutcome, Error> {
    match cli.command {
        Command::Inspect { transcript_path } => {
            codex::inspect(transcript_path).map(|report| CliOutcome {
                report,
                exit_code: EXIT_OK,
            })
        }
        Command::Trim {
            transcript_path,
            dry_run,
            no_backup,
            backup_dir,
            force,
        } => {
            let mut options = TrimOptions::new(transcript_path)
                .dry_run(dry_run)
                .force(force);
            if no_backup {
                options = options.without_backup();
            }
            if let Some(directory) = backup_dir {
                options = options.backup_dir(directory);
            }
            codex::trim(options).map(|report| CliOutcome {
                exit_code: if report.status() == Status::Active {
                    EXIT_ACTIVE
                } else {
                    EXIT_OK
                },
                report,
            })
        }
    }
}

const fn error_exit_code(kind: ErrorKind) -> u8 {
    match kind {
        ErrorKind::Usage => EXIT_USAGE,
        ErrorKind::Target => EXIT_TARGET,
        ErrorKind::Unsupported => EXIT_UNSUPPORTED,
        ErrorKind::Active => EXIT_ACTIVE,
        ErrorKind::Unchanged => EXIT_UNCHANGED,
        ErrorKind::Replaced => EXIT_REPLACED,
    }
}
