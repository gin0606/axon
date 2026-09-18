use super::display;
use axon::{
    Result,
    lifecycle::{Entity, Kind, Lifecycle},
    read,
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{io::Read, path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(
    version,
    styles = display::cli_styles(),
    color = display::cli_color(),
    about = "A local issue tracker for Issues and Groups",
    after_help = "Use axon docs for the lifecycle and daily workflow. Group complete explicitly confirms that the entire plan has passed final review."
)]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) command: Command,
}
#[derive(Subcommand)]
pub(super) enum Command {
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
        after_help = "PREFIX uses ASCII lowercase letters, digits and hyphens, and starts and ends with a letter or digit.\nWithout PREFIX, init lowercases the management root directory name and fails if that is not a valid prefix.\nSQLite creates only .axon/axon.db and does not change Git integration files.\nFile creates .axon/state.jsonl and creates or appends .axon/.gitignore and root .gitattributes, preserving unrelated lines.\nInit does not stage or commit any files."
    )]
    Init {
        /// ID prefix for generated Entity IDs
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
pub(super) enum Import {
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
pub(super) enum Docs {
    /// Explain declaration fields and the prepare/check/apply workflow
    Declaration {
        /// Print only a canonical YAML template for a new plan
        #[arg(long)]
        example: bool,
    },
}
#[derive(Subcommand)]
pub(super) enum Storage {
    Check { snapshot: PathBuf },
}
#[derive(Subcommand)]
#[command(
    after_help = "Example: axon merge prepare --base base.jsonl --ours ours.jsonl --theirs theirs.jsonl --output .axon/state.jsonl --workspace .axon/merge-review\nEdit choices in resolution.json, then run axon merge check WORKSPACE and axon merge apply WORKSPACE.\nGit driver configuration, staging and commits are separate user operations. Retain the workspace for recovery."
)]
pub(super) enum Merge {
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
pub(super) struct CandidateOptions {
    #[command(flatten)]
    pub(super) selection: Selection,
    /// Per-command timeout: positive integer followed by ms, s, m or h
    #[arg(long, default_value = "30s", value_parser = parse_timeout)]
    pub(super) condition_timeout: Duration,
    /// Write evaluated condition results and captured output to stderr
    #[arg(long)]
    pub(super) trace_conditions: bool,
}
#[derive(Subcommand)]
pub(super) enum Condition {
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
pub(super) enum Backend {
    Sqlite,
    File,
}
#[derive(Args)]
pub(super) struct Body {
    #[arg(short = 'm', long, conflicts_with = "description_file")]
    pub(super) description: Option<String>,
    /// Read UTF-8 text from a file; - reads standard input
    #[arg(short = 'F', long = "file")]
    pub(super) description_file: Option<PathBuf>,
}
impl Body {
    pub(super) fn read(self) -> Result<Option<String>> {
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
pub(super) struct Create {
    /// Title of the new Entity: one line, at most 200 characters
    #[arg(long)]
    pub(super) title: String,
    /// Entity kind to create
    #[arg(long, value_enum, default_value = "issue")]
    pub(super) kind: EntityKind,
    /// Register as adopted work in NotStarted instead of Undecided
    #[arg(long)]
    pub(super) accept: bool,
    /// Initial shell condition; stored without evaluation
    #[arg(long)]
    pub(super) command: Option<String>,
    #[command(flatten)]
    pub(super) body: Body,
    /// Containing Group; the new Entity starts only while that Group is InProgress
    #[arg(long)]
    pub(super) parent: Option<String>,
    /// Dependency that must be Completed first; repeat for several
    #[arg(long)]
    pub(super) needs: Vec<String>,
}
#[derive(Args)]
pub(super) struct Change {
    pub(super) id: String,
    #[arg(short, long)]
    pub(super) reason: Option<String>,
}
#[derive(Subcommand)]
pub(super) enum Parent {
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
pub(super) enum Dependency {
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
pub(super) enum Notes {
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
#[derive(Clone, Copy, ValueEnum)]
pub enum EntityKind {
    Issue,
    Group,
}
impl EntityKind {
    pub fn kind(self) -> Kind {
        match self {
            Self::Issue => Kind::Issue,
            Self::Group => Kind::Group,
        }
    }
    fn matches(self, entity: &Entity) -> bool {
        self.kind() == entity.kind
    }
}
#[derive(Clone, Copy, ValueEnum)]
pub enum LifecycleFilter {
    Undecided,
    NotStarted,
    InProgress,
    Completed,
    Cancelled,
}
impl LifecycleFilter {
    fn state(self) -> Lifecycle {
        match self {
            Self::Undecided => Lifecycle::Undecided,
            Self::NotStarted => Lifecycle::NotStarted,
            Self::InProgress => Lifecycle::InProgress,
            Self::Completed => Lifecycle::Completed,
            Self::Cancelled => Lifecycle::Cancelled,
        }
    }
}
#[derive(Args)]
pub struct Selection {
    /// Restrict the Entity kind before evaluating any conditions
    #[arg(long)]
    pub(super) kind: Option<EntityKind>,
    /// Literal, case-sensitive text in current title or description; AND with other filters. Search Note bodies with axon note search
    #[arg(long, value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub search: Option<String>,
}
impl Selection {
    pub fn matches(&self, entity: &Entity) -> bool {
        self.kind.is_none_or(|k| k.matches(entity))
            && self
                .search
                .as_ref()
                .is_none_or(|query| !read::matches_in(entity, query).is_empty())
    }
}
#[derive(Args)]
pub struct ListOptions {
    #[command(flatten)]
    pub selection: Selection,
    /// Restrict the saved lifecycle (independent of surfacing and blocking)
    #[arg(long)]
    pub(super) lifecycle: Option<LifecycleFilter>,
    /// Select terminal (true) or non-terminal (false) Entities; omit for both
    #[arg(long, action = clap::ArgAction::Set)]
    pub(super) terminal: Option<bool>,
}
impl ListOptions {
    pub fn matches(&self, entity: &Entity) -> bool {
        self.selection.matches(entity)
            && self
                .lifecycle
                .is_none_or(|l| l.state() == entity.current.lifecycle)
            && self
                .terminal
                .is_none_or(|terminal| terminal != entity.current.lifecycle.editable())
    }
}
pub fn parse_timeout(value: &str) -> std::result::Result<Duration, String> {
    let (number, factor) = if let Some(n) = value.strip_suffix("ms") {
        (n, 1_u64)
    } else if let Some(n) = value.strip_suffix('s') {
        (n, 1_000)
    } else if let Some(n) = value.strip_suffix('m') {
        (n, 60_000)
    } else if let Some(n) = value.strip_suffix('h') {
        (n, 3_600_000)
    } else {
        return Err(
            "use a positive integer followed by ms, s, m or h (for example 500ms or 2m)".into(),
        );
    };
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return Err("timeout requires a positive integer".into());
    }
    let milliseconds = number
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(factor))
        .filter(|n| *n > 0)
        .ok_or("timeout must be positive and fit in milliseconds")?;
    Ok(Duration::from_millis(milliseconds))
}
pub fn operation_label(command: &Command) -> String {
    let (operation, target): (&str, Option<&str>) = match command {
        Command::Show { id, .. } => ("show", Some(id)),
        Command::Write { id, .. } => ("write", Some(id)),
        Command::Log { id, .. } => ("log", Some(id)),
        Command::Accept(c) => ("accept", Some(&c.id)),
        Command::Withdraw(c) => ("withdraw", Some(&c.id)),
        Command::Start(c) => ("start", Some(&c.id)),
        Command::Release(c) => ("release", Some(&c.id)),
        Command::Complete(c) => ("complete", Some(&c.id)),
        Command::Cancel(c) => ("cancel", Some(&c.id)),
        Command::Reconsider(c) => ("reconsider", Some(&c.id)),
        Command::Condition {
            command: Condition::Set { id, .. },
        } => ("condition set", Some(id)),
        Command::Condition {
            command: Condition::Unset { id },
        } => ("condition unset", Some(id)),
        Command::Dep {
            command: Dependency::Add { id, .. },
        } => ("dep add", Some(id)),
        Command::Dep {
            command: Dependency::Rm { id, .. },
        } => ("dep rm", Some(id)),
        Command::Parent {
            command: Parent::Set { id, .. },
        } => ("parent set", Some(id)),
        Command::Parent {
            command: Parent::Unset { id },
        } => ("parent unset", Some(id)),
        Command::Note {
            command: Notes::Add { id, .. },
        } => ("note add", Some(id)),
        Command::Note {
            command: Notes::List { id, .. },
        } => ("note list", Some(id)),
        Command::Note {
            command: Notes::Show { id, .. },
        } => ("note show", Some(id)),
        Command::Capture(_) => ("capture", None),
        Command::List(_) => ("list", None),
        Command::Tasks(_) => ("tasks", None),
        Command::Proposals(_) => ("proposals", None),
        Command::Init { .. } => ("init", None),
        Command::Merge { .. } => ("merge", None),
        Command::Storage { .. } => ("storage check", None),
        Command::Docs { .. } => ("docs", None),
        Command::Import { .. } => ("import", None),
        Command::Export { .. } => ("export", None),
        Command::Actor => ("actor", None),
        Command::Note {
            command: Notes::Search { .. },
        } => ("note search", None),
        Command::Completion { .. } => ("completion", None),
    };
    target
        .map(|t| format!("{t} {operation}"))
        .unwrap_or_else(|| operation.into())
}
