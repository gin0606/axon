use super::{args::Cli, display};
use axon::{
    Result,
    lifecycle::{Context, EntityId, Note, Snapshot},
    location::{IntegrationChange, IntegrationFile, Location},
    read,
};
use clap::CommandFactory;

pub(super) fn status_label(status: read::Status) -> &'static str {
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
    let parent_wait = !details
        && value
            .prerequisites
            .as_ref()
            .is_some_and(|p| p.parent.is_some());
    if let Some(parent) = value.parent.filter(|_| !parent_wait) {
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
pub(super) fn file_init_output(location: &Location, files: &[IntegrationFile]) -> String {
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
            status_label(read::status(after, entity))
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
