use super::display;
use axon::{
    Result,
    lifecycle::{EntityId, Kind, Lifecycle},
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
    /// Check the store for corrupt files, conflicts, violations and missing records
    Storage {
        #[command(subcommand)]
        command: Storage,
    },
    /// Initialize a new management root
    #[command(
        after_help = "PREFIX uses ASCII lowercase letters, digits and hyphens, and starts and ends with a letter or digit.\nWithout PREFIX, init lowercases the management root directory name and fails if that is not a valid prefix.\nInit creates .axon/records/, .axon/header.json, .axon/.gitignore and an .axon/.gitattributes holding only \"* -text\", which stops Git line-ending conversion of record files; it sets no merge attributes. Inside Git it prints how to keep the store ignored or to track it; which one applies is your choice.\nInit does not create or edit the repository root's .gitignore, .gitattributes or Git config, and does not stage or commit any files."
    )]
    Init {
        /// ID prefix for generated Entity IDs
        prefix: Option<String>,
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
    /// Show text, the situation from evaluated conditions, unmet prerequisites and all Group descendants
    #[command(
        after_help = "Like tasks, show runs the resurfacing conditions that decide the situation of a NotStarted Entity: the ancestors, the Entity itself and, for a Group, its startable descendants. Undecided, InProgress and terminal Entities run nothing. An unsurfaced Issue is shown as Unsurfaced, and a stalled Group lists unsurfaced candidates, its own unsatisfied condition and an unsurfaced ancestor under Stalled.
Conditions run through /bin/sh -c: exit 0 is satisfied, 1 is unsatisfied, other exits fail the whole command.
To read saved information without running any condition, add --skip-conditions; the situation is then derived as if every condition were satisfied."
    )]
    Show {
        id: String,
        /// Include saved lifecycle, condition and all direct relationships without duplicate wait sections
        #[arg(long)]
        details: bool,
        /// Run no condition and derive the situation as if every condition were satisfied
        #[arg(long, conflicts_with = "trace_conditions")]
        skip_conditions: bool,
        #[command(flatten)]
        conditions: ConditionOptions,
    },
    /// Append or read immutable Notes
    Note {
        #[command(subcommand)]
        command: Notes,
    },
    /// Read the records of an Entity: state changes, edits, relationships, conversions and resolutions
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
    /// Start an Issue after checking ancestor adoption and dependency prerequisites
    Start(Change),
    /// Release an InProgress Issue back to NotStarted
    Release(Change),
    /// Complete work; for a Group, explicitly confirm final review of the entire plan
    Complete(Change),
    /// Cancel work
    Cancel(Change),
    /// Return a Cancelled Entity to Undecided
    Reconsider(Change),
    /// Return a Completed Entity to NotStarted; rejected while Completed dependents remain
    Reopen(Change),
    /// Convert an unstarted Entity between Issue and Group without changing anything else
    #[command(
        after_help = "Example: axon convert ID --kind group\nOnly an Undecided or NotStarted Entity converts; release an InProgress Issue first, and a Group with children is not converted to an Issue. Lifecycle, parent, dependencies, text, condition and Notes stay as they are. Converting to the kind the Entity already has is No changes. The kind is not a lifecycle transition, so there is no --reason.\nA Group's description states the outcome of the whole plan and what its final review confirms, so reread an Issue's description after converting it."
    )]
    Convert {
        id: String,
        /// The kind to convert to
        #[arg(long, value_enum)]
        kind: EntityKind,
    },
    /// List conflicted Entities with their heads, or resolve one by taking a head's value
    #[command(
        after_help = "Example: axon resolve\n         axon resolve ID --head RECORD_ID -r 'keep the side with remaining work'\nWithout ID every conflicted Entity is listed; with ID alone, that Entity. Each head line has the record ID, time, actor, record kind, and the lifecycle, kind and title of that head. A head whose parent record is missing is marked \"parent missing; likely newer\": a cherry-pick or revert left a gap, and that head is probably the later record.\nWith ID and --head RECORD_ID (a complete record ID from the listing) a resolve record takes that head's value and joins every head; the Entity is settled at once. Violations that remain are shown by axon show and axon storage check and repaired with ordinary commands."
    )]
    Resolve {
        /// The conflicted Entity; without it every conflicted Entity is listed
        id: Option<String>,
        /// The complete record ID of the head whose value the Entity takes
        #[arg(long, value_name = "RECORD_ID", requires = "id")]
        head: Option<String>,
        /// Why this head is taken: one line, stored in the resolve record like other reasons
        #[arg(short, long, requires = "head")]
        reason: Option<String>,
    },
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
    /// Report corrupt files, conflicted Entities, structural violations and missing parent records
    #[command(
        after_help = "Without ROOT the store found by discovery is checked; with ROOT that management root is checked without discovery.\nCorrupt files, conflicts and violations exit 1; missing parent records (gaps) are reported as information and exit 0.\nNo condition runs and nothing is written."
    )]
    Check {
        /// The management root holding .axon/ to check instead of the discovered store
        root: Option<PathBuf>,
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
    #[command(flatten)]
    pub(super) conditions: ConditionOptions,
}
/// How one command runs the conditions it evaluates.
#[derive(Args)]
pub(super) struct ConditionOptions {
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
    /// Containing Group; an Issue starts only after every ancestor Group is adopted
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
        after_help = "Matches case-sensitive literal text without trimming or Unicode normalization. Prints one line per Note: complete Entity and Note IDs, local timestamp and an escaped excerpt around the first match. Entities are listed in creation order and Notes in time order (record ID order at the same time). Read the original with axon note show ID NOTE_ID. No matches succeeds with empty stdout. For a query beginning with a hyphen use: axon note search -- '--text'."
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
    fn matches(self, kind: Kind) -> bool {
        self.kind() == kind
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
    /// Whether the Entity's presented value (its current value, or its first head's while
    /// conflicted) matches the kind and search filters.
    pub fn matches(&self, view: &read::View<'_>, id: &EntityId) -> bool {
        let Some(current) = view.presented(id) else {
            return false;
        };
        self.kind.is_none_or(|k| k.matches(current.kind))
            && self
                .search
                .as_ref()
                .is_none_or(|query| !read::matches_in(current, query).is_empty())
    }
}
#[derive(Args)]
pub struct ListOptions {
    #[command(flatten)]
    pub selection: Selection,
    /// Restrict the lifecycle; a Group matches by its effective value (independent of surfacing and blocking)
    #[arg(long)]
    pub(super) lifecycle: Option<LifecycleFilter>,
    /// Select terminal (true) or non-terminal (false) Entities; omit for both
    #[arg(long, action = clap::ArgAction::Set)]
    pub(super) terminal: Option<bool>,
}
impl ListOptions {
    /// A conflicted Entity has no lifecycle, so a lifecycle or terminal filter leaves it out.
    pub fn matches(&self, view: &read::View<'_>, id: &EntityId) -> bool {
        self.selection.matches(view, id)
            && self
                .lifecycle
                .is_none_or(|l| view.effective(id) == Some(l.state()))
            && self.terminal.is_none_or(|terminal| {
                view.current(id)
                    .is_some_and(|c| terminal != c.lifecycle.editable())
            })
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
        Command::Reopen(c) => ("reopen", Some(&c.id)),
        Command::Convert { id, .. } => ("convert", Some(id)),
        Command::Resolve { id, .. } => ("resolve", id.as_deref()),
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
