mod actor;
mod db;
mod declaration;
mod derived;
mod domain;

use chrono::{NaiveDate, SecondsFormat, Utc};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use db::{Change, Ctx, Store};
use derived::{TriageReason, View};
use domain::*;

const CLI_GUIDE: &str = include_str!("../docs/help.md");

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
    /// Initialize axon at the management root
    Init {
        /// Prefix for generated Entity IDs; defaults to the management-root directory name
        prefix: Option<String>,
    },
    /// Generate a shell completion script
    Completion {
        /// Shell whose completion script is written to standard output
        #[arg(value_enum)]
        shell: Shell,
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
    },
    /// List the active decision frontier
    Triage {
        /// Include only one Entity kind
        #[arg(long, value_enum)]
        kind: Option<KindFilter>,
    },
    /// List every active claim
    Claims {
        /// Include only one Entity kind
        #[arg(long, value_enum)]
        kind: Option<KindFilter>,
    },
    /// List every active claim without inferring staleness from age
    Stale {
        /// Include only one Entity kind
        #[arg(long, value_enum)]
        kind: Option<KindFilter>,
    },
    /// Claim one ready Entity
    Start {
        /// Entity ID or unique ID suffix
        id: String,
    },
    /// End one InProgress Entity
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
    },
    /// Show one Entity and its relationships
    Show {
        /// Entity ID or unique ID suffix
        id: String,
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
enum ImportCmd {
    /// Assign final IDs and rewrite a declaration into canonical form without changing the DB
    Prepare {
        /// YAML declaration file to rewrite
        file: std::path::PathBuf,
    },
    /// Validate a canonical declaration and show structural and derived changes
    Check {
        /// Canonical YAML declaration file
        file: std::path::PathBuf,
    },
    /// Validate and atomically apply a canonical declaration
    Apply {
        /// Canonical YAML declaration file to apply and refresh
        file: std::path::PathBuf,
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
    /// Show one note by its Entity-local number
    Show {
        /// Entity ID or unique ID suffix
        id: String,
        /// Entity-local note number
        number: i64,
    },
}

#[derive(Subcommand)]
enum RevisionCmd {
    /// List plan declaration revisions for an Entity
    List {
        /// Entity ID or unique ID suffix
        id: String,
    },
    /// Show one revision by its Entity-local number
    Show {
        /// Entity ID or unique ID suffix
        id: String,
        /// Entity-local revision number
        number: i64,
    },
    /// Compare two plan declaration revisions
    Diff {
        /// Entity ID or unique ID suffix
        id: String,
        /// Older Entity-local revision number
        from: i64,
        /// Newer Entity-local revision number
        to: i64,
    },
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if requests_complete_help(&args) {
        if let Err(error) = write_complete_help() {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run(args) {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

fn run(args: Vec<std::ffi::OsString>) -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse_from(args).command {
        Command::Init { prefix } => cmd_init(prefix),
        Command::Completion { shell } => write_completion(shell).map_err(Into::into),
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
        Command::Ready { kind } => cmd_ready(kind),
        Command::Triage { kind } => cmd_triage(kind),
        Command::Claims { kind } | Command::Stale { kind } => cmd_claims(kind),
        Command::Start { id } => cmd_start(&id),
        Command::Done { id } => cmd_done(&id),
        Command::Release { id, reason } => cmd_release(&id, reason),
        Command::Write {
            id,
            title,
            message,
            file,
        } => cmd_write(&id, title, message, file),
        Command::Log { id } => cmd_log(&id),
        Command::List { kind } => cmd_list(kind),
        Command::Show { id } => cmd_show(&id),
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
    let mut store = Store::open()?;
    print!(
        "{}",
        declaration::export(&mut store, &ids, &groups, recursive)?
    );
    Ok(())
}

fn cmd_import(command: ImportCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    match command {
        ImportCmd::Prepare { file } => {
            declaration::prepare(&mut store, &file)?;
            println!("Prepared {}", file.display());
        }
        ImportCmd::Check { file } => {
            print!("{}", declaration::check(&mut store, &file)?);
        }
        ImportCmd::Apply { file } => {
            print!("{}", declaration::apply(&mut store, &file)?);
            println!("Applied {}", file.display());
        }
    }
    Ok(())
}

fn cmd_init(prefix: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let (path, prefix) = Store::init(prefix.as_deref())?;
    println!("Initialized axon at {}", path.display());
    println!("Entity IDs will use the form {prefix}-xxxxxx");
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
    let mut store = Store::open()?;
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
        current_revision: (disposition != Disposition::Undecided).then_some(1),
        resurface_condition: ResurfaceCondition::Always,
        parent,
        created_at: now,
        updated_at: now,
    };
    store.insert(&entity)?;
    println!(
        "{}  {}  {}  [{}]",
        entity.id,
        kind.label(),
        entity.title,
        disposition.label()
    );
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
            let mut store = Store::open()?;
            let id = store.resolve_id(&id)?;
            let parent = store.resolve_id(&parent)?;
            store.apply(&id, Change::SetParent(Some(parent.clone())), &ctx(None))?;
            println!("{id} parent set to {parent}");
            Ok(())
        }
        GroupCmd::Unset { id } => {
            let mut store = Store::open()?;
            let id = store.resolve_id(&id)?;
            store.apply(&id, Change::SetParent(None), &ctx(None))?;
            println!("{id} parent removed");
            Ok(())
        }
    }
}

fn load() -> Result<(Store, View), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let view = store.view()?;
    Ok((store, view))
}

fn included(kind: Option<KindFilter>, entity: &Entity) -> bool {
    kind.is_none_or(|filter| filter.matches(entity))
}

fn cmd_ready(kind: Option<KindFilter>) -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load()?;
    let rows = view
        .ready()
        .into_iter()
        .filter(|entity| included(kind, entity))
        .map(|entity| format!("{}  {}  {}\n", entity.id, entity.kind.label(), entity.title))
        .collect::<String>();
    print_rows(&rows, "No ready entities");
    Ok(())
}

fn cmd_triage(kind: Option<KindFilter>) -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load()?;
    let mut rows = String::new();
    for (entity, reason) in view
        .triage()
        .into_iter()
        .filter(|(entity, _)| included(kind, entity))
    {
        match reason {
            TriageReason::Undecided => rows.push_str(&format!(
                "{}  {}  Undecided  {}\n",
                entity.id,
                entity.kind.label(),
                entity.title
            )),
            TriageReason::Orphaned => {
                let lost = view
                    .dependency_targets(&entity.id)
                    .into_iter()
                    .filter(|target| target.disposition == Disposition::Rejected)
                    .map(|target| target.id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                rows.push_str(&format!(
                    "{}  {}  Orphaned  {} <- {} is Rejected\n",
                    entity.id,
                    entity.kind.label(),
                    entity.title,
                    lost
                ));
            }
        }
    }
    print_rows(&rows, "No entities need triage");
    Ok(())
}

fn cmd_claims(kind: Option<KindFilter>) -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load()?;
    let rows = view
        .claims()
        .into_iter()
        .filter(|(entity, _)| included(kind, entity))
        .map(|(entity, claim)| {
            format!(
                "{}  {}  {}  Claim: {}\n",
                entity.id,
                entity.kind.label(),
                entity.title,
                claim_details(claim)
            )
        })
        .collect::<String>();
    print_rows(&rows, "No active claims");
    Ok(())
}

