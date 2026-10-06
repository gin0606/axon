//! Display text for the core's values. The spelling of labels stays the one records and the
//! CLI use; states, situations and record kinds are shown in Japanese.

use crate::board::{Difference, State, WaitKind};
use axon::lifecycle::{Kind, Lifecycle, Operation, record::RecordKind, record::ViolationKind};
use axon::read::{PrerequisiteOperation, Status};
use chrono::{DateTime, Local, Utc};

pub fn state(state: State) -> &'static str {
    match state {
        State::Undecided => "未判断",
        State::NotStarted => "未着手",
        State::InProgress => "着手中",
        State::Completed => "完了",
        State::Cancelled => "取りやめ",
        State::Conflicted => "衝突",
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

pub fn lifecycle(lifecycle: Lifecycle) -> &'static str {
    state(State::of(Some(lifecycle)))
}

pub fn kind(kind: Kind) -> &'static str {
    match kind {
        Kind::Issue => "Issue",
        Kind::Group => "Group",
    }
}

/// What the situation adds to the state, when it adds anything. Conditions are taken as
/// satisfied, so nothing is called unsurfaced by this reading.
pub fn situation(status: Status) -> Option<&'static str> {
    match status {
        Status::Ready => Some("着手可"),
        Status::Blocked => Some("前提待ち"),
        Status::InProgressBlocked => Some("前提待ち"),
        Status::Unsurfaced => Some("再浮上待ち"),
        Status::Empty => Some("子なし"),
        Status::Confirmable => Some("完了確認待ち"),
        Status::Undecided
        | Status::InProgress
        | Status::Completed
        | Status::Cancelled
        | Status::Conflicted => None,
    }
}

pub fn waiting_for(operation: Option<PrerequisiteOperation>) -> &'static str {
    match operation {
        Some(PrerequisiteOperation::Start) => "着手の前提を待っています",
        Some(PrerequisiteOperation::Complete) => "完了の前提を待っています",
        None => "この Group は進められる仕事がなく止まっています",
    }
}

pub fn wait(kind: WaitKind) -> &'static str {
    match kind {
        WaitKind::UnadoptedAncestor => "採用されていない祖先",
        WaitKind::Dependency => "完了していない依存先",
        WaitKind::AncestorDependency => "祖先の未完了の依存先",
        WaitKind::UnsurfacedAncestor => "再浮上条件を満たさない祖先",
        WaitKind::DescendantDependency => "依存先を待つ子孫",
        WaitKind::UndecidedChild => "未判断の子",
        WaitKind::OpenSubgroup => "進められる仕事のない子 Group",
        WaitKind::UnsurfacedCandidate => "再浮上条件を満たさない子孫",
        WaitKind::OwnCondition => "自身の再浮上条件",
        WaitKind::UndecidedAncestor => "未判断の祖先",
    }
}

pub fn violation(kind: ViolationKind) -> &'static str {
    match kind {
        ViolationKind::ContainmentCycle => "包含が循環しています",
        ViolationKind::UnknownParent => "親が存在しないか Group ではありません",
        ViolationKind::OpenUnderTerminal => "終了した親の下で未終了です",
        ViolationKind::UnadoptedAncestor => "採用されていない祖先の下で着手しています",
        ViolationKind::CompletedWithOpenDependency => {
            "完了していない依存先があるのに完了しています"
        }
        ViolationKind::UnknownDependency => "存在しない依存先があります",
        ViolationKind::CompletionCycle => "完了の前提が循環しています",
    }
}

pub fn operation(operation: Operation) -> &'static str {
    match operation {
        Operation::Accept => "採用",
        Operation::Withdraw => "採用撤回",
        Operation::Start => "着手",
        Operation::Release => "作業を解放",
        Operation::Complete => "完了",
        Operation::Cancel => "取りやめ",
        Operation::Reconsider => "再検討",
        Operation::Reopen => "再開",
    }
}

pub fn record(kind: &RecordKind) -> String {
    match kind {
        RecordKind::Created => "作成".into(),
        RecordKind::Transition(op) => operation(*op).into(),
        RecordKind::Edit => "編集".into(),
        RecordKind::Label => "label の変更".into(),
        RecordKind::Parent => "所属の変更".into(),
        RecordKind::Dependency => "依存の変更".into(),
        RecordKind::Condition => "再浮上条件の変更".into(),
        RecordKind::Convert => "種類の変換".into(),
        // A declaration applied with `axon import apply`; earlier versions of this app also
        // wrote one for an edit of text and label together.
        RecordKind::Import => "まとめて変更".into(),
        RecordKind::Resolve { chosen } => format!("衝突の解決（{chosen} を採用）"),
    }
}

pub fn difference(difference: &Difference) -> String {
    match difference {
        Difference::Lifecycle(before, after) => {
            format!("状態: {} → {}", lifecycle(*before), lifecycle(*after))
        }
        Difference::Kind(before, after) => format!("種類: {} → {}", kind(*before), kind(*after)),
        Difference::Title(before, after) => format!("タイトル: {before} → {after}"),
        Difference::Description => "本文を変更".into(),
        Difference::Label(before, after) => format!("label: {before} → {after}"),
        Difference::Parent(before, after) => format!(
            "所属: {} → {}",
            before.as_ref().map_or("なし".into(), ToString::to_string),
            after.as_ref().map_or("なし".into(), ToString::to_string)
        ),
        Difference::DependencyAdded(id) => format!("依存先を追加: {id}"),
        Difference::DependencyRemoved(id) => format!("依存先を削除: {id}"),
        Difference::Condition => "再浮上条件を変更".into(),
    }
}

pub fn time(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}
