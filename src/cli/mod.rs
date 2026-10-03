mod args;
mod condition;
mod display;
mod read;
mod render;
mod setup;
mod store;
mod write;
use args::{Cli, Command, Notes, operation_label};
use axon::{Result, lifecycle::Operation};
use clap::{
    CommandFactory, Parser,
    error::{ContextKind, ContextValue},
};
use render::render_root_help;
use std::io::Write;

pub(super) enum Publication {
    None,
    Storage,
    Declaration,
    StorageAndDeclaration,
}
pub(super) struct Output {
    pub(super) text: String,
    pub(super) publication: Publication,
    pub(super) diagnostic: String,
}
pub(super) fn output(text: String, saved: bool) -> Output {
    Output {
        text,
        publication: if saved {
            Publication::Storage
        } else {
            Publication::None
        },
        diagnostic: String::new(),
    }
}
fn run(command: Command) -> Result<Output> {
    match command {
        Command::Actor => setup::actor(),
        Command::Docs { command } => setup::docs(command),
        Command::Completion { shell } => setup::completion(shell),
        Command::Init { prefix } => setup::init(prefix),
        Command::Storage { command } => setup::storage(command),
        Command::Import { command } => write::import(command),
        Command::Export { ids } => read::export(ids),
        Command::List(options) => read::list(options),
        Command::Proposals(options) => read::proposals(options),
        Command::Tasks(options) => read::tasks(options),
        Command::Show {
            id,
            details,
            skip_conditions,
            conditions,
        } => read::show(id, details, skip_conditions, conditions),
        Command::Log {
            id,
            recorder_details,
        } => read::log(id, recorder_details),
        Command::Note {
            command: Notes::Search { query },
        } => read::search_notes(query),
        Command::Note {
            command:
                Notes::Show {
                    id,
                    note_id,
                    recorder_details,
                },
        } => read::show_note(id, note_id, recorder_details),
        Command::Note {
            command:
                Notes::List {
                    id,
                    recorder_details,
                },
        } => read::list_notes(id, recorder_details),
        Command::Note {
            command: Notes::Add { id, message, file },
        } => write::add_note(id, message, file),
        Command::Capture(args) => write::capture(args),
        Command::Write { id, title, body } => write::write(id, title, body),
        Command::Label { command } => write::label(command),
        Command::Parent { command } => write::parent(command),
        Command::Condition { command } => write::condition(command),
        Command::Dep { command } => write::dependency(command),
        Command::Accept(args) => write::transition(args, Operation::Accept),
        Command::Withdraw(args) => write::transition(args, Operation::Withdraw),
        Command::Start(args) => write::transition(args, Operation::Start),
        Command::Release(args) => write::transition(args, Operation::Release),
        Command::Complete(args) => write::transition(args, Operation::Complete),
        Command::Cancel(args) => write::transition(args, Operation::Cancel),
        Command::Reconsider(args) => write::transition(args, Operation::Reconsider),
        Command::Reopen(args) => write::transition(args, Operation::Reopen),
        Command::Convert { id, kind } => write::convert(id, kind.kind()),
        Command::Resolve { id, head, reason } => match (id, head) {
            (Some(id), Some(head)) => write::resolve_conflict(id, head, reason),
            (id, None) => read::conflicts(id),
            (None, Some(_)) => unreachable!("clap requires ID with --head"),
        },
    }
}

pub(crate) fn main() -> std::process::ExitCode {
    let mut args = std::env::args_os();
    let program = args.next().expect("process has a program name");
    let args: Vec<_> = args.collect();
    // Help and parse errors can exit before Clap returns the global flag. No argument
    // accepts a separate hyphen-prefixed value; after `--` this token is positional text.
    let no_color = args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--no-color");
    display::configure_color(no_color);
    let mut positional = false;
    let help_args: Vec<_> = args
        .iter()
        .filter(|arg| {
            positional |= *arg == "--";
            positional || *arg != "--no-color"
        })
        .collect();
    let root_help = help_args.is_empty()
        || (help_args.len() == 1 && ["help", "-h", "--help"].iter().any(|v| help_args[0] == *v));
    let result = if root_help {
        Ok(output(render_root_help(), false))
    } else {
        let parsed =
            Cli::try_parse_from(std::iter::once(program.clone()).chain(args.iter().cloned()));
        // Clap's synthetic `help` command treats trailing flags as command names.
        // Retry only that rejection; normal parsing must retain option/value boundaries.
        let parsed = parsed.or_else(|error| {
            if no_color
                && matches!(error.get(ContextKind::InvalidSubcommand), Some(ContextValue::String(value)) if value == "--no-color")
            {
                Cli::try_parse_from(
                    [program, "--no-color".into()]
                        .into_iter()
                        .chain(help_args.into_iter().cloned()),
                )
            } else {
                Err(error)
            }
        });
        let cli = match parsed {
            Ok(cli) => cli,
            Err(error) => {
                let color = display::cli_color(error.use_stderr());
                error.with_cmd(&Cli::command().color(color)).exit();
            }
        };
        debug_assert_eq!(cli.no_color, no_color);
        let command = cli.command;
        let label = operation_label(&command);
        run(command).map_err(|error| axon::Error::Invalid(format!("{label}: {error}")))
    };
    match result {
        Ok(output) => {
            if !output.diagnostic.is_empty()
                && std::io::stderr()
                    .write_all(output.diagnostic.as_bytes())
                    .is_err()
            {
                return std::process::ExitCode::from(1);
            }
            let mut stdout = std::io::stdout().lock();
            match stdout
                .write_all(output.text.as_bytes())
                .and_then(|_| stdout.flush())
            {
                Ok(()) => std::process::ExitCode::SUCCESS,
                Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                    std::process::ExitCode::SUCCESS
                }
                Err(error) => {
                    let _ = writeln!(
                        std::io::stderr(),
                        "{}: {error}",
                        match output.publication {
                            Publication::Storage =>
                                "Error: output failed\nApplied: storage applied; output failed; inspect saved state before retrying",
                            Publication::Declaration =>
                                "Error: output failed\nApplied: declaration updated; storage unchanged; output failed; inspect declaration before retrying",
                            Publication::StorageAndDeclaration =>
                                "Error: output failed\nApplied: storage applied; declaration updated; output failed; inspect saved state before retrying",
                            Publication::None => "Error: output failed",
                        }
                    );
                    std::process::ExitCode::from(1)
                }
            }
        }
        Err(error) => {
            let _ = writeln!(
                std::io::stderr(),
                "{} {}",
                display::error_label(),
                display::human_text(error)
            );
            std::process::ExitCode::from(1)
        }
    }
}
