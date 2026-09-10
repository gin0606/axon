# Progress と採否を統合する検討用モデル

この literate specification は、Progress と採否を単一の lifecycle にまとめる案を扱う。単独 Issue の再浮上を基本モデルで扱い、計画・子 Issue の着手境界、所属変更、Issue・計画間の dependency、再浮上による候補の選別を統合モデルで検討する。既存機能やデータ形式との互換性を前提にせず、新しい土台として設計するためのモデルであり、このファイルの追加・更新で現行実装や既存モデルは変更しない。

説明と実行可能なモデルをこのファイルで管理する。[Quint の literate 形式](https://quint.sh/docs/literate)に従い、`lmt` で `target/literate/` 配下に共有する基本遷移、単独 Issue、計画と子 Issue の統合モデル、到達性の補助探索、候補一覧の評価の5ファイルを生成する。生成ファイルは直接編集しない。

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

条件定義は一つに固定して抽象化し、成立・不成立だけを外部入力 `conditionSatisfied` として扱う。この基本モデルでは条件の設定・変更操作を扱わず、未設定と外部コマンドの区別は後述の候補一覧モデルで扱う。

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

計画3件、Issue 用の固定 ID 3件を使う。最初は2 Issue が計画0に所属し、残り1件は未登録の枠とする。未登録はモデルの探索領域を有限にする仕組みであり、新しい lifecycle ではない。初期配置の計画・Issue は未判断から始め、計画はすべて所属なしとする。計画も最大一つの親計画を持ち、Issue と子計画を同じ階層に置ける。追加する Issue には、未判断の提案と採用済みの仕事の二つの登録経路を設ける。計画そのものの登録操作は今回のモデル外である。

Issue・計画とも所属は最大一つで、所属なしも許す。計画の自己包含と、子孫の下への移動による循環を禁止する。所属変更は既存の親子の制約を壊さない限り許可し、lifecycle を変えない。完了・取りやめの計画への追加と、そこからの取り外しは不可とする。取りやめた計画は、明示的に再検討へ戻せば構成を変更できる。この取りやめ時の構成固定と計画の再検討は暫定採用の判断であり、運用上の負担が分かれば見直す。

```quint target/literate/group_lifecycle_proposal.qnt +=
module group_lifecycle_proposal {
  import lifecycle_rules.* from "lifecycle_rules"

  type Id = int
  type Parent = Unassigned | InGroup(Id)
  type Entity = IssueRef(Id) | GroupRef(Id)
  type Requirement = ParentInProgress(Id) | EntityCompleted(Entity) | ChildEnded(Entity)
  type Registration = Proposal | AdoptedWork
  pure val GROUPS = Set(0, 1, 2)
  pure val ISSUES = Set(0, 1, 2)
  pure val REFS = ISSUES.map(IssueRef).union(GROUPS.map(GroupRef))
  pure val PARENTS = Set(Unassigned, InGroup(0), InGroup(1), InGroup(2))
  pure val OPERATIONS = Set(Accept, Withdraw, Start, Release, Complete, Cancel, Reconsider)
  type PlanState = {
    groups: Id -> Lifecycle,
    issues: Id -> Lifecycle,
    registered: Set[Id],
    parents: Id -> Parent,
    groupParents: Id -> Parent,
    dependencies: Entity -> Set[Entity],
  }
  type Conditions = { groups: Id -> bool, issues: Id -> bool }
  type ConditionResult = NotEvaluated | Evaluated(bool)
  type PlanEvent = Initial
    | GroupConditionChanged(Id)
    | IssueConditionChanged(Id)
    | GroupOperation({ id: Id, op: Operation })
    | IssueOperation({ id: Id, op: Operation })
    | Reparent({ id: Id, parent: Parent })
    | ReparentGroup({ id: Id, parent: Parent })
    | RegisterIssue({ id: Id, parent: Parent, registration: Registration })
    | AddDependency({ source: Entity, target: Entity })
    | RemoveDependency({ source: Entity, target: Entity })
  type PlanObservation = { before: PlanState, beforeConditions: Conditions, event: PlanEvent, reviewPassed: bool }
  var plan: PlanState
  var conditions: Conditions
  var observation: PlanObservation
```

### 親子の lifecycle

子への着手には、子自身の採用と親の進行中を要求する。Issue・計画とも、基本遷移に加えて dependency の着手・完了条件を適用する。進行中の子があれば親を未着手へ戻せない。計画の完了・取りやめには直属の Issue・子計画のすべてが終了している必要があり、未判断もその妨げになる。子計画への着手にも親の進行中を要求するため、進行中の Entity の祖先はすべて進行中になる。各階層で明示的に着手し、子への着手で親を自動変更しない。

完了にはさらに、その計画全体の最終確認が通ったという入力を要求する。画面単位の子計画にも、機能全体の親計画にも、それぞれ独立した最終確認がある。`reviewPassed` は確認結果の抽象入力であり、CLI 引数や保存方法、専用操作か最終チェック Issue かを決めるものではない。空の計画でもこの確認を省略しない。子の終了だけで親を自動終了しない。

着手・完了・取りやめで要求する前提を `Requirement` にまとめる。

| 操作 | 包含からの前提 | dependency からの前提 |
| --- | --- | --- |
| 着手 | 親があれば、現在進行中である | 全依存先が完了している |
| 完了 | 計画なら、直属の子が全員終了している | 全依存先が完了している |
| 取りやめ | 計画なら、直属の子が全員終了している | 要求しない |

`ChildEnded` は完了・取りやめのどちらでも満たす。計画の最終確認と、終了した親の配下を変更しない制約は、この前提に加えて適用する。親の条件は過去の着手履歴ではなく現在の進行中を要求する。

循環検査では「全員が通常どおり完了する経路」を使う。各 Entity の着手から完了への辺、親の着手から子の着手への辺、子の完了から親の完了への辺、依存先の完了から依存元の着手・完了への辺を導出する。取りやめによる回避は、この経路には加えない。

モデルでは着手の節点を展開し、各 Entity の完了に先行するものを次の集合へ縮約する。

```text
完了の前提 = 直属の子 ∪ 自身の依存先 ∪ 全祖先の依存先
```

着手の前提を親へさかのぼると祖先の依存先へ到達し、子の終了は通常完了経路では子の完了に対応する。この縮約で得たグラフから、前提の残っていない節点を順に除去し、全節点を除去できるか検査する。これは構造の循環判定用の導出であり、実際の操作可否には上表の現在状態を使う。保存する関係は包含と明示的な dependency のままである。

```quint target/literate/group_lifecycle_proposal.qnt +=
  pure def terminal(current: Lifecycle): bool = Set(Completed, Cancelled).contains(current)
  pure def children(s: PlanState, group: Id): Set[Id] =
    s.registered.filter(id => s.parents.get(id) == InGroup(group))
  pure def childGroups(s: PlanState, group: Id): Set[Id] =
    GROUPS.filter(id => s.groupParents.get(id) == InGroup(group))
  pure def parentSet(parent: Parent): Set[Id] = match parent {
    | Unassigned => Set()
    | InGroup(id) => Set(id)
  }
  pure def ancestorGroups(s: PlanState, parent: Parent): Set[Id] =
    GROUPS.fold(parentSet(parent), (seen, _) =>
      seen.union(seen.map(id => parentSet(s.groupParents.get(id))).flatten()))
  pure def descendantGroups(s: PlanState, group: Id): Set[Id] =
    GROUPS.filter(id => ancestorGroups(s, s.groupParents.get(id)).contains(group))
  pure def descendantIssues(s: PlanState, group: Id): Set[Id] =
    s.registered.filter(id => ancestorGroups(s, s.parents.get(id)).contains(group))
  pure def allChildrenTerminal(s: PlanState, group: Id): bool =
    children(s, group).forall(id => terminal(s.issues.get(id)))
      and childGroups(s, group).forall(id => terminal(s.groups.get(id)))
  pure def hasWorkingChild(s: PlanState, group: Id): bool =
    children(s, group).exists(id => s.issues.get(id) == InProgress)
      or childGroups(s, group).exists(id => s.groups.get(id) == InProgress)
  pure def parentOpen(s: PlanState, parent: Parent): bool = match parent {
    | Unassigned => true
    | InGroup(group) => not(terminal(s.groups.get(group)))
  }
  pure def parentWorking(s: PlanState, parent: Parent): bool = match parent {
    | Unassigned => true
    | InGroup(group) => s.groups.get(group) == InProgress
  }

  pure def entities(s: PlanState): Set[Entity] = s.registered.map(IssueRef).union(GROUPS.map(GroupRef))
  pure def entityState(s: PlanState, entity: Entity): Lifecycle = match entity {
    | IssueRef(id) => s.issues.get(id)
    | GroupRef(id) => s.groups.get(id)
  }
  pure def entityParent(s: PlanState, entity: Entity): Parent = match entity {
    | IssueRef(id) => s.parents.get(id)
    | GroupRef(id) => s.groupParents.get(id)
  }
  pure def entityChildren(s: PlanState, entity: Entity): Set[Entity] = match entity {
    | IssueRef(_) => Set()
    | GroupRef(id) => children(s, id).map(IssueRef).union(childGroups(s, id).map(GroupRef))
  }
  pure def requirements(s: PlanState, entity: Entity, op: Operation): Set[Requirement] = {
    val parent = if (op == Start) parentSet(entityParent(s, entity)).map(ParentInProgress) else Set()
    val dependencies = if (Set(Start, Complete).contains(op))
      s.dependencies.get(entity).map(EntityCompleted) else Set()
    val children = if (Set(Complete, Cancel).contains(op))
      entityChildren(s, entity).map(ChildEnded) else Set()
    parent.union(dependencies).union(children)
  }
  pure def requirementSatisfied(s: PlanState, requirement: Requirement): bool = match requirement {
    | ParentInProgress(id) => s.groups.get(id) == InProgress
    | EntityCompleted(entity) => entityState(s, entity) == Completed
    | ChildEnded(entity) => terminal(entityState(s, entity))
  }
  pure def requirementsSatisfied(s: PlanState, entity: Entity, op: Operation): bool =
    requirements(s, entity, op).forall(requirement => requirementSatisfied(s, requirement))
  pure def prerequisitesComplete(s: PlanState, entity: Entity): bool =
    s.dependencies.get(entity).forall(target => entityState(s, target) == Completed)

  pure def completionPredecessors(s: PlanState, entity: Entity): Set[Entity] =
    entityChildren(s, entity).union(s.dependencies.get(entity))
      .union(ancestorGroups(s, entityParent(s, entity))
        .map(id => s.dependencies.get(GroupRef(id))).flatten())
  pure def preconditionsAcyclic(s: PlanState): bool = {
    val nodes = entities(s)
    val predecessors = nodes.mapBy(entity => completionPredecessors(s, entity))
    nodes.fold(nodes, (remaining, _) => remaining.filter(entity =>
      predecessors.get(entity).intersect(remaining).size() > 0)).size() == 0
  }

  pure def canGroup(s: PlanState, id: Id, op: Operation, passed: bool): bool = and {
    canPerform(s.groups.get(id), op),
    parentOpen(s, s.groupParents.get(id)),
    requirementsSatisfied(s, GroupRef(id), op),
    op != Release or not(hasWorkingChild(s, id)),
    op != Complete or passed,
  }
  pure def applyGroup(s: PlanState, id: Id, op: Operation): PlanState =
    { ...s, groups: s.groups.set(id, applyOperation(s.groups.get(id), op)) }

  pure def canIssue(s: PlanState, id: Id, op: Operation): bool = and {
    s.registered.contains(id),
    canPerform(s.issues.get(id), op),
    parentOpen(s, s.parents.get(id)),
    requirementsSatisfied(s, IssueRef(id), op),
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
      groupParents: GROUPS.mapBy(_ => Unassigned),
      dependencies: REFS.mapBy(_ => Set()),
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

進行中の Issue・計画は、所属なしにするか、進行中の別計画へ移せる。移動元・移動先に計画がある場合は、どちらも完了・取りやめでないことを検査する。移動のためだけに release と start を挟む必要はなく、移動によって採用や進行の状態は変わらない。変更後の通常完了経路が循環する所属変更も拒否する。例えば A が B の完了に依存している場合、B を A の配下へ移すことはできない。

計画の移動は、その計画の親だけを付け替える。配下の所属・lifecycle・dependency は保持され、部分木全体が移る。完了・取りやめた計画でも、自身の親が終了していなければ、その内部構成を変えずに移せる。これは終了済み Issue の移動と同じ制約である。

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
    preconditionsAcyclic(applyMove(s, id, parent)),
  }
  pure def applyMove(s: PlanState, id: Id, parent: Parent): PlanState =
    { ...s, parents: s.parents.set(id, parent) }
  action moveIssue(id: Id, parent: Parent): bool = all {
    canMove(plan, id, parent),
    plan' = applyMove(plan, id, parent),
    conditions' = conditions,
    observation' = observe(plan, conditions, Reparent({ id: id, parent: parent }), false),
  }

  pure def canMoveGroup(s: PlanState, id: Id, parent: Parent): bool = and {
    s.groupParents.get(id) != parent,
    parentOpen(s, s.groupParents.get(id)),
    destinationAllowed(s, parent, s.groups.get(id)),
    not(ancestorGroups(s, parent).contains(id)),
    preconditionsAcyclic(applyMoveGroup(s, id, parent)),
  }
  pure def applyMoveGroup(s: PlanState, id: Id, parent: Parent): PlanState =
    { ...s, groupParents: s.groupParents.set(id, parent) }
  action moveGroup(id: Id, parent: Parent): bool = all {
    canMoveGroup(plan, id, parent),
    plan' = applyMoveGroup(plan, id, parent),
    conditions' = conditions,
    observation' = observe(plan, conditions, ReparentGroup({ id: id, parent: parent }), false),
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

### Issue・計画間の dependency

依存先がすべて完了するまで、依存元は着手・完了できない。依存先の取りやめは条件を満たさない。進行中でも未完了の依存先を追加でき、lifecycle は変えない。この着手後の追加は暫定採用の判断である。

完了した Entity 自身の依存関係は固定する。完了済み Entity を、別の Entity が前提として参照することは許す。未完了の Entity の前提は、判断に応じて追加・削除する。所属変更は dependency を保持し、計画をまたぐ依存と所属なしの Entity への依存を許す。

計画から Issue への依存を許す。Issue・計画から別の計画への依存も暫定採用とし、依存先の計画自身が最終確認を経て完了するまで待つ。配下がすべて終了しただけでは依存先の完了条件を満たさない。

自己依存と、包含を合わせた通常完了経路の循環を追加時に拒否する。親・祖先と子孫の間の明示的な dependency はどちら向きも禁止される。兄弟や別計画同士でも、他の包含・依存を経由して循環する場合は拒否する。取りやめ済み・完了済みの Entity も構造のグラフから除外しない。拒否する操作は無効となり、状態・所属・dependency を自動調整しない。

```quint target/literate/group_lifecycle_proposal.qnt +=
  pure def reachable(s: PlanState, source: Entity): Set[Entity] =
    REFS.fold(Set(), (seen, _) => seen.union(s.dependencies.get(source))
      .union(seen.map(id => s.dependencies.get(id)).flatten()))
  pure def canAddDependency(s: PlanState, source: Entity, target: Entity): bool = and {
    entities(s).contains(source),
    entities(s).contains(target),
    entityState(s, source) != Completed,
    source != target,
    not(s.dependencies.get(source).contains(target)),
    preconditionsAcyclic(applyAddDependency(s, source, target)),
  }
  pure def applyAddDependency(s: PlanState, source: Entity, target: Entity): PlanState =
    { ...s, dependencies: s.dependencies.set(source, s.dependencies.get(source).union(Set(target))) }
  action addDependency(source: Entity, target: Entity): bool = all {
    canAddDependency(plan, source, target),
    plan' = applyAddDependency(plan, source, target),
    conditions' = conditions,
    observation' = observe(plan, conditions, AddDependency({ source: source, target: target }), false),
  }
```

```quint target/literate/group_lifecycle_proposal.qnt +=
  pure def canRemoveDependency(s: PlanState, source: Entity, target: Entity): bool = and {
    entities(s).contains(source),
    entityState(s, source) != Completed,
    s.dependencies.get(source).contains(target),
  }
  pure def applyRemoveDependency(s: PlanState, source: Entity, target: Entity): PlanState =
    { ...s, dependencies: s.dependencies.set(source, s.dependencies.get(source).exclude(Set(target))) }
  action removeDependency(source: Entity, target: Entity): bool = all {
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
| 判断候補の Issue | 未判断で、自身とすべての祖先が浮上している。親の着手・依存先の完了は要求しない |
| 着手可能な Issue | 未着手、親があれば進行中、全依存先が完了 |
| 着手候補の Issue | 着手可能で、自身とすべての祖先が浮上している |
| 着手中の Issue | 進行中。自身・親の浮上状態や追加された未完了の依存に関係なく含む |

未判断の計画は、自身とすべての祖先が浮上していれば判断候補とする。判断候補は採否を検討するための集合であり、候補への出入りで採否や進行状態を変えない。dependency は着手・完了の前提であり、採否を先に決めることを妨げない。

計画自身も、未着手で、親がある場合はその親が進行中で、全依存先が完了していれば明示的に着手可能とし、候補に出すときだけ自身とすべての祖先の浮上を要求する。進行中の計画は非浮上でも着手中に含む。再浮上は候補の表示を制御し、明示的な採否判断、着手や完了、所属・依存変更の可否には加えない。この表示と明示操作の分離は暫定採用の判断である。

取りやめ・完了の Entity は条件を評価せず、浮上しない。祖先のいずれかが非浮上なら子孫を判断候補・着手候補から外すが、子の状態や条件は変えない。評価結果は保存せず、入力と状態から導出する。

```quint target/literate/group_lifecycle_proposal.qnt +=
  pure def groupConditionResult(s: PlanState, c: Conditions, id: Id): ConditionResult =
    if (terminal(s.groups.get(id))) NotEvaluated else Evaluated(c.groups.get(id))
  pure def issueConditionResult(s: PlanState, c: Conditions, id: Id): ConditionResult =
    if (not(s.registered.contains(id)) or terminal(s.issues.get(id))) NotEvaluated
    else Evaluated(c.issues.get(id))
  pure def parentSurfaced(s: PlanState, c: Conditions, parent: Parent): bool =
    ancestorGroups(s, parent).forall(id => groupConditionResult(s, c, id) == Evaluated(true))
  pure def judgmentIssues(s: PlanState, c: Conditions): Set[Id] =
    s.registered.filter(id => s.issues.get(id) == Undecided
      and issueConditionResult(s, c, id) == Evaluated(true)
      and parentSurfaced(s, c, s.parents.get(id)))
  pure def judgmentGroups(s: PlanState, c: Conditions): Set[Id] =
    GROUPS.filter(id => s.groups.get(id) == Undecided
      and groupConditionResult(s, c, id) == Evaluated(true)
      and parentSurfaced(s, c, s.groupParents.get(id)))
  pure def startableIssues(s: PlanState): Set[Id] = s.registered.filter(id => canIssue(s, id, Start))
  pure def candidateIssues(s: PlanState, c: Conditions): Set[Id] =
    startableIssues(s).filter(id => issueConditionResult(s, c, id) == Evaluated(true)
      and parentSurfaced(s, c, s.parents.get(id)))
  pure def workingIssues(s: PlanState): Set[Id] =
    s.registered.filter(id => s.issues.get(id) == InProgress)
  pure def startableGroups(s: PlanState): Set[Id] = GROUPS.filter(id => canGroup(s, id, Start, false))
  pure def candidateGroups(s: PlanState, c: Conditions): Set[Id] =
    startableGroups(s).filter(id => groupConditionResult(s, c, id) == Evaluated(true)
      and parentSurfaced(s, c, s.groupParents.get(id)))
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

基本遷移、Issue・計画の所属変更、新規登録、dependency の追加・削除、再浮上条件の成立・不成立を混ぜて実行する。`step` は Group の lifecycle、Issue の lifecycle、それ以外の操作の三枝から選ぶ。許可される遷移は維持しつつ、前提を満たして着手・完了へ進む経路も探索しやすくする。条件入力はすべて不成立から開始し、登録済み Issue と計画について両方向へ変化できる。これは探索の開始点であり、製品の再浮上条件の初期値を決めるものではない。登録済み集合と所属は別に保持し、計画の外へ出しても Issue を削除しない。同じ所属の再指定は無効とする。ID は固定集合から選び、未登録 ID の再利用・削除は扱わない。

```quint target/literate/group_lifecycle_proposal.qnt +=
  action step = {
    nondet group = GROUPS.oneOf()
    nondet id = ISSUES.oneOf()
    nondet op = OPERATIONS.oneOf()
    nondet passed = Set(false, true).oneOf()
    nondet parent = PARENTS.oneOf()
    nondet source = REFS.oneOf()
    nondet target = REFS.oneOf()
    nondet registration = Set(Proposal, AdoptedWork).oneOf()
    any {
      performGroup(group, op, passed),
      performIssue(id, op),
      any {
        moveIssue(id, parent),
        moveGroup(group, parent),
        registerIssue(id, parent, registration),
        addDependency(source, target),
        removeDependency(source, target),
        changeGroupCondition(group, passed),
        changeIssueCondition(id, passed),
      },
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
- 未登録の枠が計画に混入せず、Issue・計画の所属は高々一つで、包含は循環しない。
- 進行中の Entity の祖先はすべて進行中で、終了した計画の子孫はすべて終了している。
- 計画を移動しても、その子孫の集合・内部の所属・状態は変わらない。
- dependency は登録済み Issue・計画だけを参照し、包含と合わせた通常完了経路が循環しない。
- 親・祖先と子孫の間の明示的な dependency を持たない。
- 着手・完了時に全依存先が完了しており、完了後もその前提は満たされたままである。
- 完了した Entity 自身の dependency は固定される。
- dependency 操作は lifecycle や所属を変えず、他の操作は dependency を変えない。
- 条件入力の変化は保存状態・着手可否・着手中の集合を変えず、明示操作は条件入力を変えない。
- 判断候補は未判断かつ自身・全祖先が浮上しているものとし、dependency の変更は判断候補を変えない。
- 判断候補は着手候補・着手中と重ならない。
- 着手候補は着手可能なものに限り、自身と全祖先の浮上条件に一致する。
- 進行中の Issue と計画は、非浮上でも着手中に含む。
- 未登録・取りやめ・完了の Issue と、取りやめ・完了の計画は条件を評価しない。
- 未終了の仕事や再検討可能な計画が残っている間、全 lifecycle 操作が行き止まりにならない。

到達性では各操作に加え、進行中の移動、未判断を計画外へ出して完了可能になる場合、最終確認待ちからの新規 Issue 追加を観測する。dependency については、進行中の追加、計画をまたぐ依存、前提の取りやめによる阻害、前提の完了による着手・完了解禁、直接・間接循環の拒否を確認する。再浮上については、親の条件変化による候補の除外と復帰、非浮上のままの明示的な着手、進行中の仕事の保持を確認する。判断候補については、親が未着手のケース、未完了の依存先があるケース、親の浮上による除外と復帰を観測する。親が非浮上でも明示的に採用でき、依存先の完了前にも採用できることを確認する。入れ子については三階層の包含、Issue と子計画の混在、進行中の部分木の移動、子計画が親の着手・終了に与える制約、各階層の完了・取りやめ、間接的な包含循環の拒否を観測する。Group を含む dependency では三種類の組み合わせ、計画の最終確認待ち、計画の完了による後続着手の解禁、着手後の前提追加、依存に反する所属変更の拒否、別の枝を経由する循環の拒否を観測する。

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
    and children(plan, g).size() > 0 and children(plan, g).forall(id => plan.issues.get(id) == Completed)
    and childGroups(plan, g).forall(id => plan.groups.get(id) == Completed))
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
    implies (children(plan, g) == children(observation.before, g)
      and childGroups(plan, g) == childGroups(observation.before, g)))
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
    | ReparentGroup(_) => plan.groups == observation.before.groups
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
    | RemoveDependency(change) => REFS.exclude(Set(change.source)).forall(id =>
        plan.dependencies.get(id) == observation.before.dependencies.get(id))
    | AddDependency(change) => REFS.exclude(Set(change.source)).forall(id =>
        plan.dependencies.get(id) == observation.before.dependencies.get(id))
    | _ => plan.dependencies == observation.before.dependencies
  }
  val invDependenciesValid = REFS.forall(source =>
    plan.dependencies.get(source).subseteq(entities(plan))
      and (not(entities(plan).contains(source)) implies plan.dependencies.get(source).size() == 0))
  val invPreconditionsAcyclic = preconditionsAcyclic(plan)
  val invCompletedPrerequisites = entities(plan).forall(entity =>
    entityState(plan, entity) == Completed implies prerequisitesComplete(plan, entity))
  val invStartAndCompleteRequirePrerequisites = match observation.event {
    | IssueOperation(change) => Set(Start, Complete).contains(change.op)
        implies prerequisitesComplete(observation.before, IssueRef(change.id))
    | GroupOperation(change) => Set(Start, Complete).contains(change.op)
        implies prerequisitesComplete(observation.before, GroupRef(change.id))
    | _ => true
  }
  val invCompletedDependenciesFrozen = entities(observation.before).forall(entity =>
    entityState(observation.before, entity) == Completed
      implies plan.dependencies.get(entity) == observation.before.dependencies.get(entity))
  val wAddDependency = match observation.event {
    | AddDependency(_) => true
    | _ => false
  }
  val wAddDuringWorkBlocksCompletion = match observation.event {
    | AddDependency(change) => entityState(plan, change.source) == InProgress
        and entityState(plan, change.target) != Completed
        and not(requirementsSatisfied(plan, change.source, Complete))
    | _ => false
  }

  val wRemoveDependency = match observation.event {
    | RemoveDependency(_) => true
    | _ => false
  }
  val wCrossPlanDependency = match observation.event {
    | AddDependency(change) => entityParent(plan, change.source) != Unassigned
        and entityParent(plan, change.target) != Unassigned
        and entityParent(plan, change.source) != entityParent(plan, change.target)
    | _ => false
  }
  val wDependOnCompletedIssue = match observation.event {
    | AddDependency(change) => (match change.target { | IssueRef(_) => true | _ => false })
      and entityState(plan, change.target) == Completed
    | _ => false
  }
  val wDependencyRetainedOnMove = match observation.event {
    | Reparent(change) => plan.dependencies == observation.before.dependencies
        and (plan.dependencies.get(IssueRef(change.id)).size() > 0
          or plan.registered.exists(id => plan.dependencies.get(IssueRef(id)).contains(IssueRef(change.id))))
    | _ => false
  }
  val wCancelledPrerequisiteBlocks = plan.registered.exists(id =>
    Set(NotStarted, InProgress).contains(plan.issues.get(id))
      and parentWorking(plan, plan.parents.get(id))
      and plan.dependencies.get(IssueRef(id)).exists(target => entityState(plan, target) == Cancelled)
      and not(canIssue(plan, id, Start)) and not(canIssue(plan, id, Complete)))
  val wPrerequisiteCompletionUnblocksStart = match observation.event {
    | IssueOperation(change) => change.op == Complete and plan.registered.exists(id =>
        plan.dependencies.get(IssueRef(id)).contains(IssueRef(change.id))
          and not(prerequisitesComplete(observation.before, IssueRef(id))) and canIssue(plan, id, Start))
    | _ => false
  }
  val wPrerequisiteCompletionUnblocksFinish = match observation.event {
    | IssueOperation(change) => change.op == Complete and plan.registered.exists(id =>
        plan.dependencies.get(IssueRef(id)).contains(IssueRef(change.id))
          and not(prerequisitesComplete(observation.before, IssueRef(id))) and canIssue(plan, id, Complete))
    | _ => false
  }
  val wIndirectCycleRejected = plan.registered.map(IssueRef).exists(source => plan.registered.map(IssueRef).exists(target => and {
    source != target,
    entityState(plan, source) != Completed,
    not(plan.dependencies.get(source).contains(target)),
    not(plan.dependencies.get(target).contains(source)),
    reachable(plan, target).contains(source),
    not(canAddDependency(plan, source, target)),
  }))
  val wDirectCycleRejected = plan.registered.map(IssueRef).exists(source => plan.registered.map(IssueRef).exists(target =>
    entityState(plan, source) != Completed and plan.dependencies.get(target).contains(source)
      and not(canAddDependency(plan, source, target))))
  val invSelfDependencyRejected = entities(plan).forall(entity => not(canAddDependency(plan, entity, entity)))

  val invNoAncestorDependencies = entities(plan).forall(entity =>
    ancestorGroups(plan, entityParent(plan, entity)).forall(g =>
      not(plan.dependencies.get(entity).contains(GroupRef(g)))
        and not(plan.dependencies.get(GroupRef(g)).contains(entity))))
  val wGroupDependsOnIssue = match observation.event {
    | AddDependency(change) => (match change.source { | GroupRef(_) => true | _ => false })
      and (match change.target { | IssueRef(_) => true | _ => false })
    | _ => false
  }
  val wIssueDependsOnGroup = match observation.event {
    | AddDependency(change) => (match change.source { | IssueRef(_) => true | _ => false })
      and (match change.target { | GroupRef(_) => true | _ => false })
    | _ => false
  }
  val wGroupDependsOnGroup = match observation.event {
    | AddDependency(change) => (match change.source { | GroupRef(_) => true | _ => false })
      and (match change.target { | GroupRef(_) => true | _ => false })
    | _ => false
  }
  val wGroupWaitsForIssue = GROUPS.exists(g => plan.groups.get(g) == NotStarted
    and parentWorking(plan, plan.groupParents.get(g))
    and plan.dependencies.get(GroupRef(g)).exists(target =>
      (match target { | IssueRef(_) => true | _ => false }) and entityState(plan, target) != Completed)
    and not(canGroup(plan, g, Start, false)))
  val wDependentWaitsForGroupReview = GROUPS.exists(g => plan.groups.get(g) == InProgress
    and entityChildren(plan, GroupRef(g)).size() > 0 and canGroup(plan, g, Complete, true)
    and entities(plan).exists(entity => entityState(plan, entity) == NotStarted
      and plan.dependencies.get(entity).contains(GroupRef(g))
      and not(requirementsSatisfied(plan, entity, Start))))
  val wGroupCompletionUnblocksDependent = match observation.event {
    | GroupOperation(change) => change.op == Complete and entities(plan).exists(entity =>
      entityState(plan, entity) == NotStarted
      and plan.dependencies.get(entity).contains(GroupRef(change.id))
      and not(requirementsSatisfied(observation.before, entity, Start))
      and requirementsSatisfied(plan, entity, Start))
    | _ => false
  }
  val wGroupDependencyAddedDuringWork = match observation.event {
    | AddDependency(change) => (match change.source { | GroupRef(_) => true | _ => false })
      and entityState(plan, change.source) == InProgress and entityState(plan, change.target) != Completed
      and not(requirementsSatisfied(plan, change.source, Complete))
    | _ => false
  }
  val wIssueMoveRejectedByPrerequisites = match observation.event {
    | AddDependency(change) => match change.source {
      | IssueRef(_) => false
      | GroupRef(g) => match change.target {
        | GroupRef(_) => false
        | IssueRef(id) => parentOpen(plan, plan.parents.get(id))
          and destinationAllowed(plan, InGroup(g), plan.issues.get(id))
          and not(canMove(plan, id, InGroup(g)))
      }
    }
    | _ => false
  }
  val wGroupMoveRejectedByPrerequisites = match observation.event {
    | AddDependency(change) => match change.source {
      | IssueRef(_) => false
      | GroupRef(g) => match change.target {
        | IssueRef(_) => false
        | GroupRef(id) => parentOpen(plan, plan.groupParents.get(id))
          and destinationAllowed(plan, InGroup(g), plan.groups.get(id))
          and not(ancestorGroups(plan, InGroup(g)).contains(id))
          and not(canMoveGroup(plan, id, InGroup(g)))
      }
    }
    | _ => false
  }
  val wAncestorDependencyRejected = match observation.event {
    | Reparent(change) => plan.issues.get(change.id) != Completed and match change.parent {
      | Unassigned => false
      | InGroup(g) => not(canAddDependency(plan, IssueRef(change.id), GroupRef(g)))
    }
    | _ => false
  }
  val wDescendantDependencyRejected = match observation.event {
    | Reparent(change) => match change.parent {
      | Unassigned => false
      | InGroup(g) => plan.groups.get(g) != Completed
        and not(canAddDependency(plan, GroupRef(g), IssueRef(change.id)))
    }
    | _ => false
  }
  val wCancelledEscapeStillRejected = match observation.event {
    | IssueOperation(change) => change.op == Cancel and match plan.parents.get(change.id) {
      | Unassigned => false
      | InGroup(g) => not(canAddDependency(plan, GroupRef(g), IssueRef(change.id)))
    }
    | _ => false
  }
  val wCrossBranchCycleRejected = match observation.event {
    | AddDependency(change) => change.source == GroupRef(0) and change.target == IssueRef(1)
      and plan.parents.get(0) == InGroup(0) and plan.parents.get(1) == InGroup(1)
      and plan.groupParents.get(0) == Unassigned and plan.groupParents.get(1) == Unassigned
      and plan.groups.get(1) != Completed
      and not(canAddDependency(plan, GroupRef(1), IssueRef(0)))
    | _ => false
  }

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
      prerequisitesComplete(plan, IssueRef(id)),
      conditions.issues.get(id),
      ancestorGroups(plan, plan.parents.get(id)).forall(g => conditions.groups.get(g)),
    }),
    candidateGroups(plan, conditions) == GROUPS.filter(id =>
      plan.groups.get(id) == NotStarted and parentWorking(plan, plan.groupParents.get(id))
        and prerequisitesComplete(plan, GroupRef(id))
        and conditions.groups.get(id)
        and ancestorGroups(plan, plan.groupParents.get(id)).forall(g => conditions.groups.get(g))),
  }
  val invJudgmentDefinition = and {
    judgmentIssues(plan, conditions) == plan.registered.filter(id => and {
      plan.issues.get(id) == Undecided,
      conditions.issues.get(id),
      ancestorGroups(plan, plan.parents.get(id)).forall(g =>
        not(terminal(plan.groups.get(g))) and conditions.groups.get(g)),
    }),
    judgmentGroups(plan, conditions) == GROUPS.filter(id =>
      plan.groups.get(id) == Undecided and conditions.groups.get(id)
        and ancestorGroups(plan, plan.groupParents.get(id)).forall(g =>
          not(terminal(plan.groups.get(g))) and conditions.groups.get(g))),
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
    not(prerequisitesComplete(plan, IssueRef(id))))
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
    | IssueOperation(change) => change.op == Accept and not(prerequisitesComplete(observation.before, IssueRef(change.id)))
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
    GROUPS.forall(id => PARENTS.contains(plan.groupParents.get(id))),
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
  val invGroupParentsScope = match observation.event {
    | ReparentGroup(change) => plan.groupParents.get(change.id) == change.parent
      and GROUPS.exclude(Set(change.id)).forall(g => plan.groupParents.get(g) == observation.before.groupParents.get(g))
      and descendantGroups(plan, change.id) == descendantGroups(observation.before, change.id)
      and descendantIssues(plan, change.id) == descendantIssues(observation.before, change.id)
    | _ => plan.groupParents == observation.before.groupParents
  }
  val invContainmentAcyclic = GROUPS.forall(g => not(ancestorGroups(plan, plan.groupParents.get(g)).contains(g)))
  val invWorkingAncestors = and {
    GROUPS.forall(g => plan.groups.get(g) == InProgress implies
      ancestorGroups(plan, plan.groupParents.get(g)).forall(a => plan.groups.get(a) == InProgress)),
    plan.registered.forall(id => plan.issues.get(id) == InProgress implies
      ancestorGroups(plan, plan.parents.get(id)).forall(a => plan.groups.get(a) == InProgress)),
  }
  val invClosedDescendantsTerminal = GROUPS.forall(g => terminal(plan.groups.get(g)) implies and {
    descendantGroups(plan, g).forall(id => terminal(plan.groups.get(id))),
    descendantIssues(plan, g).forall(id => terminal(plan.issues.get(id))),
  })
  val invGroupClosedEditsDisabled = GROUPS.forall(g => terminal(plan.groups.get(g)) implies
    GROUPS.forall(id => not(canMoveGroup(plan, id, InGroup(g)))
      and (plan.groupParents.get(id) == InGroup(g) implies
        PARENTS.forall(parent => not(canMoveGroup(plan, id, parent))))))
  val invContainmentCycleEditsDisabled = GROUPS.forall(g =>
    not(canMoveGroup(plan, g, InGroup(g)))
      and descendantGroups(plan, g).forall(d => not(canMoveGroup(plan, g, InGroup(d)))))
  val wGroupAttach = match observation.event {
    | ReparentGroup(change) => observation.before.groupParents.get(change.id) == Unassigned and change.parent != Unassigned
    | _ => false
  }
  val wGroupDetach = match observation.event {
    | ReparentGroup(change) => change.parent == Unassigned
    | _ => false
  }
  val wGroupMoveBetweenPlans = match observation.event {
    | ReparentGroup(change) => observation.before.groupParents.get(change.id) != Unassigned and change.parent != Unassigned
    | _ => false
  }
  val wWorkingSubtreeMove = wGroupMoveBetweenPlans and match observation.event {
    | ReparentGroup(change) => plan.groups.get(change.id) == InProgress
      and descendantIssues(plan, change.id).exists(id => plan.issues.get(id) == InProgress)
    | _ => false
  }
  val wThreeGroupLevels = GROUPS.exists(g => ancestorGroups(plan, plan.groupParents.get(g)).size() == 2)
  val wMixedChildren = GROUPS.exists(g => children(plan, g).size() > 0 and childGroups(plan, g).size() > 0)
  val wNestedIssueWorking = plan.registered.exists(id => plan.issues.get(id) == InProgress
    and ancestorGroups(plan, plan.parents.get(id)).size() >= 2)
  val wGroupWaitsForParentStart = GROUPS.exists(g => plan.groups.get(g) == NotStarted
    and not(parentWorking(plan, plan.groupParents.get(g))) and not(canGroup(plan, g, Start, false)))
  val wGroupReleaseBlockedByGroup = GROUPS.exists(g => plan.groups.get(g) == InProgress
    and childGroups(plan, g).exists(id => plan.groups.get(id) == InProgress)
    and not(canGroup(plan, g, Release, false)))
  val wUndecidedGroupBlocksClosure = GROUPS.exists(g => plan.groups.get(g) == InProgress
    and childGroups(plan, g).exists(id => plan.groups.get(id) == Undecided)
    and not(canGroup(plan, g, Complete, true)) and not(canGroup(plan, g, Cancel, false)))
  val wParentCompletesAfterGroup = match observation.event {
    | GroupOperation(change) => change.op == Complete
      and childGroups(plan, change.id).exists(g => plan.groups.get(g) == Completed)
    | _ => false
  }
  val wNestedGroupCancel = match observation.event {
    | GroupOperation(change) => change.op == Cancel and plan.groupParents.get(change.id) != Unassigned
      and children(plan, change.id).size() > 0
    | _ => false
  }
  val wIndirectContainmentCycleRejected = GROUPS.exists(g => GROUPS.exists(d =>
    ancestorGroups(plan, plan.groupParents.get(d)).contains(g)
      and plan.groupParents.get(d) != InGroup(g) and not(canMoveGroup(plan, g, InGroup(d)))))
  val invNoUnexpectedDeadEnd =
    (GROUPS.exists(g => plan.groups.get(g) != Completed
      and ancestorGroups(plan, plan.groupParents.get(g)).forall(a => plan.groups.get(a) != Completed))
      or plan.registered.exists(id => not(terminal(plan.issues.get(id))))) implies or {
        GROUPS.exists(g => OPERATIONS.exists(op => canGroup(plan, g, op, true))),
        plan.registered.exists(id => OPERATIONS.exists(op => canIssue(plan, id, op))),
      }
}
```

### 到達性の補助探索

通常の探索では、依存の追加や取りやめが先行し、複数の仕事を進行中に保った移動や、未登録枠を残した最終確認待ちに到達しにくい。到達性の確認には、同じ `init` と操作を使う二つの補助入口も設ける。これらは探索する操作を限定するだけで、操作の許可条件・状態更新・到達目標は変えない。

`workAndMove` は依存のない初期状態から採用・着手・移動・登録を選び、進行中の部分木移動を調べる。`finishAndExtend` は採用・着手・完了・取りやめと、計画1を計画0へ所属させる操作を選ぶ。新規登録は、計画0が最終確認可能になった時点で選ぶ。終了後も実際の外部条件変更を選べる。これらの選び方は製品の workflow を定めるものではない。

```quint target/literate/lifecycle_reachability.qnt +=
module lifecycle_reachability {
  import lifecycle_rules.* from "lifecycle_rules"
  import group_lifecycle_proposal.* from "group_lifecycle_proposal"