fn claim_details(claim: &Claim) -> String {
    format!(
        "{}  Worktree: {}  Started: {}",
        claim.actor,
        claim.worktree,
        claim.at.to_rfc3339_opts(SecondsFormat::Secs, true)
    )
}

fn cmd_start(raw: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let id = store.resolve_id(raw)?;
    let claim = Claim {
        actor: actor::actor(),
        worktree: actor::worktree()?,
        at: Utc::now(),
    };
    store.apply(&id, Change::Start(claim.clone()), &ctx(None))?;
    let entity = store.get(&id)?;
    println!("Started {id} ({})", claim.actor);
    println!("{}  {}", entity.kind.label(), entity.title);
    Ok(())
}

fn cmd_done(raw: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let id = store.resolve_id(raw)?;
    store.apply(&id, Change::Done, &ctx(None))?;
    println!("Ended {id}");
    Ok(())
}

fn cmd_release(raw: &str, reason: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let id = store.resolve_id(raw)?;
    store.apply(&id, Change::Release, &ctx(reason))?;
    println!("Released {id}");
    Ok(())
}

fn cmd_list(kind: Option<KindFilter>) -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load()?;
    let mut rows = String::new();
    for entity in view.iter().filter(|entity| included(kind, entity)) {
        let mut marks = Vec::new();
        if view.is_orphaned(&entity.id) {
            marks.push("orphaned".to_string());
        } else if view.is_blocked(&entity.id) {
            marks.push("blocked".to_string());
        }
        if !view.is_surfaced(entity) {
            marks.push(entity.resurface_condition.label());
        }
        if let Some(reason) = inactive_scope_reason(&view, &entity.id) {
            marks.push(format!("inactive: {reason}"));
        }
        let marks = if marks.is_empty() {
            String::new()
        } else {
            format!(" {}", marks.join(" "))
        };
        rows.push_str(&format!(
            "{}  {}  [{}/{}]{}  {}\n",
            entity.id,
            entity.kind.label(),
            entity.progress.label(),
            entity.disposition.label(),
            marks,
            entity.title
        ));
    }
    print_rows(&rows, "No entities");
    Ok(())
}

