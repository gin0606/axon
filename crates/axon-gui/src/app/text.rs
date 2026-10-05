//! Display text for the core's values. The spelling of labels stays the one records and the
//! CLI use; states, situations and record kinds are shown in Japanese.

use crate::board::{Change, Difference, Rejection, State, WaitKind};
use axon::lifecycle::{
    Kind, Lifecycle, Line, LineProblem, Operation, Refusal, record::RecordKind,
    record::ViolationKind,
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
        // The CLI's declaration import and the window's edit of text and label together.
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

/// What the state menu calls a transition of an Entity of `kind`. Completing a Group is the
/// final confirmation of the whole plan, told apart from its children ending.
pub fn step(kind: Kind, operation: Operation) -> &'static str {
    match (kind, operation) {
        (_, Operation::Accept) => "採用する",
        (_, Operation::Start) => "着手する",
        (Kind::Group, Operation::Complete) => "Group 全体を確認して完了にする",
        (Kind::Issue, Operation::Complete) => "完了にする",
        (_, Operation::Release) => "未着手に戻す",
        (_, Operation::Withdraw) => "採用を撤回する",
        (_, Operation::Reopen) => "再開する",
        (_, Operation::Reconsider) => "再検討する",
        (_, Operation::Cancel) => "取りやめる",
    }
}

/// What a refused change was doing, for wording why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Doing {
    Transition(Operation),
    Structure,
    Create,
    Edit,
    Note,
}
impl Doing {
    pub fn of(change: &Change) -> Self {
        match change {
            Change::Transition { operation, .. } => Self::Transition(*operation),
            Change::Create { .. } => Self::Create,
            Change::Edit { .. } => Self::Edit,
            Change::AddNote { .. } => Self::Note,
            Change::Move { .. }
            | Change::AddDependency { .. }
            | Change::RemoveDependency { .. }
            | Change::Convert { .. } => Self::Structure,
        }
    }
    /// What cannot be done, as the end of a sentence.
    fn cannot(self) -> &'static str {
        match self {
            Self::Transition(_) => "状態を変更できません",
            Self::Structure => "構造を変更できません",
            Self::Create => "作成できません",
            Self::Edit => "編集できません",
            Self::Note => "Note を追加できません",
        }
    }
}

/// Why the core refused a change. The Entities it names are shown beside it.
pub fn rejection(rejection: &Rejection, doing: Doing) -> String {
    let refusal = match rejection {
        Rejection::Refused(refusal) => refusal,
        Rejection::Other(message) => return format!("{}。（{message}）", doing.cannot()),
    };
    let progress = matches!(doing, Doing::Transition(_));
    match refusal {
        Refusal::Conflicted(_) => {
            return format!(
                "衝突している Issue・Group があるため、解決するまで{}。解決は CLI の axon resolve で行います。",
                doing.cannot()
            );
        }
        Refusal::InvalidLine { field, problem } => {
            let field = match field {
                Line::Title => "タイトル",
                Line::Reason => "理由",
            };
            return match problem {
                LineProblem::Empty => format!("{field}を入力してください。"),
                LineProblem::ControlCharacter => {
                    format!(
                        "{field}に改行・タブなどの制御文字は使えません。1 行で入力してください。"
                    )
                }
                LineProblem::TooLong { length, limit } => {
                    format!("{field}は {limit} 文字以内にしてください（今は {length} 文字）。")
                }
            };
        }
        _ => {}
    }
    match refusal {
        Refusal::Conflicted(_) | Refusal::InvalidLine { .. } => unreachable!("answered above"),
        Refusal::EntityConflicted(_) => "衝突しているため変更できません。CLI の axon resolve で解決してから変更してください。",
        Refusal::Missing(_) => "次の Issue・Group が記録にないため変更できません。「再読み込み」で記録を読み直してください。",
        Refusal::ParentClosed(_) if progress => "所属している Group が完了・取りやめか、記録にないため、状態を変更できません。先に Group を再開か再検討してください。",
        Refusal::ParentClosed(_) => "所属している Group が完了または取りやめのため、その中からは外せず、移動もできません。Group を再開か再検討すると変更できます。",
        Refusal::DestinationNotOpenGroup(_) if doing == Doing::Create => "作成先は、このプロジェクトの記録にあり、完了も取りやめもしていない Group である必要があります。",
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
        Refusal::NotApplicable { .. } => "今の状態からはこの操作はできません。「再読み込み」で記録を読み直してください。",
        Refusal::AncestorsNotAdopted(_) => "祖先の Group が採用済み（未着手）でないため、できません。先に次の Group を採用してください。",
        Refusal::DependenciesNotCompleted(_) => "依存先が完了していないため、できません。次の仕事の完了を待っています。",
        Refusal::AncestorDependenciesNotCompleted { .. } => "祖先の Group の依存先が完了していないため、着手できません。次の Group と依存先です。",
        Refusal::ChildrenNotEnded(_) if doing == Doing::Transition(Operation::Complete) => "配下の仕事がすべて終了（完了か取りやめ）するまで、完了にできません。次の仕事が終了していません。",
        Refusal::ChildrenNotEnded(_) => "配下の仕事がすべて終了（完了か取りやめ）するまで、取りやめられません。次の仕事が終了していません。",
        Refusal::WorkingGroupWithdrawn(_) => "配下に着手中・完了の仕事があり、この Group は着手中として扱われるため、採用を撤回できません。次の仕事です。",
        Refusal::StartedWorkNeedsAdoptedAncestors(_) => "配下に着手中・完了の仕事がある Group は、祖先の Group がすべて採用済みのときだけ採用できます。次の Group が採用済みではありません。",
        Refusal::CompletedDependents(_) => "この仕事に依存している次の仕事が完了しているため、再開できません。先にそれらを再開してください。",
        Refusal::TerminalTextFixed | Refusal::TerminalLabelFixed => "完了・取りやめの仕事のタイトル・本文・label は編集できません。再開か再検討をすると編集できます。Note はどの状態でも追加できます。",
        Refusal::EmptyNote => "Note の本文を入力してください。",
    }
    .into()
}