  action workAndMove = {
    nondet group = GROUPS.oneOf()
    nondet id = ISSUES.oneOf()
    nondet op = Set(Accept, Start).oneOf()
    nondet parent = PARENTS.oneOf()
    nondet registration = Set(Proposal, AdoptedWork).oneOf()
    any {
      performGroup(group, op, false),
      performIssue(id, op),
      moveGroup(group, parent),
      moveIssue(id, parent),
      registerIssue(id, parent, registration),
    }
  }
  action finishAndExtend = {
    nondet group = GROUPS.oneOf()
    nondet id = ISSUES.oneOf()
    nondet groupOp = Set(Accept, Start, Complete).oneOf()
    nondet issueOp = Set(Accept, Start, Complete, Cancel).oneOf()
    nondet satisfied = Set(false, true).oneOf()
    any {
      performGroup(group, groupOp, true),
      performIssue(id, issueOp),
      moveGroup(1, InGroup(0)),
      all {
        children(plan, 0).size() > 0,
        canGroup(plan, 0, Complete, true),
        registerIssue(2, InGroup(0), Proposal),
      },
      changeGroupCondition(group, satisfied),
    }
  }
}
```

### 今回の境界

入れ子の計画と所属変更、提案・採用済みの新規登録、Issue・計画間の dependency、再浮上と判断候補・着手候補・着手中の関係を、計画3件・Issue枠3件の範囲で扱う。取りやめた計画は再検討後に構成を変更できるが、完了した計画には再開経路を持たせない。

最終確認が通るかは抽象入力であり、確認工程の実装や不合格理由は扱わない。claim、declaration 編集、実際の ID 発行や永続化は検証対象外である。一覧やコマンドの構成はまだ決めない。条件の種類・設定と評価失敗は、後述の「候補一覧の評価と失敗」で扱う。

`NotEvaluated` は意味上の評価除外であり、外部コマンドを実際に呼ばないことや評価コストは実装側で別途検証する。この統合モデルでは自身の条件の意味と親による候補の除外を分ける。実際に観測する範囲と呼び出し内の結果共有は、後述の候補一覧モデルへ具体化する。

前提の循環検査は計画3件・Issue枠3件の範囲を対象とする。より多い Entity を含む具体的なグラフは探索していない。依存先が取りやめでも依存元の取りやめを強制せず、依存関係の見直しや再検討を明示操作に残す。完了した祖先の配下にある取りやめ済み Entity は、再検討可能な仕事として数えない。行き止まりがないという性質は整理・再判断の操作も含み、当初の計画どおり必ず完了できることを意味しない。

包含は計画3件の範囲で検査し、計画が四階層以上となる具体的な木は探索しない。このモデルには、追加できる Issue が1件だけという探索上の上限がある。新規登録が無効になることは、製品で追加件数を制限する提案ではない。無限件数への一般化や、必ず最終確認に合格し計画が完了することは保証しない。

### 統合モデルの再現と結果

2026-09-10、lmt `v0.0.0-20210421124901-62fe18f2f6a6` で生成・型検査後、Quint 0.32.0、Rust backend、8 threads、10,000 traces、最大200 steps、入力 seed `2026091010` で通常探索を実行した。34 invariant に反例はなく、97 witness 中96 witness が1 trace以上で観測された。計画の最終確認待ちは1,144 traces、計画の完了による後続着手の解禁は104 traces、別の枝を経由する循環の拒否は140 tracesで観測された。進行中の部分木移動は通常探索では未到達だった。

補助探索は同じ34 invariant を使い、各100 traces、最大100 steps、入力 seed `2026091011`、同じ backend・thread 数で実行し、反例はなかった。`workAndMove` で進行中の部分木移動を82 tracesで観測した。`finishAndExtend` では最終確認待ちからの追加を88 traces、完了した子計画を含む親の完了を78 tracesで観測した。同入口の `wCompletedAllChildrenDone` は未到達だったが、通常探索では19 tracesで観測されている。通常・補助探索を合わせ、全97 witness がいずれかの探索で1 trace以上に到達した。

これらは bounded random simulation の結果であり、全状態の証明ではない。補助入口は到達性の確認用に操作を選び分けており、通常探索の分布と区別する。

単独 Issue と再浮上のモデル、および共有する基本遷移のコードは変更していない。

```sh
lmt spec/lifecycle_proposal.md
quint typecheck target/literate/group_lifecycle_proposal.qnt
quint typecheck target/literate/lifecycle_reachability.qnt
quint run target/literate/group_lifecycle_proposal.qnt \
  --invariants \
    invParentOfWorkingChild invClosedChildrenTerminal invCompletedStayCompleted \
    invClosedStructureFrozen invCompletionReviewed invOperationScope \
    invDependencyOperationScope invDependenciesValid invPreconditionsAcyclic \
    invCompletedPrerequisites invStartAndCompleteRequirePrerequisites invCompletedDependenciesFrozen \
    invSelfDependencyRejected invNoAncestorDependencies invConditionInputScope \
    invTerminalConditionsNotEvaluated invCandidatesAreStartable invCandidateDefinition \
    invJudgmentDefinition invJudgmentSeparateFromWork invDependencyEditsPreserveJudgment \
    invWorkingVisibility invConditionChangesPreserveEligibility invMembershipValid \
    invRegistrationMatchesDecision invRegistrationNeverStarts invClosedEditsDisabled \
    invGroupParentsScope invContainmentAcyclic invWorkingAncestors \
    invClosedDescendantsTerminal invGroupClosedEditsDisabled invContainmentCycleEditsDisabled \
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
    wWorkingMove wRegister wRegisterProposal \
    wRegisterAdopted wAdoptedRegistrationWaitsForParent wAdoptedRegistrationCanStart \
    wRegisterOutside wAddAfterFinalCheckReady wReconsiderAllowsMembership \
    wRemoveUndecidedForCompletion wAddDependency wAddDuringWorkBlocksCompletion \
    wRemoveDependency wCrossPlanDependency wDependOnCompletedIssue \
    wDependencyRetainedOnMove wCancelledPrerequisiteBlocks wPrerequisiteCompletionUnblocksStart \
    wPrerequisiteCompletionUnblocksFinish wIndirectCycleRejected wDirectCycleRejected \
    wGroupDependsOnIssue wIssueDependsOnGroup wGroupDependsOnGroup \
    wGroupWaitsForIssue wDependentWaitsForGroupReview wGroupCompletionUnblocksDependent \
    wGroupDependencyAddedDuringWork wIssueMoveRejectedByPrerequisites wGroupMoveRejectedByPrerequisites \
    wAncestorDependencyRejected wDescendantDependencyRejected wCancelledEscapeStillRejected \
    wCrossBranchCycleRejected wGroupConditionRose wGroupConditionFell \
    wIssueConditionRose wIssueConditionFell wJudgmentIssue \
    wJudgmentGroup wJudgmentBeforeParentStart wJudgmentWithUnfinishedPrerequisite \
    wParentHidesJudgment wParentResurfacesJudgment wAcceptUnderHiddenParent \
    wAcceptWithUnfinishedPrerequisite wCandidateIssue wCandidateGroup \
    wStartHiddenIssue wStartUnderHiddenParent wStartHiddenGroup \
    wParentHidesCandidate wParentResurfacesCandidate wParentHiddenKeepsWorkingChild \
    wIssueHiddenKeepsWorking wCompletedIssueConditionIgnored wCancelledGroupConditionIgnored \
    wGroupAttach wGroupDetach wGroupMoveBetweenPlans \
    wWorkingSubtreeMove wThreeGroupLevels wMixedChildren \
    wNestedIssueWorking wGroupWaitsForParentStart wGroupReleaseBlockedByGroup \
    wUndecidedGroupBlocksClosure wParentCompletesAfterGroup wNestedGroupCancel \
    wIndirectContainmentCycleRejected \
  --max-samples 10000 --max-steps 200 --seed 2026091010 --backend rust --n-threads 8 --verbosity 1
