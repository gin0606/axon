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
    CommandFactory, Parser, ValueHint,
    builder::StyledStr,
    error::{ContextKind, ContextValue, ErrorKind},
};
use render::render_root_help;
use std::io::Write;

pub(super) enum Publication {
    None,
    Storage,
    Declaration,
    StorageAndDeclaration,
    /// An approval of the named Entity's condition was saved; it is not a record.
    Approval(String),
}
pub(super) struct Output {
    pub(super) text: String,
    pub(super) publication: Publication,
    pub(super) diagnostic: String,
    /// Stderr written after stdout, for what follows an applied change: a failure to write it
    /// changes neither the output nor the exit code.
    pub(super) trailer: String,
    /// The invocation exits 1 after writing its output, as when the record set is unchanged
    /// but the approval it was asked for could not be saved.
    pub(super) failed: bool,
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
        trailer: String::new(),
        failed: false,
    }
}
fn run(command: Command) -> Result<Output> {
    match command {
        Command::Actor => setup::actor(),
        Command::Gui => setup::gui(),
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
        Command::Write {
            id,
            title,
            body,
            reason,
        } => write::write(id, title, body, reason),
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
        Command::Convert { id, kind, reason } => write::convert(id, kind.kind(), reason),
        Command::Resolve { id, head, reason } => match (id, head) {
            (Some(id), Some(head)) => write::resolve_conflict(id, head, reason),
            (id, None) => read::conflicts(id),
            (None, Some(_)) => unreachable!("clap requires ID with --head"),
        },
    }
}

/// How Axon changes Clap's tips for a parse error.
enum Tip {
    Keep,
    /// Clap's only tip is to pass the unexpected argument after `--`, where it would not be taken
    /// as the value it may have been meant as.
    Remove,
    /// The argument rejected is a value starting with a hyphen.
    HyphenValue,
}

