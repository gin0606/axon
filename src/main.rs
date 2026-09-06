mod actor;
mod codec;
mod core;
mod db;
mod declaration;
mod derived;
mod display;
mod domain;
mod history;
mod record_id;
mod status;

use anstyle::{AnsiColor, Color, Style};
use chrono::{NaiveDate, Utc};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use db::{Change, Ctx};
use storage::Store;
mod storage;
use derived::{TriageReason, View};
use domain::*;

struct HelpSection {
    heading: &'static str,
    commands: &'static [&'static str],
}

const HELP_SECTIONS: &[HelpSection] = &[
    HelpSection {
        heading: "Workflow",
        commands: &[
            "plan", "capture", "ready", "triage", "start", "done", "release",
        ],
    },
    HelpSection {
        heading: "Inspect",
        commands: &[
            "status", "show", "list", "claims", "log", "note", "revision",
        ],
    },
    HelpSection {
        heading: "Plan management",
        commands: &[
            "write", "group", "dep", "decide", "when", "export", "import",
        ],
    },
    HelpSection {
        heading: "Setup & utilities",
        commands: &["init", "migrate", "completion", "docs", "help"],
    },
];

const OUTPUT_HEADING: Style = Style::new().bold();
const OUTPUT_ID: Style = Style::new()
    .bold()
    .fg_color(Some(Color::Ansi(AnsiColor::Cyan)));
const OUTPUT_ACTIVE: Style = Style::new()
    .bold()
    .fg_color(Some(Color::Ansi(AnsiColor::Cyan)));
const OUTPUT_POSITIVE: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)));
const OUTPUT_WAITING: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Yellow)));
const OUTPUT_DECISION: Style = Style::new()
    .bold()
    .fg_color(Some(Color::Ansi(AnsiColor::Yellow)));
const OUTPUT_FAILURE: Style = Style::new()
    .bold()
    .fg_color(Some(Color::Ansi(AnsiColor::Red)));
const OUTPUT_INDEX: Style = Style::new().bold();
const OUTPUT_MUTED: Style = Style::new().dimmed();

#[derive(Clone, Copy)]
enum OutputDecoration {
    Plain,
    Ansi,
}

impl OutputDecoration {
    fn paint(self, style: Style, value: impl std::fmt::Display) -> String {
        match self {
            Self::Plain => value.to_string(),
            Self::Ansi => format!("{style}{value}{style:#}"),
        }
    }
}

fn output_decoration(is_terminal: bool, no_color: bool) -> OutputDecoration {
    if is_terminal && !no_color {
        OutputDecoration::Ansi
    } else {
        OutputDecoration::Plain
    }
}

fn current_output_decoration() -> OutputDecoration {
    use std::io::IsTerminal;

    output_decoration(
        std::io::stdout().is_terminal(),
        std::env::var_os("NO_COLOR").is_some(),
    )
}

fn current_error_decoration() -> OutputDecoration {
    use std::io::IsTerminal;

    output_decoration(
        std::io::stderr().is_terminal(),
        std::env::var_os("NO_COLOR").is_some(),
    )
}

fn write_output(output: &str, decoration: OutputDecoration) -> std::io::Result<()> {
    match decoration {
        OutputDecoration::Plain => write_output_to(std::io::stdout(), output),
        OutputDecoration::Ansi => write_output_to(
            anstream::AutoStream::new(std::io::stdout(), anstream::ColorChoice::Always),
            output,
        ),
    }
}

fn write_plain_output(output: &str) -> std::io::Result<()> {
    write_output_to(std::io::stdout(), output)
}

fn write_output_to(mut stream: impl std::io::Write, output: &str) -> std::io::Result<()> {
    match stream.write_all(output.as_bytes()) {
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

#[derive(Parser)]
#[command(
    name = "axon",
    version,
    about = "A local issue tracker with independent state axes"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args)]
struct TraceConditionsArgs {
    /// Trace evaluated Command conditions to stderr, including unredacted child output
    #[arg(
        long,
        long_help = "Trace each Command condition actually evaluated by this invocation to stderr. Each block includes the Entity ID, working directory, exit result, and captured stdout/stderr. Captured output is not redacted or truncated; non-UTF-8 bytes are rendered lossily, empty streams are marked (empty), and a trace write failure fails the Axon invocation. Memoized references and abnormal exits do not produce a trace block."
    )]
    trace_conditions: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum KindFilter {
    Issue,
    Group,
}

impl KindFilter {
    fn matches(self, entity: &Entity) -> bool {
        matches!(
            (self, entity.kind),
            (Self::Issue, EntityKind::Issue) | (Self::Group, EntityKind::Group)
        )
    }
}

#[derive(Subcommand)]
enum Command {
    /// Convert a v11/v12 SQLite snapshot to causal history without modifying the source
    Migrate {
        /// Existing v11 or v12 database (stop its writers before the final conversion)
        #[arg(long)]
        source: std::path::PathBuf,
        /// New directory for the backup, converted database and ID mapping manifest
        #[arg(long)]
        output: std::path::PathBuf,
    },
    /// Initialize axon at the management root
    Init {
        /// Storage backend (default: sqlite)
        #[arg(long, value_enum)]
        backend: Option<storage::Backend>,
        /// Prefix for generated Entity IDs; defaults to the management-root directory name
        prefix: Option<String>,
    },
    /// Generate a shell completion script
    Completion {
        /// Shell whose completion script is written to standard output
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Explain Axon's state model and basic workflow
    Docs {
        #[command(subcommand)]
        topic: Option<DocsCmd>,
    },
    /// Create an Accepted issue
    Plan {
        /// Parent group ID or unique ID suffix
        #[arg(long)]
        parent: Option<String>,
        /// Initial description; an empty value leaves it absent
        #[arg(short = 'm', long)]
        message: Option<String>,
        /// File containing the initial description, or - for standard input
        #[arg(short = 'F', long)]
        file: Option<String>,
        /// One or more words joined with spaces to form the issue title
        #[arg(required = true, value_name = "TITLE")]
        title: Vec<String>,
    },
    /// Create an Undecided issue
    Capture {
        /// Parent group ID or unique ID suffix
        #[arg(long)]
        parent: Option<String>,
        /// Initial description; an empty value leaves it absent
        #[arg(short = 'm', long)]
        message: Option<String>,
        /// File containing the initial description, or - for standard input
        #[arg(short = 'F', long)]
        file: Option<String>,
        /// One or more words joined with spaces to form the issue title
        #[arg(required = true, value_name = "TITLE")]
        title: Vec<String>,
    },
    /// List Entities that can be started now
    Ready {
        /// Include only one Entity kind
        #[arg(long, value_enum)]
        kind: Option<KindFilter>,
        #[command(flatten)]
        trace: TraceConditionsArgs,
    },
    /// List the active decision frontier
    Triage {
        /// Include only one Entity kind
        #[arg(long, value_enum)]
        kind: Option<KindFilter>,
        #[command(flatten)]
        trace: TraceConditionsArgs,
    },
    /// Summarize plans, saved claims, candidates, and waits
    #[command(
        long_about = "Summarize root plans and ungrouped Issues whose root is non-terminal or whose subtree has saved claims. --group includes the specified Group and every descendant, even when terminal. Each item combines its candidates, saved claim, and waits; empty sections are omitted. Candidate sets match ready and triage; saved claims do not imply agent activity. Ended and Rejected Groups omit completion and descendant-gate prompts. Rejected Groups retain their own stored dependency and resurface-condition waits. Use show <ID> for complete details and terminal Group structure."
    )]
    Status {
        /// Group ID or unique ID suffix; include its complete descendant scope
        #[arg(long)]
        group: Option<String>,
        #[command(flatten)]
        trace: TraceConditionsArgs,
    },
    /// List every active claim
    Claims {
        /// Include only one Entity kind
        #[arg(long, value_enum)]
        kind: Option<KindFilter>,
    },
    /// Claim one ready Entity
    Start {
        /// Entity ID or unique ID suffix
        id: String,
        #[command(flatten)]
        trace: TraceConditionsArgs,
    },
    /// Mark one InProgress Entity as Ended
    Done {
        /// Entity ID or unique ID suffix
        id: String,
    },
    /// Release one InProgress Entity
    Release {
        /// Entity ID or unique ID suffix
        id: String,
        /// Release reason or handoff recorded in progress history
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// Update an Entity title or description
    Write {
        /// Entity ID or unique ID suffix
        id: String,
        /// Replacement title
        #[arg(long)]
        title: Option<String>,
        /// Replacement description; an empty value removes it
        #[arg(short = 'm', long)]
        message: Option<String>,
        /// File containing the replacement description, or - for standard input
        #[arg(short = 'F', long)]
        file: Option<String>,
    },
    /// Show decision history for one Entity
    Log {
        /// Entity ID or unique ID suffix
        id: String,
    },
    /// List every Entity regardless of state
    List {
        /// Include only one Entity kind
        #[arg(long, value_enum)]
        kind: Option<KindFilter>,
        #[command(flatten)]
        trace: TraceConditionsArgs,
    },
    /// Inspect one Entity from situation and waits to its full details
    Show {
        /// Entity ID or unique ID suffix
        id: String,
        #[command(flatten)]
        trace: TraceConditionsArgs,
    },
    /// Add and inspect durable notes
    #[command(subcommand)]
    Note(NoteCmd),
    /// Inspect immutable plan declaration revisions
    #[command(subcommand)]
    Revision(RevisionCmd),
    /// Export an editable plan declaration as canonical YAML
    Export {
        /// Entity IDs or unique ID suffixes to edit
        #[arg(value_name = "ID")]
        ids: Vec<String>,
        /// Group ID or unique suffix; includes the group and its direct children
        #[arg(long = "group", value_name = "GROUP")]
        groups: Vec<String>,
        /// Include every descendant of each --group selector
        #[arg(long, requires = "groups")]
        recursive: bool,
    },
    /// Prepare, validate, or atomically apply a plan declaration
    #[command(subcommand)]
    #[command(after_help = "Use axon docs declaration for the format and workflow.
Use axon docs declaration --example for a complete new-plan YAML example.")]
    Import(ImportCmd),
    /// Change an Entity Disposition
    #[command(subcommand)]
    Decide(DecideCmd),
    /// Change an Entity resurface condition
    #[command(subcommand)]
    When(WhenCmd),
    /// Manage Entity dependencies
    #[command(subcommand)]
    Dep(DepCmd),
    /// Create groups and manage containment
    #[command(subcommand)]
    Group(GroupCmd),
}

#[derive(Subcommand)]
enum GroupCmd {
    /// Create an Accepted group
    Plan {
        /// Parent group ID or unique ID suffix
        #[arg(long)]
        parent: Option<String>,
        /// Initial description; an empty value leaves it absent
        #[arg(short = 'm', long)]
        message: Option<String>,
        /// File containing the initial description, or - for standard input
        #[arg(short = 'F', long)]
        file: Option<String>,
        /// One or more words joined with spaces to form the group title
        #[arg(required = true, value_name = "TITLE")]
        title: Vec<String>,
    },
    /// Create an Undecided group
    Capture {
        /// Parent group ID or unique ID suffix
        #[arg(long)]
        parent: Option<String>,
        /// Initial description; an empty value leaves it absent
        #[arg(short = 'm', long)]
        message: Option<String>,
        /// File containing the initial description, or - for standard input
        #[arg(short = 'F', long)]
        file: Option<String>,
        /// One or more words joined with spaces to form the group title
        #[arg(required = true, value_name = "TITLE")]
        title: Vec<String>,
    },
    /// Set or move an Entity's parent group
    Set {
        /// Entity ID or unique ID suffix to contain
        id: String,
        /// Parent group ID or unique ID suffix
        parent: String,
    },
    /// Remove an Entity from its parent group
    Unset {
        /// Entity ID or unique ID suffix
        id: String,
    },
}

#[derive(Subcommand)]
enum DocsCmd {
    /// Explain declaration fields and the prepare/check/apply workflow
    Declaration {
        /// Write only a complete new-plan YAML example to stdout
        #[arg(long)]
        example: bool,
    },
}

#[derive(Subcommand)]
enum ImportCmd {
    /// Assign final IDs and rewrite a declaration into canonical form without changing the DB
    #[command(after_help = "Use axon docs declaration for the format and workflow.
Save axon docs declaration --example output to a file, then pass it to prepare.")]
    Prepare {
        /// YAML declaration file to rewrite
        file: std::path::PathBuf,
    },
    /// Validate a canonical declaration and show structural and derived changes
    Check {
        /// Canonical YAML declaration file
        file: std::path::PathBuf,
        #[command(flatten)]
        trace: TraceConditionsArgs,
    },
    /// Validate and atomically apply a canonical declaration
    Apply {
        /// Canonical YAML declaration file to apply and refresh
        file: std::path::PathBuf,
        #[command(flatten)]
        trace: TraceConditionsArgs,
    },
}

#[derive(Subcommand)]
enum DecideCmd {
    /// Set Disposition to Accepted
    Accept(DecisionArgs),
    /// Set Disposition to Rejected
    Reject(DecisionArgs),
    /// Set Disposition to Undecided
    Undecide(DecisionArgs),
}

