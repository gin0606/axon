// 単独 Issue の lifecycle と再浮上条件を扱う基本モデル。
// 条件定義は一つに固定して抽象化し、成立・不成立だけを外部入力として与える。
// 対象外: 包含、dependency、Group、登録操作、条件の種類と設定操作、一覧への表示、CLI、永続化。
// 遷移規則は lifecycle_rules を import する。包含と dependency は group_lifecycle が扱う。
// 対応する契約: ../docs/reference/lifecycle.md
module issue_lifecycle {
  import lifecycle_rules.* from "lifecycle_rules"

  // Event は検証用の観測が区別するイベントの種類。
  type Event = Initialized | LifecycleOperation(Operation) | ConditionChanged

  // 保存する状態は lifecycle だけで、conditionSatisfied は条件の成立・不成立を表す外部入力。
  var lifecycle: Lifecycle
  var conditionSatisfied: bool

  // 再浮上条件の評価。Undecided・NotStarted・InProgress だけを評価対象とし、
  // Cancelled・Completed は入力の真偽にかかわらず NotEvaluated として浮上しない。
  // Reopen で NotStarted へ戻った Issue は再び評価対象になる。判定は導出であり、保存する状態を書き換えない。
  type ConditionResult = NotEvaluated | Evaluated(bool)

  pure def evaluationEligible(current: Lifecycle): bool =
    Set(Undecided, NotStarted, InProgress).contains(current)

  pure def evaluateCondition(current: Lifecycle, satisfied: bool): ConditionResult =
    if (evaluationEligible(current)) Evaluated(satisfied) else NotEvaluated

  val conditionResult = evaluateCondition(lifecycle, conditionSatisfied)
  val surfaced = match conditionResult {
    | NotEvaluated => false
    | Evaluated(satisfied) => satisfied
  }
  // 到達性と操作順を調べる ghost state。製品の保存情報ではない。
  // 直前の状態・外部入力とイベント、直近の Withdraw・Cancel・Reconsider より後に
  // 採用されたか、一度でも Reopen したかを保持する。
  type Observation = {
    previous: Lifecycle,
    event: Event,
    previousCondition: bool,
    acceptedSinceReview: bool,
    reopenedEver: bool,
  }
  var observed: Observation

  // 明示操作と外部入力の変化で、それぞれ観測を更新する。
  pure def observe(current: Lifecycle, satisfied: bool, op: Operation, before: Observation): Observation = {
    previous: current,
    event: LifecycleOperation(op),
    previousCondition: satisfied,
    acceptedSinceReview:
      if (op == Accept) true
      else if (Set(Withdraw, Cancel, Reconsider).contains(op)) false
      else before.acceptedSinceReview,
    reopenedEver: before.reopenedEver or op == Reopen,
  }

  pure def observeConditionChange(
    current: Lifecycle, satisfied: bool, before: Observation
  ): Observation = {
    ...before,
    previous: current,
    event: ConditionChanged,
    previousCondition: satisfied,
  }