/// Whether Clap rejected a value starting with a hyphen that the argument it was meant for
/// accepts, and so whether its tip to pass the argument after `--` is replaced or left out. Such a
/// value is written apart from its option and is not a flag of the command; Clap failed at it when
/// the arguments up to it give the same error, but not with the value attached by `=`. Without
/// one, an unexpected hyphen-prefixed argument may be a positional value, or after `--` a value
/// meant for an option, unless Clap suggests a flag for it.
fn hyphen_value_tip_for(
    error: &clap::Error,
    cli: &clap::Command,
    args: &[std::ffi::OsString],
) -> Tip {
    let Some(ContextValue::String(rejected)) = error.get(ContextKind::InvalidArg) else {
        return Tip::Keep;
    };
    let mut command = cli.clone();
    command.build();
    let tokens: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
    let written = |arg: &clap::Arg, token: &str| {
        arg.get_long()
            .is_some_and(|long| token.strip_prefix("--") == Some(long))
            || arg.get_short().is_some_and(|short| {
                token
                    .strip_prefix('-')
                    .is_some_and(|rest| rest.chars().eq([short]))
            })
    };
    // Subcommands are named before any argument but a flag.
    let mut invoked = &command;
    let mut path = Vec::new();
    for token in &tokens {
        if let Some(subcommand) = invoked.find_subcommand(token.as_ref()) {
            invoked = subcommand;
            path.push(subcommand.get_name().to_owned());
        } else if !invoked
            .get_arguments()
            .any(|arg| !arg.get_action().takes_values() && written(arg, token))
        {
            break;
        }
    }
    // A flag of the command, alone or as `--name=VALUE`.
    let flag = |token: &str| {
        let name = token.split_once('=').map_or(token, |(name, _)| name);
        invoked
            .get_arguments()
            .any(|arg| written(arg, token) || name.starts_with("--") && written(arg, name))
    };
    // A value from a fixed set never starts with a hyphen, so the tip would not help.
    let open =
        |arg: &clap::Arg| arg.get_action().takes_values() && arg.get_possible_values().is_empty();
    let run = |command: clap::Command, args: Vec<&std::ffi::OsString>| {
        command.try_get_matches_from(std::iter::once(&"axon".into()).chain(args))
    };
    let parse = |args: Vec<&std::ffi::OsString>| run(command.clone(), args);
    let same_error = |result: &std::result::Result<clap::ArgMatches, clap::Error>| {
        result.as_ref().is_err_and(|again| {
            again.kind() == error.kind()
                && again.get(ContextKind::InvalidArg) == error.get(ContextKind::InvalidArg)
        })
    };
    // Whether the arguments pass a value to its option, which may still reject it or be given
    // already.
    let accepts = |result: &std::result::Result<clap::ArgMatches, clap::Error>| {
        !matches!(result, Err(again) if matches!(
            again.kind(),
            ErrorKind::InvalidValue
                | ErrorKind::ValueValidation
                | ErrorKind::InvalidUtf8
                | ErrorKind::ArgumentConflict
        ))
    };
    let options = tokens
        .iter()
        .position(|token| token == "--")
        .unwrap_or(tokens.len());
    // Where Clap failed at an option's value: whether the option takes it once attached.
    let failed = (0..options).find_map(|at| {
        // Clap's default tip, `-- VALUE`, puts the value after a `--` that follows the option.
        let value = match tokens.get(at + 1)? {
            next if next == "--" && at + 2 < tokens.len() => at + 2,
            next if !flag(next) => at + 1,
            _ => return None,
        };
        if !invoked
            .get_arguments()
            .any(|arg| arg.get_action().takes_values() && written(arg, &tokens[at]))
            || !tokens[value].starts_with('-')
            || !same_error(&parse(args[..=value].iter().collect()))
        {
            return None;
        }
        let mut attached = args[at].clone();
        attached.push("=");
        attached.push(&args[value]);
        let attached = parse(args[..at].iter().chain([&attached]).collect());
        (!same_error(&attached)).then(|| accepts(&attached))
    });
    let value = match failed {
        Some(accepted) => accepted,
        None => {
            error.kind() == ErrorKind::UnknownArgument
                && rejected.starts_with('-')
                && error.get(ContextKind::SuggestedArg).is_none()
                && if options == args.len() || same_error(&parse(args[..options].iter().collect()))
                {
                    // Before `--`, the value helps only if a positional takes it, as it would after
                    // a `--`. Taking hyphen values, positionals may also take an unknown flag or
                    // a short option with its value attached, which leaves no tip.
                    let moved = run(
                        hyphen_positionals(cli.clone(), &path),
                        args.iter().collect(),
                    );
                    accepts(&moved)
                        && moved
                            .as_ref()
                            .err()
                            .is_none_or(|again| again.kind() != ErrorKind::UnknownArgument)
                } else {
                    // After `--`, a flag of the command was placed there by mistake, and a value
                    // helps only if an option of the command takes it.
                    // The value as written, which Clap shows lossily.
                    let written = args[options + 1..]
                        .iter()
                        .find(|arg| arg.to_string_lossy() == *rejected);
                    !flag(rejected)
                        && written.is_some_and(|written| {
                            invoked
                                .get_arguments()
                                // A file option takes a path, not the value.
                                .filter(|arg| {
                                    open(arg)
                                        && !matches!(
                                            arg.get_value_hint(),
                                            ValueHint::AnyPath | ValueHint::FilePath
                                        )
                                })
                                .filter_map(|arg| arg.get_long())
                                .any(|long| {
                                    let mut attached =
                                        std::ffi::OsString::from(format!("--{long}="));
                                    attached.push(written);
                                    let moved = args[..options].iter().chain([&attached]).collect();
                                    accepts(&parse(moved))
                                })
                        })
                }
        }
    };
    if value {
        Tip::HyphenValue
    // Clap gives this tip only where the command takes a positional value.
    } else if error.kind() == ErrorKind::UnknownArgument
        && invoked.get_positionals().next().is_some()
    {
        Tip::Remove
    } else {
        Tip::Keep
    }
}