#[derive(clap::Args)]
struct DecisionArgs {
    /// Entity ID or unique ID suffix whose Disposition is changed
    id: String,
    /// Reason recorded in decision history
    #[arg(short, long)]
    reason: Option<String>,
}

#[derive(Subcommand)]
enum WhenCmd {
    /// Keep an Entity unsurfaced until its condition is explicitly changed
    Manual {
        /// Entity ID or unique ID suffix
        id: String,
        /// Reason recorded in decision history
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// Set an AtDate resurface condition
    At {
        /// Entity ID or unique ID suffix
        id: String,
        /// Resurface date in YYYY-MM-DD format
        date: String,
        /// Reason recorded in decision history
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// Set an AfterEntity resurface condition
    After {
        /// Entity ID or unique ID suffix
        id: String,
        /// Entity whose terminal state satisfies the condition
        reference: String,
        /// Reason recorded in decision history
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// Evaluate a shell command when derived status is needed
    #[command(
        long_about = "Store a Command condition for an Issue or Group. Run /bin/sh -c in the current worktree root (Axon management root outside Git), inheriting the caller's environment without interactive or login startup. Exit 0 satisfies the condition, 1 does not; other exits, signals, and spawn failures fail Axon. Adapt other tools' exit codes in your script. Each Entity is evaluated at most once per invocation; the next invocation reevaluates, and satisfaction can revert. Results are not stored. Normal stdout/stderr is suppressed; failures include diagnostics. Timeouts, persistent caches, intervals, and replay are the script's responsibility: Axon waits indefinitely for it to finish. Setting or clearing a condition does not evaluate it."
    )]
    Command {
        /// Entity ID or unique ID suffix
        id: String,
        /// Shell string passed as one argument to /bin/sh -c
        command: String,
        /// Reason recorded in decision history
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// Set the resurface condition to Always
    Clear {
        /// Entity ID or unique ID suffix
        id: String,
        /// Reason recorded in decision history
        #[arg(short, long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand)]
enum DepCmd {
    /// Add a hard prerequisite
    Add {
        /// Dependent Entity ID or unique ID suffix
        id: String,
        /// Required Entity ID or unique ID suffix
        #[arg(long)]
        needs: String,
    },
    /// Remove a hard prerequisite
    Rm {
        /// Dependent Entity ID or unique ID suffix
        id: String,
        /// Entity ID or unique ID suffix no longer required
        #[arg(long)]
        needs: String,
    },
}

#[derive(Subcommand)]
enum NoteCmd {
    /// Append a note to an Entity
    Add {
        /// Entity ID or unique ID suffix
        id: String,
        /// Note body
        #[arg(short = 'm', long)]
        message: Option<String>,
        /// File containing the note body, or - for standard input
        #[arg(short = 'F', long)]
        file: Option<String>,
    },
    /// List notes for an Entity
    List {
        /// Entity ID or unique ID suffix
        id: String,
    },
    /// Show one note by its stable ID
    Show {
        /// Entity ID or unique ID suffix
        id: String,
        /// Note stable ID or unique prefix (at least 4 characters)
        number: String,
    },
}

#[derive(Subcommand)]
enum RevisionCmd {
    /// List plan declaration revisions for an Entity
    List {
        /// Entity ID or unique ID suffix
        id: String,
    },
    /// Show one revision by its stable ID
    Show {
        /// Entity ID or unique ID suffix
        id: String,
        /// Revision stable ID or unique prefix (at least 4 characters)
        number: String,
    },
    /// Compare two plan declaration revisions
    Diff {
        /// Entity ID or unique ID suffix
        id: String,
        /// Older Revision stable ID or unique prefix (at least 4 characters)
        from: String,
        /// Newer Revision stable ID or unique prefix (at least 4 characters)
        to: String,
    },
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 1 {
        if let Err(error) = cli_command().print_help() {
            if error.kind() == std::io::ErrorKind::BrokenPipe {
                return;
            }
            let decoration = current_error_decoration();
            eprintln!("{} {error}", decoration.paint(OUTPUT_FAILURE, "Error:"));
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run(args) {
        let decoration = current_error_decoration();
        eprintln!("{} {error}", decoration.paint(OUTPUT_FAILURE, "Error:"));
        if let Some(guidance) = error_guidance(error.as_ref()) {
            eprintln!("{} {guidance}", decoration.paint(OUTPUT_HEADING, "Help:"));
        }
        std::process::exit(1);
    }
}

fn open_store(trace_conditions: bool) -> db::Result<Store> {
    Store::open(trace_conditions, |outcome| {
        if let db::migration::Outcome::Migrated {
            from, to, backup, ..
        } = outcome
        {
            let decoration = current_error_decoration();
            eprintln!(
                "{} database v{from} -> v{to}; backup: {}. Migration committed; continuing command.",
                decoration.paint(OUTPUT_POSITIVE, "Migrated"),
                backup.display()
            );
        }
    })
}

fn migration_guidance(failure: &db::migration::Failure) -> String {
    use db::migration::ApplicationState;
    let state = match failure.applied {
        ApplicationState::NotApplied => {
            "Migration was not applied to the source; the requested operation did not run. Incomplete output may remain."
        }
        ApplicationState::Applied => {
            "Migration was committed to the output; the source was not switched. Preserve both databases."
        }
        ApplicationState::Unknown => {
            "Migration output durability is unknown; the source was not switched. Preserve and inspect the output and backup before any retry."
        }
    };
    let cause = match failure.source.as_ref() {
        db::DbError::UnsupportedSchema { found, expected } if found > expected =>
            format!("Use a newer axon build that supports DB schema {found}; automatic downgrade is not supported."),
        db::DbError::UnsupportedSchema { found, .. } =>
            format!("Manual migration accepts schema v11/v12: `axon migrate --source <v11-or-v12-db> --output <new-directory>`. For v9/v10, first use a v11 build. Use a build that supports DB schema {found} to inspect this older database."),
        db::DbError::InvalidSchema(_) => "Preserve the database; its schema or stored data did not pass validation. Inspect the reported structure/integrity error with SQLite tooling.".to_string(),
        db::DbError::Sqlite(rusqlite::Error::SqliteFailure(error, _))
            if matches!(error.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) =>
            "Check for other writers, including linked worktrees, holding the database lock.".to_string(),
        _ => "Check the reported cause and stage: file/directory write permissions, available disk space, and SQLite database integrity at the displayed paths.".to_string(),
    };
    format!(
        "{state} {cause} Keep any backup (a failed backup may be incomplete). init cannot upgrade an existing DB; do not delete it or change user_version. See `axon docs` for storage recovery guidance."
    )
}

fn error_guidance(error: &(dyn std::error::Error + 'static)) -> Option<String> {
    use db::DbError;

    if let Some(error) = error.downcast_ref::<DbError>() {
        return Some(match error {
            DbError::Migration(failure) => migration_guidance(failure),
            DbError::UnsupportedSchema { found, .. } => format!(
                "Use an axon build that supports DB schema {found}. Automatic migration supports v9 and later; init cannot upgrade an existing DB. Preserve the database and do not change user_version to bypass this check. See `axon docs` for storage recovery guidance."
            ),
            DbError::CannotStart { id, .. } | DbError::CannotProgress { id, .. } => format!(
                "Inspect `axon show {id}` for state, relationships, and Group completion constraints. `axon docs` explains the operation prerequisites."
            ),
            DbError::DeclarationFixed(id) => format!(
                "Inspect `axon show {id}`. If changing the plan is intended, `axon decide undecide {id}` makes its declaration editable and withdraws its current disposition; edit, review the full declaration, then decide separately. Supplemental information can be appended with `axon note add {id}` without changing the plan."
            ),
            DbError::NoSuchEntity(_) => "Use `axon list` to inspect Entity IDs in this active root.".to_string(),
            DbError::AmbiguousId { .. } => "Use one of the full candidate IDs to identify the intended Entity.".to_string(),
            DbError::NotGroup(_) => "Use `axon list --kind group` to inspect Group IDs. A parent must be a Group.".to_string(),
            DbError::NoSuchNote { id, .. } => format!("Use `axon note list {id}` to inspect this Entity's Note IDs."),
            DbError::NoSuchRevision { id, .. } => format!("Use `axon revision list {id}` to inspect this Entity's Revision IDs."),
            DbError::Unchanged { id, .. } => format!("No transition was applied. Use `axon show {id}` to inspect the current state; repeating the same transition is an error."),
            DbError::Cycle { .. } | DbError::Containment(_) => "Inspect the involved Entities with `axon show <ID>` and use `axon docs` for relationship constraints. Changing a relationship changes the plan; select any revision according to the intended plan.".to_string(),
            DbError::AlreadyInitialized(_) => "init creates a new database; it does not reset or upgrade an existing one. Use `axon list` to inspect it, or `axon docs` for storage recovery guidance.".to_string(),
            DbError::Evaluation(_) => evaluation_guidance().to_string(),
            _ => return None,
        });
    }
    if error.is::<derived::EvaluationError>() {
        return Some(evaluation_guidance().to_string());
    }
    error.source().and_then(error_guidance)
}

fn evaluation_guidance() -> &'static str {
    "See `axon when command --help` for the exit-status contract. Correcting a Command condition, or explicitly replacing or clearing it, does not evaluate the failing condition; replacing or clearing it changes when the Entity surfaces."
}

fn run(args: Vec<std::ffi::OsString>) -> Result<(), Box<dyn std::error::Error>> {
    let mut matches = cli_command().get_matches_from(args);
    match Cli::from_arg_matches_mut(&mut matches)?.command {
        Command::Migrate { source, output } => {
            if let db::migration::Outcome::Migrated {
                database, backup, ..
            } = db::migration::migrate(&source, &output)?
            {
                write_plain_output(&format!(
                    "Converted: {}\nSource backup: {}\nManifest: {}\nSource unchanged; verify the output before switching the live database.\n",
                    database.display(),
                    backup.display(),
                    output.join("manifest.yaml").display()
                ))?;
            }
            Ok(())
        }
        Command::Init { prefix, backend } => cmd_init(prefix, backend),
        Command::Completion { shell } => write_completion(shell).map_err(Into::into),
        Command::Docs { topic } => cmd_docs(topic),
        Command::Plan {
            title,
            parent,
            message,
            file,
        } => cmd_create(
            EntityKind::Issue,
            Disposition::Accepted,
            title,
            parent,
            message,
            file,
        ),
        Command::Capture {
            title,
            parent,
            message,
            file,
        } => cmd_create(
            EntityKind::Issue,
            Disposition::Undecided,
            title,
            parent,
            message,
            file,
        ),
        Command::Ready { kind, trace } => cmd_ready(kind, trace.trace_conditions),
        Command::Triage { kind, trace } => cmd_triage(kind, trace.trace_conditions),
        Command::Claims { kind } => cmd_claims(kind),
        Command::Status { group, trace } => status::run(group.as_deref(), trace.trace_conditions),
        Command::Start { id, trace } => cmd_start(&id, trace.trace_conditions),
        Command::Done { id } => cmd_done(&id),
        Command::Release { id, reason } => cmd_release(&id, reason),
        Command::Write {
            id,
            title,
            message,
            file,
        } => cmd_write(&id, title, message, file),
        Command::Log { id } => cmd_log(&id),
        Command::List { kind, trace } => cmd_list(kind, trace.trace_conditions),
        Command::Show { id, trace } => cmd_show(&id, trace.trace_conditions),
        Command::Note(command) => cmd_note(command),
        Command::Revision(command) => cmd_revision(command),
        Command::Export {
            ids,
            groups,
            recursive,
        } => cmd_export(ids, groups, recursive),
        Command::Import(command) => cmd_import(command),
        Command::Decide(command) => cmd_decide(command),
        Command::When(command) => cmd_when(command),
        Command::Dep(command) => cmd_dep(command),
        Command::Group(command) => cmd_group(command),
    }
}

fn cmd_export(
    ids: Vec<String>,
    groups: Vec<String>,
    recursive: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = open_store(false)?;
    write_plain_output(&declaration::export(&mut store, &ids, &groups, recursive)?)?;
    Ok(())
}

fn cmd_import(command: ImportCmd) -> Result<(), Box<dyn std::error::Error>> {
    let decoration = current_output_decoration();
    match command {
        ImportCmd::Prepare { file } => {
            let mut store = open_store(false)?;
            declaration::prepare(&mut store, &file)?;
            write_output(
                &format!(
                    "{}  {}\n",
                    decoration.paint(OUTPUT_POSITIVE, "Prepared"),
                    file.display()
                ),
                decoration,
            )?;
        }
        ImportCmd::Check { file, trace } => {
            let mut store = open_store(trace.trace_conditions)?;
            write_output(
                &decorate_import_report(&declaration::check(&mut store, &file)?, decoration),
                decoration,
            )?;
        }
        ImportCmd::Apply { file, trace } => {
            let mut store = open_store(trace.trace_conditions)?;
            let mut output =
                decorate_import_report(&declaration::apply(&mut store, &file)?, decoration);
            output.push_str(&format!(
                "{}  {}\n",
                decoration.paint(OUTPUT_POSITIVE, "Applied"),
                file.display()
            ));
            write_output(&output, decoration)?;
        }
    }
    Ok(())
}

fn decorate_import_report(report: &str, decoration: OutputDecoration) -> String {
    report
        .split_inclusive('\n')
        .map(|line| {
            let (body, newline) = line
                .strip_suffix('\n')
                .map_or((line, ""), |body| (body, "\n"));
            let decorated = match body {
                "Plan is valid." => decoration.paint(OUTPUT_POSITIVE, body),
                "Changes:"
                | "Derived changes (ready, blocked, orphaned, active_scope, group_completable):" => {
                    decoration.paint(OUTPUT_HEADING, body)
                }
                _ if body.starts_with("Warning: ") => decoration.paint(OUTPUT_DECISION, body),
                _ => body.to_string(),
            };
            format!("{decorated}{newline}")
        })
        .collect()
}

fn cmd_init(
    prefix: Option<String>,
    backend: Option<storage::Backend>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (path, prefix) = Store::init(prefix.as_deref(), backend)?;
    let decoration = current_output_decoration();
    write_output(
        &format!(
            "{}  {}\n{}  {prefix}-xxxxxx\n",
            decoration.paint(OUTPUT_POSITIVE, "Initialized"),
            path.display(),
            decoration.paint(OUTPUT_MUTED, "Entity ID format:"),
        ),
        decoration,
    )?;
    Ok(())
}

fn cmd_create(
    kind: EntityKind,
    disposition: Disposition,
    title: Vec<String>,
    parent: Option<String>,
    message: Option<String>,
    file: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let title = normalize_title(&title.join(" "))?;
    let description = read_description(message, file)?.and_then(normalize_description);
    let mut store = open_store(false)?;
    let parent = parent
        .as_deref()
        .map(|value| store.resolve_id(value))
        .transpose()?;
    let now = Utc::now();
    let entity = Entity {
        id: EntityId::generate(&store.prefix()?),
        kind,
        title,
        description,
        progress: Progress::NotStarted,
        disposition,
        current_revision: (disposition != Disposition::Undecided)
            .then(|| RecordId::new(RecordKind::Revision)),
        resurface_condition: ResurfaceCondition::Always,
        parent,
        created_at: now,
        updated_at: now,
    };
    store.insert(&entity)?;
    let decoration = current_output_decoration();
    write_output(
        &format!(
            "{}  {}  {}  [{}]  {}\n",
            decoration.paint(OUTPUT_ID, &entity.id),
            decoration.paint(OUTPUT_POSITIVE, "Created"),
            decoration.paint(OUTPUT_MUTED, kind.label()),
            decoration.paint(disposition_style(disposition), disposition.label()),
            entity.title,
        ),
        decoration,
    )?;
    Ok(())
}

fn cmd_group(command: GroupCmd) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        GroupCmd::Plan {
            title,
            parent,
            message,
            file,
        } => cmd_create(
            EntityKind::Group,
            Disposition::Accepted,
            title,
            parent,
            message,
            file,
        ),
        GroupCmd::Capture {
            title,
            parent,
            message,
            file,
        } => cmd_create(
            EntityKind::Group,
            Disposition::Undecided,
            title,
            parent,
            message,
            file,
        ),
        GroupCmd::Set { id, parent } => {
            let mut store = open_store(false)?;
            let id = store.resolve_id(&id)?;
            let parent = store.resolve_id(&parent)?;
            store.apply(&id, Change::SetParent(Some(parent.clone())), &ctx(None))?;
            write_confirmation(
                &id,
                render_inline_fields(
                    current_output_decoration(),
                    vec![("Parent", parent.to_string())],
                ),
                Style::new(),
            )?;
            Ok(())
        }
        GroupCmd::Unset { id } => {
            let mut store = open_store(false)?;
            let id = store.resolve_id(&id)?;
            store.apply(&id, Change::SetParent(None), &ctx(None))?;
            write_confirmation(&id, "Parent: (none)".to_string(), Style::new())?;
            Ok(())
        }
    }
}

fn load(trace_conditions: bool) -> Result<(Store, View), Box<dyn std::error::Error>> {
    let mut store = open_store(trace_conditions)?;
    let view = store.view()?;
    Ok((store, view))
}

fn included(kind: Option<KindFilter>, entity: &Entity) -> bool {
    kind.is_none_or(|filter| filter.matches(entity))
}

fn render_entity_identity(entity: &Entity, decoration: OutputDecoration) -> String {
    format!(
        "{}  {}  {}",
        decoration.paint(OUTPUT_ID, &entity.id),
        decoration.paint(OUTPUT_MUTED, entity.kind.label()),
        entity.title
    )
}

fn write_confirmation(id: &EntityId, result: String, style: Style) -> std::io::Result<()> {
    let decoration = current_output_decoration();
    write_output(
        &format!(
            "{}  {}\n",
            decoration.paint(OUTPUT_ID, id),
            decoration.paint(style, result)
        ),
        decoration,
    )
}

fn cmd_ready(
    kind: Option<KindFilter>,
    trace_conditions: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load(trace_conditions)?;
    let decoration = current_output_decoration();
    let rows = view
        .ready(|entity| included(kind, entity))?
        .into_iter()
        .map(|entity| format!("{}\n", render_entity_identity(entity, decoration)))
        .collect::<String>();
    write_rows(
        &rows,
        "No ready entities match the current scope and filter. Use `axon list` to inspect stored Entities and `axon docs` for readiness conditions.",
        decoration,
    )?;
    Ok(())
}

fn cmd_triage(
    kind: Option<KindFilter>,
    trace_conditions: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load(trace_conditions)?;
    let decoration = current_output_decoration();
    let mut rows = String::new();
    for (entity, reason) in view.triage(|entity| included(kind, entity))? {
        rows.push_str(&render_triage_row(&view, entity, reason, decoration));
    }
    write_rows(
        &rows,
        "No entities match the active decision frontier and filter. Use `axon list` to inspect Entities outside this frontier.",
        decoration,
    )?;
    Ok(())
}

fn render_triage_row(
    view: &View,
    entity: &Entity,
    reason: TriageReason,
    decoration: OutputDecoration,
) -> String {
    let fields = match reason {
        TriageReason::Undecided => {
            vec![("Reason", decoration.paint(OUTPUT_DECISION, "Undecided"))]
        }
        TriageReason::Orphaned => {
            let rejected = view
                .dependency_targets(&entity.id)
                .into_iter()
                .filter(|target| target.disposition == Disposition::Rejected)
                .map(|target| decoration.paint(OUTPUT_ID, &target.id))
                .collect::<Vec<_>>()
                .join(", ");
            vec![
                ("Reason", decoration.paint(OUTPUT_FAILURE, "Orphaned")),
                ("Rejected dependencies", rejected),
            ]
        }
    };
    format!(
        "{}  {}\n",
        render_entity_identity(entity, decoration),
        render_inline_fields(decoration, fields)
    )
}

fn cmd_claims(kind: Option<KindFilter>) -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load(false)?;
    let decoration = current_output_decoration();
    let rows = view
        .claims()
        .into_iter()
        .filter(|(entity, _)| included(kind, entity))
        .map(|(entity, claim)| render_claim_row(entity, claim, decoration))
        .collect::<String>();
    write_rows(&rows, "No active claims", decoration)?;
    Ok(())
}

fn render_claim_row(entity: &Entity, claim: &Claim, decoration: OutputDecoration) -> String {
    format!(
        "{}  {}\n",
        render_entity_identity(entity, decoration),
        render_inline_fields(
            decoration,
            vec![
                ("Claim", claim.actor.clone()),
                ("Worktree", claim.worktree.clone()),
                ("Started", display::timestamp(&claim.at)),
            ]
        )
    )
}

fn claim_details(claim: &Claim, decoration: OutputDecoration) -> String {
    format!(
        "{}  {}",
        claim.actor,
        render_inline_fields(
            decoration,
            vec![
                ("Worktree", claim.worktree.clone()),
                ("Started", display::timestamp(&claim.at)),
            ],
        )
    )
}

fn cmd_start(raw: &str, trace_conditions: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = open_store(trace_conditions)?;
    let id = store.resolve_id(raw)?;
    let claim = Claim {
        actor: actor::actor(),
        worktree: actor::worktree()?,
        at: Utc::now(),
    };
    store.apply(&id, Change::Start(claim.clone()), &ctx(None))?;
    let decoration = current_output_decoration();
    write_confirmation(
        &id,
        format!(
            "{}  {} {}",
            decoration.paint(OUTPUT_ACTIVE, "Started"),
            decoration.paint(OUTPUT_MUTED, "Claim:"),
            claim.actor
        ),
        Style::new(),
    )?;
    Ok(())
}

fn cmd_done(raw: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = open_store(false)?;
    let id = store.resolve_id(raw)?;
    store.apply(&id, Change::Done, &ctx(None))?;
    write_confirmation(&id, "Ended".to_string(), OUTPUT_MUTED)?;
    Ok(())
}

fn cmd_release(raw: &str, reason: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = open_store(false)?;
    let id = store.resolve_id(raw)?;
    store.apply(&id, Change::Release, &ctx(reason))?;
    write_confirmation(&id, "Released".to_string(), Style::new())?;
    Ok(())
}

fn cmd_list(
    kind: Option<KindFilter>,
    trace_conditions: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load(trace_conditions)?;
    let decoration = current_output_decoration();
    let mut rows = String::new();
    for entity in view.iter().filter(|entity| included(kind, entity)) {
        rows.push_str(&render_list_row(&view, entity, decoration)?);
    }
    write_rows(&rows, "No entities", decoration)?;
    Ok(())
}

fn render_list_row(
    view: &View,
    entity: &Entity,
    decoration: OutputDecoration,
) -> derived::Result<String> {
    let marks = render_entity_marks(view, entity, false, decoration)?;
    Ok(format!(
        "{}  {}  [{}/{}]  {}{}\n",
        decoration.paint(OUTPUT_ID, &entity.id),
        decoration.paint(OUTPUT_MUTED, entity.kind.label()),
        decoration.paint(progress_style(&entity.progress), entity.progress.label()),
        decoration.paint(
            disposition_style(entity.disposition),
            entity.disposition.label()
        ),
        entity.title,
        marks
    ))
}

fn render_entity_marks(
    view: &View,
    entity: &Entity,
    include_ready: bool,
    decoration: OutputDecoration,
) -> derived::Result<String> {
    let mut marks = Vec::new();
    if include_ready && view.is_ready(entity)? {
        marks.push(decoration.paint(OUTPUT_POSITIVE, "Ready"));
    }
    if view.is_orphaned(&entity.id) {
        marks.push(decoration.paint(OUTPUT_FAILURE, "Orphaned"));
    } else if view.is_blocked(&entity.id) {
        marks.push(decoration.paint(OUTPUT_WAITING, "Blocked"));
    }
    if !view.is_surfaced(entity)? {
        marks.push(decoration.paint(
            OUTPUT_MUTED,
            format!("Not surfaced: {}", entity.resurface_condition.label()),
        ));
    }
    if let Some(reason) = inactive_scope_reason(view, &entity.id)? {
        marks.push(decoration.paint(OUTPUT_MUTED, format!("Inactive: {reason}")));
    }
    Ok(if marks.is_empty() {
        String::new()
    } else {
        format!("  {}", marks.join("  "))
    })
}

fn cmd_show(raw: &str, trace_conditions: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = open_store(trace_conditions)?;
    let db::ShowSnapshot {
        id,
        view,
        decision_events,
        progress_events,
        revisions,
        notes,
        counts,
        causal,
    } = store.show_snapshot(raw)?;
    debug_assert_eq!(counts.decisions, decision_events.len());
    debug_assert_eq!(counts.progressions, progress_events.len());
    debug_assert_eq!(counts.revisions, revisions.len());
    debug_assert_eq!(counts.notes, notes.len());
    let entity = view.get(&id).ok_or("entity not found")?;
    let decoration = current_output_decoration();
    let mut output = render_show(&view, entity, &progress_events, &notes, counts, decoration)?;
    output.push_str(&causal.display(&id));
    write_output(&output, decoration)?;
    Ok(())
}

fn render_show(
    view: &View,
    entity: &Entity,
    progress_events: &[db::ProgressEvent],
    notes: &[Note],
    counts: RecordCounts,
    decoration: OutputDecoration,
) -> derived::Result<String> {
    let mut blocks = Vec::<Vec<String>>::new();
    let mut overview = vec![format!(
        "{}  {}  {}",
        decoration.paint(OUTPUT_ID, &entity.id),
        decoration.paint(OUTPUT_MUTED, entity.kind.label()),
        entity.title,
    )];

    overview.push(format!(
        "{} {}",
        decoration.paint(OUTPUT_MUTED, "Situation:"),
        show_situation(view, entity, decoration)?,
    ));
    let rejected_group =
        entity.kind == EntityKind::Group && entity.disposition == Disposition::Rejected;
    if rejected_group {
        overview.push(decoration.paint(
            OUTPUT_MUTED,
            "Rejected Group: already terminal; descendant scope is inactive. Unfinished descendants are saved states and do not require follow-up by themselves.",
        ));
        let saved_claims = std::iter::once(entity)
            .chain(view.descendants(&entity.id))
            .filter(|member| member.progress.claim().is_some())
            .count();
        if saved_claims > 0 {
            overview.push(format!(
                "{} Inspect each claimed Entity and decide separately whether its external work should end, be released, or continue outside this Group.",
                decoration.paint(
                    OUTPUT_WAITING,
                    format!("Saved claims remain: {saved_claims}.")
                )
            ));
        }
    }
    let mut structural_details = Vec::new();
    if rejected_group {
        overview.extend(show_waits(view, entity, "", decoration, false)?);
    } else {
        if matches!(entity.progress, Progress::Ended) {
            structural_details.extend(show_waits(view, entity, "", decoration, true)?);
            if entity.kind == EntityKind::Group {
                structural_details
                    .push(decoration.paint(OUTPUT_MUTED, "Can complete: no (Group has ended)."));
            }
        } else {
            overview.extend(show_waits(view, entity, "", decoration, true)?);
        }
    }
    for ancestor in view.ancestors(&entity.id) {
        if ancestor.disposition == Disposition::Rejected
            && !matches!(entity.progress, Progress::Ended)
        {
            overview.push(format!(
                "{} {}; {}",
                decoration.paint(OUTPUT_MUTED, "Rejected ancestor scope:"),
                decoration.paint(OUTPUT_ID, &ancestor.id),
                decoration.paint(
                    OUTPUT_MUTED,
                    "descendant scope is inactive; saved states and claims are unchanged."
                )
            ));
            let saved_conditions = show_waits(view, ancestor, "  ", decoration, false)?;
            if !saved_conditions.is_empty() {
                overview.push(format!(
                    "{} {}",
                    decoration.paint(OUTPUT_MUTED, "Ancestor conditions:"),
                    decoration.paint(OUTPUT_ID, &ancestor.id)
                ));
                overview.extend(saved_conditions);
            }
        }
        if ancestor.disposition != Disposition::Rejected
            && has_local_descendant_gate(view, ancestor)?
        {
            let target = if matches!(entity.progress, Progress::Ended)
                || matches!(ancestor.progress, Progress::Ended)
            {
                &mut structural_details
            } else {
                &mut overview
            };
            target.push(format!(
                "{} {}",
                decoration.paint(OUTPUT_MUTED, "Ancestor scope:"),
                decoration.paint(OUTPUT_ID, &ancestor.id)
            ));
            target.extend(show_waits(view, ancestor, "  ", decoration, true)?);
        }
    }
    blocks.push(overview);
    let mut overview = Vec::new();
    let inactive_reason = inactive_scope_reason(view, &entity.id)?;
    let active_scope = inactive_reason
        .as_ref()
        .map(|reason| format!("no ({reason})"))
        .unwrap_or_else(|| "yes".to_string());
    let blocked = view.is_blocked(&entity.id);
    let orphaned = view.is_orphaned(&entity.id);
    let surfaced = view.is_surfaced(entity)?;
    overview.push(render_inline_fields(
        decoration,
        vec![
            (
                "Progress",
                decoration.paint(progress_style(&entity.progress), entity.progress.label()),
            ),
            (
                "Disposition",
                decoration.paint(
                    disposition_style(entity.disposition),
                    entity.disposition.label(),
                ),
            ),
            ("Resurface condition", entity.resurface_condition.label()),
        ],
    ));
    overview.push(render_inline_fields(
        decoration,
        vec![
            (
                "Active scope",
                decoration.paint(
                    if inactive_reason.is_some() {
                        OUTPUT_MUTED
                    } else {
                        Style::new()
                    },
                    active_scope,
                ),
            ),
            (
                "Surfaced",
                decoration.paint(
                    if surfaced { Style::new() } else { OUTPUT_MUTED },
                    yes_no(surfaced),
                ),
            ),
            (
                "Blocked",
                decoration.paint(
                    if blocked {
                        OUTPUT_WAITING
                    } else {
                        OUTPUT_MUTED
                    },
                    yes_no(blocked),
                ),
            ),
            (
                "Orphaned",
                decoration.paint(
                    if orphaned {
                        OUTPUT_FAILURE
                    } else {
                        OUTPUT_MUTED
                    },
                    yes_no(orphaned),
                ),
            ),
        ],
    ));
    if let Some(claim) = entity.progress.claim() {
        overview.push(render_inline_fields(
            decoration,
            vec![("Claim", claim_details(claim, decoration))],
        ));
    }

    let (declaration, declaration_style) = match entity.current_revision {
        Some(revision) => (format!("fixed at Revision {revision}"), Style::new()),
        None => ("draft".to_string(), OUTPUT_DECISION),
    };
    let mut plan = vec![(
        "Plan declaration",
        decoration.paint(declaration_style, declaration),
    )];
    if let Some(parent) = &entity.parent {
        plan.push(("Parent", decoration.paint(OUTPUT_ID, parent)));
    }
    overview.push(render_inline_fields(decoration, plan));
    overview.push(render_inline_fields(
        decoration,
        vec![(
            "Records",
            format!(
                "Notes: {}  Revisions: {}  Decision history: {}  Progress history: {}",
                counts.notes, counts.revisions, counts.decisions, counts.progressions
            ),
        )],
    ));
    overview.extend(structural_details);
    let details = render_section(decoration, "Details", overview);
    let mut group_details = Vec::new();
    let mut dependencies = Vec::new();

    if entity.kind == EntityKind::Group {
        let summary = view.group_summary(&entity.id);
        let counts = &summary.descendants;
        let overlap = counts.ended + counts.rejected - counts.terminal;
        let unfinished = counts.total - counts.terminal;
        let mut group = vec![
            format!(
                "  {} {}  {} {}  {} {}",
                decoration.paint(OUTPUT_MUTED, "Descendants: Ended:"),
                decoration.paint(OUTPUT_MUTED, counts.ended),
                decoration.paint(OUTPUT_MUTED, "Rejected:"),
                decoration.paint(OUTPUT_MUTED, counts.rejected),
                decoration.paint(OUTPUT_MUTED, "Unfinished:"),
                decoration.paint(
                    if unfinished == 0 { OUTPUT_MUTED } else { OUTPUT_WAITING },
                    unfinished
                )
            ),
            decoration.paint(
                OUTPUT_MUTED,
                format!(
                    "  Ended and Rejected overlap: {overlap}; Unfinished excludes both (not a commitment)."
                ),
            ),
        ];
        if !entity.is_terminal() {
            let can_complete = view.can_complete_group(&entity.id);
            group.push(format!(
                "  {} {}",
                decoration.paint(OUTPUT_MUTED, "Can complete:"),
                decoration.paint(
                    if can_complete {
                        OUTPUT_POSITIVE
                    } else {
                        OUTPUT_WAITING
                    },
                    yes_no(can_complete)
                )
            ));
            if !matches!(entity.progress, Progress::InProgress(_)) {
                group.push(format!(
                    "  {}",
                    decoration.paint(
                        OUTPUT_WAITING,
                        "Completion requires Group Progress=InProgress."
                    )
                ));
            }
            if counts.total > counts.terminal {
                group.push(format!(
                    "  {}",
                    decoration.paint(
                        OUTPUT_WAITING,
                        format!(
                            "Completion requires {} unfinished descendants to become terminal.",
                            counts.total - counts.terminal
                        )
                    )
                ));
            } else if can_complete {
                group.push(format!(
                    "  {}",
                    decoration.paint(
                        OUTPUT_POSITIVE,
                        "All descendants are terminal; awaiting explicit done."
                    )
                ));
            }
        }
        blocks.push(render_section(decoration, "Group", group));
        let mut group = Vec::new();
        group.extend(render_entity_counts(
            "Direct children",
            &summary.direct,
            decoration,
        ));
        group.push(String::new());
        group.extend(render_entity_counts(
            "Descendants",
            &summary.descendants,
            decoration,
        ));
        group_details = group;

        let subtree = ordered_subtree(view, &entity.id);
        for (_, descendant) in &subtree {
            if matches!(descendant.progress, Progress::Ended) {
                let waits = show_waits(
                    view,
                    descendant,
                    "    ",
                    decoration,
                    descendant.disposition != Disposition::Rejected,
                )?;
                if !waits.is_empty() {
                    group_details.push(format!(
                        "  {} {}",
                        decoration.paint(OUTPUT_MUTED, "Ended descendant structure:"),
                        decoration.paint(OUTPUT_ID, &descendant.id)
                    ));
                    group_details.extend(waits);
                }
            }
        }
        blocks.push(render_section(
            decoration,
            "Subtree",
            render_subtree(view, &subtree, decoration)?,
        ));
        dependencies = render_subtree_dependencies(view, entity, &subtree, decoration);
    }

    blocks.push(details);
    if !group_details.is_empty() {
        blocks.push(render_section(decoration, "Group counts", group_details));
    }

    if !dependencies.is_empty() {
        blocks.push(render_section(decoration, "Dependencies", dependencies));
    }

    let mut relations = Vec::<(&str, String, Style)>::new();
    let direct_group_dependencies = if entity.kind == EntityKind::Group {
        view.direct_dependencies(&entity.id)
            .into_iter()
            .map(|target| target.id.clone())
            .collect::<std::collections::HashSet<_>>()
    } else {
        std::collections::HashSet::new()
    };
    for target in view.dependency_targets(&entity.id) {
        if direct_group_dependencies.contains(&target.id) {
            continue;
        }
        let (label, style) = if target.disposition == Disposition::Rejected {
            ("Orphaned:", OUTPUT_FAILURE)
        } else if target.is_terminal() {
            ("Satisfied dependency:", OUTPUT_MUTED)
        } else {
            ("Dependency:", OUTPUT_WAITING)
        };
        relations.push((label, render_related_entity(target, decoration), style));
    }
    for dependent in view.direct_dependents(&entity.id) {
        relations.push((
            "Dependent:",
            render_related_entity(dependent, decoration),
            OUTPUT_MUTED,
        ));
    }
    for cause in view.blocking_causes(&entity.id)? {
        if direct_group_dependencies.contains(&cause.id) {
            continue;
        }
        relations.push((
            "Root cause:",
            render_related_entity(cause, decoration),
            OUTPUT_FAILURE,
        ));
    }
    if !relations.is_empty() {
        blocks.push(render_section(
            decoration,
            "Relationships",
            render_fields(decoration, "  ", relations),
        ));
    }

    if let Some(description) = &entity.description {
        blocks.push(render_section(
            decoration,
            "Description",
            vec![description.clone()],
        ));
    }

    if !notes.is_empty() {
        let mut rendered_notes = Vec::new();
        for note in notes {
            if !rendered_notes.is_empty() {
                rendered_notes.push(String::new());
            }
            rendered_notes.push(format!(
                "{}  {}  {}",
                decoration.paint(OUTPUT_HEADING, format!("Note {}", note.id)),
                decoration.paint(OUTPUT_MUTED, display::timestamp(&note.created_at)),
                note.actor
            ));
            rendered_notes.push(note.body.clone());
        }
        blocks.push(render_section(decoration, "Notes", rendered_notes));
    }

    if !progress_events.is_empty() {
        let mut history = Vec::new();
        for event in progress_events {
            let (action, action_style) = match event.kind {
                db::ProgressEventKind::Start => ("Started", OUTPUT_ACTIVE),
                db::ProgressEventKind::Done => ("Ended", OUTPUT_MUTED),
                db::ProgressEventKind::Release => ("Released", Style::new()),
            };
            let reason = event
                .reason
                .as_ref()
                .map(|reason| format!("  ({reason})"))
                .unwrap_or_default();
            history.push(format!(
                "  {}  {}  {}{reason}  [{}]",
                display::timestamp(&event.at),
                event.actor,
                decoration.paint(action_style, action),
                event.id,
            ));
        }
        blocks.push(render_section(decoration, "Progress history", history));
    }

    Ok(format!(
        "{}\n",
        blocks
            .into_iter()
            .filter(|block| !block.is_empty())
            .map(|block| block.join("\n"))
            .collect::<Vec<_>>()
            .join("\n\n")
    ))
}

fn ordered_subtree<'a>(view: &'a View, root: &EntityId) -> Vec<(usize, &'a Entity)> {
    fn append_children<'a>(
        view: &'a View,
        parent: &EntityId,
        depth: usize,
        result: &mut Vec<(usize, &'a Entity)>,
    ) {
        let mut children = view.direct_children(parent);
        children.sort_by(|left, right| left.id.cmp(&right.id));
        for child in children {
            result.push((depth, child));
            append_children(view, &child.id, depth + 1, result);
        }
    }

    let mut result = Vec::new();
    append_children(view, root, 1, &mut result);
    result
}

fn show_situation(
    view: &View,
    entity: &Entity,
    decoration: OutputDecoration,
) -> derived::Result<String> {
    let mut facts = Vec::new();
    if matches!(entity.progress, Progress::Ended) {
        facts.push(decoration.paint(OUTPUT_MUTED, "Ended"));
    }
    if entity.disposition == Disposition::Rejected {
        facts.push(decoration.paint(OUTPUT_MUTED, "Rejected"));
    }
    if facts.is_empty() {
        if matches!(entity.progress, Progress::InProgress(_)) {
            facts.push(decoration.paint(OUTPUT_ACTIVE, "InProgress (saved claim)"));
        } else if view.is_ready(entity)? {
            facts.push(decoration.paint(OUTPUT_POSITIVE, "Ready to start"));
        } else {
            facts.push("Not started".to_string());
        }
        if entity.disposition == Disposition::Undecided {
            facts.push(decoration.paint(OUTPUT_DECISION, "Undecided"));
        }
        if view.is_orphaned(&entity.id) {
            facts.push(decoration.paint(OUTPUT_FAILURE, "Orphaned"));
        } else if view.is_blocked(&entity.id) {
            facts.push(decoration.paint(OUTPUT_WAITING, "Blocked"));
        }
        if !view.within_active_scope(&entity.id)? {
            facts.push(decoration.paint(OUTPUT_MUTED, "outside active scope (see ancestor gates)"));
        }
    }
    Ok(facts.join("; "))
}

fn has_local_descendant_gate(view: &View, entity: &Entity) -> derived::Result<bool> {
    Ok(entity.kind == EntityKind::Group
        && (!matches!(entity.progress, Progress::InProgress(_))
            || entity.disposition != Disposition::Accepted
            || !view.is_surfaced(entity)?
            || view.direct_dependencies(&entity.id).iter().any(|target| {
                target.disposition == Disposition::Rejected || !target.is_terminal()
            })))
}

fn show_waits(
    view: &View,
    entity: &Entity,
    indent: &str,
    decoration: OutputDecoration,
    show_descendant_gate: bool,
) -> derived::Result<Vec<String>> {
    let mut lines = Vec::new();
    if show_descendant_gate && has_local_descendant_gate(view, entity)? {
        lines.push(format!(
            "{indent}{} {} (Progress={}, Disposition={})",
            decoration.paint(OUTPUT_WAITING, "Descendant gate closed:"),
            decoration.paint(OUTPUT_ID, &entity.id),
            decoration.paint(progress_style(&entity.progress), entity.progress.label()),
            decoration.paint(
                disposition_style(entity.disposition),
                entity.disposition.label()
            )
        ));
        if entity.disposition == Disposition::Rejected {
            lines.push(format!(
                "{indent}{}",
                decoration.paint(
                    OUTPUT_MUTED,
                    "Rejected Group leaves descendant saved states unchanged."
                )
            ));
        }
    }
    if !view.is_surfaced(entity)? {
        lines.push(format!(
            "{indent}{}",
            decoration.paint(
                OUTPUT_MUTED,
                format!("Not surfaced: {}", entity.resurface_condition.label())
            )
        ));
    }
    if let ResurfaceCondition::AfterEntity(id) = &entity.resurface_condition {
        lines.push(format!(
            "{indent}{} {}; satisfied by Ended or Rejected ({}).",
            decoration.paint(OUTPUT_MUTED, "AfterEntity:"),
            decoration.paint(OUTPUT_ID, id),
            yes_no(view.is_surfaced(entity)?)
        ));
    }
    let mut targets = view.direct_dependencies(&entity.id);
    targets.sort_by(|a, b| a.id.cmp(&b.id));
    for target in targets {
        let (label, style) = if target.disposition == Disposition::Rejected {
            ("Rejected prerequisite (Orphaned)", OUTPUT_FAILURE)
        } else if !target.is_terminal() {
            ("Unresolved dependency (Blocked)", OUTPUT_WAITING)
        } else {
            continue;
        };
        lines.push(format!(
            "{indent}{} {}",
            decoration.paint(style, format!("{label}:")),
            render_related_entity(target, decoration)
        ));
    }
    Ok(lines)
}

fn render_subtree(
    view: &View,
    subtree: &[(usize, &Entity)],
    decoration: OutputDecoration,
) -> derived::Result<Vec<String>> {
    if subtree.is_empty() {
        return Ok(vec![format!(
            "  {}",
            decoration.paint(OUTPUT_MUTED, "No descendants")
        )]);
    }

    let mut lines = Vec::new();
    for (depth, entity) in subtree {
        let indent = "  ".repeat(*depth);
        let candidate = if view.is_ready(entity)? {
            format!("  {}", decoration.paint(OUTPUT_POSITIVE, "Ready"))
        } else {
            String::new()
        };
        lines.push(format!(
            "{indent}{}  {}  [{}/{}]  {}{candidate}",
            decoration.paint(OUTPUT_ID, &entity.id),
            decoration.paint(OUTPUT_MUTED, entity.kind.label()),
            decoration.paint(progress_style(&entity.progress), entity.progress.label()),
            decoration.paint(
                disposition_style(entity.disposition),
                entity.disposition.label()
            ),
            entity.title,
        ));
        let rejected_group =
            entity.kind == EntityKind::Group && entity.disposition == Disposition::Rejected;
        if rejected_group {
            lines.push(format!(
                "{indent}  {}",
                decoration.paint(
                    OUTPUT_MUTED,
                    "Rejected Group: already terminal; descendant scope is inactive. Unfinished descendants are saved states and do not require follow-up by themselves."
                )
            ));
        }
        if !matches!(entity.progress, Progress::Ended) || rejected_group {
            lines.extend(show_waits(
                view,
                entity,
                &format!("{indent}  "),
                decoration,
                !rejected_group,
            )?);
        }
    }
    Ok(lines)
}

fn render_subtree_dependencies(
    view: &View,
    group: &Entity,
    subtree: &[(usize, &Entity)],
    decoration: OutputDecoration,
) -> Vec<String> {
    let scope = std::iter::once(group)
        .chain(subtree.iter().map(|(_, entity)| *entity))
        .map(|entity| entity.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut owners = std::iter::once(group)
        .chain(subtree.iter().map(|(_, entity)| *entity))
        .filter_map(|owner| {
            let mut targets = view.direct_dependencies(&owner.id);
            targets.sort_by(|left, right| left.id.cmp(&right.id));
            (!targets.is_empty()).then_some((owner, targets))
        })
        .collect::<Vec<_>>();
    owners.sort_by(|(left, _), (right, _)| left.id.cmp(&right.id));

    let mut lines = Vec::new();
    for (owner, targets) in owners {
        lines.push(format!("  {}", decoration.paint(OUTPUT_ID, &owner.id)));
        for target in targets {
            let (state, style) = if target.disposition == Disposition::Rejected {
                ("Rejected", OUTPUT_FAILURE)
            } else if target.is_terminal() {
                ("Satisfied", OUTPUT_MUTED)
            } else {
                ("Unresolved", OUTPUT_WAITING)
            };
            let external = if scope.contains(&target.id) {
                String::new()
            } else {
                format!(
                    "  {}",
                    render_inline_fields(decoration, vec![("External", target.title.clone())])
                )
            };
            lines.push(format!(
                "    {}  {}{external}",
                render_inline_fields(
                    decoration,
                    vec![("Needs", decoration.paint(OUTPUT_ID, &target.id))],
                ),
                decoration.paint(style, state),
            ));
        }
    }
    lines
}

fn render_related_entity(entity: &Entity, decoration: OutputDecoration) -> String {
    format!(
        "{}  {}  {}",
        decoration.paint(OUTPUT_ID, &entity.id),
        decoration.paint(OUTPUT_MUTED, entity.kind.label()),
        entity.title
    )
}

fn render_section(decoration: OutputDecoration, heading: &str, lines: Vec<String>) -> Vec<String> {
    std::iter::once(decoration.paint(OUTPUT_HEADING, heading))
        .chain(lines)
        .collect()
}

fn render_inline_fields(decoration: OutputDecoration, fields: Vec<(&str, String)>) -> String {
    fields
        .into_iter()
        .map(|(label, value)| {
            format!(
                "{} {value}",
                decoration.paint(OUTPUT_MUTED, format!("{label}:"))
            )
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn render_fields(
    decoration: OutputDecoration,
    indent: &str,
    fields: Vec<(&str, String, Style)>,
) -> Vec<String> {
    let width = fields
        .iter()
        .map(|(label, _, _)| label.chars().count())
        .max()
        .unwrap_or_default();
    fields
        .into_iter()
        .map(|(label, value, style)| {
            let label = decoration.paint(style, format!("{label:<width$}"));
            format!("{indent}{label}  {value}")
        })
        .collect()
}

fn progress_style(progress: &Progress) -> Style {
    match progress {
        Progress::NotStarted => Style::new(),
        Progress::InProgress(_) => OUTPUT_ACTIVE,
        Progress::Ended => OUTPUT_MUTED,
    }
}

fn disposition_style(disposition: Disposition) -> Style {
    match disposition {
        Disposition::Undecided => OUTPUT_DECISION,
        Disposition::Accepted => OUTPUT_POSITIVE,
        Disposition::Rejected => OUTPUT_MUTED,
    }
}

fn cmd_note(command: NoteCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = open_store(false)?;
    let decoration = current_output_decoration();
    match command {
        NoteCmd::Add { id, message, file } => {
            let id = store.resolve_id(&id)?;
            let body = read_description(message, file)?
                .ok_or("a Note body is required; provide -m or -F")?;
            let note = store.add_note(&id, &body, &actor::actor())?;
            write_confirmation(&id, format!("Note {} recorded", note.id), OUTPUT_POSITIVE)?;
        }
        NoteCmd::List { id } => {
            let id = store.resolve_id(&id)?;
            let notes = store.notes(&id)?;
            let rows = notes
                .into_iter()
                .map(|note| {
                    let first_line = note.body.lines().next().unwrap_or_default();
                    format!(
                        "{}  {}  {}  {}\n",
                        decoration.paint(OUTPUT_INDEX, note.id),
                        decoration.paint(OUTPUT_MUTED, display::timestamp(&note.created_at)),
                        note.actor,
                        first_line
                    )
                })
                .collect::<String>();
            write_rows(&rows, "No notes", decoration)?;
        }
        NoteCmd::Show { id, number } => {
            let id = store.resolve_id(&id)?;
            let note = store.note(&id, &number)?;
            let mut output = format!(
                "{}  {}\n{}\n\n{}\n{}",
                decoration.paint(OUTPUT_ID, &id),
                decoration.paint(OUTPUT_HEADING, format!("Note {}", note.id)),
                render_inline_fields(
                    decoration,
                    vec![
                        ("Recorded", display::timestamp(&note.created_at)),
                        ("Actor", note.actor),
                    ],
                ),
                decoration.paint(OUTPUT_HEADING, "Body"),
                note.body,
            );
            if !note.body.ends_with('\n') {
                output.push('\n');
            }
            write_output(&output, decoration)?;
        }
    }
    Ok(())
}

fn cmd_revision(command: RevisionCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = open_store(false)?;
    let decoration = current_output_decoration();
    match command {
        RevisionCmd::List { id } => {
            let id = store.resolve_id(&id)?;
            let snapshot = store.revision_snapshot(&id)?;
            let rows = snapshot
                .revisions
                .into_iter()
                .map(|revision| {
                    let marks = render_revision_marks(
                        snapshot.entity.current_revision == Some(revision.id),
                        revision.baseline,
                        " ",
                        decoration,
                    );
                    format!(
                        "{}  {}{}  {}\n",
                        decoration.paint(OUTPUT_INDEX, revision.id),
                        decoration.paint(OUTPUT_MUTED, display::timestamp(&revision.created_at)),
                        marks,
                        revision.title
                    )
                })
                .collect::<String>();
            write_rows(&rows, "No declaration revisions", decoration)?;
        }
        RevisionCmd::Show { id, number } => {
            let id = store.resolve_id(&id)?;
            let snapshot = store.revision_snapshot(&id)?;
            let revision = snapshot.revision(&number)?;
            write_output(
                &render_revision(&id, &snapshot.entity, revision, decoration),
                decoration,
            )?;
        }
        RevisionCmd::Diff { id, from, to } => {
            let id = store.resolve_id(&id)?;
            let snapshot = store.revision_snapshot(&id)?;
            let from_revision = snapshot.revision(&from)?;
            let to_revision = snapshot.revision(&to)?;
            write_output(
                &render_revision_diff(&id, from_revision, to_revision, decoration),
                decoration,
            )?;
        }
    }
    Ok(())
}

fn render_revision(
    id: &EntityId,
    entity: &Entity,
    revision: &DeclarationRevision,
    decoration: OutputDecoration,
) -> String {
    let marks = render_revision_marks(
        entity.current_revision == Some(revision.id),
        revision.baseline,
        "  ",
        decoration,
    );
    let description = match revision.description.as_deref() {
        Some(description) => format!("present\n{description}"),
        None => "absent".to_string(),
    };
    let parent = revision
        .parent
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| "(none)".to_string());
    let dependencies = if revision.dependencies.is_empty() {
        "  (none)".to_string()
    } else {
        revision
            .dependencies
            .iter()
            .map(|dependency| format!("  {}", decoration.paint(OUTPUT_ID, dependency)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let metadata = [
        render_inline_fields(
            decoration,
            vec![("Created", display::timestamp(&revision.created_at))],
        ),
        render_inline_fields(decoration, vec![("Title", revision.title.clone())]),
        render_inline_fields(decoration, vec![("Parent", parent)]),
    ]
    .join("\n");
    format!(
        "{}  {}{marks}\n{}\n\n{}\n{description}\n\n{}\n{dependencies}\n",
        decoration.paint(OUTPUT_ID, id),
        decoration.paint(
            OUTPUT_HEADING,
            format!("Declaration Revision {}", revision.id)
        ),
        metadata,
        decoration.paint(OUTPUT_HEADING, "Description"),
        decoration.paint(OUTPUT_HEADING, "Outgoing dependencies"),
    )
}

fn render_revision_marks(
    current: bool,
    baseline: bool,
    leading: &str,
    decoration: OutputDecoration,
) -> String {
    let mut marks = Vec::new();
    if current {
        marks.push(decoration.paint(OUTPUT_INDEX, "current"));
    }
    if baseline {
        marks.push(decoration.paint(OUTPUT_MUTED, "baseline"));
    }
    if marks.is_empty() {
        String::new()
    } else {
        format!("{leading}[{}]", marks.join(", "))
    }
}

fn render_revision_diff(
    id: &EntityId,
    from: &DeclarationRevision,
    to: &DeclarationRevision,
    decoration: OutputDecoration,
) -> String {
    let mut output = format!(
        "{}  {}\n\n",
        decoration.paint(OUTPUT_ID, id),
        decoration.paint(
            OUTPUT_HEADING,
            format!("Declaration Revision {} -> {}", from.id, to.id)
        )
    );
    push_value_diff(&mut output, "Title", &from.title, &to.title, decoration);
    push_optional_value_diff(
        &mut output,
        "Description",
        from.description.as_deref(),
        to.description.as_deref(),
        decoration,
    );
    push_value_diff(
        &mut output,
        "Parent",
        &from
            .parent
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "(none)".to_string()),
        &to.parent
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "(none)".to_string()),
        decoration,
    );

    let mut removed = from
        .dependencies
        .iter()
        .filter(|dependency| !to.dependencies.contains(dependency))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let mut added = to
        .dependencies
        .iter()
        .filter(|dependency| !from.dependencies.contains(dependency))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    removed.sort();
    added.sort();
    output.push_str(&format!(
        "{}\n",
        decoration.paint(OUTPUT_HEADING, "Outgoing dependencies")
    ));
    if removed.is_empty() && added.is_empty() {
        output.push_str(&format!(
            "  {}\n",
            decoration.paint(OUTPUT_MUTED, "unchanged")
        ));
    } else {
        for dependency in removed {
            output.push_str(&format!(
                "{} {dependency}\n",
                decoration.paint(OUTPUT_FAILURE, "-")
            ));
        }
        for dependency in added {
            output.push_str(&format!(
                "{} {dependency}\n",
                decoration.paint(OUTPUT_POSITIVE, "+")
            ));
        }
    }
    output
}

fn push_value_diff(
    output: &mut String,
    label: &str,
    from: &str,
    to: &str,
    decoration: OutputDecoration,
) {
    output.push_str(&format!("{}\n", decoration.paint(OUTPUT_HEADING, label)));
    if from == to {
        output.push_str(&format!(
            "  {}\n",
            decoration.paint(OUTPUT_MUTED, "unchanged")
        ));
    } else {
        for line in from.lines() {
            output.push_str(&format!(
                "{} {line}\n",
                decoration.paint(OUTPUT_FAILURE, "-")
            ));
        }
        for line in to.lines() {
            output.push_str(&format!(
                "{} {line}\n",
                decoration.paint(OUTPUT_POSITIVE, "+")
            ));
        }
    }
}

fn push_optional_value_diff(
    output: &mut String,
    label: &str,
    from: Option<&str>,
    to: Option<&str>,
    decoration: OutputDecoration,
) {
    output.push_str(&format!("{}\n", decoration.paint(OUTPUT_HEADING, label)));
    if from == to {
        output.push_str(&format!(
            "  {}\n",
            decoration.paint(OUTPUT_MUTED, "unchanged")
        ));
        return;
    }
    push_optional_diff_side(output, '-', from, decoration);
    push_optional_diff_side(output, '+', to, decoration);
}

fn push_optional_diff_side(
    output: &mut String,
    prefix: char,
    value: Option<&str>,
    decoration: OutputDecoration,
) {
    let style = if prefix == '-' {
        OUTPUT_FAILURE
    } else {
        OUTPUT_POSITIVE
    };
    let prefix = decoration.paint(style, prefix);
    match value {
        None => output.push_str(&format!("{prefix} absent\n")),
        Some(value) => {
            output.push_str(&format!("{prefix} present\n"));
            for line in value.split('\n') {
                output.push_str(&format!("{prefix} {line}\n"));
            }
        }
    }
}

fn render_entity_counts(
    label: &str,
    counts: &derived::EntityCounts,
    decoration: OutputDecoration,
) -> Vec<String> {
    vec![
        format!(
            "  {}: {}  {}",
            decoration.paint(OUTPUT_HEADING, label),
            counts.total,
            render_inline_fields(
                decoration,
                vec![
                    ("Issue", counts.issues.to_string()),
                    ("Group", counts.groups.to_string()),
                    ("Terminal", counts.terminal.to_string()),
                ],
            )
        ),
        format!(
            "    {}",
            render_inline_fields(
                decoration,
                vec![
                    ("Progress", format!("NotStarted: {}", counts.not_started)),
                    (
                        "InProgress",
                        decoration.paint(OUTPUT_ACTIVE, counts.in_progress),
                    ),
                    ("Ended", decoration.paint(OUTPUT_MUTED, counts.ended)),
                ],
            )
        ),
        format!(
            "    {}",
            render_inline_fields(
                decoration,
                vec![
                    (
                        "Disposition",
                        format!(
                            "Undecided: {}",
                            decoration.paint(OUTPUT_DECISION, counts.undecided)
                        ),
                    ),
                    (
                        "Accepted",
                        decoration.paint(OUTPUT_POSITIVE, counts.accepted),
                    ),
                    ("Rejected", decoration.paint(OUTPUT_MUTED, counts.rejected)),
                ],
            )
        ),
    ]
}

fn inactive_scope_reason(view: &View, id: &EntityId) -> derived::Result<Option<String>> {
    for group in view.ancestors(id) {
        if view.opens_descendants(group)? {
            continue;
        }
        let mut reasons = Vec::new();
        if !matches!(group.progress, Progress::InProgress(_)) {
            reasons.push(format!("Progress={}", group.progress.label()));
        }
        if group.disposition != Disposition::Accepted {
            reasons.push(format!("Disposition={}", group.disposition.label()));
        }
        if !view.is_surfaced(group)? {
            reasons.push(format!(
                "not surfaced: {}",
                group.resurface_condition.label()
            ));
        }
        if view.is_orphaned(&group.id) {
            reasons.push("orphaned".to_string());
        } else if view.is_blocked(&group.id) {
            reasons.push("blocked".to_string());
        }
        return Ok(Some(format!("{} ({})", group.id, reasons.join(", "))));
    }
    Ok(None)
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn cmd_decide(command: DecideCmd) -> Result<(), Box<dyn std::error::Error>> {
    let (args, disposition) = match command {
        DecideCmd::Accept(args) => (args, Disposition::Accepted),
        DecideCmd::Reject(args) => (args, Disposition::Rejected),
        DecideCmd::Undecide(args) => (args, Disposition::Undecided),
    };
    let mut store = open_store(false)?;
    let id = store.resolve_id(&args.id)?;
    store.apply(&id, Change::Decide(disposition), &ctx(args.reason))?;
    let decoration = current_output_decoration();
    write_confirmation(
        &id,
        render_inline_fields(
            decoration,
            vec![(
                "Disposition",
                decoration.paint(disposition_style(disposition), disposition.label()),
            )],
        ),
        Style::new(),
    )?;
    Ok(())
}

fn cmd_when(command: WhenCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = open_store(false)?;
    match command {
        WhenCmd::Command {
            id,
            command,
            reason,
        } => {
            let id = store.resolve_id(&id)?;
            let condition = ResurfaceCondition::Command(command);
            store.apply(
                &id,
                Change::SetResurfaceCondition(condition.clone()),
                &ctx(reason),
            )?;
            write_confirmation(
                &id,
                format!("Resurface condition: {}", condition.label()),
                Style::new(),
            )?;
        }
        WhenCmd::At { id, date, reason } => {
            let id = store.resolve_id(&id)?;
            let date: NaiveDate = date
                .parse()
                .map_err(|_| format!("invalid date: {date} (expected YYYY-MM-DD)"))?;
            store.apply(
                &id,
                Change::SetResurfaceCondition(ResurfaceCondition::AtDate(date)),
                &ctx(reason),
            )?;
            write_confirmation(
                &id,
                format!("Resurface condition: AtDate({date})"),
                Style::new(),
            )?;
        }
        WhenCmd::After {
            id,
            reference,
            reason,
        } => {
            let id = store.resolve_id(&id)?;
            let reference = store.resolve_id(&reference)?;
            store.apply(
                &id,
                Change::SetResurfaceCondition(ResurfaceCondition::AfterEntity(reference.clone())),
                &ctx(reason),
            )?;
            write_confirmation(
                &id,
                format!("Resurface condition: AfterEntity({reference})"),
                Style::new(),
            )?;
        }
        WhenCmd::Manual { id, reason } => {
            let id = store.resolve_id(&id)?;
            store.apply(
                &id,
                Change::SetResurfaceCondition(ResurfaceCondition::Manual),
                &ctx(reason),
            )?;
            write_confirmation(&id, "Resurface condition: Manual".to_string(), Style::new())?;
        }
        WhenCmd::Clear { id, reason } => {
            let id = store.resolve_id(&id)?;
            store.apply(
                &id,
                Change::SetResurfaceCondition(ResurfaceCondition::Always),
                &ctx(reason),
            )?;
            write_confirmation(&id, "Resurface condition: Always".to_string(), Style::new())?;
        }
    }
    Ok(())
}

fn cmd_dep(command: DepCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = open_store(false)?;
    match command {
        DepCmd::Add { id, needs } => {
            let id = store.resolve_id(&id)?;
            let needs = store.resolve_id(&needs)?;
            store.add_dep(&id, &needs)?;
            let decoration = current_output_decoration();
            write_confirmation(
                &id,
                render_inline_fields(
                    decoration,
                    vec![("Dependency added", decoration.paint(OUTPUT_ID, &needs))],
                ),
                Style::new(),
            )?;
        }
        DepCmd::Rm { id, needs } => {
            let id = store.resolve_id(&id)?;
            let needs = store.resolve_id(&needs)?;
            store.remove_dep(&id, &needs)?;
            let decoration = current_output_decoration();
            write_confirmation(
                &id,
                render_inline_fields(
                    decoration,
                    vec![("Dependency removed", decoration.paint(OUTPUT_ID, &needs))],
                ),
                Style::new(),
            )?;
        }
    }
    Ok(())
}

fn cmd_log(raw: &str) -> Result<(), Box<dyn std::error::Error>> {
    let store = open_store(false)?;
    let id = store.resolve_id(raw)?;
    let events = store.events(&id)?;
    let decoration = current_output_decoration();
    if events.is_empty() {
        write_output("No decision history\n", decoration)?;
        return Ok(());
    }
    let mut output = String::new();
    for event in events {
        output.push_str(&format!(
            "{}  {}  {}",
            decoration.paint(OUTPUT_MUTED, display::timestamp(&event.at)),
            event.actor,
            format_decision(&event, decoration)
        ));
        if let Some(reason) = event.reason {
            output.push_str(&format!(
                "  {} {reason}",
                decoration.paint(OUTPUT_MUTED, "Reason:")
            ));
        }
        if let Some(revision) = event.revision {
            output.push_str(&format!(
                "  {} {revision}",
                decoration.paint(OUTPUT_MUTED, "Revision:")
            ));
        }
        output.push_str(&format!("  [{}]\n", event.id));
    }
    write_output(&output, decoration)?;
    Ok(())
}

fn format_decision(event: &db::Event, decoration: OutputDecoration) -> String {
    let label = |value: &Option<String>| match value.as_deref() {
        Some("undecided") => "Undecided".to_string(),
        Some("accepted") => "Accepted".to_string(),
        Some("rejected") => "Rejected".to_string(),
        Some(other) => other.to_string(),
        None => "Always".to_string(),
    };
    let field = match event.field.as_str() {
        "disposition" => "Disposition".to_string(),
        "resurface_condition" => "Resurface condition".to_string(),
        field => field.to_string(),
    };
    let old = label(&event.old_value);
    let new = label(&event.new_value);
    let old_style = if event.field == "disposition" {
        disposition_label_style(&old)
    } else {
        OUTPUT_MUTED
    };
    let new_style = if event.field == "disposition" {
        disposition_label_style(&new)
    } else if new == "Always" {
        Style::new()
    } else {
        OUTPUT_MUTED
    };
    format!(
        "{} {} {} {}",
        decoration.paint(OUTPUT_MUTED, format!("{field}:")),
        decoration.paint(old_style, old),
        decoration.paint(OUTPUT_MUTED, "->"),
        decoration.paint(new_style, new)
    )
}

fn disposition_label_style(value: &str) -> Style {
    match value {
        "Undecided" => OUTPUT_DECISION,
        "Accepted" => OUTPUT_POSITIVE,
        "Rejected" => OUTPUT_MUTED,
        _ => Style::new(),
    }
}

fn cmd_write(
    raw: &str,
    title: Option<String>,
    message: Option<String>,
    file: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let body = read_description(message, file)?;
    if title.is_none() && body.is_none() {
        return Err("nothing to write; provide --title, -m, or -F".into());
    }

    let mut store = open_store(false)?;
    let id = store.resolve_id(raw)?;
    let current = store.get(&id)?;
    let mut changed = Vec::new();
    if let Some(title) = title {
        let title = normalize_title(&title)?;
        if title != current.title {
            store.apply(&id, Change::SetTitle(title), &ctx(None))?;
            changed.push("Title updated");
        }
    }
    if let Some(body) = body {
        let body = normalize_description(body);
        if body != current.description {
            let removed = body.is_none();
            store.apply(&id, Change::SetDescription(body), &ctx(None))?;
            changed.push(if removed {
                "Description removed"
            } else {
                "Description updated"
            });
        }
    }
    if changed.is_empty() {
        write_confirmation(&id, "No changes".to_string(), OUTPUT_MUTED)?;
    } else {
        write_confirmation(&id, changed.join("  "), OUTPUT_POSITIVE)?;
    }
    Ok(())
}

fn read_description(
    message: Option<String>,
    file: Option<String>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    use std::io::Read;

    match (message, file) {
        (Some(message), None) => Ok(Some(message)),
        (None, Some(file)) if file == "-" => {
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer)?;
            Ok(Some(buffer))
        }
        (None, Some(file)) => Ok(Some(std::fs::read_to_string(file)?)),
        (Some(_), Some(_)) => Err("-m and -F cannot be used together".into()),
        (None, None) => Ok(None),
    }
}

fn normalize_description(description: String) -> Option<String> {
    (!description.trim().is_empty()).then(|| description.trim().to_string())
}

fn normalize_title(title: &str) -> Result<String, Box<dyn std::error::Error>> {
    let title = title.trim();
    if title.is_empty() || title.contains(['\n', '\r']) {
        return Err("title must be a non-empty single-line string".into());
    }
    Ok(title.to_string())
}

fn ctx(reason: Option<String>) -> Ctx {
    Ctx {
        actor: actor::actor(),
        reason,
    }
}

fn write_rows(rows: &str, empty_note: &str, decoration: OutputDecoration) -> std::io::Result<()> {
    if rows.is_empty() {
        eprintln!(
            "{}",
            current_error_decoration().paint(OUTPUT_MUTED, empty_note)
        );
        Ok(())
    } else {
        write_output(rows, decoration)
    }
}

fn cli_command() -> clap::Command {
    Cli::command()
        .styles(cli_styles())
        .override_help(render_root_help(OutputDecoration::Ansi))
}

fn cli_styles() -> clap::builder::Styles {
    clap::builder::Styles::styled()
        .header(OUTPUT_HEADING)
        .error(OUTPUT_FAILURE)
        .usage(OUTPUT_HEADING)
        .literal(OUTPUT_INDEX)
        .placeholder(Style::new())
        .valid(OUTPUT_POSITIVE)
        .invalid(OUTPUT_DECISION)
        .context(OUTPUT_MUTED)
        .context_value(Style::new())
}

fn render_root_help(decoration: OutputDecoration) -> String {
    use std::fmt::Write;

    let mut command = Cli::command().styles(cli_styles()).term_width(0);
    command.build();
    let standard = command.render_help().to_string();
    let (preamble, remainder) = standard
        .split_once("\n\nCommands:\n")
        .expect("root help must contain a command section");
    let (_, options) = remainder
        .rsplit_once("\n\nOptions:\n")
        .expect("root help must contain an option section");
    let command_width = HELP_SECTIONS
        .iter()
        .flat_map(|section| section.commands)
        .map(|name| name.len())
        .max()
        .unwrap_or_default();

    let styles = command.get_styles();
    let usage_heading = decoration.paint(*styles.get_usage(), "Usage:");
    let mut output = preamble.replacen("Usage:", &usage_heading, 1);
    for section in HELP_SECTIONS {
        let heading = decoration.paint(*styles.get_header(), section.heading);
        write!(output, "\n\n{heading}:\n").unwrap();
        for name in section.commands {
            let child = command
                .get_subcommands()
                .find(|child| child.get_name() == *name)
                .expect("help section must reference an existing command");
            let about = child
                .get_about()
                .or_else(|| child.get_long_about())
                .unwrap_or_default();
            let styled_name = decoration.paint(*styles.get_literal(), name);
            let padding = command_width - name.len();
            writeln!(output, "  {styled_name}{:padding$}  {about}", "").unwrap();
        }
        output.pop();
    }
    let more_help = decoration.paint(*styles.get_header(), "More help");
    let help_path = decoration.paint(*styles.get_literal(), "axon help <COMMAND PATH>");
    let docs = decoration.paint(*styles.get_literal(), "axon docs");
    write!(
        output,
        "\n\n{more_help}:\n  {help_path}  Show detailed help for a command\n  {docs}                 Explain Axon's state model and basic workflow",
    )
    .unwrap();
    let options_heading = decoration.paint(*styles.get_header(), "Options");
    write!(output, "\n\n{options_heading}:\n").unwrap();
    output.push_str(options);
    output
}

fn cmd_docs(topic: Option<DocsCmd>) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(DocsCmd::Declaration { example }) = topic {
        let output = if example {
            include_str!("docs/declaration-example.yaml")
        } else {
            include_str!("docs/declaration.txt")
        };
        write_plain_output(output)?;
        return Ok(());
    }
    let decoration = current_output_decoration();
    write_output(&render_docs(decoration), decoration)?;
    Ok(())
}

fn render_docs(decoration: OutputDecoration) -> String {
    use std::fmt::Write;

    let mut output = String::new();
    writeln!(
        output,
        "{}\n",
        decoration.paint(OUTPUT_HEADING, "Axon concepts")
    )
    .unwrap();
    writeln!(
        output,
        "An Entity is either an Issue or a Group. Both kinds share the same state axes,\nrelationships, generated IDs, and commands.\n"
    )
    .unwrap();

    writeln!(output, "{}", decoration.paint(OUTPUT_HEADING, "State axes")).unwrap();
    writeln!(
        output,
        "  Progress             NotStarted, InProgress, or Ended. Ended means no more work."
    )
    .unwrap();
    writeln!(
        output,
        "  Disposition          Undecided, Accepted, or Rejected. This records whether to pursue it."
    )
    .unwrap();
    writeln!(
        output,
        "  Resurface condition  Always, AtDate, AfterEntity, Manual, or Command. This controls when it returns to attention.\n"
    )
    .unwrap();

    writeln!(
        output,
        "{}",
        decoration.paint(OUTPUT_HEADING, "Relationships and derived state")
    )
    .unwrap();
    writeln!(
        output,
        "  A dependency requires another Entity's result. An Ended non-Rejected Entity satisfies\n  it; a Rejected dependency makes its dependent orphaned. AfterEntity instead finishes\n  waiting when its reference is Ended or Rejected.\n\n  Containment places an Issue or Group under one parent Group. ready, blocked, orphaned,\n  surfaced, terminal, and active scope are derived when data is read; they are not stored.\n  An Entity is terminal when it is Ended or Rejected.\n"
    )
    .unwrap();

    writeln!(output, "{}", decoration.paint(OUTPUT_HEADING, "Groups")).unwrap();
    writeln!(
        output,
        "  An InProgress Group opens its descendants only while it is Accepted, surfaced, and\n  neither blocked nor orphaned. Starting a Group does not start its descendants. A Group\n  can end only after every descendant is terminal, and can be released only when no\n  descendant is InProgress. A Rejected Group is already terminal; unfinished descendants\n  remain inactive saved states and do not require follow-up by themselves.\n"
    )
    .unwrap();

    writeln!(
        output,
        "{}",
        decoration.paint(OUTPUT_HEADING, "Plan declarations and notes")
    )
    .unwrap();
    writeln!(
        output,
        "  Title, description, parent, and outgoing dependencies form the plan declaration.\n  Accepted and Rejected declarations are fixed; return one to Undecided before editing it.\n  Notes append durable information without changing the declaration or state.\n"
    )
    .unwrap();

    writeln!(
        output,
        "{}",
        decoration.paint(OUTPUT_HEADING, "Basic workflow")
    )
    .unwrap();
    for (command, description) in [
        ("axon plan / axon capture", "Create an Issue"),
        (
            "axon ready / axon triage",
            "Find the active work or decision frontier",
        ),
        ("axon start", "Claim one ready Entity"),
        (
            "axon done / axon release",
            "End the work or release its claim",
        ),
        (
            "axon status",
            "Compare plans, candidates, saved claims, and waits",
        ),
        ("axon show", "Inspect one Entity and its current context"),
    ] {
        let styled_command = decoration.paint(OUTPUT_HEADING, command);
        let padding = 28 - command.len();
        writeln!(output, "  {styled_command}{:padding$}  {description}", "").unwrap();
    }
    writeln!(
        output,
        "\nUse {} for command syntax and options.",
        decoration.paint(OUTPUT_HEADING, "axon help <COMMAND PATH>")
    )
    .unwrap();
    writeln!(output, "\nUse axon docs declaration for declaration fields and the import workflow.\nUse axon docs declaration --example for a complete new-plan YAML example.").unwrap();
    writeln!(output, "\nStorage recovery\n  DB commands require schema v13 and do not migrate old databases implicitly.\n  Use axon migrate --source <v11-or-v12-db> --output <new-directory> to create a new DB,\n  an intact source backup and an ID mapping manifest. The source is not switched.\n  v9/v10 must first be migrated to v11 using an older compatible build.\n  Keep an old binary, stop writers, verify copies, then switch all affected roots.\n  Failed outputs may be incomplete; preserve and inspect them before retry.\n  Unknown schemas are rejected. init only creates new DBs; init cannot upgrade.\n  --version identifies the executable release, not its DB schema.\n  Before recovery stop all writers and preserve .axon with SQLite journal/WAL files.\n  Do not delete the DB or edit user_version to bypass compatibility checks.\n  export also requires a compatible build and is not a complete database backup.\n  .axon/config.json selects backend and store ID at the active worktree root.\n  Outside Git, use the nearest ancestor config. Missing/invalid files never fall back.\n  init --backend file creates .axon/state.jsonl; the default backend is sqlite.\n  SQLite uses Git common directory/axon/state.db, or .axon/state.db outside Git.\n  Track file state and config in Git; ignore .axon/write.lock and temporary files.\n  File writers lock, read, validate, sync a temporary, compare original bytes, replace,\n  and sync the directory before success. No-op preserves bytes.\n  Failure before replace is not applied; failure after replace is result unknown.\n  Inspect state before retrying an unknown result; do not repeat an append blindly.\n  init publishes state before config. Preserve partial files and restore a matching\n  config/state pair; init never regenerates missing state or overwrites existing data.\n  Valid shared SQLite can be registered with init in another worktree.\n  Do not overlap Git checkout/merge or editor writes with Axon writes in one worktree.\n  OS locks are local; network filesystem/distributed guarantees are not provided.\n  help, docs, version and completion do not open the DB.\n").unwrap();
    output
}

fn write_completion(shell: Shell) -> std::io::Result<()> {
    use std::io::Write;
    let mut command = Cli::command();
    let mut completion = Vec::new();
    generate(shell, &mut command, "axon", &mut completion);
    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(&completion) {
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_decoration_pair(plain: String, styled: String) {
        assert!(!plain.contains('\u{1b}'));
        assert!(styled.contains("\u{1b}["));
        assert_eq!(anstream::adapter::strip_str(&styled).to_string(), plain);
    }

    fn entity(id: &str, kind: EntityKind) -> Entity {
        let now = Utc::now();
        Entity {
            id: EntityId::from_stored(id),
            kind,
            title: format!("{id} title"),
            description: None,
            progress: Progress::NotStarted,
            disposition: Disposition::Accepted,
            current_revision: Some(RecordId::new(RecordKind::Revision)),
            resurface_condition: ResurfaceCondition::Always,
            parent: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn status_styles_preserve_text_and_stored_strings() {
        let mut group = entity("t-g", EntityKind::Group);
        group.title = " 計画\n  continued ".into();
        let mut issue = entity("t-i", EntityKind::Issue);
        issue.parent = Some(group.id.clone());
        issue.progress = Progress::InProgress(Claim {
            actor: " actor ".into(),
            worktree: " /tmp/my worktree ".into(),
            at: Utc::now(),
        });
        let view = View::new(vec![group, issue], vec![]);
        let plain = status::render(&view, None, OutputDecoration::Plain).unwrap();
        let ansi = status::render(&view, None, OutputDecoration::Ansi).unwrap();
        assert_eq!(plain, anstream::adapter::strip_str(&ansi).to_string());
        assert!(plain.contains(" 計画\n  continued "));
        assert!(plain.contains("Claim:  actor   Worktree:  /tmp/my worktree "));
    }

    #[test]
    fn status_styles_actions_waits_failures_and_inactive_state_by_meaning() {
        let group = entity("t-group", EntityKind::Group);
        let ready = entity("t-ready", EntityKind::Issue);

        let mut undecided = entity("t-undecided", EntityKind::Issue);
        undecided.disposition = Disposition::Undecided;
        undecided.current_revision = None;

        let blocked = entity("t-blocked", EntityKind::Issue);
        let prerequisite = entity("t-prerequisite", EntityKind::Issue);

        let orphaned = entity("t-orphaned", EntityKind::Issue);
        let mut rejected = entity("t-rejected", EntityKind::Issue);
        rejected.disposition = Disposition::Rejected;

        let mut hidden = entity("t-hidden", EntityKind::Issue);
        hidden.resurface_condition = ResurfaceCondition::Manual;

        let view = View::new(
            vec![
                group,
                ready,
                undecided,
                blocked.clone(),
                prerequisite.clone(),
                orphaned.clone(),
                rejected.clone(),
                hidden,
            ],
            vec![(blocked.id, prerequisite.id), (orphaned.id, rejected.id)],
        );
        let ansi = status::render(&view, None, OutputDecoration::Ansi).unwrap();

        for expected in [
            OutputDecoration::Ansi.paint(OUTPUT_ID, "t-ready"),
            OutputDecoration::Ansi.paint(OUTPUT_POSITIVE, "Ready candidate"),
            OutputDecoration::Ansi.paint(OUTPUT_WAITING, "Descendant gate closed:"),
            OutputDecoration::Ansi.paint(OUTPUT_WAITING, "Unresolved dependency:"),
            OutputDecoration::Ansi.paint(OUTPUT_DECISION, "Undecided"),
            OutputDecoration::Ansi.paint(OUTPUT_FAILURE, "Orphaned"),
            OutputDecoration::Ansi.paint(OUTPUT_MUTED, "Resurface condition not satisfied: Manual"),
        ] {
            assert!(
                ansi.contains(&expected),
                "missing semantic style: {expected:?}"
            );
        }
    }

    #[test]
    fn migration_failure_guidance_preserves_application_uncertainty() {
        use db::migration::{ApplicationState, Failure};
        for (applied, expected) in [
            (ApplicationState::NotApplied, "Migration was not applied"),
            (ApplicationState::Applied, "Migration was committed"),
            (ApplicationState::Unknown, "before any retry"),
        ] {
            let failure = Failure {
                database: "/fixture/.axon/axon.db".into(),
                stage: "committing migration",
                from: Some(9),
                to: 11,
                backup: Some("/fixture/.axon/migration-backups/backup.db".into()),
                applied,
                source: Box::new(db::DbError::Io(std::io::Error::other("injected I/O error"))),
            };
            let guidance = error_guidance(&db::DbError::Migration(failure)).unwrap();
            assert!(guidance.contains(expected));
            assert!(guidance.contains("source"));
            if applied != ApplicationState::NotApplied {
                assert!(!guidance.contains("Migration was not applied"));
            }
        }
    }

    #[test]
    fn show_identifies_kind_and_group_summary() {
        let group = entity("g", EntityKind::Group);
        let mut issue = entity("i", EntityKind::Issue);
        issue.parent = Some(group.id.clone());
        let view = View::new(vec![group.clone(), issue], vec![]);
        let output = render_show(
            &view,
            &group,
            &[],
            &[],
            RecordCounts::default(),
            OutputDecoration::Plain,
        )
        .unwrap();
        assert!(output.starts_with("g  Group  g title\n"));
        assert!(output.contains("  Can complete: no\n"));
        assert!(output.contains("  Descendants: 1  Issue: 1  Group: 0  Terminal: 0"));
        assert!(output.contains("    Progress: NotStarted: 1  InProgress: 0  Ended: 0"));
        assert!(output.contains("    Disposition: Undecided: 0  Accepted: 1  Rejected: 0"));
    }

    #[test]
    fn show_can_render_ansi_styles_without_changing_its_text() {
        let issue = entity("i", EntityKind::Issue);
        let view = View::new(vec![issue.clone()], vec![]);
        let plain = render_show(
            &view,
            &issue,
            &[],
            &[],
            RecordCounts::default(),
            OutputDecoration::Plain,
        )
        .unwrap();
        let styled = render_show(
            &view,
            &issue,
            &[],
            &[],
            RecordCounts::default(),
            OutputDecoration::Ansi,
        )
        .unwrap();

        assert!(!plain.contains('\u{1b}'));
        assert!(plain.find("Situation:").unwrap() < plain.find("Details\n").unwrap());
        assert!(!plain.contains("Status\n"));
        assert!(!plain.contains("Plan\n"));
        assert!(styled.contains("\u{1b}["));
        assert!(styled.contains("\u{1b}[32mAccepted\u{1b}[0m"));
        assert!(styled.contains("\u{1b}[0m  i title\n"));
        assert_eq!(anstream::adapter::strip_str(&styled).to_string(), plain);
    }

    #[test]
    fn output_only_decorates_attended_terminals() {
        assert!(matches!(
            output_decoration(true, false),
            OutputDecoration::Ansi
        ));
        assert!(matches!(
            output_decoration(false, false),
            OutputDecoration::Plain
        ));
        assert!(matches!(
            output_decoration(true, true),
            OutputDecoration::Plain
        ));
    }

    #[test]
    fn show_reports_surfaced_state_in_plain_text() {
        let mut issue = entity("i", EntityKind::Issue);
        issue.resurface_condition =
            ResurfaceCondition::AfterEntity(EntityId::from_stored("target"));
        let target = entity("target", EntityKind::Issue);
        let view = View::new(vec![issue.clone(), target.clone()], vec![]);
        let waiting = render_show(
            &view,
            &issue,
            &[],
            &[],
            RecordCounts::default(),
            OutputDecoration::Plain,
        )
        .unwrap();

        assert!(waiting.contains("Resurface condition: AfterEntity(target)"));
        assert!(waiting.contains("Surfaced: no"));

        let mut ended_target = target;
        ended_target.progress = Progress::Ended;
        let view = View::new(vec![issue.clone(), ended_target], vec![]);
        let surfaced = render_show(
            &view,
            &issue,
            &[],
            &[],
            RecordCounts::default(),
            OutputDecoration::Plain,
        )
        .unwrap();

        assert!(surfaced.contains("Surfaced: yes"));
    }

    #[test]
    fn show_orders_current_graph_and_long_form_information() {
        let mut issue = entity("i", EntityKind::Issue);
        issue.description = Some("description body\nwith two lines".to_string());
        let dependency = entity("d", EntityKind::Issue);
        let view = View::new(
            vec![issue.clone(), dependency.clone()],
            vec![(issue.id.clone(), dependency.id.clone())],
        );
        let at = Utc::now();
        let notes = vec![Note {
            id: RecordId::new(RecordKind::Note),
            body: "note body\nwith two lines".to_string(),
            actor: "tester".to_string(),
            created_at: at,
        }];
        let progress_events = vec![db::ProgressEvent {
            id: RecordId::new(RecordKind::Progress),
            kind: db::ProgressEventKind::Release,
            actor: "tester".to_string(),
            reason: Some("handoff".to_string()),
            at,
        }];
        let output = render_show(
            &view,
            &issue,
            &progress_events,
            &notes,
            RecordCounts {
                notes: 1,
                revisions: 1,
                decisions: 0,
                progressions: 1,
            },
            OutputDecoration::Plain,
        )
        .unwrap();

        let relationships = output.find("\n\nRelationships\n").unwrap();
        let description = output.find("\n\nDescription\n").unwrap();
        let notes_section = output.find("\n\nNotes\n").unwrap();
        let progress = output.find("\n\nProgress history\n").unwrap();
        assert!(relationships < description);
        assert!(description < notes_section);
        assert!(notes_section < progress);
        assert!(output.contains("Description\ndescription body\nwith two lines"));
        assert!(output.contains(&format!("Notes\nNote {}", notes[0].id)));
        assert!(output.contains("note body\nwith two lines"));

        let styled = render_show(
            &view,
            &issue,
            &progress_events,
            &notes,
            RecordCounts {
                notes: 1,
                revisions: 1,
                decisions: 0,
                progressions: 1,
            },
            OutputDecoration::Ansi,
        )
        .unwrap();
        assert!(styled.contains("\u{1b}[0m  tester\nnote body\nwith two lines"));
        assert_eq!(anstream::adapter::strip_str(&styled).to_string(), output);
    }

    #[test]
    fn human_output_renderers_share_plain_and_ansi_text() {
        let issue = entity("i", EntityKind::Issue);
        let mut rejected = entity("rejected", EntityKind::Issue);
        rejected.disposition = Disposition::Rejected;
        let view = View::new(
            vec![issue.clone(), rejected],
            vec![(issue.id.clone(), EntityId::from_stored("rejected"))],
        );
        let claim = Claim {
            actor: "raw actor".to_string(),
            worktree: "/raw/worktree".to_string(),
            at: Utc::now(),
        };
        let event = db::Event {
            id: RecordId::new(RecordKind::Decision),
            field: "disposition".to_string(),
            old_value: Some("undecided".to_string()),
            new_value: Some("accepted".to_string()),
            revision: Some(RecordId::new(RecordKind::Revision)),
            actor: "raw actor".to_string(),
            reason: Some("raw reason".to_string()),
            at: Utc::now(),
        };

        for (plain, styled) in [
            (
                render_entity_identity(&issue, OutputDecoration::Plain),
                render_entity_identity(&issue, OutputDecoration::Ansi),
            ),
            (
                render_triage_row(
                    &view,
                    &issue,
                    TriageReason::Orphaned,
                    OutputDecoration::Plain,
                ),
                render_triage_row(
                    &view,
                    &issue,
                    TriageReason::Orphaned,
                    OutputDecoration::Ansi,
                ),
            ),
            (
                render_claim_row(&issue, &claim, OutputDecoration::Plain),
                render_claim_row(&issue, &claim, OutputDecoration::Ansi),
            ),
            (
                render_list_row(&view, &issue, OutputDecoration::Plain).unwrap(),
                render_list_row(&view, &issue, OutputDecoration::Ansi).unwrap(),
            ),
            (
                format_decision(&event, OutputDecoration::Plain),
                format_decision(&event, OutputDecoration::Ansi),
            ),
            (
                decorate_import_report(
                    "Plan is valid.\nChanges:\n  none\n",
                    OutputDecoration::Plain,
                ),
                decorate_import_report(
                    "Plan is valid.\nChanges:\n  none\n",
                    OutputDecoration::Ansi,
                ),
            ),
        ] {
            assert_decoration_pair(plain, styled);
        }
    }

    #[test]
    fn revision_renderers_style_structure_without_styling_stored_text() {
        let issue = entity("i", EntityKind::Issue);
        let at = Utc::now();
        let from = DeclarationRevision {
            id: RecordId::new(RecordKind::Revision),
            title: "raw old title".to_string(),
            description: Some("raw old description".to_string()),
            parent: None,
            dependencies: vec![],
            created_at: at,
            baseline: true,
        };
        let to = DeclarationRevision {
            id: RecordId::new(RecordKind::Revision),
            title: "raw new title".to_string(),
            description: Some("raw new description".to_string()),
            parent: Some(EntityId::from_stored("parent")),
            dependencies: vec![EntityId::from_stored("dependency")],
            created_at: at,
            baseline: false,
        };
        let plain = render_revision(&issue.id, &issue, &from, OutputDecoration::Plain);
        let styled = render_revision(&issue.id, &issue, &from, OutputDecoration::Ansi);
        assert!(styled.contains("present\nraw old description\n\n"));
        assert_decoration_pair(plain, styled);

        let plain = render_revision_diff(&issue.id, &from, &to, OutputDecoration::Plain);
        let styled = render_revision_diff(&issue.id, &from, &to, OutputDecoration::Ansi);
        assert!(styled.contains("\u{1b}[0m raw old title\n"));
        assert!(styled.contains("\u{1b}[0m raw new description\n"));
        assert_decoration_pair(plain, styled);
    }

    #[test]
    fn output_writer_ignores_only_broken_pipes() {
        struct ErrorWriter(std::io::ErrorKind);

        impl std::io::Write for ErrorWriter {
            fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(self.0))
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        assert!(write_output_to(ErrorWriter(std::io::ErrorKind::BrokenPipe), "output").is_ok());
        assert_eq!(
            write_output_to(ErrorWriter(std::io::ErrorKind::PermissionDenied), "output")
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn root_help_and_docs_can_render_clap_style_without_changing_text() {
        for render in [render_root_help, render_docs] {
            let plain = render(OutputDecoration::Plain);
            let styled = render(OutputDecoration::Ansi);
            assert!(styled.contains("\u{1b}["));
            assert!(!styled.contains("\u{1b}[4m"));
            assert_eq!(anstream::adapter::strip_str(&styled).to_string(), plain);
        }
    }

    #[test]
    fn help_sections_cover_every_top_level_command_once() {
        let mut command = Cli::command();
        command.build();
        let actual = command
            .get_subcommands()
            .filter(|command| !command.is_hide_set())
            .map(|command| command.get_name())
            .collect::<std::collections::BTreeSet<_>>();
        let classified = HELP_SECTIONS
            .iter()
            .flat_map(|section| section.commands.iter().copied())
            .collect::<Vec<_>>();
        let unique = classified
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(
            classified.len(),
            unique.len(),
            "duplicate help classification"
        );
        assert_eq!(actual, unique);
    }
}
