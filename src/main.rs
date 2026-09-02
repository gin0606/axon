mod actor;
mod db;
mod derived;
mod domain;

use chrono::{NaiveDate, SecondsFormat, Utc};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::aot::{Shell, generate};
use db::{ApplyOutcome, Change, Ctx, Store};
use derived::View;
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

#[derive(Subcommand)]
enum Command {
    /// Initialize axon at the management root
    ///
    /// In Git, the management root contains the common Git directory, so all worktrees share one
    /// database. Outside Git, the current directory becomes the management root. Later commands
    /// outside Git search its ancestors for the nearest axon database; init refuses to create a
    /// nested database below an existing non-Git management root.
    Init {
        /// Prefix for generated issue IDs; defaults to the management-root directory name
        prefix: Option<String>,
    },
    /// Generate a shell completion script
    Completion {
        /// Shell whose completion script is written to standard output
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Create an issue with Disposition Accepted
    Plan {
        /// One or more words joined with spaces to form the issue title
        #[arg(required = true, value_name = "TITLE")]
        title: Vec<String>,
    },
    /// Create an issue with Disposition Undecided
    Capture {
        /// One or more words joined with spaces to form the issue title
        #[arg(required = true, value_name = "TITLE")]
        title: Vec<String>,
    },
    /// List issues that can be started now
    Ready,
    /// List issues that need a human disposition decision
    Triage,
    /// List every active claim with its actor, worktree, and start time
    Claims,
    /// Claim one ready issue and record the actor, worktree, and start time
    Start {
        /// Issue ID or unique six-character suffix to claim
        id: String,
    },
    /// End an InProgress issue and confirm that issue only
    Done {
        /// Issue ID or unique six-character suffix to end
        id: String,
    },
    /// Update an issue's title or description
    Write {
        /// Issue ID or unique six-character suffix to update
        id: String,
        /// Replacement title; quote it to include spaces
        #[arg(long)]
        title: Option<String>,
        /// Replacement description text; an empty value removes the description
        #[arg(short = 'm', long)]
        message: Option<String>,
        /// File containing the replacement description, or - for standard input
        #[arg(short = 'F', long)]
        file: Option<String>,
    },
    /// Show Disposition and resurface-condition decision history
    Log {
        /// Issue ID or unique six-character suffix whose decision history is shown
        id: String,
    },
    /// Release a claim and record an optional reason or handoff
    Release {
        /// Issue ID or unique six-character suffix whose claim is released
        id: String,
        /// Release reason or handoff to record in progress history
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// List every issue regardless of state
    List,
    /// Show issue details, relationships, and progress history
    Show {
        /// Issue ID or unique six-character suffix to show
        id: String,
    },
    /// Change an issue's Disposition
    #[command(subcommand)]
    Decide(DecideCmd),
    /// Change an issue's resurface condition
    #[command(subcommand)]
    When(WhenCmd),
    /// Manage issue dependencies
    #[command(subcommand)]
    Dep(DepCmd),
    /// Manage groups and group dependencies
    #[command(subcommand)]
    Group(GroupCmd),
}

#[derive(Subcommand)]
enum GroupCmd {
    /// Create a group
    New {
        /// Slug for the new group
        slug: String,
        /// Words joined with spaces as the display name; defaults to the slug
        name: Vec<String>,
        /// Slug of the parent group
        #[arg(long)]
        parent: Option<String>,
    },
    /// List groups
    List,
    /// Show a group's issues and progress
    Show {
        /// Slug of the group to show
        slug: String,
    },
    /// Put an issue in a group
    Set {
        /// Issue ID or unique six-character suffix to put in the group
        id: String,
        /// Slug of the group that will contain the issue
        slug: String,
    },
    /// Remove an issue from its group
    Unset {
        /// Issue ID or unique six-character suffix to remove from its group
        id: String,
    },
    /// Reject every descendant issue in a group
    Reject {
        /// Slug of the group whose descendant issues are rejected
        slug: String,
        /// Reason for each Disposition change, recorded in decision history
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// Manage group dependencies
    #[command(subcommand)]
    Dep(GroupDepCmd),
}

#[derive(Subcommand)]
enum GroupDepCmd {
    /// Add a dependency
    Add {
        /// Slug of the group that will depend on another group
        slug: String,
        /// Slug of the group whose result is required
        #[arg(long)]
        needs: String,
    },
    /// Remove a dependency
    Rm {
        /// Slug of the group that currently depends on another group
        slug: String,
        /// Slug of the group whose requirement is removed
        #[arg(long)]
        needs: String,
    },
}

#[derive(Subcommand)]
enum DecideCmd {
    /// Set Disposition to Accepted
    Accept {
        /// Issue ID or unique six-character suffix whose Disposition is changed
        id: String,
        /// Reason for the Disposition decision, recorded in decision history
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// Set Disposition to Rejected
    Reject {
        /// Issue ID or unique six-character suffix whose Disposition is changed
        id: String,
        /// Reason for the Disposition decision, recorded in decision history
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// Set Disposition back to Undecided
    Undecide {
        /// Issue ID or unique six-character suffix whose Disposition is changed
        id: String,
        /// Reason for the Disposition decision, recorded in decision history
        #[arg(short, long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand)]
enum WhenCmd {
    /// Set an AtDate resurface condition
    At {
        /// Issue ID or unique six-character suffix whose resurface condition is changed
        id: String,
        /// Date when the issue resurfaces, in YYYY-MM-DD format
        date: String,
        /// Reason for the resurface-condition decision, recorded in decision history
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// Set an AfterIssue resurface condition
    After {
        /// Issue ID or unique six-character suffix whose resurface condition is changed
        id: String,
        /// Issue ID or unique six-character suffix whose terminal state resurfaces the issue
        reference: String,
        /// Reason for the resurface-condition decision, recorded in decision history
        #[arg(short, long)]
        reason: Option<String>,
    },
    /// Set the resurface condition to Always
    Clear {
        /// Issue ID or unique six-character suffix whose resurface condition is cleared
        id: String,
        /// Reason for the resurface-condition decision, recorded in decision history
        #[arg(short, long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand)]
enum DepCmd {
    /// Add a dependency
    Add {
        /// Issue ID or unique six-character suffix of the dependent issue
        id: String,
        /// Issue ID or unique six-character suffix of the required issue
        #[arg(long)]
        needs: String,
    },
    /// Remove a dependency
    Rm {
        /// Issue ID or unique six-character suffix of the dependent issue
        id: String,
        /// Issue ID or unique six-character suffix of the issue no longer required
        #[arg(long)]
        needs: String,
    },
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if requests_complete_help(&args) {
        if let Err(e) = write_complete_help() {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
        return;
    }

    if let Err(e) = run(args) {
        eprintln!("Error: {e}");
        std::process::exit(1);
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

fn requests_complete_help(args: &[std::ffi::OsString]) -> bool {
    match args.get(1).and_then(|arg| arg.to_str()) {
        Some("--help") => true,
        Some("help") => args.len() == 2,
        _ => false,
    }
}

fn render_complete_help() -> String {
    let mut command = Cli::command().term_width(0);
    command.build();

    let mut out = String::new();
    out.push_str(CLI_GUIDE.trim_end());
    out.push_str("\n\n# Command reference\n");
    write_leaf_help(&mut out, &mut command, "axon");
    out
}

fn write_leaf_help(out: &mut String, command: &mut clap::Command, path: &str) {
    let has_generated_help = !command.is_disable_help_subcommand_set();
    let has_visible_children = command
        .get_subcommands()
        .any(|child| !child.is_hide_set() && !(has_generated_help && child.get_name() == "help"));

    if !has_visible_children {
        out.push_str(&format!("\n## `{path}`\n\n"));
        out.push_str(command.render_long_help().to_string().trim_end());
        out.push('\n');
        return;
    }

    for child in command.get_subcommands_mut() {
        if child.is_hide_set() || (has_generated_help && child.get_name() == "help") {
            continue;
        }
        write_leaf_help(out, child, &format!("{path} {}", child.get_name()));
    }
}

fn run(args: Vec<std::ffi::OsString>) -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse_from(args);
    match cli.command {
        Command::Init { prefix } => cmd_init(prefix),
        Command::Completion { shell } => {
            write_completion(shell)?;
            Ok(())
        }
        Command::Plan { title } => cmd_create(title, Disposition::Accepted),
        Command::Capture { title } => cmd_create(title, Disposition::Undecided),
        Command::Ready => cmd_ready(),
        Command::Triage => cmd_triage(),
        Command::Claims => cmd_claims(),
        Command::Start { id } => cmd_start(&id),
        Command::Done { id } => cmd_done(&id),
        Command::Write {
            id,
            title,
            message,
            file,
        } => cmd_write(&id, title, message, file),
        Command::Log { id } => cmd_log(&id),
        Command::Release { id, reason } => cmd_release(&id, reason),
        Command::List => cmd_list(),
        Command::Show { id } => cmd_show(&id),
        Command::Decide(c) => cmd_decide(c),
        Command::When(c) => cmd_when(c),
        Command::Dep(c) => cmd_dep(c),
        Command::Group(c) => cmd_group(c),
    }
}

fn cmd_group(c: GroupCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    match c {
        GroupCmd::New { slug, name, parent } => {
            let parent = match parent {
                Some(p) => Some(store.resolve_slug(&p)?),
                None => None,
            };
            let display = if name.is_empty() {
                slug.clone()
            } else {
                name.join(" ")
            };
            let group = Group {
                id: GroupId::generate(),
                slug: slug.clone(),
                name: display.clone(),
                description: None,
                parent,
            };
            store.insert_group(&group)?;
            println!("{slug}  {display}");
        }
        GroupCmd::List => {
            let view = view_of(&store)?;
            if view.groups().is_empty() {
                println!("No groups");
                return Ok(());
            }
            for g in view.groups() {
                let p = view.group_progress(&g.id);
                let waiting = view.group_waiting_on(&g.id);
                let mut extra = Vec::new();
                if p.undecided > 0 {
                    extra.push(format!("Undecided: {}", p.undecided));
                }
                if !waiting.is_empty() {
                    let names: Vec<_> = waiting.iter().map(|w| w.slug.as_str()).collect();
                    extra.push(format!("Waiting on: {}", names.join(", ")));
                }
                let suffix = if extra.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", extra.join(" / "))
                };
                println!("{}  {}/{}{}  {}", g.slug, p.done, p.total, suffix, g.name);
            }
        }
        GroupCmd::Show { slug } => {
            let view = view_of(&store)?;
            let id = store.resolve_slug(&slug)?;
            let g = view.group(&id).ok_or("group not found")?;
            println!("{}  {}", g.slug, g.name);
            if let Some(pid) = &g.parent
                && let Some(parent) = view.group(pid)
            {
                println!("Parent: {}", parent.slug);
            }
            let p = view.group_progress(&id);
            match p.ratio() {
                Some(_) => println!(
                    "Progress: {}/{} (Undecided: {})",
                    p.done, p.total, p.undecided
                ),
                None => println!("Progress: no Accepted issues (Undecided: {})", p.undecided),
            }
            let waiting = view.group_waiting_on(&id);
            for w in waiting {
                println!("Waiting on: {} ({})", w.slug, w.name);
            }
            println!();
            for i in view.issues_in(&id) {
                println!(
                    "  {}  [{}/{}]  {}",
                    i.id,
                    i.progress.label(),
                    i.disposition.label(),
                    i.title
                );
            }
        }
        GroupCmd::Set { id, slug } => {
            let id = store.resolve_id(&id)?;
            let gid = store.resolve_slug(&slug)?;
            store.apply(&id, Change::SetGroup(Some(gid)), &ctx(None))?;
            println!("{id} added to group {slug}");
        }
        GroupCmd::Unset { id } => {
            let id = store.resolve_id(&id)?;
            store.apply(&id, Change::SetGroup(None), &ctx(None))?;
            println!("{id} removed from its group");
        }
        GroupCmd::Reject { slug, reason } => {
            let view = view_of(&store)?;
            let id = store.resolve_slug(&slug)?;
            let targets: Vec<_> = view
                .issues_in(&id)
                .into_iter()
                .filter(|i| i.disposition != Disposition::Rejected)
                .map(|i| i.id.clone())
                .collect();
            let c = ctx(reason);
            let mut changed = 0;
            for t in &targets {
                if store.apply(t, Change::ConvergeDisposition(Disposition::Rejected), &c)?
                    == ApplyOutcome::Changed
                {
                    changed += 1;
                }
            }
            println!("{changed} issues in {slug} set to Rejected");
        }
        GroupCmd::Dep(d) => match d {
            GroupDepCmd::Add { slug, needs } => {
                let a = store.resolve_slug(&slug)?;
                let b = store.resolve_slug(&needs)?;
                store.add_group_dep(&a, &b)?;
                println!("{slug} now depends on {needs}");
            }
            GroupDepCmd::Rm { slug, needs } => {
                let a = store.resolve_slug(&slug)?;
                let b = store.resolve_slug(&needs)?;
                store.remove_group_dep(&a, &b)?;
                println!("{slug} no longer depends on {needs}");
            }
        },
    }
    Ok(())
}

fn cmd_init(prefix: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let (path, prefix) = Store::init(prefix.as_deref())?;
    println!("Initialized axon at {}", path.display());
    println!("Issue IDs will use the form {prefix}-xxxxxx");
    Ok(())
}

fn cmd_create(
    title: Vec<String>,
    disposition: Disposition,
) -> Result<(), Box<dyn std::error::Error>> {
    let title = title.join(" ");
    if title.trim().is_empty() {
        return Err("title must not be empty".into());
    }
    let mut store = Store::open()?;
    let now = Utc::now();
    let issue = Issue {
        id: IssueId::generate(&store.prefix()?),
        title,
        description: None,
        progress: Progress::NotStarted,
        disposition,
        resurface_condition: ResurfaceCondition::Always,
        group: None,
        created_at: now,
        updated_at: now,
    };
    store.insert(&issue)?;
    println!("{}  {}  [{}]", issue.id, issue.title, disposition.label());
    Ok(())
}

fn load() -> Result<(Store, View), Box<dyn std::error::Error>> {
    let store = Store::open()?;
    let view = view_of(&store)?;
    Ok((store, view))
}

fn view_of(store: &Store) -> Result<View, Box<dyn std::error::Error>> {
    Ok(View::new(
        store.all()?,
        store.deps()?,
        store.groups()?,
        store.group_deps()?,
    ))
}

fn cmd_ready() -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load()?;
    print_rows(&render_ready(&view), "No ready issues");
    Ok(())
}

/// 案内を標準出力に混ぜると `axon ready | fzf` でその 1 行が選択肢になる。
fn print_rows(rows: &str, empty_note: &str) {
    if rows.is_empty() {
        eprintln!("{empty_note}");
        return;
    }
    print!("{rows}");
}

fn render_ready(view: &View) -> String {
    view.ready()
        .into_iter()
        .map(|i| format!("{}  {}\n", i.id, i.title))
        .collect()
}

fn cmd_triage() -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load()?;
    print_rows(&render_triage(&view), "No issues need triage");
    Ok(())
}

fn render_triage(view: &View) -> String {
    use derived::TriageReason;
    let mut out = String::new();
    for (issue, reason) in view.triage() {
        match reason {
            TriageReason::Undecided => {
                out.push_str(&format!("{}  Undecided  {}\n", issue.id, issue.title));
            }
            TriageReason::Orphaned => {
                let lost: Vec<_> = view
                    .depends_on(&issue.id)
                    .into_iter()
                    .filter(|d| d.disposition == Disposition::Rejected)
                    .map(|d| d.id.to_string())
                    .collect();
                out.push_str(&format!(
                    "{}  Orphaned   {} <- {} is Rejected\n",
                    issue.id,
                    issue.title,
                    lost.join(", ")
                ));
            }
        }
    }
    out
}

fn cmd_claims() -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load()?;
    print_rows(&render_claims(&view), "No active claims");
    Ok(())
}

fn render_claims(view: &View) -> String {
    view.claims()
        .into_iter()
        .map(|(issue, claim)| {
            format!(
                "{}  {}  Claim: {}\n",
                issue.id,
                issue.title,
                claim_details(claim)
            )
        })
        .collect()
}

fn claim_details(claim: &Claim) -> String {
    format!(
        "{}  Worktree: {}  Started: {}",
        claim.actor,
        claim.worktree,
        claim.at.to_rfc3339_opts(SecondsFormat::Secs, true)
    )
}

fn cmd_start(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let id = store.resolve_id(id)?;
    let claim = Claim {
        actor: actor::actor(),
        worktree: actor::worktree()?,
        at: Utc::now(),
    };
    store.apply(&id, Change::Claim(claim.clone()), &ctx(None))?;
    let issue = store.get(&id)?;
    println!("Started {id} ({})", claim.actor);
    println!("{}", issue.title);
    Ok(())
}

fn ctx(reason: Option<String>) -> Ctx {
    Ctx {
        actor: actor::actor(),
        reason,
    }
}

fn cmd_done(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let id = store.resolve_id(id)?;
    store.apply(&id, Change::End, &ctx(None))?;
    println!("Ended {id}");
    Ok(())
}

fn cmd_list() -> Result<(), Box<dyn std::error::Error>> {
    let (_, view) = load()?;
    print_rows(&render_list(&view), "No issues");
    Ok(())
}

fn render_list(view: &View) -> String {
    let mut out = String::new();
    for i in view.iter() {
        let mut marks = Vec::new();
        if view.is_orphaned(&i.id) {
            marks.push("orphaned".to_string());
        } else if view.is_blocked(&i.id) {
            marks.push("blocked".to_string());
        }
        if !view.is_surfaced(i) {
            marks.push(match &i.resurface_condition {
                ResurfaceCondition::AtDate(d) => format!("AtDate({d})"),
                ResurfaceCondition::AfterIssue(r) => format!("AfterIssue({r})"),
                ResurfaceCondition::Always => String::new(),
            });
        }
        let mark = if marks.is_empty() {
            String::new()
        } else {
            format!(" {}", marks.join(" "))
        };
        out.push_str(&format!(
            "{}  [{}/{}]{}  {}\n",
            i.id,
            i.progress.label(),
            i.disposition.label(),
            mark,
            i.title
        ));
    }
    out
}

fn cmd_show(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let db::ShowSnapshot {
        id,
        issues,
        deps,
        groups,
        group_deps,
        progress_events,
    } = store.show_snapshot(id)?;
    let view = View::new(issues, deps, groups, group_deps);
    let issue = view.get(&id).ok_or("issue not found")?;
    print!("{}", render_show(&view, issue, &progress_events));
    Ok(())
}

/// show の本文。該当が無い区分で空の見出しを出さないため、行の無いブロックは落とす。
fn render_show(view: &View, issue: &Issue, progress_events: &[db::ProgressEvent]) -> String {
    let mut blocks: Vec<Vec<String>> = Vec::new();

    let when = match &issue.resurface_condition {
        ResurfaceCondition::Always => "Always".to_string(),
        ResurfaceCondition::AtDate(d) => format!("AtDate({d})"),
        ResurfaceCondition::AfterIssue(r) => format!("AfterIssue({r})"),
    };
    let mut head = vec![
        format!("{}  {}", issue.id, issue.title),
        format!(
            "Progress: {}  Disposition: {}  Resurface condition: {}",
            issue.progress.label(),
            issue.disposition.label(),
            when
        ),
    ];
    let group = issue.group.as_ref().and_then(|g| view.group(g));
    if let Some(g) = group {
        head.push(if g.name == g.slug {
            format!("Group: {}", g.slug)
        } else {
            format!("Group: {}  {}", g.slug, g.name)
        });
    }
    if let Some(c) = issue.progress.claim() {
        head.push(format!("Claim: {}", claim_details(c)));
    }
    blocks.push(head);

    if let Some(d) = &issue.description {
        blocks.push(vec![d.clone()]);
    }

    let mut progress = Vec::new();
    if !progress_events.is_empty() {
        progress.push("Progress history:".to_string());
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
            progress.push(format!(
                "  {}  {}  {action}{reason}",
                event.at.format("%Y-%m-%d %H:%M"),
                event.actor
            ));
        }
    }
    blocks.push(progress);

    let deps = view.depends_on(&issue.id);
    let waiting: Vec<&Issue> = deps.iter().copied().filter(|d| !d.is_terminal()).collect();
    let mut relations = Vec::new();
    for d in &waiting {
        relations.push(format!(
            "Dependency: {}  {}{}",
            d.id,
            d.title,
            blocker_note(view, d)
        ));
    }
    for d in deps
        .iter()
        .filter(|d| d.is_terminal() && d.disposition != Disposition::Rejected)
    {
        relations.push(format!("Satisfied dependency: {}  {}", d.id, d.title));
    }
    for d in deps
        .iter()
        .filter(|d| d.disposition == Disposition::Rejected)
    {
        relations.push(format!("Orphaned: {} is Rejected", d.id));
    }

    // 直接の依存より奥に原因があるときだけ、遡った結果を出す
    let direct: Vec<&IssueId> = waiting.iter().map(|d| &d.id).collect();
    for c in view
        .blocking_causes(&issue.id)
        .iter()
        .filter(|c| !direct.contains(&&c.id))
    {
        relations.push(format!(
            "Root cause: {}  {}{}",
            c.id,
            c.title,
            blocker_note(view, c)
        ));
    }

    for d in view.dependents(&issue.id) {
        relations.push(format!(
            "Dependent: {}  {}{}",
            d.id,
            d.title,
            dependent_note(d)
        ));
    }
    blocks.push(relations);

    for (g, blockers) in view.group_blocking_causes(issue) {
        let mut block = vec![format!(
            "Group {} depends on {}",
            group.map(|x| x.slug.as_str()).unwrap_or("?"),
            g.slug
        )];
        for b in blockers {
            block.push(format!(
                "  {}  [{}/{}]  {}",
                b.id,
                b.progress.label(),
                b.disposition.label(),
                b.title
            ));
        }
        blocks.push(block);
    }

    let body = blocks
        .into_iter()
        .filter(|b| !b.is_empty())
        .map(|b| b.join("\n"))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{body}\n")
}

/// 後続がすでに終端なら、これを終わらせても動き出さないことを添える。
fn dependent_note(issue: &Issue) -> &'static str {
    if issue.disposition == Disposition::Rejected {
        " <- Rejected"
    } else if matches!(issue.progress, Progress::Ended) {
        " <- Ended"
    } else {
        ""
    }
}

/// 止まっている理由のうち、状態表示だけでは読み取れないものを添える。
fn blocker_note(view: &View, issue: &Issue) -> String {
    if view.is_orphaned(&issue.id) {
        " <- orphaned".to_string()
    } else if !view.is_surfaced(issue) {
        match &issue.resurface_condition {
            ResurfaceCondition::AtDate(d) => format!(" <- not surfaced until {d}"),
            ResurfaceCondition::AfterIssue(r) => {
                format!(" <- not surfaced until {r} is terminal")
            }
            ResurfaceCondition::Always => String::new(),
        }
    } else {
        String::new()
    }
}

fn cmd_decide(c: DecideCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let (raw, disposition, reason) = match c {
        DecideCmd::Accept { id, reason } => (id, Disposition::Accepted, reason),
        DecideCmd::Reject { id, reason } => (id, Disposition::Rejected, reason),
        DecideCmd::Undecide { id, reason } => (id, Disposition::Undecided, reason),
    };
    let id = store.resolve_id(&raw)?;
    store.apply(&id, Change::Decide(disposition), &ctx(reason))?;
    println!("{id} Disposition set to {}", disposition.label());

    // 着手中のまま不採用にすると「作業は止まっているのに着手中」が残るため促す。
    // 進行と採否は別の軸なので、こちらでは終了させない。
    if disposition == Disposition::Rejected && store.get(&id)?.progress.claim().is_some() {
        println!("The issue remains InProgress; if work has stopped, also run `axon done {id}`");
    }
    Ok(())
}

fn cmd_when(c: WhenCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    match c {
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
            let target = store.resolve_id(&reference)?;
            store.apply(
                &id,
                Change::SetResurfaceCondition(ResurfaceCondition::AfterIssue(target.clone())),
                &ctx(reason),
            )?;
            println!("{id} resurface condition set to AfterIssue({target})");
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

fn cmd_dep(c: DepCmd) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    match c {
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

fn cmd_log(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open()?;
    let id = store.resolve_id(id)?;
    let events = store.events(&id)?;
    if events.is_empty() {
        println!("No decision history");
        return Ok(());
    }
    for e in events {
        print!(
            "{}  {}  {}",
            e.at.format("%Y-%m-%d %H:%M"),
            e.actor,
            format_decision(&e)
        );
        match e.reason {
            Some(r) => println!("  ({r})"),
            None => println!(),
        }
    }
    Ok(())
}

/// DB values are stable lowercase tokens; output uses the public glossary.
fn format_decision(e: &db::Event) -> String {
    let label = |v: &Option<String>| -> String {
        match v.as_deref() {
            Some("undecided") => "Undecided".to_string(),
            Some("accepted") => "Accepted".to_string(),
            Some("rejected") => "Rejected".to_string(),
            Some(other) => other.to_string(),
            None => "Always".to_string(),
        }
    };
    match e.field.as_str() {
        "disposition" => format!(
            "Disposition: {} -> {}",
            label(&e.old_value),
            label(&e.new_value)
        ),
        "resurface_condition" => format!(
            "Resurface condition: {} -> {}",
            label(&e.old_value),
            label(&e.new_value)
        ),
        other => format!(
            "{other}: {} -> {}",
            label(&e.old_value),
            label(&e.new_value)
        ),
    }
}

fn cmd_release(id: &str, reason: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open()?;
    let id = store.resolve_id(id)?;
    store.apply(&id, Change::Release, &ctx(reason))?;
    println!("Released {id}");
    Ok(())
}

fn cmd_write(
    id: &str,
    title: Option<String>,
    message: Option<String>,
    file: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;

    let mut store = Store::open()?;
    let id = store.resolve_id(id)?;
    let current = store.get(&id)?;

    // 書き込みの前に入力を出し切る。ファイルが読めずに
    // 表題だけ書き換わった状態を残さないため。
    let body = match (message, file) {
        (Some(m), None) => Some(m),
        (None, Some(f)) if f == "-" => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            Some(buf)
        }
        (None, Some(f)) => Some(std::fs::read_to_string(&f)?),
        (Some(_), Some(_)) => return Err("-m and -F cannot be used together".into()),
        (None, None) => None,
    };

    if title.is_none() && body.is_none() {
        return Err("nothing to write; provide --title, -m, or -F".into());
    }

    let mut written = Vec::new();

    if let Some(t) = title {
        let t = t.trim();
        if t.is_empty() {
            return Err("title must not be empty".into());
        }
        if t != current.title {
            store.apply(&id, Change::SetTitle(t.to_string()), &ctx(None))?;
            written.push("title updated");
        }
    }

    if let Some(text) = body {
        let trimmed = text.trim();
        let new = (!trimmed.is_empty()).then(|| trimmed.to_string());
        if new != current.description {
            let removed = new.is_none();
            store.apply(&id, Change::SetDescription(new), &ctx(None))?;
            written.push(if removed {
                "description removed"
            } else {
                "description updated"
            });
        }
    }

    if written.is_empty() {
        println!("No changes");
    }
    for w in written {
        println!("{id}: {w}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf_paths(command: &mut clap::Command, path: &str, out: &mut Vec<String>) {
        let has_generated_help = !command.is_disable_help_subcommand_set();
        let has_visible_children = command.get_subcommands().any(|child| {
            !child.is_hide_set() && !(has_generated_help && child.get_name() == "help")
        });

        if !has_visible_children {
            out.push(path.to_string());
            return;
        }

        for child in command.get_subcommands_mut() {
            if child.is_hide_set() || (has_generated_help && child.get_name() == "help") {
                continue;
            }
            let child_path = format!("{path} {}", child.get_name());
            leaf_paths(child, &child_path, out);
        }
    }

    fn iid(id: &str) -> IssueId {
        IssueId::from_stored(id)
    }

    fn issue(id: &str, progress: Progress, disposition: Disposition) -> Issue {
        let now = Utc::now();
        Issue {
            id: iid(id),
            title: format!("{id} の作業"),
            description: None,
            progress,
            disposition,
            resurface_condition: ResurfaceCondition::Always,
            group: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn accepted(id: &str) -> Issue {
        issue(id, Progress::NotStarted, Disposition::Accepted)
    }

    fn done(id: &str) -> Issue {
        issue(id, Progress::Ended, Disposition::Accepted)
    }

    fn claimed(id: &str, actor: &str, worktree: &str, at: &str) -> Issue {
        let claim = Claim {
            actor: actor.to_string(),
            worktree: worktree.to_string(),
            at: chrono::DateTime::parse_from_rfc3339(at)
                .unwrap()
                .with_timezone(&Utc),
        };
        issue(id, Progress::InProgress(claim), Disposition::Accepted)
    }

    fn rejected(id: &str) -> Issue {
        issue(id, Progress::NotStarted, Disposition::Rejected)
    }

    fn group(slug: &str, name: &str) -> Group {
        Group {
            id: GroupId::from_stored(slug),
            slug: slug.to_string(),
            name: name.to_string(),
            description: None,
            parent: None,
        }
    }

    fn view(issues: Vec<Issue>, deps: &[(&str, &str)], groups: Vec<Group>) -> View {
        View::new(
            issues,
            deps.iter().map(|(a, b)| (iid(a), iid(b))).collect(),
            groups,
            Vec::new(),
        )
    }

    fn show(view: &View, id: &str) -> String {
        render_show(view, view.get(&iid(id)).unwrap(), &[])
    }

    fn undecided(id: &str) -> Issue {
        issue(id, Progress::NotStarted, Disposition::Undecided)
    }

    fn waiting_until(id: &str, resurface_condition: ResurfaceCondition) -> Issue {
        let mut i = accepted(id);
        i.resurface_condition = resurface_condition;
        i
    }

    /// 一覧の各行から、空白区切りの 1 列目を取る。
    ///
    /// fzf 連携 (`axon ready | fzf --preview 'axon show {1}'`) は id をこの位置から取る。
    /// list は状態次第で列 (待ち / 前提喪失 / 浮上日) が増減するので、id を後ろに置くと
    /// 行ごとに位置がずれて preview が id を拾えなくなる。1 列目に固定しておく。
    fn first_columns(out: &str) -> Vec<&str> {
        out.lines()
            .map(|l| l.split_whitespace().next().unwrap_or(""))
            .collect()
    }

    #[test]
    fn show_without_relations_is_state_only() {
        let v = view(vec![accepted("a")], &[], Vec::new());
        assert_eq!(
            show(&v, "a"),
            "a  a の作業\nProgress: NotStarted  Disposition: Accepted  Resurface condition: Always\n"
        );
    }

    #[test]
    fn claims_and_show_report_the_same_claim_details() {
        let v = view(
            vec![
                accepted("not-started"),
                claimed("active", "codex", "/repo/worktree", "2026-09-01T01:23:45Z"),
            ],
            &[],
            Vec::new(),
        );
        let details = "Claim: codex  Worktree: /repo/worktree  Started: 2026-09-01T01:23:45Z";
        let claims = render_claims(&v);

        assert_eq!(claims, format!("active  active の作業  {details}\n"));
        assert_eq!(first_columns(&claims), ["active"]);
        assert!(show(&v, "active").contains(details));
    }

    #[test]
    fn show_lists_typed_progress_history() {
        let v = view(vec![done("a")], &[], Vec::new());
        let at = chrono::DateTime::parse_from_rfc3339("2026-09-01T01:23:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let events = vec![
            db::ProgressEvent {
                kind: db::ProgressEventKind::Start,
                actor: "codex".to_string(),
                reason: None,
                at,
            },
            db::ProgressEvent {
                kind: db::ProgressEventKind::Done,
                actor: "codex".to_string(),
                reason: Some("テストまで完了".to_string()),
                at,
            },
        ];

        let out = render_show(&v, v.get(&iid("a")).unwrap(), &events);
        assert!(
            out.contains(
                "Progress history:\n  2026-09-01 01:23  codex  Started\n  2026-09-01 01:23  codex  Ended  (テストまで完了)"
            ),
            "{out}"
        );
    }

    #[test]
    fn show_lists_resolved_dependencies() {
        let v = view(
            vec![accepted("a"), done("b"), accepted("c")],
            &[("a", "b"), ("a", "c")],
            Vec::new(),
        );
        let out = show(&v, "a");
        assert!(out.contains("Dependency: c  c の作業"), "{out}");
        assert!(out.contains("Satisfied dependency: b  b の作業"), "{out}");
    }

    #[test]
    fn rejected_dependency_is_not_listed_as_resolved() {
        let v = view(
            vec![
                accepted("a"),
                issue("b", Progress::Ended, Disposition::Rejected),
            ],
            &[("a", "b")],
            Vec::new(),
        );
        let out = show(&v, "a");
        assert!(out.contains("Orphaned: b is Rejected"), "{out}");
        assert!(!out.contains("Satisfied dependency:"), "{out}");
    }

    #[test]
    fn show_lists_dependents() {
        let v = view(
            vec![accepted("a"), accepted("b"), done("c"), rejected("d")],
            &[("b", "a"), ("c", "a"), ("d", "a")],
            Vec::new(),
        );
        let out = show(&v, "a");
        assert!(out.contains("Dependent: b  b の作業\n"), "{out}");
        assert!(out.contains("Dependent: c  c の作業 <- Ended"), "{out}");
        assert!(out.contains("Dependent: d  d の作業 <- Rejected"), "{out}");
    }

    #[test]
    fn show_lists_group() {
        let mut i = accepted("a");
        i.group = Some(GroupId::from_stored("cli"));
        let v = view(vec![i], &[], vec![group("cli", "CLI")]);
        assert!(
            show(&v, "a").contains("Group: cli  CLI"),
            "{}",
            show(&v, "a")
        );
    }

    #[test]
    fn ready_puts_id_in_the_first_column() {
        let v = view(vec![accepted("a"), accepted("b")], &[], Vec::new());
        assert_eq!(first_columns(&render_ready(&v)), ["a", "b"]);
    }

    #[test]
    fn triage_puts_id_in_the_first_column() {
        let v = view(
            vec![undecided("a"), accepted("b"), rejected("c")],
            &[("b", "c")],
            Vec::new(),
        );
        assert_eq!(first_columns(&render_triage(&v)), ["a", "b"]);
    }

    #[test]
    fn list_puts_id_in_the_first_column() {
        let v = view(
            vec![
                accepted("a"),
                accepted("blocked"),
                accepted("orphaned"),
                waiting_until("dated", ResurfaceCondition::AtDate(NaiveDate::MAX)),
                waiting_until("after", ResurfaceCondition::AfterIssue(iid("blocked"))),
                accepted("dep"),
                rejected("gone"),
            ],
            &[("blocked", "dep"), ("orphaned", "gone")],
            Vec::new(),
        );
        assert_eq!(
            first_columns(&render_list(&v)),
            ["a", "blocked", "orphaned", "dated", "after", "dep", "gone"]
        );
    }

    #[test]
    fn group_line_omits_name_equal_to_slug() {
        let mut i = accepted("a");
        i.group = Some(GroupId::from_stored("cli"));
        let v = view(vec![i], &[], vec![group("cli", "cli")]);
        assert!(show(&v, "a").contains("Group: cli\n"), "{}", show(&v, "a"));
    }

    #[test]
    fn complete_help_embeds_the_cli_guide_verbatim() {
        assert!(render_complete_help().starts_with(CLI_GUIDE.trim_end()));
    }

    #[test]
    fn complete_help_contains_every_visible_leaf_once() {
        let mut command = Cli::command();
        command.build();
        let mut paths = Vec::new();
        leaf_paths(&mut command, "axon", &mut paths);

        let help = render_complete_help();
        for path in &paths {
            let heading = format!("## `{path}`\n");
            assert_eq!(help.matches(&heading).count(), 1, "{path}");
        }
        assert_eq!(help.matches("## `axon ").count(), paths.len());
    }

    #[test]
    fn every_visible_leaf_argument_has_help() {
        fn check(command: &mut clap::Command, path: &str) {
            let has_generated_help = !command.is_disable_help_subcommand_set();
            let has_visible_children = command.get_subcommands().any(|child| {
                !child.is_hide_set() && !(has_generated_help && child.get_name() == "help")
            });

            if !has_visible_children {
                for argument in command
                    .get_arguments()
                    .filter(|argument| !argument.is_hide_set())
                {
                    let help = argument
                        .get_help()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    assert!(
                        !help.trim().is_empty(),
                        "{path}: argument {:?} has no help",
                        argument.get_id()
                    );
                }
                return;
            }

            for child in command.get_subcommands_mut() {
                if child.is_hide_set() || (has_generated_help && child.get_name() == "help") {
                    continue;
                }
                check(child, &format!("{path} {}", child.get_name()));
            }
        }

        let mut command = Cli::command();
        command.build();
        check(&mut command, "axon");
    }

    #[test]
    fn only_root_long_help_and_bare_help_request_the_complete_reference() {
        let args = |values: &[&str]| {
            values
                .iter()
                .map(std::ffi::OsString::from)
                .collect::<Vec<_>>()
        };

        assert!(requests_complete_help(&args(&["axon", "--help"])));
        assert!(requests_complete_help(&args(&["axon", "help"])));
        assert!(!requests_complete_help(&args(&["axon", "-h"])));
        assert!(!requests_complete_help(&args(&["axon", "help", "all"])));
        assert!(!requests_complete_help(&args(&[
            "axon", "decide", "reject", "--help"
        ])));
        assert!(!requests_complete_help(&args(&[
            "axon", "help", "decide", "reject"
        ])));
    }
}