/// The command whose subcommand at `path` takes hyphen-prefixed positional values.
fn hyphen_positionals(command: clap::Command, path: &[String]) -> clap::Command {
    match path.split_first() {
        Some((name, rest)) => {
            command.mut_subcommand(name, |subcommand| hyphen_positionals(subcommand, rest))
        }
        None => command.mut_args(|arg| {
            if arg.is_positional() {
                arg.allow_hyphen_values(true)
            } else {
                arg
            }
        }),
    }
}

fn hyphen_value_tip(valid: &clap::builder::styling::Style) -> StyledStr {
    StyledStr::from(format!(
        "pass a value starting with '-' to an option as {valid}--option='VALUE'{valid:#} or, for a description or Note, from a file with {valid}-F{valid:#}; put a positional value after {valid}--{valid:#}, following the options"
    ))
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
                let command = Cli::command().color(color);
                let mut error = error.with_cmd(&command);
                let tip = hyphen_value_tip_for(&error, &command, &args);
                // Clap quotes the rejected arguments in its message. Escape them as one-line
                // values before it renders, so Axon's own decoration stays intact.
                let escaped: Vec<_> = error
                    .context()
                    .filter_map(|(kind, value)| {
                        let escaped = match value {
                            ContextValue::String(value) => {
                                ContextValue::String(display::line(value))
                            }
                            ContextValue::Strings(values) => {
                                ContextValue::Strings(values.iter().map(display::line).collect())
                            }
                            _ => return None,
                        };
                        (escaped != *value).then_some((kind, escaped))
                    })
                    .collect();
                // Its tips repeat the argument inside text that already carries decoration, so
                // they are left out rather than shown unescaped.
                if !escaped.is_empty() {
                    error.remove(ContextKind::Suggested);
                }
                for (kind, value) in escaped {
                    error.insert(kind, value);
                }
                // Clap's tip to pass an unexpected argument after `--` makes a value meant for
                // an option positional instead. Replace it with the forms that pass the value
                // where it belongs, or leave it out where no such value can be passed.
                match tip {
                    Tip::Keep => {}
                    Tip::Remove => {
                        error.remove(ContextKind::Suggested);
                    }
                    Tip::HyphenValue => {
                        let tip = hyphen_value_tip(command.get_styles().get_valid());
                        error.insert(ContextKind::Suggested, ContextValue::StyledStrs(vec![tip]));
                    }
                }
                error.exit();
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
            let written = stdout
                .write_all(output.text.as_bytes())
                .and_then(|_| stdout.flush());
            drop(stdout);
            if !output.trailer.is_empty() {
                let _ = std::io::stderr().write_all(output.trailer.as_bytes());
            }
            let success = if output.failed {
                std::process::ExitCode::from(1)
            } else {
                std::process::ExitCode::SUCCESS
            };
            match written {
                Ok(()) => success,
                Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => success,
                Err(error) => {
                    let _ = writeln!(
                        std::io::stderr(),
                        "{}: {error}",
                        match &output.publication {
                            Publication::Storage =>
                                "Error: output failed\nApplied: storage applied; output failed; inspect saved state before retrying".to_owned(),
                            Publication::Declaration =>
                                "Error: output failed\nApplied: declaration updated; storage unchanged; output failed; inspect declaration before retrying".to_owned(),
                            Publication::StorageAndDeclaration =>
                                "Error: output failed\nApplied: storage applied; declaration updated; output failed; inspect saved state before retrying".to_owned(),
                            Publication::Approval(id) => format!(
                                "Error: output failed\nApplied: approval saved; output failed; check whether {id} is listed by axon condition unapproved"
                            ),
                            Publication::None => "Error: output failed".to_owned(),
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