  // 提案として登録した直後の Undecided から始め、外部入力は不成立、観測はどちらも偽にする。
  action init = all {
    lifecycle' = Undecided,
    conditionSatisfied' = false,
    observed' = {
      previous: Undecided,
      event: Initialized,
      previousCondition: false,
      acceptedSinceReview: false,
      reopenedEver: false,
    },
  }

  // 明示操作は前提を満たすときだけ実行し、外部入力を変えない。
  action perform(op: Operation): bool = all {
    canPerform(lifecycle, op),
    lifecycle' = applyOperation(lifecycle, op),
    conditionSatisfied' = conditionSatisfied,
    observed' = observe(lifecycle, conditionSatisfied, op, observed),
  }

  // 外部入力は両方向へ変化できる。lifecycle は変えない。Cancelled・Completed の間も外界は
  // 変化しうるので入力の変化は許すが、条件の評価対象には戻さない。
  pure def canChangeCondition(before: bool, after: bool): bool = before != after

  action changeCondition(satisfied: bool): bool = all {
    canChangeCondition(conditionSatisfied, satisfied),
    lifecycle' = lifecycle,
    conditionSatisfied' = satisfied,
    observed' = observeConditionChange(lifecycle, conditionSatisfied, observed),
  }

  // 8操作と外部入力の変化を探索候補にし、前提を満たすものだけが実行される。
  action step = any {
    perform(Accept),
    perform(Withdraw),
    perform(Start),
    perform(Release),
    perform(Complete),
    perform(Cancel),
    perform(Reconsider),
    perform(Reopen),
    changeCondition(true),
    changeCondition(false),
  }

  // witness: 8操作それぞれの到達に加え、未判断からの見送り、未着手からの取りやめ、
  // 進行中からの打ち切りを区別する。外部入力の両方向の変化、未判断・未着手の浮上、
  // 進行中の非浮上化、取りやめ・完了の評価除外、再検討後の浮上・非浮上、
  // Reopen 後の再着手と Reopen 直後の浮上・非浮上も観測する。
  val wAccept = observed.event == LifecycleOperation(Accept)
  val wWithdraw = observed.event == LifecycleOperation(Withdraw)
  val wStart = observed.event == LifecycleOperation(Start)
  val wRelease = observed.event == LifecycleOperation(Release)
  val wComplete = observed.event == LifecycleOperation(Complete)
  val wCancel = observed.event == LifecycleOperation(Cancel)
  val wReconsider = observed.event == LifecycleOperation(Reconsider)
  val wReopen = observed.event == LifecycleOperation(Reopen)
  val wRejectProposal = wCancel and observed.previous == Undecided
  val wCancelBeforeStart = wCancel and observed.previous == NotStarted
  val wCancelDuringWork = wCancel and observed.previous == InProgress
  val wStartAfterReopen = wStart and observed.reopenedEver

  val wConditionRose = observed.event == ConditionChanged and conditionSatisfied
  val wConditionFell = observed.event == ConditionChanged and not(conditionSatisfied)
  val wUndecidedSurfaced = lifecycle == Undecided and surfaced
  val wNotStartedSurfaced = lifecycle == NotStarted and surfaced
  val wInProgressSurfaceFell = lifecycle == InProgress and wConditionFell
  val wCancelledSuppressed = lifecycle == Cancelled and wConditionRose and not(surfaced)
  val wCompletedSuppressed = lifecycle == Completed and wConditionRose and not(surfaced)
  val wReconsiderSurfaced = wReconsider and surfaced
  val wReconsiderWaiting = wReconsider and not(surfaced)
  val wReopenSurfaced = wReopen and surfaced
  val wReopenWaiting = wReopen and not(surfaced)

  // invariant: 未着手・進行中・完了には直近の見直しより後の採用が要ること、
  // 進行中の入口は Start、完了の入口は Complete に限ること、
  // 完了から抜ける経路は Reopen だけで、Reopen は未着手へ戻すこと。
  val invAdoptedBeforeWork =
    Set(NotStarted, InProgress, Completed).contains(lifecycle)
      implies observed.acceptedSinceReview
  val invCompletedLeavesOnlyByReopen =
    (observed.previous == Completed and lifecycle != Completed)
      implies (observed.event == LifecycleOperation(Reopen) and lifecycle == NotStarted)
  val invWorkEnteredThroughStart =
    (lifecycle == InProgress and observed.previous != InProgress)
      implies (observed.event == LifecycleOperation(Start) and observed.previous == NotStarted)
  val invCompletionRequiresWork =
    (lifecycle == Completed and observed.previous != Completed)
      implies (observed.event == LifecycleOperation(Complete) and observed.previous == InProgress)

  // 外部入力ではなく、明示的な lifecycle 操作の行き止まりを調べる。
  // 完了も Reopen で戻せるため、どの状態にも実行できる明示操作がある。
  // 構成の確認: 前提の定義から直接従う性質で、規則の書き誤りを検出するために置く。
  val invNoLifecycleDeadEnd = OPERATIONS.exists(op => canPerform(lifecycle, op))

  // invariant: 外部入力と明示操作が互いの値を変えないこと、
  // 取りやめ・完了が評価されず浮上しないこと、評価対象の浮上が入力と一致すること。
  val invConditionChangePreservesLifecycle =
    observed.event == ConditionChanged implies lifecycle == observed.previous
  val invLifecycleOperationsPreserveCondition =
    observed.event != ConditionChanged implies conditionSatisfied == observed.previousCondition
  val invTerminalNotEvaluated =
    Set(Cancelled, Completed).contains(lifecycle)
      implies (conditionResult == NotEvaluated and not(surfaced))
  val invActiveEvaluationMatchesInput =
    Set(Undecided, NotStarted, InProgress).contains(lifecycle)
      implies (conditionResult == Evaluated(conditionSatisfied) and (surfaced iff conditionSatisfied))
}
