//! Display text for the core's values. The spelling of labels stays the one records and the
//! CLI use; states, situations and record kinds are shown in Japanese.

use crate::board::{Change, Rejection, State, WaitKind};
use axon::lifecycle::{
    Kind, Lifecycle, Operation, Refusal, record::RecordKind, record::ViolationKind,
};
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
        RecordKind::Import => "import".into(),
        RecordKind::Resolve { chosen } => format!("衝突の解決（{chosen} を採用）"),
    }
}

pub fn change(change: &Change) -> String {
    match change {
        Change::Lifecycle(before, after) => {
            format!("状態: {} → {}", lifecycle(*before), lifecycle(*after))
        }
        Change::Kind(before, after) => format!("種類: {} → {}", kind(*before), kind(*after)),
        Change::Title(before, after) => format!("タイトル: {before} → {after}"),
        Change::Description => "本文を変更".into(),
        Change::Label(before, after) => format!("label: {before} → {after}"),
        Change::Parent(before, after) => format!(
            "所属: {} → {}",
            before.as_ref().map_or("なし".into(), ToString::to_string),
            after.as_ref().map_or("なし".into(), ToString::to_string)
        ),
        Change::DependencyAdded(id) => format!("依存先を追加: {id}"),
        Change::DependencyRemoved(id) => format!("依存先を削除: {id}"),
        Change::Condition => "再浮上条件を変更".into(),
    }
}

pub fn time(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// Why the core refused a structural change. The Entities it names are shown beside it.
pub fn rejection(rejection: &Rejection) -> String {
    let refusal = match rejection {
        Rejection::Refused(refusal) => refusal,
        Rejection::Other(message) => return format!("変更できません。（{message}）"),
    };
    match refusal {
        Refusal::Conflicted(_) => "衝突している Issue・Group があるため、解決するまで構造を変更できません。解決は CLI の axon resolve で行います。",
        Refusal::EntityConflicted(_) => "衝突しているため変更できません。CLI の axon resolve で解決してから変更してください。",
        Refusal::Missing(_) => "次の Issue・Group が記録にないため変更できません。「再読み込み」で記録を読み直してください。",
        Refusal::ParentClosed(_) => "所属している Group が完了または取りやめのため、その中からは外せず、移動もできません。Group を再開か再検討すると変更できます。",
        Refusal::DestinationNotOpenGroup(_) => "移動先は、このプロジェクトの記録にあり、完了も取りやめもしていない Group である必要があります。",
        Refusal::SelfContainment => "Group を自身の中に入れることはできません。",
        Refusal::ContainmentCycle { .. } => "移動先がこの仕事の内側にあるため、包含が循環します。",
        Refusal::StartedWorkNeedsAdoptedDestination { .. } => "着手中・完了の仕事は、移動先とその祖先がすべて採用済み（未着手）の Group の下にだけ移動できます。次の Group が採用済みではありません。",
        Refusal::NewViolations(_) => "この変更は次の構造の違反を生じるため、行えません。",
        Refusal::CompletionCycle { .. } => "この変更は、包含と依存を合わせた完了の前提を循環させます。",
        Refusal::SelfDependency => "自身に依存することはできません。",
        Refusal::DependencyOnAncestor { .. } => "この仕事を含む Group には依存できません。",
        Refusal::DependencyOnDescendant { .. } => "この仕事の内側にある仕事には依存できません。",
        Refusal::CompletedDependenciesFixed => "完了した仕事の依存先は変更できません。再開すると変更できます。",
        Refusal::ConvertInProgress => "着手中の Issue は変換できません。作業を解放してから変換してください。",
        Refusal::ConvertTerminal => "完了・取りやめの仕事は変換できません。再開か再検討をしてから変換してください。",
        Refusal::GroupWithChildren(_) => "子を持つ Group は Issue に変換できません。子を外すか移動してから変換してください。",
    }
    .into()
}