```

補助探索も上記と同じ全 invariant を指定する。以下は生成した定義名を読み取って再現する。

```python
from pathlib import Path
import re
import subprocess

model = Path("target/literate/group_lifecycle_proposal.qnt").read_text()
invariants = re.findall(r"val (inv\w+)\s*=", model)
for step, witnesses in [
    ("workAndMove", ["wWorkingSubtreeMove", "wNestedIssueWorking", "wBothChildrenWorking"]),
    ("finishAndExtend", ["wAddAfterFinalCheckReady", "wParentCompletesAfterGroup", "wCompletedAllChildrenDone"]),
]:
    subprocess.run([
        "quint", "run", "target/literate/lifecycle_reachability.qnt",
        "--step", step, "--invariants", *invariants, "--witnesses", *witnesses,
        "--max-samples", "100", "--max-steps", "100", "--seed", "2026091011",
        "--backend", "rust", "--n-threads", "8", "--verbosity", "1",
    ], check=True)
```

## 候補一覧の評価と失敗

### 条件の種類と選択理由

再浮上条件は、未設定と外部コマンドだけを持つ。一つの Entity に設定できる外部コマンドは一つで、変更は置き換え、解除は未設定への変更とする。Axon 内に条件の AND / OR や複数条件の登録は設けない。

| 条件 | 意味 |
| --- | --- |
| 未設定 | 常に成立する |
| 外部コマンド | 実行時の観測結果から成立・未成立・判定失敗を得る |

日時まで待つ用途は残し、日時・タイムゾーンの解釈や複合条件は外部コマンド側で表現する。現行の日時入力は RFC 3339 の形式とタイムゾーンを指定する必要がある。専用条件に限定せず、設定する側が分かりやすい方法で判定できる柔軟さを優先する。Axon は日時を解釈・検証する専用の条件型を持たない。

現行の `AfterEntity` と `Manual` も採用しない。2026-09-10 にユーザーが提示した既存利用例では、`AfterEntity` の主用途は成果の必須依存ではない監査対応の推奨順だった。候補から隠す必要はなく、推奨順の管理自体も必須ではないため、代替の順序機能は追加しない。実装・設計・運用の結果が必要な例は dependency で扱える見込みと整理した。過去の各 Issue の意図を断定するものではなく、固有の用途が現れたら再検討する。

`Manual` は設定後に忘れやすく、明示解除まで無期限に隠す用途を必要としない。日時まで待って見返し、必要なら日時を設定し直す運用を選ぶ。その日時待ちも外部コマンドで表現する。これらは専用条件を採用しない判断であり、外部コマンドの表現を制限するものではない。

### 評価契約

条件が未設定なら常に浮上する。設定済みの条件は、その時点で候補に出してよいかを判定し、表示によって消費しない。成立後も再評価し、外界が変われば未成立や判定失敗にもなりうる。日時を判定する外部コマンドのように、成立が持続する条件もこの契約に含む。

現行は再浮上を着手の成立検査にも使うが、今回のモデルでは候補表示に限定する。明示操作は従来の lifecycle・包含・dependency の許可条件だけを使い、再浮上条件を評価しない。条件の設定・訂正・解除も評価せずに行い、壊れた条件を修復できる。

候補一覧に必要な判定が失敗したら、一覧の取得自体をエラーとする。未成立へ丸めず、判定できた一部の候補を成功結果として返さない。この失敗方針は現行と共通だが、評価が必要な範囲は新モデルの候補定義から決め直す。

- 状態・親の着手・dependency だけで候補にならないと確定する Entity は、自身の候補判定のためには評価しない。判断候補は dependency の完了や親の着手を求めない。
- 残った候補の祖先を上から評価し、祖先が未浮上ならその配下を評価しない。完了・取りやめは評価しない。
- 候補自身ではない進行中の親でも、子の候補判定に必要なら評価する。
- 一回の一覧取得で各 Entity を最大一回評価し、子や別の参照から同じ結果を共有する。次の取得では結果を引き継がない。

この節は、上の統合モデルの状態・許可条件を再利用して、一覧取得を複数の観測ステップへ具体化する。上の bool 入力と候補集合は失敗のない場合の意味を定める抽象モデルとして残し、評価回数・省略・失敗はこの節で扱う。両者の候補が一致することも検査する。

保存する条件は `Unset | ExternalCommand(int)` で表す。コマンド内容は不透明な識別子へ抽象化し、探索では二つの識別子を使って設定の置き換えを区別する。識別子の数はコマンド内容の探索上の制限であり、製品のコマンド種類や長さの上限ではない。一つの値を保持する型と `editSetting` の置き換えによって、条件を複数同時に保持しない。外部の結果は評価時に成立・未成立・失敗から選び、同じ呼び出し中だけ `Listing` に保持する。`Listing` と直前の観測は検証用の一時状態であり、Entity への保存項目ではない。`previousCache` は前回と異なる結果への到達性を調べる ghost state で、評価や候補判定では参照しない。

```quint target/literate/candidate_evaluation.qnt +=
module candidate_evaluation {
  import lifecycle_rules.* from "lifecycle_rules"
  import group_lifecycle_proposal.* from "group_lifecycle_proposal"

