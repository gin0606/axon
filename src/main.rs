mod cli_support;
mod condition;
mod display;
use cli_support::*;

use axon::{
    Result,
    lifecycle::*,
    location::{InitResult, IntegrationChange, IntegrationFile, Location},
    read, sqlite,
};
use chrono::Utc;
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::PathBuf,
    time::Duration,
};

#[derive(Parser)]
#[command(
    version = env!("AXON_VERSION"),
    styles = display::cli_styles(),
    color = display::cli_color(),
    about = "A local issue tracker for Issues and Groups",
    after_help = "Use axon docs for the lifecycle and daily workflow. Group complete explicitly confirms that the entire plan has passed final review."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Explain the lifecycle, daily workflow and storage boundaries
    Docs {
        #[command(subcommand)]
        command: Option<Docs>,
    },
    /// Export Issues and complete Group subtrees as canonical declaration YAML
    #[command(
        after_help = "Example: axon export ID... > plan.yaml\nEdit title, description, parent or needs in plan.yaml.\nNext: axon import prepare plan.yaml\nFor a new plan: axon docs declaration --example > new-plan.yaml"
    )]
    Export {
        #[arg(required = true, num_args = 1.., value_name = "ID")]
        ids: Vec<String>,
    },
    /// Prepare, check and atomically apply declaration changes
    Import {
        #[command(subcommand)]
        command: Import,
    },
    /// Show the optional recorder actor detected in the current environment
    Actor,
    /// Write an unstyled shell completion script to stdout
    Completion { shell: clap_complete::Shell },
    /// Merge snapshots through a retained review workspace
    Merge {
        #[command(subcommand)]
        command: Merge,
    },
    /// Validate stored snapshots without running conditions
    Storage {
        #[command(subcommand)]
        command: Storage,
    },
    /// Initialize a new management root (default: SQLite)
    #[command(
        after_help = "SQLite creates only .axon/axon.db and does not change Git integration files.\nFile creates .axon/state.jsonl and creates or appends .axon/.gitignore and root .gitattributes, preserving unrelated lines.\nInit does not stage or commit any files."
    )]
    Init {
        prefix: Option<String>,
        #[arg(long, value_enum, default_value = "sqlite")]
        backend: Backend,
    },
    /// Register an Issue or Group; --accept registers it as adopted work
    Capture(Create),
    /// List saved Entities in creation order without running conditions
    List(ListOptions),
    /// List surfaced Undecided Entities with surfaced ancestors
    Proposals(CandidateOptions),
    /// List surfaced NotStarted Entities and all InProgress work, including blocked work
    Tasks(CandidateOptions),
    /// Set or repair resurfacing conditions without running them
    Condition {
        #[command(subcommand)]
        command: Condition,
    },
    /// Show text, immediate unmet prerequisites and all Group descendants
    Show {
        id: String,
        /// Include saved lifecycle, condition and all direct relationships without duplicate wait sections
        #[arg(long)]
        details: bool,
    },
    /// Append or read immutable Notes
    Note {
        #[command(subcommand)]
        command: Notes,
    },
    /// Read state changes and integrations
    Log {
        id: String,
        /// Include the stored recorder data as JSON
        #[arg(long)]
        recorder_details: bool,
    },
    /// Adopt an Undecided Entity
    Accept(Change),
    /// Withdraw adoption of a NotStarted Entity
    Withdraw(Change),
    /// Start work after checking parent and dependency prerequisites
    Start(Change),
    /// Release InProgress work back to NotStarted
    Release(Change),
    /// Complete work; for a Group, explicitly confirm final review of the entire plan
    Complete(Change),
    /// Cancel work
    Cancel(Change),
    /// Return a Cancelled Entity to Undecided
    Reconsider(Change),
    /// Edit title or description without changing lifecycle
    Write {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[command(flatten)]
        body: Body,
    },
    /// Add or remove explicit dependencies
    Dep {
        #[command(subcommand)]
        command: Dependency,
    },
    /// Set or unset the parent Group without changing lifecycle
    Parent {
        #[command(subcommand)]
        command: Parent,
    },
}
#[derive(Subcommand)]
enum Import {
    /// Assign new IDs and atomically rewrite FILE as canonical YAML; storage is unchanged
    #[command(
        after_help = "Example: axon import prepare plan.yaml\nPrints new key -> full ID mappings. Storage is unchanged.\nNext: axon import check plan.yaml"
    )]
    Prepare { file: PathBuf },
    /// Validate canonical FILE and display changes without writing or running conditions
    #[command(
        after_help = "Example: axon import check plan.yaml\nReview title changes, description change indicators and relationship changes.\nNext, after reviewing: axon import apply plan.yaml"
    )]
    Check { file: PathBuf },
    /// Apply all changes atomically and refresh FILE; safe to retry the same declaration
    #[command(
        after_help = "Example: axon import apply plan.yaml\nSaves all changes and refreshes the declaration. Prints key -> full ID mappings for records that had base: null.\nNext: axon import check plan.yaml"
    )]
    Apply { file: PathBuf },
}
#[derive(Subcommand)]
enum Docs {
    /// Explain declaration fields and the prepare/check/apply workflow
    Declaration {
        /// Print only a canonical YAML template for a new plan
        #[arg(long)]
        example: bool,
    },
}
#[derive(Subcommand)]
enum Storage {
    Check { snapshot: PathBuf },
}
#[derive(Subcommand)]
#[command(
    after_help = "Example: axon merge prepare --base base.jsonl --ours ours.jsonl --theirs theirs.jsonl --output .axon/state.jsonl --workspace .axon/merge-review\nEdit choices in resolution.json, then run axon merge check WORKSPACE and axon merge apply WORKSPACE.\nGit driver configuration, staging and commits are separate user operations. Retain the workspace for recovery."
)]
enum Merge {
    /// Retain inputs and prepare conflicts and resolution choices without publishing
    Prepare {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        ours: PathBuf,
        #[arg(long)]
        theirs: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        workspace: PathBuf,
    },
    /// Validate resolution choices and the complete candidate
    Check { workspace: PathBuf },
    /// Recheck validated inputs and output before publishing
    Apply { workspace: PathBuf },
    /// Git merge driver: %O %A %B; preserve %A and fail on conflicts
    Driver {
        base: PathBuf,
        ours: PathBuf,
        theirs: PathBuf,
    },
}
#[derive(Args)]
#[command(
    after_help = "Conditions run through /bin/sh -c: exit 0 is satisfied, 1 is unsatisfied, other exits fail the whole list.
