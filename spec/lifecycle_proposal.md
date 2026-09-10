# Progress と採否を統合する検討用モデル

この literate specification は、Progress と採否を単一の lifecycle にまとめる案を扱う。単独 Issue の再浮上を基本モデルで扱い、計画・子 Issue の着手境界、所属変更、Issue 間の dependency、再浮上による候補の選別を統合モデルで検討する。既存機能やデータ形式との互換性を前提にせず、新しい土台として設計するためのモデルであり、このファイルの追加・更新で現行実装や既存モデルは変更しない。

説明と実行可能なモデルをこのファイルで管理する。[Quint の literate 形式](https://quint.sh/docs/literate)に従い、`lmt` で `target/literate/` 配下に共有する基本遷移、単独 Issue、計画と子 Issue の3ファイルを生成する。生成ファイルは直接編集しない。

## 状態

提案として登録した直後の `Undecided`（未判断）から開始する。登録操作自体は扱わない。未判断は記録済みの提案であり、実施の約束を意味しない。採用によって `NotStarted`（未着手）へ進む。

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

提案として登録した直後の未判断から始め、採用済み・完了済みの観測はどちらも偽にする。各操作は許可条件と状態更新を組み合わせる。`step` は7操作と外部入力の変化を探索候補にし、許可条件を満たすものだけが実行される。外部入力はまず不成立から開始し、成立・不成立の両方向へ変化できる。

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

## 計画・所属・dependency・再浮上

計画2件、Issue 用の固定 ID 3件を使う。最初は2 Issue が計画0に所属し、残り1件は未登録の枠とする。未登録はモデルの探索領域を有限にする仕組みであり、新しい lifecycle ではない。初期配置の計画・Issue は未判断から始める。追加する Issue には、未判断の提案と採用済みの仕事の二つの登録経路を設ける。計画そのものの登録操作は今回のモデル外である。

所属は最大一つで、所属なしも許す。所属変更は既存の親子の制約を壊さない限り許可し、lifecycle を変えない。完了・取りやめの計画への追加と、そこからの取り外しは不可とする。取りやめた計画は、明示的に再検討へ戻せば構成を変更できる。この取りやめ時の構成固定と計画の再検討は暫定採用の判断であり、運用上の負担が分かれば見直す。

```quint target/literate/group_lifecycle_proposal.qnt +=
module group_lifecycle_proposal {
  import lifecycle_rules.* from "lifecycle_rules"

  type Id = int
  type Parent = Unassigned | InGroup(Id)
  type Registration = Proposal | AdoptedWork
  pure val GROUPS = Set(0, 1)
  pure val ISSUES = Set(0, 1, 2)
  pure val PARENTS = Set(Unassigned, InGroup(0), InGroup(1))
  pure val OPERATIONS = Set(Accept, Withdraw, Start, Release, Complete, Cancel, Reconsider)
  type PlanState = {
    groups: Id -> Lifecycle,
    issues: Id -> Lifecycle,
    registered: Set[Id],
    parents: Id -> Parent,
    dependencies: Id -> Set[Id],
  }
  type Conditions = { groups: Id -> bool, issues: Id -> bool }
  type ConditionResult = NotEvaluated | Evaluated(bool)
  type PlanEvent = Initial
    | GroupConditionChanged(Id)
    | IssueConditionChanged(Id)
    | GroupOperation({ id: Id, op: Operation })
    | IssueOperation({ id: Id, op: Operation })
    | Reparent({ id: Id, parent: Parent })
    | RegisterIssue({ id: Id, parent: Parent, registration: Registration })
    | AddDependency({ source: Id, target: Id })
    | RemoveDependency({ source: Id, target: Id })
  type PlanObservation = { before: PlanState, beforeConditions: Conditions, event: PlanEvent, reviewPassed: bool }
  var plan: PlanState
  var conditions: Conditions
  var observation: PlanObservation
```

### 親子の lifecycle

子への着手には、子自身の採用と親の進行中を要求する。所属なしの Issue にも、基本遷移に加えて dependency の着手・完了条件を適用する。進行中の子があれば親を未着手へ戻せない。計画の完了・取りやめには全所属 Issue の終了が必要で、未判断もその妨げになる。

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

  pure def prerequisitesComplete(s: PlanState, id: Id): bool =
    s.dependencies.get(id).forall(target => s.issues.get(target) == Completed)

  pure def canIssue(s: PlanState, id: Id, op: Operation): bool = and {
    s.registered.contains(id),
    canPerform(s.issues.get(id), op),
    parentOpen(s, s.parents.get(id)),
    op != Start or parentWorking(s, s.parents.get(id)),
    not(Set(Start, Complete).contains(op)) or prerequisitesComplete(s, id),
  }
  pure def applyIssue(s: PlanState, id: Id, op: Operation): PlanState =
    { ...s, issues: s.issues.set(id, applyOperation(s.issues.get(id), op)) }

  pure def observe(s: PlanState, c: Conditions, event: PlanEvent, passed: bool): PlanObservation =
    { before: s, beforeConditions: c, event: event, reviewPassed: passed }

  action init = {
    val initial = {
      groups: GROUPS.mapBy(_ => Undecided),
      issues: ISSUES.mapBy(_ => Undecided),
      registered: Set(0, 1),
      parents: ISSUES.mapBy(id => if (id == 2) Unassigned else InGroup(0)),
      dependencies: ISSUES.mapBy(_ => Set()),
    }
    val initialConditions = { groups: GROUPS.mapBy(_ => false), issues: ISSUES.mapBy(_ => false) }
    all {
      plan' = initial,
      conditions' = initialConditions,
      observation' = observe(initial, initialConditions, Initial, false),
    }
  }
  action performGroup(id: Id, op: Operation, passed: bool): bool = all {
    canGroup(plan, id, op, passed),
    plan' = applyGroup(plan, id, op),
    conditions' = conditions,
    observation' = observe(plan, conditions, GroupOperation({ id: id, op: op }), passed),
  }
  action performIssue(id: Id, op: Operation): bool = all {
    canIssue(plan, id, op),
    plan' = applyIssue(plan, id, op),
    conditions' = conditions,
    observation' = observe(plan, conditions, IssueOperation({ id: id, op: op }), false),
  }

```

### 所属変更と新規登録

進行中の Issue は、所属なしにするか、進行中の別計画へ移せる。移動元・移動先に計画がある場合は、どちらも完了・取りやめでないことを検査する。移動のためだけに release と start を挟む必要はなく、移動によって採用や進行の状態は変わらない。

未登録の枠は、計画内または計画外へ登録する。提案の記録（capture 相当）は `Undecided`、採用済みの仕事の登録（plan 相当）は `NotStarted` とする。登録操作は与えられた採否を反映し、採用判断そのものは代行しない。判断の主体や CLI 名はここでは定めない。

どちらも登録だけでは進行中にならない。採用済みの登録後も、明示的な着手には親計画と dependency の条件を要求する。最終確認待ちの計画に不足 Issue を追加した場合、どちらの経路でも未終了の子が増えるため、計画は再び完了できなくなる。実際に最終確認が不合格となった理由はモデル外である。

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
    conditions' = conditions,
    observation' = observe(plan, conditions, Reparent({ id: id, parent: parent }), false),
  }

  pure def registrationLifecycle(registration: Registration): Lifecycle = match registration {
    | Proposal => Undecided
    | AdoptedWork => NotStarted
  }
  pure def canRegister(s: PlanState, id: Id, parent: Parent): bool = and {
    not(s.registered.contains(id)),
    parentOpen(s, parent),
  }
  pure def applyRegister(s: PlanState, id: Id, parent: Parent, registration: Registration): PlanState = {
    ...s,
    registered: s.registered.union(Set(id)),
    issues: s.issues.set(id, registrationLifecycle(registration)),
    parents: s.parents.set(id, parent),
  }
  action registerIssue(id: Id, parent: Parent, registration: Registration): bool = all {
    canRegister(plan, id, parent),
    plan' = applyRegister(plan, id, parent, registration),
    conditions' = conditions,
    observation' = observe(plan, conditions, RegisterIssue({ id: id, parent: parent, registration: registration }), false),
  }

```

### Issue 間の dependency

依存先がすべて完了するまで、依存元は着手・完了できない。依存先の取りやめは条件を満たさない。進行中でも未完了の依存先を追加でき、lifecycle は変えない。この着手後の追加は暫定採用の判断である。

完了した Issue 自身の依存関係は固定する。完了済み Issue を、別の Issue が前提として参照することは許す。未完了の Issue の前提は、判断に応じて追加・削除する。所属変更は dependency を保持し、計画をまたぐ依存と所属なしの Issue への依存を許す。

自己依存と、任意長の循環を追加時に拒否する。モデルでは3 ID を使い、最大3辺の到達集合を求める。完了済み Issue もグラフから除外しない。

```quint target/literate/group_lifecycle_proposal.qnt +=
  pure def reachable(s: PlanState, source: Id): Set[Id] =
    ISSUES.fold(Set(), (seen, _) => seen.union(s.dependencies.get(source))
      .union(seen.map(id => s.dependencies.get(id)).flatten()))
  pure def canAddDependency(s: PlanState, source: Id, target: Id): bool = and {
    s.registered.contains(source),
    s.registered.contains(target),
    s.issues.get(source) != Completed,
    source != target,
    not(s.dependencies.get(source).contains(target)),
    not(reachable(s, target).contains(source)),
  }
  pure def applyAddDependency(s: PlanState, source: Id, target: Id): PlanState =
    { ...s, dependencies: s.dependencies.set(source, s.dependencies.get(source).union(Set(target))) }
  action addDependency(source: Id, target: Id): bool = all {
    canAddDependency(plan, source, target),
    plan' = applyAddDependency(plan, source, target),
    conditions' = conditions,
    observation' = observe(plan, conditions, AddDependency({ source: source, target: target }), false),
  }
```

```quint target/literate/group_lifecycle_proposal.qnt +=
  pure def canRemoveDependency(s: PlanState, source: Id, target: Id): bool = and {
    s.registered.contains(source),
    s.issues.get(source) != Completed,
    s.dependencies.get(source).contains(target),
  }
  pure def applyRemoveDependency(s: PlanState, source: Id, target: Id): PlanState =
    { ...s, dependencies: s.dependencies.set(source, s.dependencies.get(source).exclude(Set(target))) }
  action removeDependency(source: Id, target: Id): bool = all {
    canRemoveDependency(plan, source, target),
    plan' = applyRemoveDependency(plan, source, target),
    conditions' = conditions,
    observation' = observe(plan, conditions, RemoveDependency({ source: source, target: target }), false),
  }
```

### 再浮上・判断候補・着手候補・着手中

再浮上条件は、各計画・Issue の成立・不成立を外部入力として扱う。条件の種類や設定操作は今回も抽象化する。以下の集合を分け、一覧のコマンド構成や表示形式は決めない。

| 導出する集合 | 条件 |
| --- | --- |
| 判断候補の Issue | 未判断で、自身と親が浮上している。親の着手・依存先の完了は要求しない |
| 着手可能な Issue | 未着手、親があれば進行中、全依存先が完了 |
| 着手候補の Issue | 着手可能で、自身と親が浮上している |
| 着手中の Issue | 進行中。自身・親の浮上状態や追加された未完了の依存に関係なく含む |

未判断の計画は、自身が浮上していれば判断候補とする。判断候補は採否を検討するための集合であり、候補への出入りで採否や進行状態を変えない。dependency は着手・完了の前提であり、採否を先に決めることを妨げない。

計画自身も、未着手なら明示的に着手可能とし、候補に出すときだけ自身の浮上を要求する。進行中の計画は非浮上でも着手中に含む。再浮上は候補の表示を制御し、明示的な採否判断、着手や完了、所属・依存変更の可否には加えない。この表示と明示操作の分離は暫定採用の判断である。

取りやめ・完了の Entity は条件を評価せず、浮上しない。親が非浮上なら子を判断候補・着手候補から外すが、子の状態や条件は変えない。評価結果は保存せず、入力と状態から導出する。

```quint target/literate/group_lifecycle_proposal.qnt +=
  pure def groupConditionResult(s: PlanState, c: Conditions, id: Id): ConditionResult =
    if (terminal(s.groups.get(id))) NotEvaluated else Evaluated(c.groups.get(id))
  pure def issueConditionResult(s: PlanState, c: Conditions, id: Id): ConditionResult =
    if (not(s.registered.contains(id)) or terminal(s.issues.get(id))) NotEvaluated
    else Evaluated(c.issues.get(id))
  pure def parentSurfaced(s: PlanState, c: Conditions, parent: Parent): bool = match parent {
    | Unassigned => true
    | InGroup(id) => groupConditionResult(s, c, id) == Evaluated(true)
  }
  pure def judgmentIssues(s: PlanState, c: Conditions): Set[Id] =
    s.registered.filter(id => s.issues.get(id) == Undecided
      and issueConditionResult(s, c, id) == Evaluated(true)
      and parentSurfaced(s, c, s.parents.get(id)))
  pure def judgmentGroups(s: PlanState, c: Conditions): Set[Id] =
    GROUPS.filter(id => s.groups.get(id) == Undecided
      and groupConditionResult(s, c, id) == Evaluated(true))
  pure def startableIssues(s: PlanState): Set[Id] = s.registered.filter(id => canIssue(s, id, Start))
  pure def candidateIssues(s: PlanState, c: Conditions): Set[Id] =
    startableIssues(s).filter(id => issueConditionResult(s, c, id) == Evaluated(true)
      and parentSurfaced(s, c, s.parents.get(id)))
  pure def workingIssues(s: PlanState): Set[Id] =
    s.registered.filter(id => s.issues.get(id) == InProgress)
  pure def startableGroups(s: PlanState): Set[Id] = GROUPS.filter(id => canGroup(s, id, Start, false))
  pure def candidateGroups(s: PlanState, c: Conditions): Set[Id] =
    startableGroups(s).filter(id => groupConditionResult(s, c, id) == Evaluated(true))
  pure def workingGroups(s: PlanState): Set[Id] = GROUPS.filter(id => s.groups.get(id) == InProgress)
```

外部入力の変化は、保存する lifecycle・所属・dependency を変えない。取りやめ・完了の間にも外界は変化するが、その Entity の条件を評価するという意味ではない。

```quint target/literate/group_lifecycle_proposal.qnt +=
  pure def canChangeGroupCondition(c: Conditions, id: Id, satisfied: bool): bool =
    c.groups.get(id) != satisfied
  pure def applyGroupCondition(c: Conditions, id: Id, satisfied: bool): Conditions =
    { ...c, groups: c.groups.set(id, satisfied) }
  action changeGroupCondition(id: Id, satisfied: bool): bool = all {
    canChangeGroupCondition(conditions, id, satisfied),
    plan' = plan,
    conditions' = applyGroupCondition(conditions, id, satisfied),
    observation' = observe(plan, conditions, GroupConditionChanged(id), false),
  }
```

```quint target/literate/group_lifecycle_proposal.qnt +=
  pure def canChangeIssueCondition(s: PlanState, c: Conditions, id: Id, satisfied: bool): bool =
    s.registered.contains(id) and c.issues.get(id) != satisfied
  pure def applyIssueCondition(c: Conditions, id: Id, satisfied: bool): Conditions =
    { ...c, issues: c.issues.set(id, satisfied) }
  action changeIssueCondition(id: Id, satisfied: bool): bool = all {
    canChangeIssueCondition(plan, conditions, id, satisfied),
    plan' = plan,
    conditions' = applyIssueCondition(conditions, id, satisfied),
    observation' = observe(plan, conditions, IssueConditionChanged(id), false),
  }
```

### 探索

基本遷移、所属変更、新規登録、dependency の追加・削除、再浮上条件の成立・不成立を混ぜて実行する。条件入力はすべて不成立から開始し、登録済み Issue と計画について両方向へ変化できる。これは探索の開始点であり、製品の再浮上条件の初期値を決めるものではない。登録済み集合と所属は別に保持し、計画の外へ出しても Issue を削除しない。同じ所属の再指定は無効とする。ID は固定集合から選び、未登録 ID の再利用・削除は扱わない。

```quint target/literate/group_lifecycle_proposal.qnt +=
  action step = {
    nondet group = GROUPS.oneOf()
    nondet id = ISSUES.oneOf()
    nondet op = OPERATIONS.oneOf()
    nondet passed = Set(false, true).oneOf()
    nondet parent = PARENTS.oneOf()
    nondet target = ISSUES.oneOf()
    nondet registration = Set(Proposal, AdoptedWork).oneOf()
    any {
      performGroup(group, op, passed),
      performIssue(id, op),
      moveIssue(id, parent),
      registerIssue(id, parent, registration),
      addDependency(id, target),
      removeDependency(id, target),
      changeGroupCondition(group, passed),
      changeIssueCondition(id, passed),
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
- 新規登録は与えられた採否に対応する初期状態となり、自動で着手しない。
- 未登録の枠が計画に混入せず、所属は高々一つである。
- dependency は登録済み Issue 間だけを参照し、自己依存・循環を持たない。
- 着手・完了時に全依存先が完了しており、完了後もその前提は満たされたままである。
- 完了した Issue 自身の dependency は固定される。
- dependency 操作は lifecycle や所属を変えず、他の操作は dependency を変えない。
- 条件入力の変化は保存状態・着手可否・着手中の集合を変えず、明示操作は条件入力を変えない。
- 判断候補は未判断かつ自身・親が浮上しているものとし、dependency の変更は判断候補を変えない。
- 判断候補は着手候補・着手中と重ならない。
- 着手候補は着手可能なものに限り、自身と親の浮上条件に一致する。
- 進行中の Issue と計画は、非浮上でも着手中に含む。
- 未登録・取りやめ・完了の Issue と、取りやめ・完了の計画は条件を評価しない。
- 未終了の仕事や再検討可能な計画が残っている間、全 lifecycle 操作が行き止まりにならない。

到達性では各操作に加え、進行中の移動、未判断を計画外へ出して完了可能になる場合、最終確認待ちからの新規 Issue 追加を観測する。dependency については、進行中の追加、計画をまたぐ依存、前提の取りやめによる阻害、前提の完了による着手・完了解禁、直接・間接循環の拒否を確認する。再浮上については、親の条件変化による候補の除外と復帰、非浮上のままの明示的な着手、進行中の仕事の保持を確認する。判断候補については、親が未着手のケース、未完了の依存先があるケース、親の浮上による除外と復帰を観測する。親が非浮上でも明示的に採用でき、依存先の完了前にも採用できることを確認する。

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
  val wRegisterProposal = match observation.event {
    | RegisterIssue(change) => change.registration == Proposal
    | _ => false
  }
  val wRegisterAdopted = match observation.event {
    | RegisterIssue(change) => change.registration == AdoptedWork
    | _ => false
  }
  val wAdoptedRegistrationWaitsForParent = match observation.event {
    | RegisterIssue(change) => change.registration == AdoptedWork
      and GROUPS.exists(g => change.parent == InGroup(g) and plan.groups.get(g) == NotStarted)
      and not(canIssue(plan, change.id, Start))
    | _ => false
  }
  val wAdoptedRegistrationCanStart = match observation.event {
    | RegisterIssue(change) => change.registration == AdoptedWork and canIssue(plan, change.id, Start)
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
    | GroupConditionChanged(_) => plan == observation.before
    | IssueConditionChanged(_) => plan == observation.before
    | RemoveDependency(_) => plan.groups == observation.before.groups
      and plan.issues == observation.before.issues
      and plan.parents == observation.before.parents
      and plan.registered == observation.before.registered
    | AddDependency(_) => plan.groups == observation.before.groups
      and plan.issues == observation.before.issues
      and plan.parents == observation.before.parents
      and plan.registered == observation.before.registered
    | Reparent(change) => and {
        plan.groups == observation.before.groups,
        plan.issues == observation.before.issues,
        plan.registered == observation.before.registered,
        ISSUES.exclude(Set(change.id)).forall(id => plan.parents.get(id) == observation.before.parents.get(id)),
      }
    | RegisterIssue(change) => and {
        plan.groups == observation.before.groups,
        plan.registered == observation.before.registered.union(Set(change.id)),
        plan.parents.get(change.id) == change.parent,
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
  val invDependencyOperationScope = match observation.event {
    | RemoveDependency(change) => ISSUES.exclude(Set(change.source)).forall(id =>
        plan.dependencies.get(id) == observation.before.dependencies.get(id))
    | AddDependency(change) => ISSUES.exclude(Set(change.source)).forall(id =>
        plan.dependencies.get(id) == observation.before.dependencies.get(id))
    | _ => plan.dependencies == observation.before.dependencies
  }
  val invDependenciesValid = ISSUES.forall(source =>
    plan.dependencies.get(source).subseteq(plan.registered)
      and (not(plan.registered.contains(source)) implies plan.dependencies.get(source).size() == 0))
  val invDependenciesAcyclic = ISSUES.forall(id => not(reachable(plan, id).contains(id)))
  val invCompletedPrerequisites = plan.registered.forall(id =>
    plan.issues.get(id) == Completed implies prerequisitesComplete(plan, id))
  val invStartAndCompleteRequirePrerequisites = match observation.event {
    | IssueOperation(change) => Set(Start, Complete).contains(change.op)
        implies prerequisitesComplete(observation.before, change.id)
    | _ => true
  }
  val invCompletedDependenciesFrozen = observation.before.registered.forall(id =>
    observation.before.issues.get(id) == Completed
      implies plan.dependencies.get(id) == observation.before.dependencies.get(id))
  val wAddDependency = match observation.event {
    | AddDependency(_) => true
    | _ => false
  }
  val wAddDuringWorkBlocksCompletion = match observation.event {
    | AddDependency(change) => plan.issues.get(change.source) == InProgress
        and plan.issues.get(change.target) != Completed
        and not(canIssue(plan, change.source, Complete))
    | _ => false
  }

  val wRemoveDependency = match observation.event {
    | RemoveDependency(_) => true
    | _ => false
  }
  val wCrossPlanDependency = match observation.event {
    | AddDependency(change) => plan.parents.get(change.source) != Unassigned
        and plan.parents.get(change.target) != Unassigned
        and plan.parents.get(change.source) != plan.parents.get(change.target)
    | _ => false
  }
  val wDependOnCompletedIssue = match observation.event {
    | AddDependency(change) => plan.issues.get(change.target) == Completed
    | _ => false
  }
  val wDependencyRetainedOnMove = match observation.event {
    | Reparent(change) => plan.dependencies == observation.before.dependencies
        and (plan.dependencies.get(change.id).size() > 0
          or plan.registered.exists(id => plan.dependencies.get(id).contains(change.id)))
    | _ => false
  }
  val wCancelledPrerequisiteBlocks = plan.registered.exists(id =>
    Set(NotStarted, InProgress).contains(plan.issues.get(id))
      and parentWorking(plan, plan.parents.get(id))
      and plan.dependencies.get(id).exists(target => plan.issues.get(target) == Cancelled)
      and not(canIssue(plan, id, Start)) and not(canIssue(plan, id, Complete)))
  val wPrerequisiteCompletionUnblocksStart = match observation.event {
    | IssueOperation(change) => change.op == Complete and plan.registered.exists(id =>
        plan.dependencies.get(id).contains(change.id)
          and not(prerequisitesComplete(observation.before, id)) and canIssue(plan, id, Start))
    | _ => false
  }
  val wPrerequisiteCompletionUnblocksFinish = match observation.event {
    | IssueOperation(change) => change.op == Complete and plan.registered.exists(id =>
        plan.dependencies.get(id).contains(change.id)
          and not(prerequisitesComplete(observation.before, id)) and canIssue(plan, id, Complete))
    | _ => false
  }
  val wIndirectCycleRejected = plan.registered.exists(source => plan.registered.exists(target => and {
    source != target,
    plan.issues.get(source) != Completed,
    not(plan.dependencies.get(source).contains(target)),
    not(plan.dependencies.get(target).contains(source)),
    reachable(plan, target).contains(source),
    not(canAddDependency(plan, source, target)),
  }))
  val wDirectCycleRejected = plan.registered.exists(source => plan.registered.exists(target =>
    plan.issues.get(source) != Completed and plan.dependencies.get(target).contains(source)
      and not(canAddDependency(plan, source, target))))
  val invSelfDependencyRejected = plan.registered.forall(id => not(canAddDependency(plan, id, id)))

  val invConditionInputScope = match observation.event {
    | IssueConditionChanged(changed) => conditions.groups == observation.beforeConditions.groups
        and ISSUES.exclude(Set(changed)).forall(id => conditions.issues.get(id) == observation.beforeConditions.issues.get(id))
    | GroupConditionChanged(changed) => conditions.issues == observation.beforeConditions.issues
        and GROUPS.exclude(Set(changed)).forall(id => conditions.groups.get(id) == observation.beforeConditions.groups.get(id))
    | _ => conditions == observation.beforeConditions
  }
  val wGroupConditionRose = match observation.event {
    | GroupConditionChanged(id) => conditions.groups.get(id)
    | _ => false
  }
  val wGroupConditionFell = match observation.event {
    | GroupConditionChanged(id) => not(conditions.groups.get(id))
    | _ => false
  }

  val wIssueConditionRose = match observation.event {
    | IssueConditionChanged(id) => conditions.issues.get(id)
    | _ => false
  }
  val wIssueConditionFell = match observation.event {
    | IssueConditionChanged(id) => not(conditions.issues.get(id))
    | _ => false
  }
  val invTerminalConditionsNotEvaluated = and {
    GROUPS.forall(id => terminal(plan.groups.get(id))
      implies groupConditionResult(plan, conditions, id) == NotEvaluated),
    ISSUES.forall(id => (not(plan.registered.contains(id)) or terminal(plan.issues.get(id)))
      implies issueConditionResult(plan, conditions, id) == NotEvaluated),
  }
  val invCandidatesAreStartable = candidateIssues(plan, conditions).subseteq(startableIssues(plan))
    and candidateGroups(plan, conditions).subseteq(startableGroups(plan))
  val invCandidateDefinition = and {
    candidateIssues(plan, conditions) == plan.registered.filter(id => and {
      plan.issues.get(id) == NotStarted,
      parentWorking(plan, plan.parents.get(id)),
      prerequisitesComplete(plan, id),
      conditions.issues.get(id),
      match plan.parents.get(id) {
        | Unassigned => true
        | InGroup(group) => conditions.groups.get(group)
      },
    }),
    candidateGroups(plan, conditions) == GROUPS.filter(id =>
      plan.groups.get(id) == NotStarted and conditions.groups.get(id)),
  }
  val invJudgmentDefinition = and {
    judgmentIssues(plan, conditions) == plan.registered.filter(id => and {
      plan.issues.get(id) == Undecided,
      conditions.issues.get(id),
      match plan.parents.get(id) {
        | Unassigned => true
        | InGroup(group) => not(terminal(plan.groups.get(group))) and conditions.groups.get(group)
      },
    }),
    judgmentGroups(plan, conditions) == GROUPS.filter(id =>
      plan.groups.get(id) == Undecided and conditions.groups.get(id)),
  }
  val invJudgmentSeparateFromWork = and {
    judgmentIssues(plan, conditions).intersect(candidateIssues(plan, conditions)).size() == 0,
    judgmentIssues(plan, conditions).intersect(workingIssues(plan)).size() == 0,
    judgmentGroups(plan, conditions).intersect(candidateGroups(plan, conditions)).size() == 0,
    judgmentGroups(plan, conditions).intersect(workingGroups(plan)).size() == 0,
  }
  val invDependencyEditsPreserveJudgment = match observation.event {
    | AddDependency(_) => judgmentIssues(plan, conditions) == judgmentIssues(observation.before, observation.beforeConditions)
      and judgmentGroups(plan, conditions) == judgmentGroups(observation.before, observation.beforeConditions)
    | RemoveDependency(_) => judgmentIssues(plan, conditions) == judgmentIssues(observation.before, observation.beforeConditions)
      and judgmentGroups(plan, conditions) == judgmentGroups(observation.before, observation.beforeConditions)
    | _ => true
  }
  val wJudgmentIssue = judgmentIssues(plan, conditions).size() > 0
  val wJudgmentGroup = judgmentGroups(plan, conditions).size() > 0
  val wJudgmentBeforeParentStart = judgmentIssues(plan, conditions).exists(id =>
    GROUPS.exists(g => plan.parents.get(id) == InGroup(g) and plan.groups.get(g) == NotStarted))
  val wJudgmentWithUnfinishedPrerequisite = judgmentIssues(plan, conditions).exists(id =>
    not(prerequisitesComplete(plan, id)))
  val wParentHidesJudgment = match observation.event {
    | GroupConditionChanged(g) => children(plan, g).exists(id =>
      judgmentIssues(observation.before, observation.beforeConditions).contains(id)
      and not(judgmentIssues(plan, conditions).contains(id)))
    | _ => false
  }
  val wParentResurfacesJudgment = match observation.event {
    | GroupConditionChanged(g) => children(plan, g).exists(id =>
      not(judgmentIssues(observation.before, observation.beforeConditions).contains(id))
      and judgmentIssues(plan, conditions).contains(id))
    | _ => false
  }
  val wAcceptUnderHiddenParent = match observation.event {
    | IssueOperation(change) => change.op == Accept
      and not(parentSurfaced(observation.before, observation.beforeConditions, observation.before.parents.get(change.id)))
    | _ => false
  }
  val wAcceptWithUnfinishedPrerequisite = match observation.event {
    | IssueOperation(change) => change.op == Accept and not(prerequisitesComplete(observation.before, change.id))
    | _ => false
  }
  val invWorkingVisibility = and {
    workingIssues(plan) == plan.registered.filter(id => plan.issues.get(id) == InProgress),
    workingGroups(plan) == GROUPS.filter(id => plan.groups.get(id) == InProgress),
  }
  val invConditionChangesPreserveEligibility = match observation.event {
    | GroupConditionChanged(_) => startableIssues(plan) == startableIssues(observation.before)
        and startableGroups(plan) == startableGroups(observation.before)
        and workingIssues(plan) == workingIssues(observation.before)
        and workingGroups(plan) == workingGroups(observation.before)
    | IssueConditionChanged(_) => startableIssues(plan) == startableIssues(observation.before)
        and startableGroups(plan) == startableGroups(observation.before)
        and workingIssues(plan) == workingIssues(observation.before)
        and workingGroups(plan) == workingGroups(observation.before)
    | _ => true
  }
  val wCandidateIssue = candidateIssues(plan, conditions).size() > 0
  val wCandidateGroup = candidateGroups(plan, conditions).size() > 0
  val wStartHiddenIssue = match observation.event {
    | IssueOperation(change) => change.op == Start
        and not(observation.beforeConditions.issues.get(change.id))
        and workingIssues(plan).contains(change.id)
    | _ => false
  }
  val wStartUnderHiddenParent = match observation.event {
    | IssueOperation(change) => change.op == Start
        and not(parentSurfaced(observation.before, observation.beforeConditions, observation.before.parents.get(change.id)))
        and workingIssues(plan).contains(change.id)
    | _ => false
  }
  val wStartHiddenGroup = match observation.event {
    | GroupOperation(change) => change.op == Start
        and not(observation.beforeConditions.groups.get(change.id))
        and workingGroups(plan).contains(change.id)
    | _ => false
  }
  val wParentHidesCandidate = match observation.event {
    | GroupConditionChanged(group) => not(conditions.groups.get(group))
        and children(plan, group).exists(id =>
          candidateIssues(observation.before, observation.beforeConditions).contains(id)
          and not(candidateIssues(plan, conditions).contains(id)))
    | _ => false
  }
  val wParentResurfacesCandidate = match observation.event {
    | GroupConditionChanged(group) => conditions.groups.get(group)
        and children(plan, group).exists(id =>
          not(candidateIssues(observation.before, observation.beforeConditions).contains(id))
          and candidateIssues(plan, conditions).contains(id))
    | _ => false
  }
  val wParentHiddenKeepsWorkingChild = match observation.event {
    | GroupConditionChanged(group) => not(conditions.groups.get(group))
        and children(plan, group).exists(id => workingIssues(plan).contains(id))
    | _ => false
  }
  val wIssueHiddenKeepsWorking = match observation.event {
    | IssueConditionChanged(id) => not(conditions.issues.get(id)) and workingIssues(plan).contains(id)
    | _ => false
  }
  val wCompletedIssueConditionIgnored = match observation.event {
    | IssueConditionChanged(id) => conditions.issues.get(id) and plan.issues.get(id) == Completed
        and issueConditionResult(plan, conditions, id) == NotEvaluated
    | _ => false
  }
  val wCancelledGroupConditionIgnored = match observation.event {
    | GroupConditionChanged(id) => conditions.groups.get(id) and plan.groups.get(id) == Cancelled
        and groupConditionResult(plan, conditions, id) == NotEvaluated
    | _ => false
  }

  val invMembershipValid = and {
    plan.registered.subseteq(ISSUES),
    ISSUES.forall(id => PARENTS.contains(plan.parents.get(id))),
    ISSUES.exclude(plan.registered).forall(id => plan.parents.get(id) == Unassigned),
  }
  val invRegistrationMatchesDecision = match observation.event {
    | RegisterIssue(change) => match change.registration {
        | Proposal => plan.issues.get(change.id) == Undecided and not(canIssue(plan, change.id, Start))
        | AdoptedWork => plan.issues.get(change.id) == NotStarted
      }
    | _ => true
  }
  val invRegistrationNeverStarts = match observation.event {
    | RegisterIssue(change) => not(workingIssues(plan).contains(change.id))
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

所属変更、提案・採用済みの新規登録、Issue 間の dependency、再浮上と判断候補・着手候補・着手中の関係を、計画2件・Issue枠3件の範囲で扱う。取りやめた計画は再検討後に構成を変更できるが、完了した計画には再開経路を持たせない。

最終確認が通るかは抽象入力であり、確認工程の実装や不合格理由は扱わない。入れ子の計画、計画そのものへの dependency、claim、declaration 編集、実際の ID 発行や永続化は検証対象外である。条件の種類・設定・評価失敗、一覧やコマンドの具体的な構成はまだ決めない。

`NotEvaluated` は意味上の評価除外であり、外部コマンドを実際に呼ばないことや評価コストは実装側で別途検証する。自身の条件の評価と親による候補の除外を分けており、親が非浮上の場合に子の外部条件の評価を省略する実装までは定めない。

循環の検査はこの3 ID の全グラフを対象とするが、4件以上の具体的なグラフは探索していない。依存先が取りやめでも依存元の取りやめを強制せず、依存関係の見直しや再検討を明示操作に残す。行き止まりがないという性質は整理・再判断の操作も含み、当初の計画どおり必ず完了できることを意味しない。

このモデルには、追加できる Issue が1件だけという探索上の上限がある。新規登録が無効になることは、製品で追加件数を制限する提案ではない。無限件数への一般化や、必ず最終確認に合格し計画が完了することは保証しない。

### 統合モデルの再現と結果

2026-09-10、lmt `v0.0.0-20210421124901-62fe18f2f6a6` で生成・型検査後、Quint 0.32.0、Rust backend、8 threads、10,000 traces、最大200 steps、入力 seed `2026091007` で実行した。27 invariant に反例はなく、71 witness はすべて1 trace以上で観測された。親が未着手でも判断候補となるケースは7,999 traces、未完了の依存先があっても判断候補となるケースは9,851 traces、親が非浮上でも明示的に採用するケースは6,548 tracesで観測された。既存の所属・dependency・再浮上の到達目標もすべて観測された。これは bounded random simulation の結果であり、全状態の証明ではない。

単独 Issue と再浮上のモデル、および共有する基本遷移のコードは変更していない。

```sh
lmt spec/lifecycle_proposal.md
quint typecheck target/literate/group_lifecycle_proposal.qnt
quint run target/literate/group_lifecycle_proposal.qnt \
  --invariants \
    invParentOfWorkingChild invClosedChildrenTerminal invCompletedStayCompleted \
    invClosedStructureFrozen invCompletionReviewed invOperationScope \
    invMembershipValid invRegistrationMatchesDecision invRegistrationNeverStarts invClosedEditsDisabled \
    invNoUnexpectedDeadEnd \
    invDependencyOperationScope invDependenciesValid invDependenciesAcyclic \
    invCompletedPrerequisites invStartAndCompleteRequirePrerequisites \
    invCompletedDependenciesFrozen invSelfDependencyRejected \
    invConditionInputScope invTerminalConditionsNotEvaluated invCandidatesAreStartable \
    invCandidateDefinition invWorkingVisibility invConditionChangesPreserveEligibility \
    invJudgmentDefinition invJudgmentSeparateFromWork invDependencyEditsPreserveJudgment \
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
    wRegisterProposal wRegisterAdopted wAdoptedRegistrationWaitsForParent wAdoptedRegistrationCanStart \
    wAddAfterFinalCheckReady wReconsiderAllowsMembership wRemoveUndecidedForCompletion \
    wAddDependency wRemoveDependency wAddDuringWorkBlocksCompletion \
    wCrossPlanDependency wDependOnCompletedIssue wDependencyRetainedOnMove \
    wCancelledPrerequisiteBlocks wPrerequisiteCompletionUnblocksStart \
    wPrerequisiteCompletionUnblocksFinish wIndirectCycleRejected wDirectCycleRejected \
    wGroupConditionRose wGroupConditionFell wIssueConditionRose wIssueConditionFell \
    wCandidateIssue wCandidateGroup wStartHiddenIssue wStartUnderHiddenParent wStartHiddenGroup \
    wParentHidesCandidate wParentResurfacesCandidate wParentHiddenKeepsWorkingChild \
    wIssueHiddenKeepsWorking wCompletedIssueConditionIgnored wCancelledGroupConditionIgnored \
    wJudgmentIssue wJudgmentGroup wJudgmentBeforeParentStart wJudgmentWithUnfinishedPrerequisite \
    wParentHidesJudgment wParentResurfacesJudgment wAcceptUnderHiddenParent wAcceptWithUnfinishedPrerequisite \
  --max-samples 10000 --max-steps 200 --seed 2026091007 --backend rust --n-threads 8 --verbosity 1
```
