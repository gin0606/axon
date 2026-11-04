//! Display text for the core's values. The spelling of labels stays the one records and the
//! CLI use; states, situations and record kinds use the selected language.

use crate::board::{Difference, Item, State, WaitKind};
use crate::i18n::Language;
use axon::lifecycle::{Kind, Lifecycle, Operation, record::RecordKind, record::ViolationKind};
use axon::read::{PrerequisiteOperation, Status};
use chrono::{DateTime, Local, Utc};
use std::fmt;

pub fn state(language: Language, state: State) -> String {
    match state {
        State::Undecided => crate::tr!(language, "state.undecided"),
        State::NotStarted => crate::tr!(language, "state.not_started"),
        State::InProgress => crate::tr!(language, "state.in_progress"),
        State::Completed => crate::tr!(language, "state.completed"),
        State::Cancelled => crate::tr!(language, "state.cancelled"),
        State::Conflicted => crate::tr!(language, "state.conflicted"),
    }
}

pub fn state_icon(state: State) -> &'static str {
    match state {
        State::Undecided => "◇",
        State::NotStarted => "○",
        State::InProgress => "◐",
        State::Completed => "✓",
        State::Cancelled => "×",
        State::Conflicted => "!",
    }
}

pub fn lifecycle(language: Language, lifecycle: Lifecycle) -> String {
    state(language, State::of(Some(lifecycle)))
}

pub fn kind(kind: Kind) -> &'static str {
    match kind {
        Kind::Issue => "Issue",
        Kind::Group => "Group",
    }
}

/// Warnings and status precede identifiers, which may truncate in a narrow list.
pub fn row_meta(language: Language, item: &Item, matched: bool) -> String {
    let mut meta = Vec::new();
    if item.invalid {
        meta.push(crate::tr!(language, "row.invalid"));
    }
    if !matched {
        meta.push(crate::tr!(language, "row.context"));
    }
    meta.push(state(language, item.state));
    meta.extend(situation(language, item.status));
    if item.kind == Kind::Group {
        meta.push(kind(item.kind).to_string());
    }
    meta.push(item.label.name().to_string());
    meta.push(item.id.to_string());
    meta.join(" · ")
}

/// What the situation adds to the state, when it adds anything. Conditions are taken as
/// satisfied, so nothing is called unsurfaced by this reading.
pub fn situation(language: Language, status: Status) -> Option<String> {
    match status {
        Status::Ready => Some(crate::tr!(language, "situation.ready")),
        Status::Blocked => Some(crate::tr!(language, "situation.blocked")),
        Status::InProgressBlocked => Some(crate::tr!(language, "situation.blocked")),
        Status::Unsurfaced => Some(crate::tr!(language, "situation.unsurfaced")),
        Status::Empty => Some(crate::tr!(language, "situation.empty")),
        Status::Confirmable => Some(crate::tr!(language, "situation.confirmable")),
        Status::Undecided
        | Status::InProgress
        | Status::Completed
        | Status::Cancelled
        | Status::Conflicted => None,
    }
}

pub fn waiting_for(language: Language, operation: Option<PrerequisiteOperation>) -> String {
    match operation {
        Some(PrerequisiteOperation::Start) => crate::tr!(language, "waiting.start"),
        Some(PrerequisiteOperation::Complete) => crate::tr!(language, "waiting.complete"),
        None => crate::tr!(language, "waiting.group"),
    }
}

pub fn wait(language: Language, kind: WaitKind) -> String {
    match kind {
        WaitKind::UnadoptedAncestor => crate::tr!(language, "wait.unadopted_ancestor"),
        WaitKind::Dependency => crate::tr!(language, "wait.dependency"),
        WaitKind::AncestorDependency => crate::tr!(language, "wait.ancestor_dependency"),
        WaitKind::UnsurfacedAncestor => crate::tr!(language, "wait.unsurfaced_ancestor"),
        WaitKind::DescendantDependency => crate::tr!(language, "wait.descendant_dependency"),
        WaitKind::UndecidedChild => crate::tr!(language, "wait.undecided_child"),
        WaitKind::OpenSubgroup => crate::tr!(language, "wait.open_subgroup"),
        WaitKind::UnsurfacedCandidate => crate::tr!(language, "wait.unsurfaced_candidate"),
        WaitKind::OwnCondition => crate::tr!(language, "wait.own_condition"),
        WaitKind::UndecidedAncestor => crate::tr!(language, "wait.undecided_ancestor"),
    }
}

pub fn violation(language: Language, kind: ViolationKind) -> String {
    match kind {
        ViolationKind::ContainmentCycle => crate::tr!(language, "violation.containment_cycle"),
        ViolationKind::UnknownParent => crate::tr!(language, "violation.unknown_parent"),
        ViolationKind::OpenUnderTerminal => crate::tr!(language, "violation.open_under_terminal"),
        ViolationKind::UnadoptedAncestor => crate::tr!(language, "violation.unadopted_ancestor"),
        ViolationKind::CompletedWithOpenDependency => {
            crate::tr!(language, "violation.open_dependency")
        }
        ViolationKind::UnknownDependency => crate::tr!(language, "violation.unknown_dependency"),
        ViolationKind::CompletionCycle => crate::tr!(language, "violation.completion_cycle"),
    }
}