The working directory is the current Git worktree root, or the management root outside Git.
Ctrl-C and timeout send TERM to the process group, then KILL after 1s.
Each stdout/stderr stream retains up to 64 KiB (first and last 32 KiB on overflow).
Examples: axon tasks --condition-timeout 500ms; axon proposals --trace-conditions
Absence from a candidate list does not mean an Entity is missing. Use list for the complete inventory."
)]
struct CandidateOptions {
    #[command(flatten)]
    selection: Selection,
    /// Per-command timeout: positive integer followed by ms, s, m or h
    #[arg(long, default_value = "30s", value_parser = parse_timeout)]
    condition_timeout: Duration,
    /// Write evaluated condition results and captured output to stderr
    #[arg(long)]
    trace_conditions: bool,
}
#[derive(Subcommand)]
enum Condition {
    /// Save or replace a shell condition without evaluating it
    #[command(
        after_help = "Example: axon condition set ID --command 'test -f ready.txt'\nExit 0=satisfied, 1=unsatisfied, others=evaluation failed. Repair broken conditions with set or unset."
    )]
    Set {
        id: String,
        #[arg(long)]
        command: String,
    },
    /// Remove the condition without changing lifecycle
    Unset { id: String },
}
#[derive(Clone, Copy, ValueEnum)]
enum Backend {
    Sqlite,
    File,
}
#[derive(Args)]
struct Body {
    #[arg(short = 'm', long, conflicts_with = "description_file")]
    description: Option<String>,
    /// Read UTF-8 text from a file; - reads standard input
    #[arg(short = 'F', long = "file")]
    description_file: Option<PathBuf>,
}
impl Body {
    fn read(self) -> Result<Option<String>> {
        match self.description_file {
            Some(path) if path.as_os_str() == "-" => {
                let mut text = String::new();
                std::io::stdin().read_to_string(&mut text)?;
                Ok(Some(text))
            }
            Some(path) => Ok(Some(std::fs::read_to_string(&path).map_err(|e| {
                axon::Error::Invalid(format!("cannot read text file {}: {e}", path.display()))
            })?)),
            None => Ok(self.description),
        }
    }
}
#[derive(Args)]
struct Create {
    /// Title of the new Entity
    #[arg(long)]
    title: String,
    /// Entity kind to create
    #[arg(long, value_enum, default_value = "issue")]
    kind: EntityKind,
    /// Register as adopted work in NotStarted instead of Undecided
    #[arg(long)]
    accept: bool,
    /// Initial shell condition; stored without evaluation
    #[arg(long)]
    command: Option<String>,
    #[command(flatten)]
    body: Body,
    /// Containing Group; the new Entity starts only while that Group is InProgress
    #[arg(long)]
    parent: Option<String>,
    /// Dependency that must be Completed first; repeat for several
    #[arg(long)]
    needs: Vec<String>,
}
#[derive(Args)]
struct Change {
    id: String,
    #[arg(short, long)]
    reason: Option<String>,
}
#[derive(Subcommand)]
enum Parent {
    /// Set or move the parent Group
    Set {
        id: String,
        #[arg(long)]
        parent: String,
    },
    /// Remove the parent Group
    Unset { id: String },
}
#[derive(Subcommand)]
enum Dependency {
    Add {
        id: String,
        #[arg(long)]
        needs: String,
    },
    Rm {
        id: String,
        #[arg(long)]
        needs: String,
    },
}
#[derive(Subcommand)]
enum Notes {
    /// Search all Note bodies, including terminal Entities, without running conditions
    #[command(
        after_help = "Matches case-sensitive literal text without trimming or Unicode normalization. Prints one line per Note: complete Entity and Note IDs, local timestamp and an escaped excerpt around the first match. Entity creation order and Note causal order are preserved. Read the original with axon note show ID NOTE_ID. No matches succeeds with empty stdout. For a query beginning with a hyphen use: axon note search -- '--text'."
    )]
    Search {
        #[arg(value_parser = clap::builder::NonEmptyStringValueParser::new())]
        query: String,
    },
    /// Read one Note by its complete stable ID
    Show {
        id: String,
        note_id: String,
        #[arg(long)]
        recorder_details: bool,
    },
    List {
        id: String,
        /// Include the stored recorder data as JSON
        #[arg(long)]
        recorder_details: bool,
    },
    Add {
        id: String,
        #[arg(
            short = 'm',
            long,
            required_unless_present = "file",
            conflicts_with = "file"
        )]
        message: Option<String>,
        #[arg(short = 'F', long)]
        file: Option<PathBuf>,
    },
}
fn context() -> Context {
    Context {
        at: Utc::now(),
        recorder: axon_recorder::detect().map(|recorder| Recorder {
            actor: recorder.actor,
            data: recorder
                .data
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
        }),
    }
}
fn status_label(status: read::Status) -> &'static str {
    match status {
        read::Status::Undecided => "Undecided",
        read::Status::Ready => "Ready",
        read::Status::Blocked => "Blocked",
        read::Status::InProgressBlocked => "InProgress+Blocked",
        read::Status::InProgress => "InProgress",
        read::Status::Completed => "Completed",
        read::Status::Cancelled => "Cancelled",
    }
}
fn row(value: &read::Row<'_>) -> String {
    let entity = value.entity;
    format!(
        "{}  {}  {}  {}\n",
        display::identity(&entity.id),
        display::muted(format!("{:?}", entity.kind)),
        display::situation(status_label(value.status)),
        display::human_text(&entity.current.title).replace('\n', "\\n")
    )
}
fn show(value: &read::Detail<'_>, details: bool) -> String {
    let entity = value.row.entity;
    let mut out = row(&value.row);
    let notes = value.note_count;
    if notes > 0 {
        out.push_str(&format!("{notes} notes\n"));
    }
    let parent_wait = !details
        && value
            .prerequisites
            .as_ref()
            .is_some_and(|p| p.parent.is_some());
    if let Some(parent) = value.parent.filter(|_| !parent_wait) {
        out.push_str(&format!(
            "Parent: {}  {}\n",
            display::identity(&parent.id),
            display::human_text(&parent.current.title)
        ));
    }
    if !details && let Some(prerequisites) = &value.prerequisites {
        out.push_str(&format!(
            "\n{}\n",
            display::heading(match prerequisites.operation {
                read::PrerequisiteOperation::Start => "Required to start",
                read::PrerequisiteOperation::Complete => "Required to complete",
            })
        ));
        if let Some(parent) = &prerequisites.parent {
            out.push_str(&format!("Parent must start: {}", row(parent)));
        }
        for dependency in &prerequisites.dependencies {
            out.push_str(&format!("Dependency must complete: {}", row(dependency)));
        }
    }
    if details {
        let lifecycle = format!("{:?}", entity.current.lifecycle);
        if status_label(value.row.status) != lifecycle {
            out.push_str(&format!("{} {lifecycle}\n", display::muted("Lifecycle:")));
        }
        if value.parent.is_none() {
            out.push_str("Parent: (none)\n");
        }
        out.push_str(&format!(
            "{} {}\n",
            display::muted("Condition:"),
            entity
                .current
                .condition
                .as_ref()
                .map(display::human_text)
                .unwrap_or_else(|| "(none)".into())
        ));
        for (label, related) in [
            (
                "Dependencies (must be Completed to start or complete):",
                &value.dependencies,
            ),
            ("Dependents:", &value.dependents),
        ] {
            out.push_str(&format!(
                "\n{}{}\n",
                display::heading(label),
                if related.is_empty() { " (none)" } else { "" }
            ));
            for other in related {
                out.push_str(&row(other));
            }
        }
    }
    out.push('\n');
    out.push_str(&display::human_text(&entity.current.description));
    out.push('\n');
    if let Some(descendants) = &value.descendants {
        let total = descendants.entries.len();
        let completed = descendants.completed;
        let cancelled = descendants.cancelled;
        out.push_str(&format!(
            "\nDescendants: {}/{total} terminal ({completed} completed, {cancelled} cancelled)\n",
            completed + cancelled
        ));
        for child in &descendants.entries {
            for last in &child.ancestor_last {
                out.push_str(if *last { "    " } else { "│   " });
            }
            out.push_str(if child.last {
                "└── "
            } else {
                "├── "
            });
            out.push_str(&row(&child.row));
        }
        if descendants.awaiting_confirmation {
            out.push_str(&format!(
                "{}\n",
                display::heading("Awaiting final confirmation")
            ));
        }
    }
    out
}
fn actor(context: &Context) -> String {
    context
        .recorder
        .as_ref()
        .map(|r| display::human_text(&r.actor))
        .unwrap_or_else(|| "—".into())
}
fn recorder_display(context: &Context, details: bool) -> String {
    let mut text = actor(context);
    if details && let Some(recorder) = &context.recorder {
        text.push_str("  data: ");
        text.push_str(&display::human_text(
            serde_json::to_string(&recorder.data).expect("JSON object"),
        ));
    }
    text
}
enum Publication {
    None,
    Storage,
    Declaration,
    StorageAndDeclaration,
}
struct Output {
    text: String,
    publication: Publication,
    diagnostic: String,
}
fn output(text: String, saved: bool) -> Output {
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
fn branch_boundary(concurrent: bool, text: &mut String) {
    if concurrent {
        text.push_str("Concurrent branch (not ordered after the preceding record)\n");
    }
}
fn run(command: Command) -> Result<Output> {
    match command {
        Command::Actor => return Ok(output(format!("{}\n", actor(&context())), false)),
        Command::Docs { command } => {
            let text = match command {
                None => include_str!("docs/lifecycle.txt").into(),
                Some(Docs::Declaration { example: false }) => {
                    include_str!("docs/declaration.txt").into()
                }
                Some(Docs::Declaration { example: true }) => axon::declaration::example()
                    .serialize(&sqlite::empty())
                    .map_err(|e| axon::Error::Invalid(e.to_string()))?,
            };
            return Ok(output(text, false));
        }
        Command::Completion { shell } => {
            let mut bytes = Vec::new();
            clap_complete::generate(shell, &mut Cli::command(), "axon", &mut bytes);
            return Ok(output(
                String::from_utf8(bytes).expect("UTF-8 completion"),
                false,
            ));
        }
        _ => {}
    }
    let cwd = std::env::current_dir()?;
    if let Command::Init { prefix, backend } = command {
        let location = Location::discover(&cwd, true)?;
        let default_root = if matches!(backend, Backend::File) {
            &location.root
        } else {
            location
                .sqlite
                .parent()
                .and_then(|p| p.parent())
                .expect("management root")
        };
        let prefix = prefix
            .as_deref()
            .or_else(|| default_root.file_name().and_then(|n| n.to_str()))
            .ok_or_else(|| {
                axon::Error::Invalid("cannot derive an ID prefix; pass axon init PREFIX".into())
            })?;
        let text = match location.init_backend(prefix, matches!(backend, Backend::File))? {
            InitResult::Sqlite => format!(
                "Initialized SQLite at {}\n",
                display::human_text(location.sqlite.display())
            ),
            InitResult::File(files) => file_init_output(&location, &files),
        };
        return Ok(output(text, true));
    }
    match command {
        Command::Storage {
            command: Storage::Check { snapshot },
        } => {
            axon::file::decode(&std::fs::read(snapshot)?)?;
            return Ok(output("Valid snapshot\n".into(), false));
        }
        Command::Merge { command } => {
            let saved = matches!(command, Merge::Apply { .. } | Merge::Driver { .. });
            match command {
                Merge::Prepare {
                    base,
                    ours,
                    theirs,
                    output,
                    workspace,
                } => axon::file_merge::prepare(
                    &base,
                    &ours,
                    &theirs,
                    &output,
                    &workspace,
                    context(),
                )?,
                Merge::Check { workspace } => axon::file_merge::check(&workspace)?,
                Merge::Apply { workspace } => axon::file_merge::apply(&workspace)?,
                Merge::Driver { base, ours, theirs } => {
                    axon::file_merge::driver(&base, &ours, &theirs, context())?
                }
            }
            return Ok(output("Merge operation succeeded\n".into(), saved));
        }
        _ => {}
    }
    let location = Location::discover(&cwd, false)?;
    let mut store = location.open()?;
    match command {
        Command::Import {
            command: Import::Apply { file },
        } => {
            let outcome = axon::declaration_file::apply(&mut store, &file, context())?;
            Ok(Output {
                text: display::human_text(format!(
                    "Applied: {}; declaration updated: {}\n{}",
                    if outcome.changed {
                        "storage applied"
                    } else {
                        "storage unchanged (no-op)"
                    },
                    file.display(),
                    declaration_ids(&outcome.new_ids)
                )),
                publication: Publication::StorageAndDeclaration,
                diagnostic: String::new(),
            })
        }
        Command::Import { command } => {
            let (prefix, snapshot) = store.read()?;
            let file = match &command {
                Import::Prepare { file } | Import::Check { file } => file,
                Import::Apply { .. } => unreachable!(),
            };
            let bytes = std::fs::read(file)?;
            let input = std::str::from_utf8(&bytes)
                .map_err(|e| axon::Error::Invalid(format!("Declaration schema: {e}")))?;
            let mut declaration =
                axon::declaration::parse(input).map_err(|e| axon::Error::Invalid(e.to_string()))?;
            let publication = if matches!(&command, Import::Prepare { .. }) {
                Publication::Declaration
            } else {
                Publication::None
            };
            let text = match command {
                Import::Apply { .. } => unreachable!(),
                Import::Prepare { ref file } => {
                    declaration
                        .prepare(&snapshot, &prefix)
                        .map_err(|e| axon::Error::Invalid(e.to_string()))?;
                    let text = declaration
                        .serialize(&snapshot)
                        .map_err(|e| axon::Error::Invalid(e.to_string()))?;
                    axon::declaration_file::rewrite(file, &bytes, text.as_bytes())?;
                    format!(
                        "Prepared {}. Storage unchanged.\n{}",
                        file.display(),
                        declaration_ids(&declaration.assigned_new_ids())
                    )
                }
                Import::Check { .. } => {
                    let checked = declaration
                        .check(
                            input,
                            &snapshot,
                            Context {
                                at: Utc::now(),
                                recorder: None,
                            },
                        )
                        .map_err(|e| axon::Error::Invalid(e.to_string()))?;
                    declaration_changes(
                        &declaration,
                        &snapshot,
                        &checked.snapshot,
                        checked.already_applied,
                    )?
                }
            };
            Ok(Output {
                text: display::human_text(text),
                publication,
                diagnostic: String::new(),
            })
        }
        Command::Export { .. }
        | Command::List(_)
        | Command::Proposals(_)
        | Command::Tasks(_)
        | Command::Show { .. }
        | Command::Log { .. }
        | Command::Note {
            command: Notes::List { .. } | Notes::Show { .. } | Notes::Search { .. },
        } => {
            let (_, snapshot) = store.read()?;
            let empty_hint = match &command {
                Command::List(_) => "No matching Entities.",
                Command::Proposals(_) => {
                    "No proposals candidates. Conditions and ancestor scope may hide saved Entities; use axon list for the inventory."
                }
                Command::Tasks(_) => {
                    "No task candidates. Conditions and ancestor scope may hide saved Entities; use axon list for the inventory."
                }
                Command::Note {
                    command: Notes::Search { .. },
                } => "No matching Notes.",
                Command::Note { .. } => "No Notes.",
                _ => "No records.",
            };
            let text = match command {
                Command::Export { ids } => {
                    let selectors = ids
                        .iter()
                        .map(|id| resolve(&snapshot, id))
                        .collect::<Result<Vec<_>>>()?;
                    axon::declaration::export(&snapshot, &selectors)
                        .and_then(|d| d.serialize(&snapshot))
                        .map_err(|e| axon::Error::Invalid(e.to_string()))?
                }
                Command::Note {
                    command: Notes::Search { query },
                } => search_notes(&read::search_notes(&snapshot, &query)?),
                Command::Proposals(ref options) | Command::Tasks(ref options) => {
                    let kind = if matches!(command, Command::Proposals(_)) {
                        CandidateList::Proposals
                    } else {
                        CandidateList::Tasks
                    };
                    let evaluation = if options.trace_conditions {
                        condition::Evaluation::tracing(location.root, options.condition_timeout)
                    } else {
                        condition::Evaluation::with_timeout(
                            location.root,
                            options.condition_timeout,
                        )
                    };
                    read::candidates(
                        &snapshot,
                        kind,
                        |e| options.selection.matches(e),
                        |entity, script| {
                            evaluation
                                .run_command(entity, script)
                                .map_err(|e| axon::Error::Invalid(e.to_string()))
                        },
                        options.selection.search.as_deref(),
                    )?
                    .into_iter()
                    .map(|e| list_row(&e, options.selection.search.is_some()))
                    .collect()
                }
                Command::List(options) => read::list(
                    &snapshot,
                    |e| options.matches(e),
                    options.selection.search.as_deref(),
                )
                .into_iter()
                .map(|e| list_row(&e, options.selection.search.is_some()))
                .collect(),
                Command::Show { id: value, details } => show(
                    &read::detail(&snapshot, &resolve(&snapshot, &value)?)?,
                    details,
                ),
                Command::Note {
                    command:
                        Notes::Show {
                            id,
                            note_id,
                            recorder_details,
                        },
                } => {
                    let entity = resolve(&snapshot, &id)?;
                    let note = snapshot.note(&note_id.try_into()?)?;
                    if note.entity != entity {
                        return Err(axon::Error::Invalid(
                            "Note does not belong to the specified Entity".into(),
                        ));
                    }
                    format_note(note, recorder_details)
                }
                Command::Log {
                    id: value,
                    recorder_details,
                } => {
                    let mut text = String::new();
                    for entry in read::history(&snapshot, &resolve(&snapshot, &value)?)? {
                        branch_boundary(entry.concurrent_with_previous, &mut text);
                        let record = entry.record;
                        let description = match &record.event {
                            StateEvent::Created { initial, .. } => format!("Created: {initial:?}"),
                            StateEvent::Transition {
                                before,
                                after,
                                reason,
                                ..
                            } => format!(
                                "{before:?} → {after:?}{}",
                                reason
                                    .as_ref()
                                    .map(|r| format!("  Reason: {}", display::human_text(r)))
                                    .unwrap_or_default()
                            ),
                            StateEvent::Integration {
                                inputs,
                                selected,
                                reason,
                            } => format!(
                                "Integrated: selected {:?}{}",
                                inputs[*selected].current.lifecycle,
                                reason
                                    .as_ref()
                                    .map(|r| format!("  Reason: {}", display::human_text(r)))
                                    .unwrap_or_default()
                            ),
                        };
                        text.push_str(&format!(
                            "{}  {}  {description}\n",
                            display::muted(display::timestamp(&record.context.at)),
                            recorder_display(&record.context, recorder_details)
                        ));
                    }
                    text
                }
                Command::Note {
                    command:
                        Notes::List {
                            id: value,
                            recorder_details,
                        },
                } => {
                    let mut text = String::new();
                    for entry in read::notes(&snapshot, &resolve(&snapshot, &value)?)? {
                        branch_boundary(entry.concurrent_with_previous, &mut text);
                        text.push_str(&format_note(entry.record, recorder_details));
                    }
                    text
                }
                _ => unreachable!(),
            };
            let diagnostic = if text.is_empty() {
                format!("{empty_hint}\n")
            } else {
                String::new()
            };
            Ok(Output {
                text,
                publication: Publication::None,
                diagnostic,
            })
        }
        Command::Capture(args) => create(&mut store, args),
        Command::Write {
            id: value,
            title,
            body,
        } => {
            let body = body.read()?;
            if title.is_none() && body.is_none() {
                return Err(axon::Error::Invalid(
                    "write requires --title, --description or --file".into(),
                ));
            }
            let text = store.update(|_, snapshot| {
                let id = resolve(snapshot, &value)?;
                let before = snapshot.entity(&id)?.current.clone();
                snapshot.write(&id, title, body)?;
                let after = &snapshot.entity(&id)?.current;
                let mut changes = Vec::new();
                if before.title != after.title {
                    changes.push("Title updated");
                }
                if before.description != after.description {
                    changes.push(if after.description.is_empty() {
                        "Description removed"
                    } else {
                        "Description updated"
                    });
                }
                if changes.is_empty() {
                    changes.push("No changes");
                }
                Ok(confirmation(&id, &changes.join("  ")))
            })?;
            Ok(output(text, true))
        }
        Command::Note {
            command:
                Notes::Add {
                    id: value,
                    message,
                    file,
                },
        } => {
            let body = Body {
                description: message,
                description_file: file,
            }
            .read()?
            .unwrap_or_default();
            let text = store.update(|_, snapshot| {
                let id = resolve(snapshot, &value)?;
                let note = snapshot.add_note(&id, body, context())?;
                Ok(confirmation(&id, &format!("Note {note} recorded")))
            })?;
            Ok(output(text, true))
        }
        Command::Parent { command } => {
            let (value, parent) = match command {
                Parent::Set { id, parent } => (id, Some(parent)),
                Parent::Unset { id } => (id, None),
            };
            let text = store.update(|_, snapshot| {
                let id = resolve(snapshot, &value)?;
                let parent = parent.map(|p| resolve(snapshot, &p)).transpose()?;
                let unchanged = snapshot.entity(&id)?.current.parent == parent;
                snapshot.set_parent(&id, parent.clone())?;
                Ok(confirmation(
                    &id,
                    &format!(
                        "{}Parent: {}",
                        if unchanged { "No changes  " } else { "" },
                        parent
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| "(none)".into())
                    ),
                ))
            })?;
            Ok(output(text, true))
        }
        Command::Condition { command } => {
            let (value, command) = match command {
                Condition::Set { id, command } => (id, Some(command)),
                Condition::Unset { id } => (id, None),
            };
            let text = store.update(|_, snapshot| {
                let id = resolve(snapshot, &value)?;
                let unchanged = snapshot.entity(&id)?.current.condition == command;
                let unset = command.is_none();
                snapshot.set_condition(&id, command)?;
                Ok(confirmation(
                    &id,
                    if unchanged {
                        "No changes"
                    } else if unset {
                        "Condition unset"
                    } else {
                        "Condition updated"
                    },
                ))
            })?;
            Ok(output(text, true))
        }
        Command::Dep { command } => {
            let (value, needs, add) = match command {
                Dependency::Add { id, needs } => (id, needs, true),
                Dependency::Rm { id, needs } => (id, needs, false),
            };
            let text = store.update(|_, snapshot| {
                let id = resolve(snapshot, &value)?;
                let needs = resolve(snapshot, &needs)?;
                let present = snapshot.entity(&id)?.current.dependencies.contains(&needs);
                if add {
                    snapshot.add_dependency(&id, &needs)?;
                } else {
                    snapshot.remove_dependency(&id, &needs)?;
                }
                let result = match (add, present) {
                    (true, false) => "Dependency added:",
                    (false, true) => "Dependency removed:",
                    (true, true) => "No changes  Dependency already present:",
                    (false, false) => "No changes  Dependency already absent:",
                };
                Ok(confirmation(&id, &format!("{result} {needs}")))
            })?;
            Ok(output(text, true))
        }
        command => {
            let (args, operation) = match command {
                Command::Accept(args) => (args, Operation::Accept),
                Command::Withdraw(args) => (args, Operation::Withdraw),
                Command::Start(args) => (args, Operation::Start),
                Command::Release(args) => (args, Operation::Release),
                Command::Complete(args) => (args, Operation::Complete),
                Command::Cancel(args) => (args, Operation::Cancel),
                Command::Reconsider(args) => (args, Operation::Reconsider),
                _ => unreachable!(),
            };
            let text = store.update(|_, snapshot| {
                let id = resolve(snapshot, &args.id)?;
                snapshot.perform(&id, operation, args.reason, context())?;
                let effect = match operation {
                    Operation::Accept => "Accepted  NotStarted",
                    Operation::Withdraw => "Withdrawn  Undecided",
                    Operation::Start => "Started  InProgress",
                    Operation::Release => "Released  NotStarted",
                    Operation::Complete => "Completed",
                    Operation::Cancel => "Cancelled",
                    Operation::Reconsider => "Reconsidered  Undecided",
                };
                Ok(confirmation(&id, effect))
            })?;
            Ok(output(text, true))
        }
    }
}