fn cmd_show(raw: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let db::ShowSnapshot {
        id,
        view,
        decision_events,
        progress_events,
        revisions,
        notes,
        counts,
    } = store.show_snapshot(raw)?;
    debug_assert_eq!(counts.decisions, decision_events.len());
    debug_assert_eq!(counts.progressions, progress_events.len());
    debug_assert_eq!(counts.revisions, revisions.len());
    debug_assert_eq!(counts.notes, notes.len());
    let entity = view.get(&id).ok_or("entity not found")?;
    print!(
        "{}",
        render_show(&view, entity, &progress_events, &notes, counts)
    );
    Ok(())
}

fn render_show(
    view: &View,
    entity: &Entity,
    progress_events: &[db::ProgressEvent],
    notes: &[Note],
    counts: RecordCounts,
) -> String {
    let mut blocks = Vec::<Vec<String>>::new();
    let mut head = vec![
        format!("{}  {}  {}", entity.id, entity.kind.label(), entity.title),
        match entity.current_revision {
            Some(revision) => format!("Plan declaration: fixed at Revision {revision}"),
            None => "Plan declaration: draft".to_string(),
        },
        format!(
            "Records: Notes: {}  Revisions: {}  Decision history: {}  Progress history: {}",
            counts.notes, counts.revisions, counts.decisions, counts.progressions
        ),
        format!(
            "Progress: {}  Disposition: {}  Resurface condition: {}",
            entity.progress.label(),
            entity.disposition.label(),
            entity.resurface_condition.label()
        ),
    ];
    if let Some(parent) = &entity.parent {
        head.push(format!("Parent: {parent}"));
    }
    let active_scope = inactive_scope_reason(view, &entity.id)
        .map(|reason| format!("no ({reason})"))
        .unwrap_or_else(|| "yes".to_string());
    head.push(format!(
        "Active scope: {active_scope}  Blocked: {}  Orphaned: {}",
        yes_no(view.is_blocked(&entity.id)),
        yes_no(view.is_orphaned(&entity.id))
    ));
    if let Some(claim) = entity.progress.claim() {
        head.push(format!("Claim: {}", claim_details(claim)));
    }
    blocks.push(head);

    if let Some(description) = &entity.description {
        blocks.push(vec![description.clone()]);
    }

    if !notes.is_empty() {
        let mut rendered_notes = vec!["Notes:".to_string()];
        for note in notes {
            rendered_notes.push(format!(
                "Note {}  {}  {}\n{}",
                note.number,
                note.created_at.to_rfc3339_opts(SecondsFormat::Secs, true),
                note.actor,
                note.body
            ));
        }
        blocks.push(rendered_notes);
    }

    if entity.kind == EntityKind::Group {
        let summary = view.group_summary(&entity.id);
        let remaining = view
            .descendants(&entity.id)
            .into_iter()
            .filter(|descendant| !descendant.is_terminal())
            .map(|descendant| descendant.id.to_string())
            .collect::<Vec<_>>();
        let mut group = vec![
            render_entity_counts("Direct children", &summary.direct),
            render_entity_counts("Descendants", &summary.descendants),
            format!(
                "Can complete: {}",
                yes_no(view.can_complete_group(&entity.id))
            ),
        ];
        if !remaining.is_empty() {
            group.push(format!(
                "Non-terminal descendants: {}",
                remaining.join(", ")
            ));
        }
        blocks.push(group);
    }

    if !progress_events.is_empty() {
        let mut history = vec!["Progress history:".to_string()];
        for event in progress_events {
            let action = match event.kind {
                db::ProgressEventKind::Start => "Started",
                db::ProgressEventKind::Done => "Ended",
                db::ProgressEventKind::Release => "Released",
            };
            let reason = event
                .reason
                .as_ref()
                .map(|reason| format!("  ({reason})"))
                .unwrap_or_default();
            history.push(format!(
                "  {}  {}  {action}{reason}",
                event.at.format("%Y-%m-%d %H:%M"),
                event.actor
            ));
        }
        blocks.push(history);
    }

    let mut relations = Vec::new();
    for target in view.dependency_targets(&entity.id) {
        let label = if target.disposition == Disposition::Rejected {
            "Orphaned"
        } else if target.is_terminal() {
            "Satisfied dependency"
        } else {
            "Dependency"
        };
        relations.push(format!(
            "{label}: {}  {}  {}",
            target.id,
            target.kind.label(),
            target.title
        ));
    }
    for dependent in view.direct_dependents(&entity.id) {
        relations.push(format!(
            "Dependent: {}  {}  {}",
            dependent.id,
            dependent.kind.label(),
            dependent.title
        ));
    }
    for cause in view.blocking_causes(&entity.id) {
        relations.push(format!(
            "Root cause: {}  {}  {}",
            cause.id,
            cause.kind.label(),
            cause.title
        ));
    }
    if !relations.is_empty() {
        blocks.push(relations);
    }

    format!(
        "{}\n",
        blocks
            .into_iter()
            .filter(|block| !block.is_empty())
            .map(|block| block.join("\n"))
            .collect::<Vec<_>>()
            .join("\n\n")
    )
}

