use super::{args::Cli, display};
use axon::{
    Result,
    lifecycle::{Context, EntityId, Kind, Note, Snapshot},
    location::{Initialized, Location},
    read,
};
use clap::CommandFactory;

pub(super) fn status_label(status: read::Status) -> &'static str {
    match status {
        read::Status::Undecided => "Undecided",
        read::Status::Ready => "Ready",
        read::Status::Blocked => "Blocked",
        read::Status::Unsurfaced => "Unsurfaced",
        read::Status::InProgressBlocked => "InProgress+Blocked",
        read::Status::InProgress => "InProgress",
        read::Status::Completed => "Completed",
        read::Status::Cancelled => "Cancelled",
        read::Status::Empty => "Empty",
        read::Status::Confirmable => "Confirmable",
    }
}
pub(super) fn row(value: &read::Row<'_>) -> String {
    let entity = value.entity;
    format!(
        "{}  {}  {}  {}\n",
        display::identity(&entity.id),
        display::muted(format!("{:?}", entity.kind)),
        display::situation(status_label(value.status)),
        display::line(&entity.current.title)
    )
}
pub(super) fn show(value: &read::Detail<'_>, details: bool) -> String {
    let entity = value.row.entity;
    let mut out = row(&value.row);
    let notes = value.note_count;
    if notes > 0 {
        out.push_str(&format!("{notes} notes\n"));
    }
    // The parent line is omitted when the wait section below already names the parent.
    let parent_named_below = !details
        && value.parent.is_some_and(|parent| {
            let named = |rows: &[read::Row<'_>]| rows.iter().any(|r| r.entity.id == parent.id);
            value
                .prerequisites
                .as_ref()
                .is_some_and(|p| named(&p.ancestors) || named(&p.unsurfaced_ancestors))
                || value.stall.as_ref().is_some_and(|s| {
                    named(&s.undecided_ancestors) || named(&s.unsurfaced_ancestors)
                })
        });
    if let Some(parent) = value.parent.filter(|_| !parent_named_below) {
        out.push_str(&format!(
            "Parent: {}  {}\n",
            display::identity(&parent.id),
            display::line(&parent.current.title)
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
        for ancestor in &prerequisites.ancestors {
            out.push_str(&format!("Ancestor must be adopted: {}", row(ancestor)));
        }
        for dependency in &prerequisites.dependencies {
            out.push_str(&format!("Dependency must complete: {}", row(dependency)));
        }
        for dependency in &prerequisites.ancestor_dependencies {
            out.push_str(&format!(
                "Ancestor dependency must complete: {}",
                row(dependency)
            ));
        }
        for ancestor in &prerequisites.unsurfaced_ancestors {
            out.push_str(&format!("Unsurfaced ancestor: {}", row(ancestor)));
        }
    }
    if !details && let Some(stall) = &value.stall {
        out.push_str(&format!("\n{}\n", display::heading("Stalled")));
        for dependency in &stall.dependencies {
            out.push_str(&format!("Dependency must complete: {}", row(dependency)));
        }
        for dependency in &stall.ancestor_dependencies {
            out.push_str(&format!(
                "Ancestor dependency must complete: {}",
                row(dependency)
            ));
        }
        for (descendant, dependency) in &stall.descendant_dependencies {
            let descendant = row(descendant);
            out.push_str(&format!(
                "Descendant dependency must complete: {}  needs  {}",
                descendant.strip_suffix('\n').unwrap_or(&descendant),
                row(dependency)
            ));
        }
        for child in &stall.undecided_children {
            out.push_str(&format!("Undecided child: {}", row(child)));
        }
        for subgroup in &stall.open_subgroups {
            out.push_str(&format!("Open subgroup: {}", row(subgroup)));
        }
        for candidate in &stall.unsurfaced_candidates {
            out.push_str(&format!("Unsurfaced candidate: {}", row(candidate)));
        }
        for group in &stall.own_condition_unsatisfied {
            out.push_str(&format!("Own condition unsatisfied: {}", row(group)));
        }
        for ancestor in &stall.undecided_ancestors {
            out.push_str(&format!("Undecided ancestor: {}", row(ancestor)));
        }
        for ancestor in &stall.unsurfaced_ancestors {
            out.push_str(&format!("Unsurfaced ancestor: {}", row(ancestor)));
        }
    }
    if details {
        let stored = format!("{:?}", entity.current.lifecycle);
        let effective = format!("{:?}", value.effective);
        if entity.kind == Kind::Group {
            out.push_str(&format!(
                "{} {effective}{}\n",
                display::muted("Lifecycle:"),
                if effective == stored {
                    String::new()
                } else {
                    format!(" (stored {stored})")
                }
            ));
        } else if status_label(value.row.status) != stored {
            out.push_str(&format!("{} {stored}\n", display::muted("Lifecycle:")));
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
                .map(display::line)
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
    out.push_str(&display::block(&entity.current.description));
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
pub(super) fn actor(context: &Context) -> String {
    context
        .recorder
        .as_ref()
        .map(|r| display::line(&r.actor))
        .unwrap_or_else(|| "—".into())
}
pub(super) fn recorder_display(context: &Context, details: bool) -> String {
    let mut text = actor(context);
    if details && let Some(recorder) = &context.recorder {
        text.push_str("  data: ");
        text.push_str(&display::human_text(
            serde_json::to_string(&recorder.data).expect("JSON object"),
        ));
    }
    text
}
pub(super) fn branch_boundary(concurrent: bool, text: &mut String) {
    if concurrent {
        text.push_str("Concurrent branch (not ordered after the preceding record)\n");
    }
}
/// Axon leaves how Git treats the store to the user; each block reaches one of the two ways.
pub(super) fn init_output(location: &Location, initialized: &Initialized) -> String {
    let mut text = format!(
        "Initialized {}\n",
        display::human_text(location.root.join(".axon/state.jsonl").display())
    );
    if initialized.linked_worktree {
        text.push_str(
            "This store belongs to this linked worktree; other worktrees do not see it.\n",
        );
    }
    if !initialized.git {
        return text;
    }
    text.push_str(&format!(
        "
Unless an ignore rule of yours already covers it, Git sees .axon as untracked
and git add -A would commit the store. Check which applies with:
       git check-ignore -v .axon/state.jsonl
Choose one way to use the store; do not mix the two in one repository. Run the
steps in
{}:

To keep the store out of Git, ignore the directory by adding this line to the
file that git rev-parse --git-path info/exclude names (this repository only) or
to your global Git ignore file:
       .axon/
Linked worktrees without a .axon of their own use the main worktree's store.
Git overwrites an ignored store without warning when you check out or merge a
commit that tracks .axon/state.jsonl.

To track the store in Git and merge it between branches instead, first remove
any ignore rule outside .axon that covers the directory, then:
  1. Create .axon/.gitignore with these lines:
       *
       !.gitignore
       !state.jsonl
  2. Add this line to .gitattributes in the repository root:
       /.axon/state.jsonl merge=axon
  3. Register the merge driver, using the absolute path of the axon binary:
       git config merge.axon.driver \"'/absolute/path/to/axon' merge driver %O %A %B\"
  4. Stage and commit the files:
       git add .axon/state.jsonl .axon/.gitignore .gitattributes
       git commit -m \"Track the Axon store\"
",
        display::human_text(location.root.display())
    ));
    text
}
pub(super) fn declaration_ids(ids: &[(String, String)]) -> String {
    ids.iter()
        .map(|(key, id)| format!("{key} -> {}\n", display::line(id)))
        .collect()
}

pub(super) fn declaration_changes(
    declaration: &axon::declaration::Declaration,
    before: &Snapshot,
    after: &Snapshot,
    applied: bool,
) -> Result<String> {
    let mut out = String::new();
    if applied {
        out.push_str("Already applied.\n");
    }
    let view = read::View::new(after);
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
                        format!("{} -> {}", display::line(&a.title), display::line(&b.title))
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
            status_label(view.status(entity))
        ));
    }
    if declaration.records().next().is_none() {
        out.push_str("No changes.\n");
    }
    out.push_str("Check passed. Storage and declaration unchanged.\n");
    Ok(out)
}
pub fn list_row(value: &read::Row<'_>, searched: bool) -> String {
    let mut text = row(value);
    if searched {
        text.push_str(&format!(
            "  {} {}\n",
            display::muted("Matched:"),
            value
                .matches
                .iter()
                .map(|location| match location {
                    read::MatchLocation::Title => "Title",
                    read::MatchLocation::Description => "Description",
                })
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    text
}
/// `listed` Notes share the output with other records, so their bodies are indented.
/// A single requested Note prints its body as stored.
pub fn format_note(note: &Note, details: bool, listed: bool) -> String {
    format!(
        "{}  {}  {}\n{}\n\n",
        display::identity(&note.id),
        display::muted(display::timestamp(&note.context.at)),
        recorder_display(&note.context, details),
        if listed {
            display::block(&note.body)
        } else {
            display::human_text(&note.body)
        }
    )
}
pub fn confirmation(id: &EntityId, effect: &str) -> String {
    let escaped = display::line(effect);
    let effect = escaped.as_str();
    let effect = if effect.starts_with("No changes") || effect == "Cancelled" {
        display::muted(effect)
    } else if effect.starts_with("Started") {
        display::situation(effect)
    } else {
        display::positive(effect)
    };
    format!("{}  {effect}\n", display::identity(id))
}
pub fn render_root_help() -> String {
    let mut command = Cli::command();
    command.build();
    let sections: &[(&str, &[&str])] = &[
        ("Registration", &["capture"]),
        (
            "Lifecycle transitions",
            &[
                "accept",
                "withdraw",
                "start",
                "release",
                "complete",
                "cancel",
                "reconsider",
                "reopen",
            ],
        ),
        (
            "Candidates & inspection",
            &[
                "proposals",
                "tasks",
                "show",
                "list",
                "log",
                "note",
                "actor",
                "export",
            ],
        ),
        (
            "Text & relationships",
            &["write", "parent", "dep", "condition", "import"],
        ),
        (
            "Setup & utilities",
            &["init", "storage", "merge", "completion", "docs", "help"],
        ),
    ];
    let mut text = format!(
        "A local issue tracker for Issues and Groups\n\n{} axon <COMMAND>\n",
        display::heading("Usage:")
    );
    for (heading, names) in sections {
        text.push_str(&format!("\n{}\n", display::heading(format!("{heading}:"))));
        for name in *names {
            let child = command.find_subcommand(name).expect("help command exists");
            let about = child
                .get_about()
                .map(ToString::to_string)
                .unwrap_or_default();
            text.push_str(&format!(
                "  {}{:padding$}  {about}\n",
                display::heading(name),
                "",
                padding = 10 - name.len()
            ));
        }
    }
    text.push_str(&format!("\n{}\n  axon help <COMMAND PATH>  Show detailed command help\n  axon docs                 Explain the lifecycle and daily workflow\n\n{}\n  -h, --help     Print help\n  -V, --version  Print version\n", display::heading("More help:"), display::heading("Options:")));
    text
}

pub fn search_notes(matches: &[read::NoteMatch<'_>]) -> String {
    let mut text = String::new();
    for found in matches {
        let note = found.note;
        text.push_str(&format!(
            "{}  {}  {}  {} {}\n",
            display::identity(&note.entity),
            display::identity(&note.id),
            display::muted(display::timestamp(&note.context.at)),
            display::muted("Excerpt:"),
            note_excerpt(&note.body, found.range.start, found.range.len())
        ));
    }
    text
}

pub(super) fn note_excerpt(body: &str, position: usize, query_len: usize) -> String {
    const CONTEXT: usize = 24;
    let start = body[..position]
        .char_indices()
        .rev()
        .nth(CONTEXT - 1)
        .map_or(0, |(index, _)| index);
    let end_match = position + query_len;
    let end = body[end_match..]
        .char_indices()
        .nth(CONTEXT)
        .map_or(body.len(), |(index, _)| end_match + index);
    let mut text = String::new();
    if start > 0 {
        text.push('…');
    }
    for character in body[start..end].chars() {
        match character {
            '\\' => text.push_str("\\\\"),
            '\n' => text.push_str("\\n"),
            '\u{061c}'
            | '\u{200e}'..='\u{200f}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}' => {
                text.extend(character.escape_unicode());
            }
            _ => text.push_str(&display::human_text(character)),
        }
    }
    if end < body.len() {
        text.push('…');
    }
    text
}