  type Setting = Unset | ExternalCommand(int)
  type ObservationResult = Unevaluated | Satisfied | Unsatisfied | EvaluationFailed
  type ListingKind = JudgmentList | StartList
  type ListingStatus = Idle | Running | Succeeded(Set[Entity]) | Failed(Entity)
  type Listing = {
    kind: ListingKind,
    status: ListingStatus,
    base: Set[Entity],
    cache: Entity -> ObservationResult,
    counts: Entity -> int,
    previousCache: Entity -> ObservationResult,
  }
  type ListingEvent = ListingInitialized | Began | EvaluatedEntity(Entity)
    | Published | SettingEdited(Entity) | PlanChanged
  type ListingObservation = {
    beforePlan: PlanState,
    beforeSettings: Entity -> Setting,
    beforeListing: Listing,
    event: ListingEvent,
  }
  var settings: Entity -> Setting
  var listing: Listing
  var listingObservation: ListingObservation

  pure def emptyListing(kind: ListingKind): Listing = {
    kind: kind, status: Idle, base: Set(),
    cache: REFS.mapBy(_ => Unevaluated), counts: REFS.mapBy(_ => 0),
    previousCache: REFS.mapBy(_ => Unevaluated),
  }
  action queryInit = all {
    init,
    settings' = REFS.mapBy(_ => Unset),
    listing' = emptyListing(JudgmentList),
    listingObservation' = {
      beforePlan: {
        groups: GROUPS.mapBy(_ => Undecided), issues: ISSUES.mapBy(_ => Undecided),
        registered: Set(0, 1), parents: ISSUES.mapBy(id => if (id == 2) Unassigned else InGroup(0)),
        groupParents: GROUPS.mapBy(_ => Unassigned), dependencies: REFS.mapBy(_ => Set()),
      },
      beforeSettings: REFS.mapBy(_ => Unset), beforeListing: emptyListing(JudgmentList),
      event: ListingInitialized,
    },
  }
  pure def ancestors(s: PlanState, e: Entity): Set[Entity] =
    ancestorGroups(s, entityParent(s, e)).map(GroupRef)
  pure def baseCandidates(s: PlanState, kind: ListingKind): Set[Entity] = {
    val selected = match kind {
      | JudgmentList => entities(s).filter(e => entityState(s, e) == Undecided)
      | StartList => startableIssues(s).map(IssueRef).union(startableGroups(s).map(GroupRef))
    }
    selected.filter(e => ancestors(s, e).forall(a => not(terminal(entityState(s, a)))))
  }
  pure def requiredEntities(s: PlanState, q: Listing): Set[Entity] =
    q.base.union(q.base.map(e => ancestors(s, e)).flatten())
  pure def parentsSatisfied(s: PlanState, q: Listing, e: Entity): bool =
    ancestors(s, e).forall(a => q.cache.get(a) == Satisfied)
  pure def pending(s: PlanState, q: Listing): Set[Entity] =
    requiredEntities(s, q).filter(e => q.cache.get(e) == Unevaluated and parentsSatisfied(s, q, e))
  pure def visible(s: PlanState, q: Listing): Set[Entity] =
    q.base.filter(e => q.cache.get(e) == Satisfied and parentsSatisfied(s, q, e))
  def recordListing(event: ListingEvent): ListingObservation = {
    beforePlan: plan, beforeSettings: settings, beforeListing: listing, event: event,
  }
  action preserveBase = all {
    plan' = plan, conditions' = conditions, observation' = observation,
  }
  action beginListing(kind: ListingKind): bool = all {
    listing.status != Running,
    preserveBase,
    settings' = settings,
    listing' = { ...emptyListing(kind), status: Running, base: baseCandidates(plan, kind),
      previousCache: listing.cache },
    listingObservation' = recordListing(Began),
  }
  action evaluateEntity(e: Entity, result: ObservationResult): bool = all {
    listing.status == Running,
    pending(plan, listing).contains(e),
    Set(Satisfied, Unsatisfied, EvaluationFailed).contains(result),
    settings.get(e) == Unset implies result == Satisfied,
    preserveBase,
    settings' = settings,
    listing' = {
      ...listing,
      status: if (result == EvaluationFailed) Failed(e) else Running,
      cache: listing.cache.set(e, result),
      counts: listing.counts.set(e, listing.counts.get(e) + 1),
    },
    listingObservation' = recordListing(EvaluatedEntity(e)),
  }
  action publishListing = all {
    listing.status == Running,
    pending(plan, listing).size() == 0,
    preserveBase,
    settings' = settings,
    listing' = { ...listing, status: Succeeded(visible(plan, listing)) },
    listingObservation' = recordListing(Published),
  }
  action editSetting(e: Entity, setting: Setting): bool = all {
    listing.status != Running,
    entities(plan).contains(e),
    settings.get(e) != setting,
    preserveBase,
    settings' = settings.set(e, setting),
    listing' = emptyListing(listing.kind),
    listingObservation' = recordListing(SettingEdited(e)),
  }
  action changePlan = all {
    listing.status != Running,
    step,
    settings' = settings,
    listing' = emptyListing(listing.kind),
    listingObservation' = recordListing(PlanChanged),
  }
  action queryStep = {
    nondet e = REFS.oneOf()
    nondet setting = Set(Unset, ExternalCommand(0), ExternalCommand(1)).oneOf()
    nondet kind = Set(JudgmentList, StartList).oneOf()
    nondet result = Set(Satisfied, Unsatisfied, EvaluationFailed).oneOf()
    any {
      changePlan,
      editSetting(e, setting),
      beginListing(kind),
      evaluateEntity(e, result),
      publishListing,
    }
  }

  pure def observedConditions(q: Listing, unseen: bool): Conditions = {
    val satisfied = REFS.mapBy(e => match q.cache.get(e) {
      | Satisfied => true
      | Unevaluated => unseen
      | _ => false
    })
    { groups: GROUPS.mapBy(g => satisfied.get(GroupRef(g))),
      issues: ISSUES.mapBy(i => satisfied.get(IssueRef(i))) }
  }
  pure def abstractCandidates(s: PlanState, q: Listing, unseen: bool): Set[Entity] = {
    val c = observedConditions(q, unseen)
    match q.kind {
      | JudgmentList => judgmentIssues(s, c).map(IssueRef).union(judgmentGroups(s, c).map(GroupRef))
      | StartList => candidateIssues(s, c).map(IssueRef).union(candidateGroups(s, c).map(GroupRef))
    }
  }
  val invQueryPreservesPlan = listingObservation.event != PlanChanged
    implies plan == listingObservation.beforePlan
  val invSettingsOwnership = match listingObservation.event {
    | SettingEdited(e) => REFS.exclude(Set(e)).forall(other =>
        settings.get(other) == listingObservation.beforeSettings.get(other))
    | _ => settings == listingObservation.beforeSettings
  }
  val invAtMostOnce = REFS.forall(e => Set(0, 1).contains(listing.counts.get(e))
    and ((listing.counts.get(e) == 0) iff (listing.cache.get(e) == Unevaluated)))
  val invEvaluationScope = REFS.forall(e => listing.counts.get(e) == 1 implies and {
    requiredEntities(plan, listing).contains(e),
    not(terminal(entityState(plan, e))),
    parentsSatisfied(plan, listing, e),
  })
  val invUnsetSatisfied = REFS.forall(e =>
    settings.get(e) == Unset and listing.counts.get(e) == 1 implies listing.cache.get(e) == Satisfied)
  val invFreshInvocation = listingObservation.event == Began implies
    REFS.forall(e => listing.counts.get(e) == 0 and listing.cache.get(e) == Unevaluated)
  val invCacheShared = match listingObservation.event {
    | EvaluatedEntity(e) => and {
        listingObservation.beforeListing.cache.get(e) == Unevaluated,
        REFS.exclude(Set(e)).forall(other => listing.cache.get(other)
          == listingObservation.beforeListing.cache.get(other)),
        listing.counts.get(e) == listingObservation.beforeListing.counts.get(e) + 1,
      }
    | Published => listing.cache == listingObservation.beforeListing.cache
      and listing.counts == listingObservation.beforeListing.counts
    | _ => true
  }
  val invFailureIsError = match listing.status {
    | Failed(e) => listing.cache.get(e) == EvaluationFailed
      and REFS.exclude(Set(e)).forall(other => listing.cache.get(other) != EvaluationFailed)
    | _ => REFS.forall(e => listing.cache.get(e) != EvaluationFailed)
  }
  val invFailureImmediate = match listingObservation.event {
    | EvaluatedEntity(e) => (listing.cache.get(e) == EvaluationFailed) iff (listing.status == Failed(e))
    | _ => true
  }
  val invSuccessfulListingComplete = match listing.status {
    | Succeeded(result) => and {
        result == abstractCandidates(plan, listing, false),
        result == abstractCandidates(plan, listing, true),
        pending(plan, listing).size() == 0,
      }
    | _ => true
  }
  val invNoEvaluationForEditsOrWork = match listingObservation.event {
    | SettingEdited(_) => listing.status == Idle and REFS.forall(e => listing.counts.get(e) == 0)
    | PlanChanged => listing.status == Idle and REFS.forall(e => listing.counts.get(e) == 0)
    | _ => true
  }
  val invBaseStableDuringQuery = listing.status != Idle
    implies listing.base == baseCandidates(plan, listing.kind)
  val invNoUnresolvedPublication = listing.status == Running and pending(plan, listing).size() == 0
    implies listing.base.forall(e => listing.cache.get(e) != Unevaluated
      or ancestors(plan, e).exists(a => listing.cache.get(a) == Unsatisfied))

  val wBeginJudgment = listingObservation.event == Began and listing.kind == JudgmentList
  val wBeginStart = listingObservation.event == Began and listing.kind == StartList
  val wListingSuccess = listingObservation.event == Published and (match listing.status {
    | Succeeded(result) => result.size() > 0
    | _ => false
  })
  val wEmptyListing = listingObservation.event == Published and listing.status == Succeeded(Set())
  val wListingFailure = match listingObservation.event {
    | EvaluatedEntity(e) => listing.status == Failed(e)
    | _ => false
  }
  val wUnsetEvaluated = match listingObservation.event {
    | EvaluatedEntity(e) => settings.get(e) == Unset and listing.cache.get(e) == Satisfied
    | _ => false
  }
  val wConfiguredSatisfied = match listingObservation.event {
    | EvaluatedEntity(e) => settings.get(e) != Unset and listing.cache.get(e) == Satisfied
    | _ => false
  }
  val wUnsatisfiedSuccess = listingObservation.event == Published
    and REFS.exists(e => listing.cache.get(e) == Unsatisfied)
  val wSetCondition = match listingObservation.event {
    | SettingEdited(e) => listingObservation.beforeSettings.get(e) == Unset and settings.get(e) != Unset
    | _ => false
  }
  val wCorrectCondition = match listingObservation.event {
    | SettingEdited(e) => listingObservation.beforeSettings.get(e) != Unset and settings.get(e) != Unset
    | _ => false
  }
  val wClearFailedCondition = match listingObservation.event {
    | SettingEdited(e) => listingObservation.beforeListing.status == Failed(e) and settings.get(e) == Unset
    | _ => false
  }
  val wCorrectFailedCondition = match listingObservation.event {
    | SettingEdited(e) => listingObservation.beforeListing.status == Failed(e) and settings.get(e) != Unset
    | _ => false
  }
  val wReevaluateChanged = match listingObservation.event {
    | EvaluatedEntity(e) => listing.previousCache.get(e) == Satisfied and listing.cache.get(e) == Unsatisfied
    | _ => false
  }
  val wFailureRecovers = match listingObservation.event {
    | EvaluatedEntity(e) => listing.previousCache.get(e) == EvaluationFailed and listing.cache.get(e) == Satisfied
    | _ => false
  }
  val wSatisfiedNotConsumed = match listingObservation.event {
    | EvaluatedEntity(e) => settings.get(e) != Unset
      and listing.previousCache.get(e) == Satisfied and listing.cache.get(e) == Satisfied
    | _ => false
  }
  val wHiddenDescendantSkipped = listingObservation.event == Published and listing.base.exists(e =>
    settings.get(e) != Unset and listing.cache.get(e) == Unevaluated
      and ancestors(plan, e).exists(a => listing.cache.get(a) == Unsatisfied))
  val wSharedParentOnce = listingObservation.event == Published and GROUPS.exists(g =>
    listing.base.filter(e => ancestors(plan, e).contains(GroupRef(g))).size() >= 2
      and settings.get(GroupRef(g)) != Unset and listing.counts.get(GroupRef(g)) == 1)
  val wWorkingParentEvaluated = listingObservation.event == Published and GROUPS.exists(g =>
    plan.groups.get(g) == InProgress and not(listing.base.contains(GroupRef(g)))
      and listing.counts.get(GroupRef(g)) == 1)
  val wTerminalSkipped = listingObservation.event == Published and entities(plan).exists(e =>
    terminal(entityState(plan, e)) and settings.get(e) != Unset and listing.counts.get(e) == 0)
  val wDependencyBlockedSkipped = listingObservation.event == Published and listing.kind == StartList
    and plan.registered.exists(i => plan.issues.get(i) == NotStarted
      and not(prerequisitesComplete(plan, IssueRef(i))) and settings.get(IssueRef(i)) != Unset
      and listing.counts.get(IssueRef(i)) == 0)
  val wJudgmentDespiteDependency = listingObservation.event == Published and listing.kind == JudgmentList
    and listing.base.exists(e => not(prerequisitesComplete(plan, e)) and visible(plan, listing).contains(e))
  val wStartAfterEvaluationFailure = listingObservation.event == PlanChanged
    and (match observation.event {
      | IssueOperation(change) => change.op == Start
        and listingObservation.beforeListing.status == Failed(IssueRef(change.id))
      | GroupOperation(change) => change.op == Start
        and listingObservation.beforeListing.status == Failed(GroupRef(change.id))
      | _ => false
    })
  val wNestedAncestorSkipped = wHiddenDescendantSkipped and listing.base.exists(e =>
    ancestors(plan, e).size() >= 2 and settings.get(e) != Unset
      and listing.cache.get(e) == Unevaluated
      and ancestors(plan, e).exists(a => listing.cache.get(a) == Unsatisfied))
  val wNoEligibleSkipsConfigured = wEmptyListing and listing.base.size() == 0
    and entities(plan).exists(e => settings.get(e) != Unset)
    and REFS.forall(e => listing.counts.get(e) == 0)

}
```

### 一覧モデルの検証範囲

成功時には、未評価の入力をすべて成立と仮定しても、すべて未成立と仮定しても、上の抽象モデルと同じ候補集合になることを検査する。これにより、評価を省いた入力が候補の欠落を隠していないかを調べる。失敗は成功の候補集合とは異なる variant で表し、失敗した Entity を保持する。途中で観測した候補は成功結果として公開しない。

評価済みの各 Entity が必要な候補かその祖先であること、全祖先の成立後にだけ評価すること、評価回数の上限、同一呼び出し内の cache 保持と次回の初期化を検査する。条件編集と既存の明示操作は評価処理を呼ばず、一覧取得や外部結果は保存した計画・条件を変えない。成立の持続、次回の未成立化、失敗からの回復はそれぞれ到達目標として扱う。

一覧取得中の lifecycle・包含・dependency・条件設定は固定し、明示操作は取得の間に行う。これは単一呼び出しを調べるための前提であり、並行更新や snapshot の実装契約を決めるものではない。外部の結果は Entity ごとの評価時に選ぶため、異なる Entity の条件を同一瞬間に観測する保証も置かない。独立した枝の評価順は非決定的で、最初の失敗で取得を終了する。エラー時にどの枝まで観測済みかは保証しない。

上の基本・統合モデルの `false` 初期入力は、未成立の外部入力から探索を始める指定であり、条件未設定の初期値ではない。この一覧モデルはすべて `Unset` から開始し、未設定が成立として扱われることを検査する。未設定の成立判定も観測ステップとして数えるが、外部コマンドを呼ぶという意味ではない。

外部コマンドの構文・終了コード、診断の表示形式、非実行閲覧のコマンド、timeout・process 管理・永続化・並行実行はモデル外である。実際の外部コマンドを呼ばないことや呼び出し回数は実装側でも検証する。判断候補と着手候補は意味上の取得単位であり、CLI を別コマンドにする決定ではない。

### 一覧モデルの再現と結果

2026-09-10、lmt `v0.0.0-20210421124901-62fe18f2f6a6` で生成し、Quint 0.32.0 で型検査後、Rust backend、8 threads、10,000 traces、最大60 steps、入力 seed `2026091012` で実行した。追加した13 invariant に反例はなく、全24 witness が1 trace以上で観測された。同じ親の評価共有は3,107 traces、入れ子の祖先による評価省略は858 traces、dependency 未完了による着手候補の評価省略は600 traces、評価に失敗した当の Entity への明示的な着手は25 tracesで観測された。これは bounded random simulation の結果であり、全状態の証明ではない。

2026-09-10、条件を未設定・外部コマンドに限定する判断を反映し、`Configured` を `ExternalCommand` へ改名した。全5モデルのコードがこのコンストラクタ名の置換以外は直前の検証対象と同一であることを機械的に比較した。再生成・型検査後、同じ backend・thread 数・入力 seed、100 traces、最大60 steps、同じ13 invariant で実行確認し、反例はなかった。この確認では witness を再集計せず、上記10,000 tracesの検査は再実行していない。

以下は repository root で実行する。既存の統合モデルの遷移・候補定義は変更せず、この一覧モデルで追加した不変条件と到達目標を検査する。

```sh
lmt spec/lifecycle_proposal.md
quint typecheck target/literate/candidate_evaluation.qnt
```

```python
from pathlib import Path
import re
import subprocess

source = Path("target/literate/candidate_evaluation.qnt").read_text()
invariants = re.findall(r"val (inv\w+)\s*=", source)
witnesses = re.findall(r"val (w\w+)\s*=", source)
subprocess.run([
    "quint", "run", "target/literate/candidate_evaluation.qnt",
    "--init", "queryInit", "--step", "queryStep",
    "--invariants", *invariants, "--witnesses", *witnesses,
    "--max-samples", "10000", "--max-steps", "60", "--seed", "2026091012",
    "--backend", "rust", "--n-threads", "8", "--verbosity", "1",
], check=True)
```