fn file_init_output(location: &Location, files: &[IntegrationFile]) -> String {
    let mut text = format!(
        "Initialized file at {}\n",
        display::human_text(location.root.join(".axon/state.jsonl").display())
    );
    for file in files {
        let label = match file.change {
            IntegrationChange::Created => "Created",
            IntegrationChange::Appended => "Appended",
            IntegrationChange::Unchanged => "Unchanged",
        };
        text.push_str(&format!(
            "{label}: {}\n",
            display::human_text(file.path.display())
        ));
    }
    text
}
fn create(store: &mut axon::location::Store, args: Create) -> Result<Output> {
    let kind = args.kind.kind();
    let lifecycle = if args.accept {
        Lifecycle::NotStarted
    } else {
        Lifecycle::Undecided
    };
    let description = args.body.read()?.unwrap_or_default();
    let title = args.title;
    let text = store.update(|prefix, snapshot| {
        let parent = args.parent.map(|p| resolve(snapshot, &p)).transpose()?;
        let dependencies = args
            .needs
            .into_iter()
            .map(|p| resolve(snapshot, &p))
            .collect::<Result<BTreeSet<_>>>()?;
        let id = fresh_entity_id(prefix, snapshot)?;
        snapshot.create(
            id.clone(),
            kind,
            Current {
                title,
                description,
                lifecycle,
                condition: args.command,
                parent,
                dependencies,
            },
            context(),
        )?;
        let title = display::human_text(&snapshot.entity(&id)?.current.title).replace('\n', "\\n");
        Ok(format!(
            "{}  {}  {}  {}  {title}\n",
            display::identity(&id),
            display::positive("Created"),
            display::muted(format!("{kind:?}")),
            display::situation(&format!("{lifecycle:?}"))
        ))
    })?;
    Ok(output(text, true))
}

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let root_help = args.is_empty()
        || (args.len() == 1 && ["help", "-h", "--help"].iter().any(|v| args[0] == *v));
    let result = if root_help {
        Ok(output(render_root_help(), false))
    } else {
        let command = Cli::parse().command;
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

fn declaration_ids(ids: &[(String, String)]) -> String {
    ids.iter()
        .map(|(key, id)| {
            format!(
                "{key} -> {}\n",
                display::human_text(id).replace('\n', "\\n")
            )
        })
        .collect()
}

fn declaration_changes(
    declaration: &axon::declaration::Declaration,
    before: &Snapshot,
    after: &Snapshot,
    applied: bool,
) -> Result<String> {
    let mut out = String::new();
    if applied {
        out.push_str("Already applied.\n");
    }
    for r in declaration.records() {
        let id: EntityId = r.id.clone().expect("checked ID").try_into()?;
        let entity = after.entity(&id)?;
        out.push_str(&format!("{id}:\n"));
        if let Ok(old) = before.entity(&id) {
            let a = &old.current;
            let b = &entity.current;
            if a == b {
                out.push_str("  No changes.\n");
            } else {
                out.push_str(&format!(
                    "  title: {}\n  description: {}\n",
                    if a.title == b.title {
                        "unchanged".into()
                    } else {
                        format!(
                            "{} -> {}",
                            display::human_text(&a.title).replace('\n', "\\n"),
                            display::human_text(&b.title).replace('\n', "\\n")
                        )
                    },
                    if a.description == b.description {
                        "unchanged"
                    } else {
                        "changed"
                    }
                ));
                out.push_str(&format!(
                    "  parent: {} -> {}\n",
                    a.parent
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "null".into()),
                    b.parent
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "null".into())
                ));
                for id in a.dependencies.difference(&b.dependencies) {
                    out.push_str(&format!("  needs: - {id}\n"));
                }
                for id in b.dependencies.difference(&a.dependencies) {
                    out.push_str(&format!("  needs: + {id}\n"));
                }
            }
        } else {
            out.push_str(&format!(
                "  Create {}\n  title: new\n  description: new\n  parent: null -> {}\n",
                axon::declaration::kind(entity.kind),
                entity
                    .current
                    .parent
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "null".into())
            ));
            for id in &entity.current.dependencies {
                out.push_str(&format!("  needs: + {id}\n"));
            }
        }
        out.push_str(&format!(
            "  Situation after: {} (conditions not evaluated)\n",
            status_label(read::status(after, entity))
        ));
    }
    if declaration.records().next().is_none() {
        out.push_str("No changes.\n");
    }
    out.push_str("Check passed. Storage and declaration unchanged.\n");
    Ok(out)
}
