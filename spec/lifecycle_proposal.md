# Progress と採否を統合する検討用モデル

この literate specification は、Progress と採否を単一の lifecycle にまとめる案を扱う。単独 Issue の再浮上と、計画・子 Issue の着手境界を別モジュールで段階的に検討する。既存機能やデータ形式との互換性を前提にせず、新しい土台として設計するためのモデルであり、このファイルの追加・更新で現行実装や既存モデルは変更しない。

説明と実行可能なモデルをこのファイルで管理する。[Quint の literate 形式](https://quint.sh/docs/literate)に従い、`lmt` で `target/literate/` 配下に共有する基本遷移、単独 Issue、計画と子 Issue の3ファイルを生成する。生成ファイルは直接編集しない。

## 状態

登録直後の `Undecided`（未判断）から開始する。登録操作自体は扱わない。未判断は記録済みの提案であり、実施の約束を意味しない。採用によって `NotStarted`（未着手）へ進む。

採否を決めるためのまとまった調査は、別の Issue として採用・着手する。元の改善案は未判断のままにできる。この使い分けのために状態を増やさず、複数 Issue の関係は後の検討に残す。

状態の種類と基本遷移は Issue と計画で共有する。共有する純粋な定義だけを `lifecycle_rules.qnt` に生成する。

```quint target/literate/lifecycle_rules.qnt +=
module lifecycle_rules {
  type Lifecycle = Undecided | NotStarted | InProgress | Completed | Cancelled
  type Operation = Accept | Withdraw | Start | Release | Complete | Cancel | Reconsider
```

```quint target/literate/lifecycle_proposal.qnt +=
module lifecycle_proposal {
  import lifecycle_rules.* from "lifecycle_rules"
  type Event = Initialized | LifecycleOperation(Operation) | ConditionChanged

  var lifecycle: Lifecycle
  var conditionSatisfied: bool
```

## 再浮上条件

条件定義は一つに固定して抽象化し、成立・不成立だけを外部入力 `conditionSatisfied` として扱う。日時、手動解除、外部コマンドの違いや、条件の設定・変更操作はまだ扱わない。

未判断・未着手・進行中は評価対象とし、取りやめ・完了は条件を評価しない。`NotEvaluated` と `Evaluated(false)` を区別し、評価対象外の場合は入力の真偽にかかわらず浮上していないと導出する。判定は保存状態を書き換えない。

```quint target/literate/lifecycle_proposal.qnt +=
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
```

「後で考える」は未判断のまま条件で浮上を制御し、「取りやめ」は明示的に再検討するまで浮上対象外にする。取りやめから未判断へ戻すと、同じ条件が再び評価対象になる。条件が成立しているならその時点で浮上し、不成立なら未判断のまま浮上しない。

## 遷移

| 操作 | 遷移元 | 遷移先 |
| --- | --- | --- |
| Accept：採用 | 未判断 | 未着手 |
| Withdraw：採用撤回 | 未着手 | 未判断 |
| Start：着手 | 未着手 | 進行中 |
| Release：作業を解放 | 進行中 | 未着手 |
| Complete：完了 | 進行中 | 完了 |
| Cancel：見送り・取りやめ | 未判断・未着手・進行中 | 取りやめ |
| Reconsider：再検討 | 取りやめ | 未判断 |

`Completed`（完了）は戻さない。`Cancelled`（取りやめ）は再検討できるが、再び着手するには採用が必要になる。進行中から採用を撤回する場合は Release、Withdraw の順に操作する。完了後の追加作業は新しい Issue で扱う方針だが、このモデルは複数 Issue や作成操作を含まない。

各操作は原子的に実行され、前提を満たさない操作は無効となる。完了ではすべての lifecycle 操作が無効になる。外部入力はその後も変化できるが、完了を取り消さない。完了への到達を強制する公平性は仮定しない。

```quint target/literate/lifecycle_rules.qnt +=
  pure def canPerform(current: Lifecycle, op: Operation): bool =
    match op {
      | Accept => current == Undecided
      | Withdraw => current == NotStarted
      | Start => current == NotStarted
      | Release => current == InProgress
      | Complete => current == InProgress
      | Cancel => Set(Undecided, NotStarted, InProgress).contains(current)
      | Reconsider => current == Cancelled
    }

  pure def applyOperation(current: Lifecycle, op: Operation): Lifecycle =
    match op {
      | Accept => NotStarted
      | Withdraw => Undecided
      | Start => InProgress
      | Release => NotStarted
      | Complete => Completed
      | Cancel => Cancelled
      | Reconsider => Undecided
    }
}
```

## 検証のための観測

直前の状態・外部入力とイベント、直近の採用撤回・取りやめ・再検討より後に採用されたか、一度でも完了したかを観測する。`observed` は検証専用の ghost state であり、製品の保存項目を増やす提案ではない。`Initialized` は初期状態、`LifecycleOperation` は明示操作、`ConditionChanged` は外部入力の変化を表す。

```quint target/literate/lifecycle_proposal.qnt +=
  // 到達性と操作順を調べる ghost state。製品の保存情報ではない。
  type Observation = {
    previous: Lifecycle,
    event: Event,
    previousCondition: bool,
    acceptedSinceReview: bool,
    completedEver: bool,
  }
  var observed: Observation
```

```quint target/literate/lifecycle_proposal.qnt +=
  pure def observe(current: Lifecycle, satisfied: bool, op: Operation, before: Observation): Observation = {
    previous: current,
    event: LifecycleOperation(op),
    previousCondition: satisfied,
    acceptedSinceReview:
      if (op == Accept) true
      else if (Set(Withdraw, Cancel, Reconsider).contains(op)) false
      else before.acceptedSinceReview,
    completedEver: before.completedEver or applyOperation(current, op) == Completed,
  }

  pure def observeConditionChange(
    current: Lifecycle, satisfied: bool, before: Observation
  ): Observation = {
    ...before,
    previous: current,
    event: ConditionChanged,
    previousCondition: satisfied,
  }
```

## 初期状態と探索

登録直後の未判断から始め、採用済み・完了済みの観測はどちらも偽にする。各操作は許可条件と状態更新を組み合わせる。`step` は7操作と外部入力の変化を探索候補にし、許可条件を満たすものだけが実行される。外部入力はまず不成立から開始し、成立・不成立の両方向へ変化できる。

外部入力の変化は lifecycle を変更しない。取りやめ・完了の間にも外界は変化し得るので、入力の変化は許すが、条件の評価対象には戻さない。これは対象外の Entity に対して外部コマンドを実行するという意味ではない。

```quint target/literate/lifecycle_proposal.qnt +=
  action init = all {
    lifecycle' = Undecided,
    conditionSatisfied' = false,
    observed' = {
      previous: Undecided,
      event: Initialized,
      previousCondition: false,
      acceptedSinceReview: false,
      completedEver: false,
    },
  }

  action perform(op: Operation): bool = all {
    canPerform(lifecycle, op),
    lifecycle' = applyOperation(lifecycle, op),
    conditionSatisfied' = conditionSatisfied,
    observed' = observe(lifecycle, conditionSatisfied, op, observed),
  }

  pure def canChangeCondition(before: bool, after: bool): bool = before != after

  action changeCondition(satisfied: bool): bool = all {
    canChangeCondition(conditionSatisfied, satisfied),
    lifecycle' = lifecycle,
    conditionSatisfied' = satisfied,
    observed' = observeConditionChange(lifecycle, conditionSatisfied, observed),
  }

  action step = any {
    perform(Accept),
    perform(Withdraw),
    perform(Start),
    perform(Release),
    perform(Complete),
    perform(Cancel),
    perform(Reconsider),
    changeCondition(true),
    changeCondition(false),
  }
```

## 到達性

7操作それぞれの到達性に加え、未判断からの見送り、未着手からの取りやめ、進行中からの打ち切りを別々に観測する。外部入力の両方向の変化、未判断・未着手の浮上、進行中の非浮上化、取りやめ・完了の評価除外、明示的な再検討後の浮上・非浮上も確認する。初期状態だけではどの witness も成立しない。

```quint target/literate/lifecycle_proposal.qnt +=
  val wAccept = observed.event == LifecycleOperation(Accept)
  val wWithdraw = observed.event == LifecycleOperation(Withdraw)
  val wStart = observed.event == LifecycleOperation(Start)
  val wRelease = observed.event == LifecycleOperation(Release)
  val wComplete = observed.event == LifecycleOperation(Complete)
  val wCancel = observed.event == LifecycleOperation(Cancel)
  val wReconsider = observed.event == LifecycleOperation(Reconsider)
  val wRejectProposal = wCancel and observed.previous == Undecided
  val wCancelBeforeStart = wCancel and observed.previous == NotStarted
  val wCancelDuringWork = wCancel and observed.previous == InProgress

  val wConditionRose = observed.event == ConditionChanged and conditionSatisfied
  val wConditionFell = observed.event == ConditionChanged and not(conditionSatisfied)
  val wUndecidedSurfaced = lifecycle == Undecided and surfaced
  val wNotStartedSurfaced = lifecycle == NotStarted and surfaced
  val wInProgressSurfaceFell = lifecycle == InProgress and wConditionFell
  val wCancelledSuppressed = lifecycle == Cancelled and wConditionRose and not(surfaced)
  val wCompletedSuppressed = lifecycle == Completed and wConditionRose and not(surfaced)
  val wReconsiderSurfaced = wReconsider and surfaced
  val wReconsiderWaiting = wReconsider and not(surfaced)
```

## 検証する性質

- 未着手・進行中・完了には、直近の採用撤回・取りやめ・再検討より後の採用が必要。
- 進行中への入口は、未着手からの Start に限る。
- 完了への入口は、進行中からの Complete に限る。
- 一度完了すると完了のまま。
- 明示的な lifecycle 操作ができなくなるのは完了だけ。
- 外部入力の変化だけでは lifecycle は変わらない。
- lifecycle の明示操作は外部入力を変えない。
- 取りやめ・完了は条件を評価せず、浮上しない。
- 未判断・未着手・進行中の浮上は条件の成立と一致する。

```quint target/literate/lifecycle_proposal.qnt +=
  val invAdoptedBeforeWork =
    Set(NotStarted, InProgress, Completed).contains(lifecycle)
      implies observed.acceptedSinceReview
  val invCompletedFinal = observed.completedEver implies lifecycle == Completed
  val invWorkEnteredThroughStart =
    (lifecycle == InProgress and observed.previous != InProgress)
      implies (observed.event == LifecycleOperation(Start) and observed.previous == NotStarted)
  val invCompletionRequiresWork =
    (lifecycle == Completed and observed.previous != Completed)
      implies (observed.event == LifecycleOperation(Complete) and observed.previous == InProgress)

  // 外部入力ではなく、明示的な lifecycle 操作の行き止まりを調べる。
  val invOnlyCompletionStops =
    Set(Accept, Withdraw, Start, Release, Complete, Cancel, Reconsider)
      .exists(op => canPerform(lifecycle, op)) iff lifecycle != Completed

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
```

## 単独 Issue モデルの対象外

claim、dependency、Group、declaration 編集、履歴保存、CLI、永続化、actor の権限判定は含めない。再浮上条件の種類・設定操作、一覧への表示、浮上と着手許可の接続もまだ扱わない。明示操作の許可条件は前段の lifecycle モデルを維持している。

条件定義を固定しているため、取りやめ時の定義の保存や読み戻しは検証しない。`NotEvaluated` は意味上の評価除外を表すもので、実際に外部コマンドが呼ばれないこと、読み取りコスト、実行失敗・副作用は実装側で別途検証する。

## 単独 Issue モデルの再現と結果

Quint と [lmt](https://github.com/driusan/lmt) を使用する。`lmt` がなければ `go install github.com/driusan/lmt@latest` で導入し、Go のインストール先の `bin` を PATH に加える。以下は repository root から実行する。

2026-09-10、lifecycle と再浮上の導出を対象に、lmt `v0.0.0-20210421124901-62fe18f2f6a6` でこの Markdown からモデルを生成し、Quint 0.32.0、Rust backend、10,000 traces、最大 80 steps、入力 seed `2026091002` で以下を実行した。型検査が成功し、9 invariant に反例はなく、19 witness はすべて 1 trace 以上で観測された。これは bounded random simulation の結果であり、全状態の証明や必ず完了することの保証ではない。

```sh
lmt spec/lifecycle_proposal.md
quint typecheck target/literate/lifecycle_proposal.qnt
quint run target/literate/lifecycle_proposal.qnt \
  --invariants invAdoptedBeforeWork invCompletedFinal \
    invWorkEnteredThroughStart invCompletionRequiresWork invOnlyCompletionStops \
    invConditionChangePreservesLifecycle invLifecycleOperationsPreserveCondition \
    invTerminalNotEvaluated invActiveEvaluationMatchesInput \
  --witnesses wAccept wWithdraw wStart wRelease wComplete wCancel wReconsider \
    wRejectProposal wCancelBeforeStart wCancelDuringWork \
    wConditionRose wConditionFell wUndecidedSurfaced wNotStartedSurfaced \
    wInProgressSurfaceFell wCancelledSuppressed wCompletedSuppressed \
    wReconsiderSurfaced wReconsiderWaiting \
  --max-samples 10000 --max-steps 80 --seed 2026091002 --backend rust --verbosity 1
```

合意した状態・遷移・初期状態・検討範囲を変更するときは、このファイルを更新し、実行用モデルを再生成して再検証する。次の検討では、この状態遷移で実際の使い方を表現できるかを確認してから範囲を広げる。

## 計画と Issue の所属

計画2件、Issue 用の固定 ID 3件を使う。最初は2 Issue が計画0に所属し、残り1件は未登録の枠とする。未登録はモデルの探索領域を有限にする仕組みであり、新しい lifecycle ではない。計画・Issue は未判断から始まり、各々を明示的に採用する。

所属は最大一つで、所属なしも許す。所属変更は既存の親子の制約を壊さない限り許可し、lifecycle を変えない。完了・取りやめの計画への追加と、そこからの取り外しは不可とする。取りやめた計画は、明示的に再検討へ戻せば構成を変更できる。この取りやめ時の構成固定と計画の再検討は暫定採用の判断であり、運用上の負担が分かれば見直す。

```quint target/literate/group_lifecycle_proposal.qnt +=
module group_lifecycle_proposal {
  import lifecycle_rules.* from "lifecycle_rules"

  type Id = int
  type Parent = Unassigned | InGroup(Id)
  pure val GROUPS = Set(0, 1)
  pure val ISSUES = Set(0, 1, 2)
  pure val PARENTS = Set(Unassigned, InGroup(0), InGroup(1))
  pure val OPERATIONS = Set(Accept, Withdraw, Start, Release, Complete, Cancel, Reconsider)
  type PlanState = {
    groups: Id -> Lifecycle,
    issues: Id -> Lifecycle,
    registered: Set[Id],
    parents: Id -> Parent,
  }
  type PlanEvent = Initial
    | GroupOperation({ id: Id, op: Operation })
    | IssueOperation({ id: Id, op: Operation })
    | Reparent({ id: Id, parent: Parent })
    | RegisterIssue({ id: Id, parent: Parent })
  type PlanObservation = { before: PlanState, event: PlanEvent, reviewPassed: bool }
  var plan: PlanState
  var observation: PlanObservation
```

### 親子の lifecycle

子への着手には、子自身の採用と親の進行中を要求する。所属なしの Issue は自身の基本遷移だけに従う。進行中の子があれば親を未着手へ戻せない。計画の完了・取りやめには全所属 Issue の終了が必要で、未判断もその妨げになる。

完了にはさらに計画全体の最終確認が通ったという入力を要求する。`reviewPassed` は確認結果の抽象入力であり、CLI 引数や保存方法、専用操作か最終チェック Issue かを決めるものではない。空の計画でもこの確認を省略しない。子の終了だけで親を自動終了しない。

```quint target/literate/group_lifecycle_proposal.qnt +=
  pure def terminal(current: Lifecycle): bool = Set(Completed, Cancelled).contains(current)
  pure def children(s: PlanState, group: Id): Set[Id] =
    s.registered.filter(id => s.parents.get(id) == InGroup(group))
  pure def allChildrenTerminal(s: PlanState, group: Id): bool =
    children(s, group).forall(id => terminal(s.issues.get(id)))
  pure def hasWorkingChild(s: PlanState, group: Id): bool =
    children(s, group).exists(id => s.issues.get(id) == InProgress)
  pure def parentOpen(s: PlanState, parent: Parent): bool = match parent {
    | Unassigned => true
    | InGroup(group) => not(terminal(s.groups.get(group)))
  }
  pure def parentWorking(s: PlanState, parent: Parent): bool = match parent {
    | Unassigned => true
    | InGroup(group) => s.groups.get(group) == InProgress
  }

  pure def canGroup(s: PlanState, id: Id, op: Operation, passed: bool): bool = and {
    canPerform(s.groups.get(id), op),
    op != Release or not(hasWorkingChild(s, id)),
    not(Set(Complete, Cancel).contains(op)) or allChildrenTerminal(s, id),
    op != Complete or passed,
  }
  pure def applyGroup(s: PlanState, id: Id, op: Operation): PlanState =
    { ...s, groups: s.groups.set(id, applyOperation(s.groups.get(id), op)) }

  pure def canIssue(s: PlanState, id: Id, op: Operation): bool = and {
    s.registered.contains(id),
    canPerform(s.issues.get(id), op),
    parentOpen(s, s.parents.get(id)),
    op != Start or parentWorking(s, s.parents.get(id)),
  }
  pure def applyIssue(s: PlanState, id: Id, op: Operation): PlanState =
    { ...s, issues: s.issues.set(id, applyOperation(s.issues.get(id), op)) }

  pure def observe(s: PlanState, event: PlanEvent, passed: bool): PlanObservation =
    { before: s, event: event, reviewPassed: passed }

  action init = {
    val initial = {
      groups: GROUPS.mapBy(_ => Undecided),
      issues: ISSUES.mapBy(_ => Undecided),
      registered: Set(0, 1),
      parents: ISSUES.mapBy(id => if (id == 2) Unassigned else InGroup(0)),
    }
    all {
      plan' = initial,
      observation' = observe(initial, Initial, false),
    }
  }
  action performGroup(id: Id, op: Operation, passed: bool): bool = all {
    canGroup(plan, id, op, passed),
    plan' = applyGroup(plan, id, op),
    observation' = observe(plan, GroupOperation({ id: id, op: op }), passed),
  }
  action performIssue(id: Id, op: Operation): bool = all {
    canIssue(plan, id, op),
    plan' = applyIssue(plan, id, op),
    observation' = observe(plan, IssueOperation({ id: id, op: op }), false),
  }

```

### 所属変更と新規登録

進行中の Issue は、所属なしにするか、進行中の別計画へ移せる。移動元・移動先に計画がある場合は、どちらも完了・取りやめでないことを検査する。移動のためだけに release と start を挟む必要はなく、移動によって採用や進行の状態は変わらない。

未登録の枠は、計画内または計画外へ未判断で登録する。登録しただけでは着手できない。最終確認待ちの計画に不足 Issue を追加した場合、その Issue が未判断なので、計画は再び完了できなくなる。実際に最終確認が不合格となった理由はモデル外である。

```quint target/literate/group_lifecycle_proposal.qnt +=
  pure def destinationAllowed(s: PlanState, parent: Parent, current: Lifecycle): bool =
    parentOpen(s, parent) and (current != InProgress or parentWorking(s, parent))
  pure def canMove(s: PlanState, id: Id, parent: Parent): bool = and {
    s.registered.contains(id),
    s.parents.get(id) != parent,
    parentOpen(s, s.parents.get(id)),
    destinationAllowed(s, parent, s.issues.get(id)),
  }
  pure def applyMove(s: PlanState, id: Id, parent: Parent): PlanState =
    { ...s, parents: s.parents.set(id, parent) }
  action moveIssue(id: Id, parent: Parent): bool = all {
    canMove(plan, id, parent),
    plan' = applyMove(plan, id, parent),
    observation' = observe(plan, Reparent({ id: id, parent: parent }), false),
  }

  pure def canRegister(s: PlanState, id: Id, parent: Parent): bool = and {
    not(s.registered.contains(id)),
    destinationAllowed(s, parent, Undecided),
  }
  pure def applyRegister(s: PlanState, id: Id, parent: Parent): PlanState = {
    ...s,
    registered: s.registered.union(Set(id)),
    issues: s.issues.set(id, Undecided),
    parents: s.parents.set(id, parent),
  }
  action registerIssue(id: Id, parent: Parent): bool = all {
    canRegister(plan, id, parent),
    plan' = applyRegister(plan, id, parent),
    observation' = observe(plan, RegisterIssue({ id: id, parent: parent }), false),
  }

```

### 探索

基本遷移、所属変更、新規登録を混ぜて実行する。登録済み集合と所属は別に保持し、計画の外へ出しても Issue を削除しない。同じ所属の再指定は無効とする。ID は固定集合から選び、未登録 ID の再利用・削除は扱わない。

```quint target/literate/group_lifecycle_proposal.qnt +=
  action step = {
    nondet group = GROUPS.oneOf()
    nondet id = ISSUES.oneOf()
    nondet op = OPERATIONS.oneOf()
    nondet passed = Set(false, true).oneOf()
    nondet parent = PARENTS.oneOf()
    any {
      performGroup(group, op, passed),
      performIssue(id, op),
      moveIssue(id, parent),
      registerIssue(id, parent),
    }
  }
```

### 性質と到達性

`observation` は直前の状態と操作の検証専用記録であり、製品の保存項目ではない。以下を確認する。

- 進行中の子の親は進行中であり、終了した計画の子はすべて終了している。
- 完了済みの計画・Issue は完了のまま。
- 完了・取りやめの計画の構成は変わらず、追加・取り外し操作も許可されない。
- 計画の完了は、最終確認を経た明示操作でのみ起きる。
- 所属変更は lifecycle を変えず、各操作は他の Entity を自動更新しない。
- 新規登録は未判断であり、採用を代行しない。
- 未登録の枠が計画に混入せず、所属は高々一つである。
- 未終了の仕事や再検討可能な計画が残っている間、全 lifecycle 操作が行き止まりにならない。

到達性では各操作に加え、進行中の移動、未判断を計画外へ出して完了可能になる場合、最終確認待ちからの新規 Issue 追加を観測する。

```quint target/literate/group_lifecycle_proposal.qnt +=
  def sawGroup(op: Operation): bool = match observation.event {
    | GroupOperation(change) => change.op == op
    | _ => false
  }
  def sawIssue(op: Operation): bool = match observation.event {
    | IssueOperation(change) => change.op == op
    | _ => false
  }
  val wGroupAccept = sawGroup(Accept)
  val wGroupWithdraw = sawGroup(Withdraw)
  val wGroupStart = sawGroup(Start)
  val wGroupRelease = sawGroup(Release)
  val wGroupComplete = sawGroup(Complete)
  val wGroupCancel = sawGroup(Cancel)
  val wGroupReconsider = sawGroup(Reconsider)
  val wIssueAccept = sawIssue(Accept)
  val wIssueWithdraw = sawIssue(Withdraw)
  val wIssueStart = sawIssue(Start)
  val wIssueRelease = sawIssue(Release)
  val wIssueComplete = sawIssue(Complete)
  val wIssueCancel = sawIssue(Cancel)
  val wIssueReconsider = sawIssue(Reconsider)
  val wMixedAdoption = GROUPS.exists(g => Set(NotStarted, InProgress).contains(plan.groups.get(g))
    and children(plan, g).exists(id => plan.issues.get(id) == Undecided)
    and children(plan, g).exists(id => plan.issues.get(id) == NotStarted))
  val wParentStartRequired = GROUPS.exists(g => plan.groups.get(g) == NotStarted
    and children(plan, g).exists(id => plan.issues.get(id) == NotStarted and not(canIssue(plan, id, Start))))
  val wParentReleaseBlocked = GROUPS.exists(g => hasWorkingChild(plan, g) and not(canGroup(plan, g, Release, false)))
  val wAwaitingFinalCheck = GROUPS.exists(g => plan.groups.get(g) == InProgress
    and children(plan, g).size() > 0 and allChildrenTerminal(plan, g))
  val wFailedCheckCannotComplete = GROUPS.exists(g => plan.groups.get(g) == InProgress
    and children(plan, g).size() > 0 and allChildrenTerminal(plan, g) and not(canGroup(plan, g, Complete, false)))
  val wCompletedWithCancelledChild = GROUPS.exists(g => plan.groups.get(g) == Completed
    and children(plan, g).exists(id => plan.issues.get(id) == Cancelled))
  val wCompletedAllChildrenDone = GROUPS.exists(g => plan.groups.get(g) == Completed
    and children(plan, g).size() > 0 and children(plan, g).forall(id => plan.issues.get(id) == Completed))
  val wBothChildrenWorking = GROUPS.exists(g =>
    children(plan, g).filter(id => plan.issues.get(id) == InProgress).size() >= 2)
  val wUndecidedChildBlocksClosure = GROUPS.exists(g => plan.groups.get(g) == InProgress
    and children(plan, g).exists(id => plan.issues.get(id) == Undecided)
    and not(canGroup(plan, g, Complete, true)) and not(canGroup(plan, g, Cancel, false)))

  val wAttach = match observation.event {
    | Reparent(change) => observation.before.parents.get(change.id) == Unassigned and change.parent != Unassigned
    | _ => false
  }
  val wDetach = match observation.event {
    | Reparent(change) => change.parent == Unassigned
    | _ => false
  }
  val wMoveBetweenPlans = match observation.event {
    | Reparent(change) => observation.before.parents.get(change.id) != Unassigned and change.parent != Unassigned
    | _ => false
  }
  val wWorkingDetach = wDetach and match observation.event {
    | Reparent(change) => plan.issues.get(change.id) == InProgress
    | _ => false
  }
  val wWorkingMove = wMoveBetweenPlans and match observation.event {
    | Reparent(change) => plan.issues.get(change.id) == InProgress
    | _ => false
  }
  val wRegister = match observation.event {
    | RegisterIssue(_) => true
    | _ => false
  }
  val wRegisterOutside = match observation.event {
    | RegisterIssue(change) => change.parent == Unassigned
    | _ => false
  }
  val wAddAfterFinalCheckReady = match observation.event {
    | RegisterIssue(change) => GROUPS.exists(g => change.parent == InGroup(g)
        and observation.before.groups.get(g) == InProgress
        and children(observation.before, g).size() > 0
        and allChildrenTerminal(observation.before, g)
        and not(canGroup(plan, g, Complete, true)))
    | _ => false
  }
  val wReconsiderAllowsMembership = match observation.event {
    | GroupOperation(change) => change.op == Reconsider and ISSUES.exists(id =>
        canMove(plan, id, InGroup(change.id))
        or (plan.parents.get(id) == InGroup(change.id) and canMove(plan, id, Unassigned)))
    | _ => false
  }
  val wRemoveUndecidedForCompletion = match observation.event {
    | Reparent(change) => observation.before.issues.get(change.id) == Undecided
      and GROUPS.exists(g => observation.before.parents.get(change.id) == InGroup(g)
        and plan.groups.get(g) == InProgress and canGroup(plan, g, Complete, true))
    | _ => false
  }

  val invParentOfWorkingChild = plan.registered.forall(id =>
    plan.issues.get(id) == InProgress implies parentWorking(plan, plan.parents.get(id)))
  val invClosedChildrenTerminal = GROUPS.forall(g =>
    terminal(plan.groups.get(g)) implies allChildrenTerminal(plan, g))
  val invCompletedStayCompleted = and {
    GROUPS.forall(g => observation.before.groups.get(g) == Completed implies plan.groups.get(g) == Completed),
    observation.before.registered.forall(id =>
      observation.before.issues.get(id) == Completed implies plan.issues.get(id) == Completed),
  }
  val invClosedStructureFrozen = GROUPS.forall(g => terminal(observation.before.groups.get(g))
    implies children(plan, g) == children(observation.before, g))
  val invCompletionReviewed = GROUPS.forall(g =>
    (plan.groups.get(g) == Completed and observation.before.groups.get(g) != Completed)
      implies (observation.event == GroupOperation({ id: g, op: Complete }) and observation.reviewPassed))
  val invOperationScope = match observation.event {
    | Reparent(change) => and {
        plan.groups == observation.before.groups,
        plan.issues == observation.before.issues,
        plan.registered == observation.before.registered,
        ISSUES.exclude(Set(change.id)).forall(id => plan.parents.get(id) == observation.before.parents.get(id)),
      }
    | RegisterIssue(change) => and {
        plan.groups == observation.before.groups,
        plan.registered == observation.before.registered.union(Set(change.id)),
        plan.issues.get(change.id) == Undecided,
        ISSUES.exclude(Set(change.id)).forall(id =>
          plan.issues.get(id) == observation.before.issues.get(id)
          and plan.parents.get(id) == observation.before.parents.get(id)),
      }
    | Initial => plan == observation.before
    | GroupOperation(change) => and {
        plan.issues == observation.before.issues,
        plan.parents == observation.before.parents,
        plan.registered == observation.before.registered,
        GROUPS.exclude(Set(change.id)).forall(g => plan.groups.get(g) == observation.before.groups.get(g)),
      }
    | IssueOperation(change) => and {
        plan.groups == observation.before.groups,
        plan.parents == observation.before.parents,
        plan.registered == observation.before.registered,
        ISSUES.exclude(Set(change.id)).forall(id => plan.issues.get(id) == observation.before.issues.get(id)),
      }
  }
  val invMembershipValid = and {
    plan.registered.subseteq(ISSUES),
    ISSUES.forall(id => PARENTS.contains(plan.parents.get(id))),
    ISSUES.exclude(plan.registered).forall(id => plan.parents.get(id) == Unassigned),
  }
  val invRegistrationNeverAdopts = match observation.event {
    | RegisterIssue(change) => plan.issues.get(change.id) == Undecided
      and not(canIssue(plan, change.id, Start))
    | _ => true
  }
  val invClosedEditsDisabled = GROUPS.forall(g => terminal(plan.groups.get(g)) implies
    ISSUES.forall(id => and {
      not(canMove(plan, id, InGroup(g))),
      not(canRegister(plan, id, InGroup(g))),
      plan.parents.get(id) == InGroup(g) implies PARENTS.forall(parent => not(canMove(plan, id, parent))),
    }))
  val invNoUnexpectedDeadEnd =
    (GROUPS.exists(g => plan.groups.get(g) != Completed)
      or plan.registered.exists(id => not(terminal(plan.issues.get(id))))) implies or {
        GROUPS.exists(g => OPERATIONS.exists(op => canGroup(plan, g, op, true))),
        plan.registered.exists(id => OPERATIONS.exists(op => canIssue(plan, id, op))),
      }
}
```

### 今回の境界

所属変更と未判断での新規登録を、計画2件・Issue枠3件の範囲で扱う。取りやめた計画は再検討後に構成を変更できるが、完了した計画には再開経路を持たせない。

最終確認が通るかは抽象入力であり、確認工程の実装や不合格理由は扱わない。入れ子の計画、dependency、claim、再浮上と親子の着手・所属変更の組み合わせ、declaration 編集、実際の ID 発行や永続化は検証対象外である。単独 Issue の再浮上モデルとの統合は後の検討に残す。

このモデルには、追加できる Issue が1件だけという探索上の上限がある。新規登録が無効になることは、製品で追加件数を制限する提案ではない。無限件数への一般化や、必ず最終確認に合格し計画が完了することは保証しない。

### 所属モデルの再現と結果

2026-09-10、lmt `v0.0.0-20210421124901-62fe18f2f6a6` で生成後、Quint 0.32.0、Rust backend、10,000 traces、最大200 steps、入力 seed `2026091004` で実行した。10 invariant に反例はなく、33 witness はすべて1 trace以上で観測された。進行中の計画間移動は194 traces、最終確認待ちからの新規登録は2 tracesで観測された。これは bounded random simulation の結果であり、全状態の証明ではない。

単独 Issue と再浮上のモデル、および共有する基本遷移のコードは変更していない。

```sh
lmt spec/lifecycle_proposal.md
quint typecheck target/literate/group_lifecycle_proposal.qnt
quint run target/literate/group_lifecycle_proposal.qnt \
  --invariants \
    invParentOfWorkingChild invClosedChildrenTerminal invCompletedStayCompleted \
    invClosedStructureFrozen invCompletionReviewed invOperationScope \
    invMembershipValid invRegistrationNeverAdopts invClosedEditsDisabled \
    invNoUnexpectedDeadEnd \
  --witnesses \
    wGroupAccept wGroupWithdraw wGroupStart \
    wGroupRelease wGroupComplete wGroupCancel \
    wGroupReconsider wIssueAccept wIssueWithdraw \
    wIssueStart wIssueRelease wIssueComplete \
    wIssueCancel wIssueReconsider wMixedAdoption \
    wParentStartRequired wParentReleaseBlocked wAwaitingFinalCheck \
    wFailedCheckCannotComplete wCompletedWithCancelledChild wCompletedAllChildrenDone \
    wBothChildrenWorking wUndecidedChildBlocksClosure wAttach \
    wDetach wMoveBetweenPlans wWorkingDetach \
    wWorkingMove wRegister wRegisterOutside \
    wAddAfterFinalCheckReady wReconsiderAllowsMembership wRemoveUndecidedForCompletion \
  --max-samples 10000 --max-steps 200 --seed 2026091004 --backend rust --verbosity 1
```