pub fn operation(language: Language, operation: Operation) -> String {
    match operation {
        Operation::Accept => crate::tr!(language, "operation.accept"),
        Operation::Withdraw => crate::tr!(language, "operation.withdraw"),
        Operation::Start => crate::tr!(language, "operation.start"),
        Operation::Release => crate::tr!(language, "operation.release"),
        Operation::Complete => crate::tr!(language, "operation.complete"),
        Operation::Cancel => crate::tr!(language, "operation.cancel"),
        Operation::Reconsider => crate::tr!(language, "operation.reconsider"),
        Operation::Reopen => crate::tr!(language, "operation.reopen"),
    }
}

pub fn record(language: Language, kind: &RecordKind) -> String {
    match kind {
        RecordKind::Created => crate::tr!(language, "record.created"),
        RecordKind::Transition(op) => operation(language, *op),
        RecordKind::Edit => crate::tr!(language, "record.edit"),
        RecordKind::Label => crate::tr!(language, "record.label"),
        RecordKind::Parent => crate::tr!(language, "record.parent"),
        RecordKind::Dependency => crate::tr!(language, "record.dependency"),
        RecordKind::Condition => crate::tr!(language, "record.condition"),
        RecordKind::Convert => crate::tr!(language, "record.convert"),
        // A declaration applied with `axon import apply`; earlier versions of this app also
        // wrote one for an edit of text and label together.
        RecordKind::Import => crate::tr!(language, "record.import"),
        RecordKind::Resolve { chosen } => crate::tr!(language, "record.resolve", chosen = chosen),
    }
}

pub fn difference(language: Language, difference: &Difference) -> String {
    match difference {
        Difference::Lifecycle(before, after) => {
            crate::tr!(
                language,
                "difference.state",
                before = lifecycle(language, *before),
                after = lifecycle(language, *after)
            )
        }
        Difference::Kind(before, after) => crate::tr!(
            language,
            "difference.kind",
            before = kind(*before),
            after = kind(*after)
        ),
        Difference::Title(before, after) => {
            crate::tr!(language, "difference.title", before = before, after = after)
        }
        Difference::Description => crate::tr!(language, "difference.description"),
        Difference::Label(before, after) => {
            crate::tr!(language, "difference.label", before = before, after = after)
        }
        Difference::Parent(before, after) => crate::tr!(
            language,
            "difference.parent",
            before = before
                .as_ref()
                .map_or_else(|| language.text("common.none"), ToString::to_string),
            after = after
                .as_ref()
                .map_or_else(|| language.text("common.none"), ToString::to_string)
        ),
        Difference::DependencyAdded(id) => {
            crate::tr!(language, "difference.dependency_added", id = id)
        }
        Difference::DependencyRemoved(id) => {
            crate::tr!(language, "difference.dependency_removed", id = id)
        }
        Difference::Condition => crate::tr!(language, "difference.condition"),
    }
}

pub fn time(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// A path as one line that shows what it holds: backslashes, control characters, line and
/// paragraph separators, and invisible formatting characters (zero-width and the ones that
/// reorder text) are escaped, so a path from a link cannot pass for other text.
pub fn path(path: impl fmt::Display) -> String {
    let mut shown = String::new();
    for c in path.to_string().chars() {
        match c {
            '\\' => shown.push_str("\\\\"),
            '\n' => shown.push_str("\\n"),
            '\t' => shown.push_str("\\t"),
            '\r' => shown.push_str("\\r"),
            c if c.is_control() || invisible(c) => {
                shown.push_str(&format!("\\u{{{:x}}}", c as u32))
            }
            c => shown.push(c),
        }
    }
    shown
}

/// Characters that take no visible place of their own, look like other whitespace or reorder
/// the text around them: the Unicode format characters (Cf), the line and paragraph
/// separators, whitespace other than the space, fillers and variation selectors.
fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{a0}'
            | '\u{34f}'
            | '\u{115f}'..='\u{1160}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{2800}'
            | '\u{3000}'
            | '\u{3164}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{ffa0}'
            | '\u{e0100}'..='\u{e01ef}'
            | '\u{ad}'
            | '\u{600}'..='\u{605}'
            | '\u{61c}'
            | '\u{6dd}'
            | '\u{70f}'
            | '\u{890}'..='\u{891}'
            | '\u{8e2}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{2028}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{110bd}'
            | '\u{110cd}'
            | '\u{13430}'..='\u{1343f}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0001}'
            | '\u{e0020}'..='\u{e007f}'
    )
}