fn cmd_note(command: NoteCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    match command {
        NoteCmd::Add { id, message, file } => {
            let id = store.resolve_id(&id)?;
            let body = read_description(message, file)?
                .ok_or("a Note body is required; provide -m or -F")?;
            let note = store.add_note(&id, &body, &actor::actor())?;
            println!("{id}  Note {} recorded", note.number);
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
                        note.number,
                        note.created_at.to_rfc3339_opts(SecondsFormat::Secs, true),
                        note.actor,
                        first_line
                    )
                })
                .collect::<String>();
            print_rows(&rows, "No notes");
        }
        NoteCmd::Show { id, number } => {
            let id = store.resolve_id(&id)?;
            let note = store.note(&id, number)?;
            println!("{id}  Note {}", note.number);
            println!(
                "Recorded: {}  {}",
                note.created_at.to_rfc3339_opts(SecondsFormat::Secs, true),
                note.actor
            );
            println!();
            print!("{}", note.body);
            if !note.body.ends_with('\n') {
                println!();
            }
        }
    }
    Ok(())
}

fn cmd_revision(command: RevisionCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    match command {
        RevisionCmd::List { id } => {
            let id = store.resolve_id(&id)?;
            let snapshot = store.revision_snapshot(&id)?;
            let rows = snapshot
                .revisions
                .into_iter()
                .map(|revision| {
                    let mut marks = Vec::new();
                    if snapshot.entity.current_revision == Some(revision.number) {
                        marks.push("current");
                    }
                    if revision.baseline {
                        marks.push("baseline");
                    }
                    let marks = if marks.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", marks.join(", "))
                    };
                    format!(
                        "{}  {}{}  {}\n",
                        revision.number,
                        revision
                            .created_at
                            .to_rfc3339_opts(SecondsFormat::Secs, true),
                        marks,
                        revision.title
                    )
                })
                .collect::<String>();
            print_rows(&rows, "No declaration revisions");
        }
        RevisionCmd::Show { id, number } => {
            let id = store.resolve_id(&id)?;
            let snapshot = store.revision_snapshot(&id)?;
            let revision = snapshot.revision(number)?;
            print!("{}", render_revision(&id, &snapshot.entity, revision));
        }
        RevisionCmd::Diff { id, from, to } => {
            let id = store.resolve_id(&id)?;
            let snapshot = store.revision_snapshot(&id)?;
            let from_revision = snapshot.revision(from)?;
            let to_revision = snapshot.revision(to)?;
            print!("{}", render_revision_diff(&id, from_revision, to_revision));
        }
    }
    Ok(())
}

