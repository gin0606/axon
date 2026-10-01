use super::{args::Cli, display};
use axon::{
    Result,
    lifecycle::{
        Context, EntityId, Kind, Recorder,
        record::{Current, Note, RecordId, RecordKind},
    },
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
        read::Status::Conflicted => "Conflicted",
    }
}
/// The situation column of a row without decoration: the status, `+Invalid` when the Entity
/// is in a violation.
pub(super) fn situation_label(value: &read::Row<'_>) -> String {
    let label = status_label(value.status);
    if value.invalid {
        format!("{label}+Invalid")
    } else {
        label.into()
    }
}
fn situation(value: &read::Row<'_>) -> String {
    display::situation(&situation_label(value))
}
pub(super) fn row(value: &read::Row<'_>) -> String {
    format!(
        "{}  {}  {}  {}  {}\n",
        display::identity(value.id),
        display::muted(format!("{:?}", value.kind)),
        situation(value),
        value.label,
        display::line(value.title)
    )
}
/// The row of an Entity named by a relation, or its bare ID when it does not exist.
pub(super) fn row_of(related: &read::Related<'_>) -> String {
    match &related.row {
        Some(row) => self::row(row),
        None => format!(
            "{}  {}\n",
            display::identity(&related.id),
            display::muted("(missing)")
        ),
    }
}
fn record_label(record: &axon::lifecycle::record::Record) -> String {
    match &record.kind {
        RecordKind::Transition(operation) => format!("{operation:?}"),
        kind => kind.name().to_string(),
    }
}
/// One head of a conflicted Entity, as `show` and `resolve` list it.
pub(super) fn head(head: &read::Head<'_>) -> String {
    let after = &head.record.after;
    format!(
        "{}  {}  {}  {}  {}  {}  {}  {}{}\n",
        display::identity(head.id),
        display::muted(display::timestamp(&head.record.at)),
        actor_of(head.record.recorder.as_ref()),
        display::muted(record_label(head.record)),
        display::situation(&format!("{:?}", after.lifecycle)),
        display::muted(format!("{:?}", after.kind)),
        after.label,
        display::line(&after.title),
        if head.likely_newer {
            display::muted("  parent missing; likely newer")
        } else {
            String::new()
        }
    )
}
/// The listing of `axon resolve`: each conflicted Entity's row followed by its heads, the
/// Entities in the given (creation) order and separated by a blank line.
pub(super) fn conflicts(view: &read::View<'_>, ids: &[&EntityId]) -> String {
    let mut out = String::new();
    for (index, id) in ids.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(&row(&view.row(id, None)));
        for entry in view.heads(id) {
            out.push_str(&head(&entry));
        }
    }
    out
}
pub(super) fn show(value: &read::Detail<'_>, details: bool) -> String {
    let mut out = row(&value.row);
    let notes = value.note_count;
    if notes > 0 {
        out.push_str(&format!("{notes} notes\n"));
    }
    // The parent line is omitted when the wait section below already names the parent.
    let parent_named_below = !details
        && value.parent.as_ref().is_some_and(|parent| {
            let named = |rows: &[read::Row<'_>]| rows.iter().any(|r| *r.id == parent.id);
            let related = |items: &[read::Related<'_>]| items.iter().any(|r| r.id == parent.id);
            value
                .prerequisites
                .as_ref()
                .is_some_and(|p| related(&p.ancestors) || named(&p.unsurfaced_ancestors))
                || value.stall.as_ref().is_some_and(|s| {
                    named(&s.undecided_ancestors)
                        || named(&s.unsurfaced_ancestors)
                        || related(&s.unadopted_ancestors)
                })
        });
    if let Some(parent) = value.parent.as_ref().filter(|_| !parent_named_below) {
        out.push_str(&format!(
            "Parent: {}  {}\n",
            display::identity(&parent.id),
            match &parent.row {
                Some(row) => display::line(row.title),
                None => display::muted("(missing)"),
            }
        ));
    }
    if !value.heads.is_empty() {
        out.push_str(&format!("\n{}\n", display::heading("Conflicted")));
        for entry in &value.heads {
            out.push_str(&head(entry));
        }
    }
    if !value.violations.is_empty() {
        out.push_str(&format!("\n{}\n", display::heading("Invalid")));
        for violation in &value.violations {
            let related: Vec<_> = violation
                .related
                .iter()
                .map(|r| {
                    let line = row_of(r);
                    line.trim_end().to_string()
                })
                .collect();
            out.push_str(&format!(
                "{}{}\n",
                violation.kind.label(),
                if related.is_empty() {
                    String::new()
                } else {
                    format!(": {}", related.join("; "))
                }
            ));
        }
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
            out.push_str(&format!("Ancestor must be adopted: {}", row_of(ancestor)));
        }
        for dependency in &prerequisites.dependencies {
            out.push_str(&format!("Dependency must complete: {}", row_of(dependency)));
        }
        for dependency in &prerequisites.ancestor_dependencies {
            out.push_str(&format!(
                "Ancestor dependency must complete: {}",
                row_of(dependency)
            ));
        }
        for ancestor in &prerequisites.unsurfaced_ancestors {
            out.push_str(&format!("Unsurfaced ancestor: {}", row(ancestor)));
        }
    }
    if !details && let Some(stall) = &value.stall {
        out.push_str(&format!("\n{}\n", display::heading("Stalled")));
        for dependency in &stall.dependencies {
            out.push_str(&format!("Dependency must complete: {}", row_of(dependency)));
        }
        for dependency in &stall.ancestor_dependencies {
            out.push_str(&format!(
                "Ancestor dependency must complete: {}",
                row_of(dependency)
            ));
        }
        for (descendant, dependency) in &stall.descendant_dependencies {
            let descendant = row(descendant);
            out.push_str(&format!(
                "Descendant dependency must complete: {}  needs  {}",
                descendant.strip_suffix('\n').unwrap_or(&descendant),
                row_of(dependency)
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
        for ancestor in &stall.unadopted_ancestors {
            out.push_str(&format!("Ancestor must be adopted: {}", row_of(ancestor)));
        }
        for ancestor in &stall.unsurfaced_ancestors {
            out.push_str(&format!("Unsurfaced ancestor: {}", row(ancestor)));
        }
    }
    if details {
        if let Some(stored) = value.stored {
            let stored = format!("{stored:?}");
            let effective = value
                .effective
                .map(|l| format!("{l:?}"))
                .unwrap_or_else(|| stored.clone());
            if value.row.kind == Kind::Group {
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
        }
        if value.parent.is_none() {
            out.push_str("Parent: (none)\n");
        }
        out.push_str(&format!(
            "{} {}\n",
            display::muted("Condition:"),
            value
                .condition
                .map(display::line)
                .unwrap_or_else(|| "(none)".into())
        ));
        out.push_str(&format!(
            "\n{}{}\n",
            display::heading("Dependencies (must be Completed to start or complete):"),
            if value.dependencies.is_empty() {
                " (none)"
            } else {
                ""
            }
        ));
        for other in &value.dependencies {
            out.push_str(&row_of(other));
        }
        out.push_str(&format!(
            "\n{}{}\n",
            display::heading("Dependents:"),
            if value.dependents.is_empty() {
                " (none)"
            } else {
                ""
            }
        ));
        for other in &value.dependents {
            out.push_str(&row(other));
        }
    }
    out.push('\n');
    out.push_str(&display::block(value.description));
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
    actor_of(context.recorder.as_ref())
}
fn actor_of(recorder: Option<&Recorder>) -> String {
    recorder
        .map(|r| display::line(&r.actor))
        .unwrap_or_else(|| "—".into())
}
pub(super) fn recorder_display(recorder: Option<&Recorder>, details: bool) -> String {
    let mut text = actor_of(recorder);
    if details && let Some(recorder) = recorder {
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
fn ids(set: &std::collections::BTreeSet<EntityId>) -> String {
    set.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
fn parent_of(current: &Current) -> String {
    current
        .parent
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| "(none)".into())
}
/// What one record changed, from the value before it when its parent record is present.
fn describe(entry: &read::RecordEntry<'_>) -> String {
    let record = entry.record;
    let after = &record.after;
    let reason = record
        .reason
        .as_ref()
        .map(|r| format!("  Reason: {}", display::line(r)))
        .unwrap_or_default();
    let before = entry.before;
    match &record.kind {
        RecordKind::Created => format!("Created: {:?}", after.lifecycle),
        RecordKind::Transition(_) => format!(
            "{} → {:?}{reason}",
            before
                .map(|b| format!("{:?}", b.lifecycle))
                .unwrap_or_else(|| "unknown".into()),
            after.lifecycle
        ),
        RecordKind::Edit => {
            let changed: Vec<&str> = match before {
                Some(before) => [
                    (before.title != after.title, "title"),
                    (before.description != after.description, "description"),
                ]
                .into_iter()
                .filter(|(changed, _)| *changed)
                .map(|(_, field)| field)
                .collect(),
                None => vec!["title/description"],
            };
            if changed.is_empty() {
                format!("Edited: no field changes{reason}")
            } else {
                format!("Edited: {}", changed.join(", "))
            }
        }
        RecordKind::Label => format!("Label set: {}", after.label),
        RecordKind::Parent => format!(
            "Parent: {} → {}",
            before.map(parent_of).unwrap_or_else(|| "unknown".into()),
            parent_of(after)
        ),
        RecordKind::Dependency => match before {
            Some(before) => {
                let added: Vec<_> = after.needs.difference(&before.needs).cloned().collect();
                let removed: Vec<_> = before.needs.difference(&after.needs).cloned().collect();
                let mut parts = Vec::new();
                if !added.is_empty() {
                    parts.push(format!(
                        "Dependency added: {}",
                        ids(&added.into_iter().collect())
                    ));
                }
                if !removed.is_empty() {
                    parts.push(format!(
                        "Dependency removed: {}",
                        ids(&removed.into_iter().collect())
                    ));
                }
                parts.join("  ")
            }
            None => format!("Dependencies: {}", ids(&after.needs)),
        },
        RecordKind::Condition => match &after.condition {
            Some(command) => format!("Condition set: {}", display::line(command)),
            None => "Condition unset".into(),
        },
        RecordKind::Convert => converted(
            match after.kind {
                Kind::Issue => Kind::Group,
                Kind::Group => Kind::Issue,
            },
            after.kind,
        ),
        RecordKind::Import => {
            let changed: Vec<&str> = match before {
                Some(before) => [
                    (before.title != after.title, "title"),
                    (before.description != after.description, "description"),
                    (before.label != after.label, "label"),
                    (before.parent != after.parent, "parent"),
                    (before.needs != after.needs, "needs"),
                ]
                .into_iter()
                .filter(|(changed, _)| *changed)
                .map(|(_, field)| field)
                .collect(),
                None => vec!["title/description/label/parent/needs"],
            };
            format!("Declaration applied: {}", changed.join(", "))
        }
        RecordKind::Resolve { chosen } => format!(
            "Resolved: {chosen}  {:?}  {:?}  {}{reason}",
            after.lifecycle, after.kind, after.label
        ),
    }
}
/// A conversion as `convert` confirms it and `log` shows it.
pub(super) fn converted(before: Kind, after: Kind) -> String {
    format!("Converted: {before:?} → {after:?}")
}
pub(super) fn log_line(entry: &read::RecordEntry<'_>, recorder_details: bool) -> String {
    format!(
        "{}  {}  {}{}\n",
        display::muted(display::timestamp(&entry.record.at)),
        recorder_display(entry.record.recorder.as_ref(), recorder_details),
        describe(entry),
        if entry.parent_missing {
            display::muted("  parent missing")
        } else {
            String::new()
        }
    )
}
/// Axon leaves how Git treats the store to the user; each block reaches one of the two ways.
pub(super) fn init_output(location: &Location, initialized: &Initialized) -> String {
    let mut text = format!(
        "Initialized {}\n",
        display::human_text(location.root.join(".axon/header.json").display())
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
       git check-ignore -v .axon/header.json
Choose one way to use the store; do not mix the two in one repository. Run the
steps in
{}:

To keep the store out of Git, ignore the directory by adding this line to the
file that git rev-parse --git-path info/exclude names (this repository only) or
to your global Git ignore file:
       .axon/
Linked worktrees without a .axon of their own use the main worktree's store.
Git overwrites an ignored store without warning when you check out or merge a
commit that tracks .axon/.

To track the store in Git and merge it between branches instead, first remove
any ignore rule outside .axon that covers the directory, then stage and commit
the store; .axon/.gitignore already excludes locks and temporary files:
       git add .axon
       git commit -m \"Track the Axon store\"
Git merges records from both sides without merge attributes or configuration;
.axon/.gitattributes only keeps Git from converting their line endings.
Undo Axon state with lifecycle commands such as reopen and release, not with
git revert: reverting a commit only removes record files.
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
    before_view: &axon::lifecycle::record::View,
    checked: &axon::declaration::Checked,
) -> Result<String> {
    let mut out = String::new();
    if checked.already_applied {
        out.push_str("Already applied.\n");
    }
    let after_view = &checked.after_view;
    let view = read::View::new(&checked.after, after_view);
    for r in declaration.records() {
        let id: EntityId = r.id.clone().expect("checked ID").try_into()?;
        let b = after_view
            .current(&id)
            .ok_or_else(|| axon::Error::Invalid(format!("missing Entity {id}")))?;
        out.push_str(&format!("{id}:\n"));
        if let Some(a) = before_view.current(&id) {
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
                    "  label: {}\n",
                    if a.label == b.label {
                        "unchanged".into()
                    } else {
                        format!("{} -> {}", a.label.name(), b.label.name())
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
                for id in a.needs.difference(&b.needs) {
                    out.push_str(&format!("  needs: - {id}\n"));
                }
                for id in b.needs.difference(&a.needs) {
                    out.push_str(&format!("  needs: + {id}\n"));
                }
            }
        } else {
            out.push_str(&format!(
                "  Create {}\n  title: new\n  description: new\n  label: {}\n  parent: null -> {}\n",
                axon::declaration::kind(b.kind),
                b.label.name(),
                b.parent
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "null".into())
            ));
            for id in &b.needs {
                out.push_str(&format!("  needs: + {id}\n"));
            }
        }
        out.push_str(&format!(
            "  Situation after: {} (conditions not evaluated)\n",
            status_label(view.status(&id))
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
pub fn format_note(id: &RecordId, note: &Note, details: bool, listed: bool) -> String {
    format!(
        "{}  {}  {}\n{}\n\n",
        display::identity(id),
        display::muted(display::timestamp(&note.at)),
        recorder_display(note.recorder.as_ref(), details),
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
            &[
                "write",
                "label",
                "parent",
                "dep",
                "condition",
                "convert",
                "import",
            ],
        ),
        (
            "Setup & utilities",
            &["init", "storage", "resolve", "completion", "docs", "help"],
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
            display::identity(found.id),
            display::muted(display::timestamp(&note.at)),
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

/// The report of `axon storage check` over an intact, derived store, and how many of its
/// lines fail the command (conflicts and violations; gaps are information).
pub(super) struct StorageReport {
    pub(super) text: String,
    pub(super) failing: usize,
}
pub(super) fn storage_report(view: &read::View<'_>) -> StorageReport {
    let derived = view.derived();
    let mut text = String::new();
    let mut failing = 0;
    for id in derived.conflicted() {
        let row = view.row(derived.key(id).expect("known"), None);
        let heads = derived.heads(id).map_or(0, |h| h.len());
        text.push_str(&format!(
            "Conflicted: {}  {heads} heads\n",
            self::row(&row).trim_end()
        ));
        failing += 1;
    }
    for violation in derived.violations() {
        let row = view.row(derived.key(&violation.entity).expect("known"), None);
        text.push_str(&format!(
            "Violation: {}  {}\n",
            self::row(&row).trim_end(),
            violation.kind.label()
        ));
        failing += 1;
    }
    for (id, missing) in derived.gaps() {
        let row = view.row(derived.key(id).expect("known"), None);
        text.push_str(&format!(
            "Missing parent records: {}  {}\n",
            self::row(&row).trim_end(),
            missing
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for id in derived.noted_only() {
        text.push_str(&format!(
            "Missing records: {}  Notes only\n",
            display::identity(id)
        ));
    }
    StorageReport { text, failing }
}