fn render_revision(id: &EntityId, entity: &Entity, revision: &DeclarationRevision) -> String {
    let mut marks = Vec::new();
    if entity.current_revision == Some(revision.number) {
        marks.push("current");
    }
    if revision.baseline {
        marks.push("baseline");
    }
    let marks = if marks.is_empty() {
        String::new()
    } else {
        format!(" [{}]", marks.join(", "))
    };
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
            .map(|dependency| format!("  {dependency}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "{id}  Declaration Revision {}{marks}\nCreated: {}\nTitle: {}\nParent: {parent}\n\nDescription:\n{description}\n\nOutgoing dependencies:\n{dependencies}\n",
        revision.number,
        revision
            .created_at
            .to_rfc3339_opts(SecondsFormat::Secs, true),
        revision.title
    )
}

fn render_revision_diff(
    id: &EntityId,
    from: &DeclarationRevision,
    to: &DeclarationRevision,
) -> String {
    let mut output = format!(
        "{id}  Declaration Revision {} -> {}\n\n",
        from.number, to.number
    );
    push_value_diff(&mut output, "Title", &from.title, &to.title);
    push_optional_value_diff(
        &mut output,
        "Description",
        from.description.as_deref(),
        to.description.as_deref(),
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
    output.push_str("Outgoing dependencies:\n");
    if removed.is_empty() && added.is_empty() {
        output.push_str("  unchanged\n");
    } else {
        for dependency in removed {
            output.push_str(&format!("- {dependency}\n"));
        }
        for dependency in added {
            output.push_str(&format!("+ {dependency}\n"));
        }
    }
    output
}

fn push_value_diff(output: &mut String, label: &str, from: &str, to: &str) {
    output.push_str(&format!("{label}:\n"));
    if from == to {
        output.push_str("  unchanged\n");
    } else {
        for line in from.lines() {
            output.push_str(&format!("- {line}\n"));
        }
        for line in to.lines() {
            output.push_str(&format!("+ {line}\n"));
        }
    }
}

fn push_optional_value_diff(
    output: &mut String,
    label: &str,
    from: Option<&str>,
    to: Option<&str>,
) {
    output.push_str(&format!("{label}:\n"));
    if from == to {
        output.push_str("  unchanged\n");
        return;
    }
    push_optional_diff_side(output, '-', from);
    push_optional_diff_side(output, '+', to);
}

fn push_optional_diff_side(output: &mut String, prefix: char, value: Option<&str>) {
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

fn render_entity_counts(label: &str, counts: &derived::EntityCounts) -> String {
    format!(
        "{label}: {} (Issue: {}, Group: {}, NotStarted: {}, InProgress: {}, Ended: {}, Undecided: {}, Accepted: {}, Rejected: {}, Terminal: {})",
        counts.total,
        counts.issues,
        counts.groups,
        counts.not_started,
        counts.in_progress,
        counts.ended,
        counts.undecided,
        counts.accepted,
        counts.rejected,
        counts.terminal
    )
}

fn inactive_scope_reason(view: &View, id: &EntityId) -> Option<String> {
    view.ancestors(id).into_iter().find_map(|group| {
        if view.opens_descendants(group) {
            return None;
        }
        let mut reasons = Vec::new();
        if !matches!(group.progress, Progress::InProgress(_)) {
            reasons.push(format!("Progress={}", group.progress.label()));
        }
        if group.disposition != Disposition::Accepted {
            reasons.push(format!("Disposition={}", group.disposition.label()));
        }
        if !view.is_surfaced(group) {
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
        Some(format!("{} ({})", group.id, reasons.join(", ")))
    })
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
    let mut store = Store::open()?;
    let id = store.resolve_id(&args.id)?;
    store.apply(&id, Change::Decide(disposition), &ctx(args.reason))?;
    println!("{id} Disposition set to {}", disposition.label());
    if disposition == Disposition::Rejected && store.get(&id)?.progress.claim().is_some() {
        println!("The Entity remains InProgress; if work has stopped, also run `axon done {id}`");
    }
    Ok(())
}

fn cmd_when(command: WhenCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    match command {
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
            println!("{id} resurface condition set to AtDate({date})");
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
            println!("{id} resurface condition set to AfterEntity({reference})");
        }
        WhenCmd::Clear { id, reason } => {
            let id = store.resolve_id(&id)?;
            store.apply(
                &id,
                Change::SetResurfaceCondition(ResurfaceCondition::Always),
                &ctx(reason),
            )?;
            println!("{id} resurface condition set to Always");
        }
    }
    Ok(())
}

fn cmd_dep(command: DepCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    match command {
        DepCmd::Add { id, needs } => {
            let id = store.resolve_id(&id)?;
            let needs = store.resolve_id(&needs)?;
            store.add_dep(&id, &needs)?;
            println!("{id} now depends on {needs}");
        }
        DepCmd::Rm { id, needs } => {
            let id = store.resolve_id(&id)?;
            let needs = store.resolve_id(&needs)?;
            store.remove_dep(&id, &needs)?;
            println!("{id} no longer depends on {needs}");
        }
    }
    Ok(())
}

fn cmd_log(raw: &str) -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open()?;
    let id = store.resolve_id(raw)?;
    let events = store.events(&id)?;
    if events.is_empty() {
        println!("No decision history");
        return Ok(());
    }
    for event in events {
        print!(
            "{}  {}  {}",
            event.at.format("%Y-%m-%d %H:%M"),
            event.actor,
            format_decision(&event)
        );
        if let Some(reason) = event.reason {
            print!("  ({reason})");
        }
        if let Some(revision) = event.revision {
            print!("  [Revision {revision}]");
        }
        println!();
    }
    Ok(())
}

fn format_decision(event: &db::Event) -> String {
    let label = |value: &Option<String>| match value.as_deref() {
        Some("undecided") => "Undecided".to_string(),
        Some("accepted") => "Accepted".to_string(),
        Some("rejected") => "Rejected".to_string(),
        Some(other) => other.to_string(),
        None => "Always".to_string(),
    };
    match event.field.as_str() {
        "disposition" => format!(
            "Disposition: {} -> {}",
            label(&event.old_value),
            label(&event.new_value)
        ),
        "resurface_condition" => format!(
            "Resurface condition: {} -> {}",
            label(&event.old_value),
            label(&event.new_value)
        ),
        field => format!(
            "{field}: {} -> {}",
            label(&event.old_value),
            label(&event.new_value)
        ),
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

    let mut store = Store::open()?;
    let id = store.resolve_id(raw)?;
    let current = store.get(&id)?;
    let mut changed = Vec::new();
    if let Some(title) = title {
        let title = normalize_title(&title)?;
        if title != current.title {
            store.apply(&id, Change::SetTitle(title), &ctx(None))?;
            changed.push("title updated");
        }
    }
    if let Some(body) = body {
        let body = normalize_description(body);
        if body != current.description {
            let removed = body.is_none();
            store.apply(&id, Change::SetDescription(body), &ctx(None))?;
            changed.push(if removed {
                "description removed"
            } else {
                "description updated"
            });
        }
    }
    if changed.is_empty() {
        println!("No changes");
    } else {
        for change in changed {
            println!("{id}: {change}");
        }
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

fn print_rows(rows: &str, empty_note: &str) {
    if rows.is_empty() {
        eprintln!("{empty_note}");
    } else {
        print!("{rows}");
    }
}

fn requests_complete_help(args: &[std::ffi::OsString]) -> bool {
    match args.get(1).and_then(|argument| argument.to_str()) {
        Some("--help") => true,
        Some("help") => args.len() == 2,
        _ => false,
    }
}

fn write_complete_help() -> std::io::Result<()> {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(render_complete_help().as_bytes()) {
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

fn render_complete_help() -> String {
    let mut command = Cli::command().term_width(0);
    command.build();
    let mut output = String::new();
    output.push_str(CLI_GUIDE.trim_end());
    output.push_str("\n\n# Command reference\n");
    write_leaf_help(&mut output, &mut command, "axon");
    output
}

fn write_leaf_help(output: &mut String, command: &mut clap::Command, path: &str) {
    let generated_help = !command.is_disable_help_subcommand_set();
    let has_children = command
        .get_subcommands()
        .any(|child| !child.is_hide_set() && !(generated_help && child.get_name() == "help"));
    if !has_children {
        output.push_str(&format!("\n## `{path}`\n\n"));
        output.push_str(command.render_long_help().to_string().trim_end());
        output.push('\n');
        return;
    }
    for child in command.get_subcommands_mut() {
        if child.is_hide_set() || (generated_help && child.get_name() == "help") {
            continue;
        }
        write_leaf_help(output, child, &format!("{path} {}", child.get_name()));
    }
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

    fn entity(id: &str, kind: EntityKind) -> Entity {
        let now = Utc::now();
        Entity {
            id: EntityId::from_stored(id),
            kind,
            title: format!("{id} title"),
            description: None,
            progress: Progress::NotStarted,
            disposition: Disposition::Accepted,
            current_revision: Some(1),
            resurface_condition: ResurfaceCondition::Always,
            parent: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn show_identifies_kind_and_group_summary() {
        let group = entity("g", EntityKind::Group);
        let mut issue = entity("i", EntityKind::Issue);
        issue.parent = Some(group.id.clone());
        let view = View::new(vec![group.clone(), issue], vec![]);
        let output = render_show(&view, &group, &[], &[], RecordCounts::default());
        assert!(output.starts_with("g  Group  g title\n"));
        assert!(output.contains("Descendants: 1 (Issue: 1, Group: 0"));
    }

    #[test]
    fn complete_help_contains_every_leaf() {
        let help = render_complete_help();
        for path in [
            "axon plan",
            "axon group plan",
            "axon group capture",
            "axon group set",
            "axon group unset",
            "axon dep add",
            "axon note add",
            "axon note list",
            "axon note show",
            "axon revision list",
            "axon revision show",
            "axon revision diff",
            "axon export",
            "axon import prepare",
            "axon import check",
            "axon import apply",
        ] {
            assert!(help.contains(&format!("## `{path}`\n")), "{path}");
        }
        assert!(!help.contains("## `axon group new`"));
    }
}
