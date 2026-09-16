# Progress と採否を統合する検討用モデル

この literate specification は、Progress と採否を単一の lifecycle にまとめる案を扱う。単独 Issue の再浮上を基本モデルで扱い、計画・子 Issue の着手境界、所属変更、Issue・計画間の dependency、再浮上による候補の選別を統合モデルで検討する。既存機能やデータ形式との互換性を前提にせず、新しい土台として設計するためのモデルであり、このファイルの追加・更新で現行実装や既存モデルは変更しない。

説明と実行可能なモデルをこのファイルで管理する。[Quint の literate 形式](https://quint.sh/docs/literate)に従い、`lmt` で `target/literate/` 配下に共有する基本遷移、単独 Issue、計画と子 Issue の統合モデル、到達性の補助探索、候補一覧の評価、文面・Note・状態変更履歴の6ファイルを生成する。生成ファイルは直接編集しない。

## モデル化と実装検証の分担

すべての設計判断を Quint の状態に追加することはしない。状態・関係・操作の相互作用に不確実性がある部分をモデル化し、実行環境や付随情報の取得・保存は設計判断として記述して実装側で検証する。モデルへ残す検証専用の観測値も、対象の性質を確かめるために必要な範囲に留める。

2026-09-11 に既存の実行可能モデルを見直し、次の分担とした。

| 対象 | 扱いと理由 |
| --- | --- |
| lifecycle、包含、dependency | 遷移の前提・循環・終了条件が相互作用するため維持する |
| 候補一覧の評価範囲・結果共有・失敗 | 評価省略が候補の取りこぼしや失敗の見落としにつながらないかを検査するため維持する |
| 条件コマンドの不透明な識別子 | 条件の置き換えと保持を区別するため維持し、実コマンドや環境はモデル化しない |
| 文面・Note・状態変更履歴 | 編集可能状態、追記専用性、履歴と状態の一致を検査するため維持する |
| 履歴の日時・任意の理由 | 遷移に影響せず値の保持を調べるだけだったため、探索から外し実装検証へ移す |
| 記録者情報・エージェント連携・外部コマンドの実行環境 | 操作の可否に使わない付随情報や実行詳細として、設計判断と実装検証で扱う |
| 重複着手 | モデル上は既存の原子的な状態遷移で扱い、実際の競合は保存処理の並行テストで確認する |

同一時刻の履歴が上書きされないこと、日時・理由・記録者情報の保持、自動取得と取得不能時の継続、Note の同内容追記の区別、保存失敗時の原子性は実装側の検証対象とする。独立した claim の状態やエージェント別の状態は追加しない。基本モデル・統合モデル・到達性の補助探索は、それぞれの検証範囲を維持し、変更に関係のある検査を選んで実行する。

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

完了にはさらに、その計画全体の最終確認が通ったという入力を要求する。画面単位の子計画にも、機能全体の親計画にも、それぞれ独立した最終確認がある。`reviewPassed` は確認結果の抽象入力である。CLI では Group に対する `complete <id>` の実行自体を最終確認済みの明示入力とし、別のレビュー済み状態や必須フラグを設けない。確認手順は呼び出し側の skill・運用で扱う。空の計画でもこの確認を省略しない。子の終了だけで親を自動終了しない。

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

未登録の枠は、計画内または計画外へ登録する。提案の記録は `Undecided`、採用済みの仕事の登録は `NotStarted` とする。登録操作は与えられた採否を反映し、採用判断そのものは代行しない。判断の主体や CLI 名はここでは定めない。

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

再浮上条件は、各計画・Issue の成立・不成立を外部入力として扱う。条件の種類や設定操作は今回も抽象化する。以下は着手可能性と浮上を分けるための集合である。後述の `tasks` は着手候補に限定せず、浮上した未着手と全着手中を取得する。

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

最終確認が通るかは抽象入力であり、確認工程の実装や不合格理由は扱わない。実際の ID 発行や永続化は検証対象外である。独立した claim を持たない判断は後述する。文面編集・Note・状態変更履歴は後述の情報モデルで扱う。一覧やコマンドの構成は後述の「CLI と表示」に定める。条件の種類・設定と評価失敗は、後述の「候補一覧の評価と失敗」で扱う。

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

`proposals`・`tasks` に必要な判定が失敗したら、一覧の取得自体をエラーとする。未成立へ丸めず、判定できた一部の候補を成功結果として返さない。`tasks` でも着手中だけの部分結果は返さない。この失敗方針は現行と共通だが、評価が必要な範囲は新モデルの候補定義から決め直す。

- `proposals` は未判断、`tasks` の浮上判定は未着手を対象とし、どちらも dependency の完了や親の着手を要求しない。完了・取りやめや対象外の状態は、自身の表示のためには評価しない。`tasks` は着手中を浮上に関係なく加えるため、着手中の条件は未着手の子孫の表示に必要な祖先としてのみ評価する。
- 残った候補の祖先を上から評価し、祖先が未浮上ならその配下を評価しない。完了・取りやめは評価しない。
- 候補自身ではない進行中の親でも、子の候補判定に必要なら評価する。
- 一回の一覧取得で各 Entity を最大一回評価し、子や別の参照から同じ結果を共有する。次の取得では結果を引き継がない。

### 外部コマンドの実行契約

現行の[外部条件の評価契約](../docs/reference/cli.md#外部条件の評価)を比較材料として、以下の実行・診断の仕組みを採用する。現行の `start` などが行う条件評価まで引き継ぐものではなく、評価対象は上の候補一覧の契約に従う。合意した CLI と通常表示は後述の「CLI と表示」に定める。timeout・trace などの細かなオプション表記は実装設計で揃える。

#### 結果の解釈

| 実行結果 | Axon の扱い |
| --- | --- |
| 終了コード `0` | 条件成立 |
| 終了コード `1` | 条件未成立 |
| その他の終了コード、起動失敗、シグナル終了、タイムアウト | 判定失敗。候補一覧の取得自体をエラーにする |

この終了コードは条件コマンドの契約であり、呼び出す任意のツールの意味を自動判別するものではない。利用するツールが終了コード `1` を実行エラーに使う場合などは、条件コマンド側で変換する。stdout の内容を成立判定には使わない。

#### 実行環境と責任範囲

条件はシェル文字列として保持し、`/bin/sh -c` で実行する。呼び出し元の環境変数を継承し、対話・ログイン用の shell 設定は読み込まない。PATH 上のコマンドや認証用の環境変数は利用できるが、普段の対話 shell の alias や独自構文を前提にしない。

作業ディレクトリは、呼び出した Git worktree のルートとし、Git 外では Axon の管理ルートとする。条件を設定したときの worktree には結び付けない。相対パスを許可するため、同じ条件でも実行する worktree のファイル・ブランチによって観測結果が変わりうる。

実行場所を定め、実際の場所を診断で示すことは Axon の責任とする。使い捨ての worktree に依存しないコマンドを選ぶことは利用者側で扱い、Axon が古い worktree を探索・復元・維持することはしない。相対パスの実行対象がない場合なども、成立判定の代替はせず、上の実行結果の契約に従って失敗を扱う。

#### タイムアウトと中断

タイムアウトは外部コマンド1件につき既定30秒とする。正の有限時間を Axon の呼び出し単位で指定でき、その呼び出しで評価するすべての外部コマンドに同じ値を適用する。条件の保存値には含めない。一覧全体で30秒という制限ではない。

各評価を専用の process group で起動する。タイムアウトまたは Ctrl-C ではその group へ TERM を送り、1秒後も終了していなければ KILL する。同じ group の子プロセスも終了対象にし、Axon の待機を終えるだけで放置しない。タイムアウトは未成立へ丸めず判定失敗とする。Ctrl-C では終了処理を行って一覧取得を中断し、成功した候補一覧としては返さない。

#### 出力と診断

正常終了（`0` / `1`）の stdout・stderr は、通常の候補一覧に混ぜない。判定失敗時は、対象 Entity、シェル文字列、実際の作業ディレクトリ、終了理由、取得できた stdout・stderr を診断する。タイムアウト時には適用時間と終了処理も示す。

明示的な trace では、実際に評価した正常終了の外部コマンドごとに Entity、作業ディレクトリ、成立可否、終了コード、stdout・stderr を stderr へ評価順に表示する。共有済みの評価結果を再利用しただけなら再表示せず、判定失敗は失敗診断だけを出す。空の出力は空と分かる表示にする。

出力の保持・表示は現行と同じ境界を採用する。stdout・stderr を並行して読み取り、保持上限を超えても読み捨てて pipe の詰まりを防ぐ。保持量は stream ごとに最大64 KiBで、超過時は先頭32 KiBと末尾32 KiBを残し、その間の省略byte数を示す。失敗診断と trace に同じ上限を使う。非 UTF-8 byte は置換表示し、端末制御文字は可視 escape にする。これは秘密情報の自動除去を保証するものではない。

trace の書き込み・flush が失敗した場合も、一覧取得をエラーにする。trace のための追加実行は行わず、条件編集や明示的な着手に評価を持ち込まない。

### 評価契約のモデル化

この節は、上の統合モデルの状態・許可条件を再利用して、一覧取得を複数の観測ステップへ具体化する。上の bool 入力と候補集合は失敗のない場合の意味を定める抽象モデルとして残し、評価回数・省略・失敗はこの節で扱う。`proposals` は上の判断候補、`tasks` は浮上した未着手と全着手中を合わせた抽象集合との一致を検査する。

保存する条件は `Unset | ExternalCommand(int)` で表す。コマンド内容は不透明な識別子へ抽象化し、探索では二つの識別子を使って設定の置き換えを区別する。識別子の数はコマンド内容の探索上の制限であり、製品のコマンド種類や長さの上限ではない。一つの値を保持する型と `editSetting` の置き換えによって、条件を複数同時に保持しない。外部の結果は評価時に成立・未成立・失敗から選び、同じ呼び出し中だけ `Listing` に保持する。`Listing` と直前の観測は検証用の一時状態であり、Entity への保存項目ではない。`previousCache` は前回と異なる結果への到達性を調べる ghost state で、評価や候補判定では参照しない。

```quint target/literate/candidate_evaluation.qnt +=
module candidate_evaluation {
  import lifecycle_rules.* from "lifecycle_rules"
  import group_lifecycle_proposal.* from "group_lifecycle_proposal"

  type Setting = Unset | ExternalCommand(int)
  type ObservationResult = Unevaluated | Satisfied | Unsatisfied | EvaluationFailed
  type ListingKind = JudgmentList | TasksList
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
      | TasksList => entities(s).filter(e => entityState(s, e) == NotStarted)
    }
    selected.filter(e => ancestors(s, e).forall(a => not(terminal(entityState(s, a)))))
  }
  pure def requiredEntities(s: PlanState, q: Listing): Set[Entity] =
    q.base.union(q.base.map(e => ancestors(s, e)).flatten())
  pure def parentsSatisfied(s: PlanState, q: Listing, e: Entity): bool =
    ancestors(s, e).forall(a => q.cache.get(a) == Satisfied)
  pure def pending(s: PlanState, q: Listing): Set[Entity] =
    requiredEntities(s, q).filter(e => q.cache.get(e) == Unevaluated and parentsSatisfied(s, q, e))
  pure def workingEntities(s: PlanState): Set[Entity] =
    entities(s).filter(e => entityState(s, e) == InProgress)
  pure def visible(s: PlanState, q: Listing): Set[Entity] = {
    val surfaced = q.base.filter(e => q.cache.get(e) == Satisfied and parentsSatisfied(s, q, e))
    if (q.kind == TasksList) surfaced.union(workingEntities(s)) else surfaced
  }
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
    nondet kind = Set(JudgmentList, TasksList).oneOf()
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
      | TasksList => workingEntities(s).union(entities(s).filter(e =>
          entityState(s, e) == NotStarted and parentSurfaced(s, c, entityParent(s, e))
            and (match e {
              | IssueRef(i) => c.issues.get(i)
              | GroupRef(g) => c.groups.get(g)
            })))
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
  val wBeginTasks = listingObservation.event == Began and listing.kind == TasksList
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
  val wDependencyBlockedVisible = listingObservation.event == Published and listing.kind == TasksList
    and visible(plan, listing).exists(e => entityState(plan, e) == NotStarted
      and not(prerequisitesComplete(plan, e)))
  val wParentBlockedVisible = listingObservation.event == Published and listing.kind == TasksList
    and visible(plan, listing).exists(e => entityState(plan, e) == NotStarted
      and not(parentWorking(plan, entityParent(plan, e))))
  val wWorkingHiddenVisible = listingObservation.event == Published and listing.kind == TasksList
    and workingEntities(plan).exists(e => listing.cache.get(e) == Unsatisfied
      and visible(plan, listing).contains(e))
  val wWorkingConditionSkipped = listingObservation.event == Published and listing.kind == TasksList
    and workingEntities(plan).exists(e => settings.get(e) != Unset
      and listing.counts.get(e) == 0 and visible(plan, listing).contains(e))
  val invTasksIncludesWorking = (match listing.status {
    | Succeeded(result) => listing.kind == TasksList implies workingEntities(plan).subseteq(result)
    | _ => true
  })
  val invWorkingEvaluationOnlyForDescendants = listing.kind == TasksList implies
    workingEntities(plan).forall(e => listing.counts.get(e) == 1 implies
      listing.base.exists(child => ancestors(plan, child).contains(e)))
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

上の実行契約は自然言語で定め、Quint では実行結果を成立・未成立・判定失敗へ抽象化する。shell・作業ディレクトリ・環境変数、終了コードの解釈、timeout・process group の終了、出力の上限・診断は実装側で検証する。実際に外部コマンドを呼ばないことや呼び出し回数も実装側の検証対象である。非実行閲覧のコマンド、永続化・並行実行は引き続きモデル外とする。このモデルは `proposals` と `tasks` の取得を扱う。前段の統合モデルに残る着手候補は着手可能性と浮上の関係を検討した集合であり、`tasks` の対象集合ではない。

### 一覧モデルの再現と結果

`tasks` 導入前の記録：2026-09-10、lmt `v0.0.0-20210421124901-62fe18f2f6a6` で生成し、Quint 0.32.0 で型検査後、Rust backend、8 threads、10,000 traces、最大60 steps、入力 seed `2026091012` で実行した。追加した13 invariant に反例はなく、全24 witness が1 trace以上で観測された。同じ親の評価共有は3,107 traces、入れ子の祖先による評価省略は858 traces、dependency 未完了による着手候補の評価省略は600 traces、評価に失敗した当の Entity への明示的な着手は25 tracesで観測された。これは bounded random simulation の結果であり、全状態の証明ではない。

2026-09-10、条件を未設定・外部コマンドに限定する判断を反映し、`Configured` を `ExternalCommand` へ改名した。全5モデルのコードがこのコンストラクタ名の置換以外は直前の検証対象と同一であることを機械的に比較した。再生成・型検査後、同じ backend・thread 数・入力 seed、100 traces、最大60 steps、同じ13 invariant で実行確認し、反例はなかった。この確認では witness を再集計せず、上記10,000 tracesの検査は再実行していない。

2026-09-11、`StartList` を `TasksList` へ置き換え、浮上した未着手と全着手中を合わせる集合、および評価範囲を検証した。lmt で再生成し Quint 0.32.0 で型検査後、Rust backend、8 threads、10,000 traces、最大60 steps、入力 seed `2026091102` で実行した。15 invariant に反例はなく、27 witness はすべて1 trace以上で観測された。依存未完了の未着手の表示は1,268 traces、親の着手待ちの表示は3,539 traces、条件が未成立でも着手中を含む結果は23 traces、着手中自身の不要な条件評価の省略は164 tracesで観測された。これは bounded random simulation であり、全状態の証明ではない。実行可能な定義を変更したのは一覧モデルだけで、通常遷移・包含・依存・情報操作のモデルは再実行していない。

以下は repository root で実行する。既存の統合モデルの遷移・候補定義は変更せず、この一覧モデルの `tasks` 対応後の不変条件と到達目標を検査する。

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
    "--max-samples", "10000", "--max-steps", "60", "--seed", "2026091102",
    "--backend", "rust", "--n-threads", "8", "--verbosity", "1",
], check=True)
```

## 文面・Note・状態変更履歴

### 情報の役割と編集範囲

Issue・Group とも、title・description は現在の内容を保持する。未判断・未着手・着手中は状態を変えずに編集でき、完了・取りやめ後は固定する。取りやめた Entity は明示的に再検討して未判断へ戻せば編集できる。完了には再開経路がないため、後からの訂正・補足は Note に追記する。この規則は title・description の規則であり、所属・dependency は前段で定めた変更条件に従う。

現行の採否による declaration の固定、採用時の全文保存、文面の編集履歴は今回採用しない。着手中の計画の具体化・修正のために、作業の解放や採用撤回を要求しない。完了条件をエージェントが都合よく緩和・削除することへの対処は、skill による運用とセッションログでの確認に任せる。Axon 単独では、完了前の文面の書き換えや、採用時からの差分を検証できない。残したい変更理由は Note で補足する。

Note は、調査結果・作業結果・申し送り・訂正など、状態変更と独立した情報を残す。どの状態でも追加でき、追加しても状態は変えない。追記専用とし、既存 Note は編集・削除せず、訂正は新しい Note とする。今の運用で困っておらず、後から制限を厳しくするより緩めるほうが運用コストが低いことを、この選択の理由とする。同じ内容の追記も、それぞれ別の記録として残す。

状態変更履歴は Note と別の役割を持ち、成功した lifecycle 遷移ごとに、変更前の状態・変更後の状態・日時・任意の理由を自動記録する。状態変更と履歴追加は一体で確定し、片方だけを保存しない。拒否された遷移は成功履歴を作らない。履歴も既存記録を保持し、後からの理由の補足・訂正は Note に追記する。文面編集や Note 追加を lifecycle 遷移として履歴へ混ぜず、状態変更時に文面の全文も保存しない。

Note と状態変更履歴を、一つの時系列に並べて表示することは可能だが、表示方法はまだ決めない。保存上の区別は、実際に起きた遷移を自動記録することと、自由な補足を追加することの区別である。

### 記録者情報とエージェント連携

Note・状態変更履歴には、任意の記録者情報 `{ actor: string, data: object }` を添えられる。`actor` は `codex`・`claude`・人間など記録元を示す文字列で、固定の列挙型にはしない。`data` は連携側が用途に応じた情報を入れるオブジェクトとし、例えば `{"actor":"codex","data":{"session_id":"..."}}` のように元の作業をたどる手掛かりを保持する。通常表示は actor を中心とし、詳細確認時には data も取得できるようにする。

記録者情報は、連携側が環境変数などから取得可能な範囲を自動で埋める。利用者が操作のたびに session ID 付きでコマンドを呼ぶことは要求しない。actor が分かり session ID が取れない場合は取得できた範囲を残し、記録元も分からない場合は記録者情報を省略できる。情報が取得できないことだけで Note 追加や状態変更を失敗させない。

エージェント固有の検出・メタデータ構築は、本体の状態モデルや記録の保存処理から分離する。Rust の crate として切り出す構成を想定し、具体的な構成、環境変数、検出の優先順位は実装設計で決める。Axon 本体は受け取った情報を保持・表示し、エージェントごとの必須項目や session の探索・生存確認・ログ解析は持たない。メタデータは記録時点の手掛かりであり、ログの存続やセッションの再開を保証するものではない。

### claim を独立して持たない

複数のエージェント・セッションによる別々の Issue の並行作業は想定するが、独立した claim・予約状態・作業所有者は持たない。重複着手は、現在の状態と既存の前提を確認して未着手から着手中へ原子的に更新することで防ぐ。先に着手された同じ Entity への二度目の着手は拒否する。

記録者情報は履歴をたどるために使い、完了・解放などの操作権限や排他制御には使わない。現在の作業の経緯を調べる場合も履歴を参照し、履歴と別の claim 情報を同期する仕組みを増やさない。自動解放や、作業者がいなくなったことを理由とする自動状態変更も行わない。

### 情報モデルの範囲

以下は Issue・Group に共通するローカルな情報操作を、固定2 Entity で調べる。lifecycle の基本遷移は `lifecycle_rules` を再利用する。包含・dependency・Group の最終確認による追加の遷移制約は、前段の統合モデルで扱う。このモデルが許す基本遷移だけで、実際の Group や依存を持つ Issue の操作が許可されるわけではない。

文面・Note の内容は、不透明な整数で更新・保持を区別する。履歴は変更前後の状態だけを持つ追記列へ抽象化する。日時・任意の理由・記録者情報は設計上の保存項目として残すが、操作の可否や状態遷移に影響しないため Quint の探索変数にはしない。同時刻の遷移も別の記録として保持する契約と、各付随情報の保存は実装側で検証する。このモデルは未判断の登録済み Entity から開始し、登録操作と初期状態の記録は扱わない。実時刻の取得、精度、時計補正、公開 ID、actor、記録の表示順、永続化と保存失敗はこのモデル外である。文面の過去値を持つ `observation` は検証専用の ghost state であり、製品に編集履歴を保存する提案ではない。

```quint target/literate/lifecycle_information.qnt +=
module lifecycle_information {
  import lifecycle_rules.* from "lifecycle_rules"

  type Id = int
  type TransitionRecord = { before: Lifecycle, after: Lifecycle }
  type EntityInformation = {
    lifecycle: Lifecycle,
    title: int,
    description: int,
    notes: List[int],
    history: List[TransitionRecord],
  }
  type Event = Initial | TitleEdited(Id) | DescriptionEdited(Id) | NoteAdded({ id: Id, body: int })
    | Transitioned({ id: Id, op: Operation })
  type Observation = { before: Id -> EntityInformation, event: Event }
  pure val IDS = Set(0, 1)
  pure val TEXTS = Set(0, 1, 2)
  pure val OPERATIONS = Set(Accept, Withdraw, Start, Release, Complete, Cancel, Reconsider)
  var information: Id -> EntityInformation
  var observation: Observation

  pure val initialInformation: Id -> EntityInformation = IDS.mapBy(_ => {
    lifecycle: Undecided, title: 0, description: 0, notes: List(), history: List(),
  })
  action init = all {
    information' = initialInformation,
    observation' = { before: initialInformation, event: Initial },
  }
  pure def editable(e: EntityInformation): bool =
    Set(Undecided, NotStarted, InProgress).contains(e.lifecycle)
  pure def canEditTitle(e: EntityInformation, value: int): bool = editable(e) and e.title != value
  pure def canEditDescription(e: EntityInformation, value: int): bool = editable(e) and e.description != value
  pure def applyTitle(e: EntityInformation, value: int): EntityInformation = { ...e, title: value }
  pure def applyDescription(e: EntityInformation, value: int): EntityInformation = { ...e, description: value }
  pure def applyNote(e: EntityInformation, body: int): EntityInformation = { ...e, notes: e.notes.append(body) }
  pure def applyTransition(e: EntityInformation, op: Operation): EntityInformation = {
    val targetState = applyOperation(e.lifecycle, op)
    { ...e, lifecycle: targetState,
      history: e.history.append({ before: e.lifecycle, after: targetState }) }
  }
  action editTitle(id: Id, value: int): bool = all {
    canEditTitle(information.get(id), value),
    information' = information.set(id, applyTitle(information.get(id), value)),
    observation' = { before: information, event: TitleEdited(id) },
  }
  action editDescription(id: Id, value: int): bool = all {
    canEditDescription(information.get(id), value),
    information' = information.set(id, applyDescription(information.get(id), value)),
    observation' = { before: information, event: DescriptionEdited(id) },
  }
  action addNote(id: Id, body: int): bool = all {
    information' = information.set(id, applyNote(information.get(id), body)),
    observation' = { before: information, event: NoteAdded({ id: id, body: body }) },
  }
  action transition(id: Id, op: Operation): bool = all {
    canPerform(information.get(id).lifecycle, op),
    information' = information.set(id, applyTransition(information.get(id), op)),
    observation' = { before: information, event: Transitioned({ id: id, op: op }) },
  }
  action step = {
    nondet id = IDS.oneOf()
    nondet value = TEXTS.oneOf()
    nondet op = OPERATIONS.oneOf()
    any { editTitle(id, value), editDescription(id, value), addNote(id, value), transition(id, op) }
  }
  pure def affected(event: Event): Set[Id] = match event {
    | Initial => Set()
    | TitleEdited(id) => Set(id)
    | DescriptionEdited(id) => Set(id)
    | NoteAdded(change) => Set(change.id)
    | Transitioned(change) => Set(change.id)
  }
  val invOtherEntitiesUnchanged = IDS.exclude(affected(observation.event)).forall(id =>
    information.get(id) == observation.before.get(id))
  val invTextEditScope = match observation.event {
    | TitleEdited(id) => editable(observation.before.get(id))
      and information.get(id) == { ...observation.before.get(id), title: information.get(id).title }
    | DescriptionEdited(id) => editable(observation.before.get(id))
      and information.get(id) == { ...observation.before.get(id), description: information.get(id).description }
    | _ => true
  }
  val invTerminalTextFixed = IDS.forall(id => not(editable(observation.before.get(id))) implies
    (information.get(id).title == observation.before.get(id).title
      and information.get(id).description == observation.before.get(id).description))
  val invNoteAppendOnly = match observation.event {
    | NoteAdded(change) => information.get(change.id) == {
        ...observation.before.get(change.id),
        notes: observation.before.get(change.id).notes.append(change.body),
      }
    | _ => IDS.forall(id => information.get(id).notes == observation.before.get(id).notes)
  }
  val invTransitionAndHistoryAtomic = match observation.event {
    | Transitioned(change) => {
        val before = observation.before.get(change.id)
        val after = information.get(change.id)
        and {
          canPerform(before.lifecycle, change.op),
          after.lifecycle == applyOperation(before.lifecycle, change.op),
          after.history == before.history.append({
            before: before.lifecycle, after: after.lifecycle,
          }),
          after.title == before.title, after.description == before.description, after.notes == before.notes,
        }
      }
    | _ => IDS.forall(id => information.get(id).lifecycle == observation.before.get(id).lifecycle
      and information.get(id).history == observation.before.get(id).history)
  }
  val invHistoryExplainsCurrentState = IDS.forall(id => {
    val e = information.get(id)
    val replay = e.history.foldl({ valid: true, current: Undecided }, (acc, entry) => {
      valid: acc.valid and acc.current == entry.before and OPERATIONS.exists(op =>
        canPerform(entry.before, op) and applyOperation(entry.before, op) == entry.after),
      current: entry.after,
    })
    replay.valid and replay.current == e.lifecycle
  })
  val invCompletedStaysCompleted = IDS.forall(id =>
    observation.before.get(id).lifecycle == Completed implies information.get(id).lifecycle == Completed)

  pure def isTransition(event: Event, op: Operation): bool = match event {
    | Transitioned(change) => change.op == op
    | _ => false
  }
  val wAccept = isTransition(observation.event, Accept)
  val wWithdraw = isTransition(observation.event, Withdraw)
  val wStart = isTransition(observation.event, Start)
  val wRelease = isTransition(observation.event, Release)
  val wComplete = isTransition(observation.event, Complete)
  val wCancel = isTransition(observation.event, Cancel)
  val wReconsider = isTransition(observation.event, Reconsider)
  val wTitleEdit = match observation.event { | TitleEdited(_) => true | _ => false }
  val wDescriptionEdit = match observation.event { | DescriptionEdited(_) => true | _ => false }
  val wEditAfterAcceptance = affected(observation.event).exists(id =>
    (wTitleEdit or wDescriptionEdit) and information.get(id).lifecycle == NotStarted)
  val wEditWhileWorking = affected(observation.event).exists(id =>
    (wTitleEdit or wDescriptionEdit) and information.get(id).lifecycle == InProgress)
  val wEditAfterReconsider = affected(observation.event).exists(id => {
    val h = information.get(id).history
    if (h.length() == 0) false else
      (wTitleEdit or wDescriptionEdit) and h.nth(h.length() - 1).before == Cancelled
        and h.nth(h.length() - 1).after == Undecided
  })
  val wNoteUndecided = match observation.event {
    | NoteAdded(change) => information.get(change.id).lifecycle == Undecided
    | _ => false
  }
  val wNoteNotStarted = match observation.event {
    | NoteAdded(change) => information.get(change.id).lifecycle == NotStarted
    | _ => false
  }
  val wNoteInProgress = match observation.event {
    | NoteAdded(change) => information.get(change.id).lifecycle == InProgress
    | _ => false
  }
  val wNoteCompleted = match observation.event {
    | NoteAdded(change) => information.get(change.id).lifecycle == Completed
    | _ => false
  }
  val wNoteCancelled = match observation.event {
    | NoteAdded(change) => information.get(change.id).lifecycle == Cancelled
    | _ => false
  }
  val wRepeatedNote = match observation.event {
    | NoteAdded(change) => information.get(change.id).notes.select(body => body == change.body).length() > 1
    | _ => false
  }

}
```

### 情報モデルの検査と再現

2026-09-11、日時・理由とその3 witness を探索から外した情報モデルを、lmt `v0.0.0-20210421124901-62fe18f2f6a6` で生成・型検査後、Quint 0.32.0、Rust backend、8 threads、10,000 traces、最大60 steps、入力 seed `2026091101` で実行した。7 invariant に反例はなく、残した18 witness はすべて1 trace以上で観測された。着手中の文面編集は2,591 traces、再検討後の編集は6,590 traces、完了後の Note 追加は922 tracesで観測された。これは bounded random simulation であり、全状態の証明ではない。

今回の変更では他の5モデルの実行可能な定義は変更していない。日時・理由・記録者情報の取得と保持、状態変更と履歴の保存の原子性、実際の並行着手はこの探索では検証しておらず、実装側で確認する。以下は簡素化後のモデルの再現手順である。

```sh
lmt spec/lifecycle_proposal.md
quint typecheck target/literate/lifecycle_information.qnt
```

```python
from pathlib import Path
import re
import subprocess

source = Path("target/literate/lifecycle_information.qnt").read_text()
invariants = re.findall(r"val (inv\w+)\s*=", source)
witnesses = re.findall(r"val (w\w+)\s*=", source)
subprocess.run([
    "quint", "run", "target/literate/lifecycle_information.qnt",
    "--invariants", *invariants, "--witnesses", *witnesses,
    "--max-samples", "10000", "--max-steps", "60", "--seed", "2026091101",
    "--backend", "rust", "--n-threads", "8", "--verbosity", "1",
], check=True)
```

## CLI と表示

### 情報を見る目的

採否判断、着手する仕事の選択、着手から完遂、最終確認の四つを、情報を見る目的として扱う。Entity の保存状態とは別の分類であり、四種類の状態や専用画面を追加するものではない。再開・引継ぎ・計画修正は着手から完遂に含め、採否の再判断が必要なら採否判断へ戻る。

利用者は ID を使って操作するため、表示の先頭は `ID / 種別 / 状況 / タイトル` とする。内部 field を並べて利用者に解読させず、その場の判断に必要な内容を短く示す。通常表示へ操作例やコマンド案内を毎回付けず、使い方は help へ置く。本文を機械的に要約・分類して判断材料を生成する機能は設けない。

### 一覧の入口

| コマンド | 表示対象・役割 |
| --- | --- |
| `axon proposals` | 自身とすべての祖先が浮上した未判断の Issue・Group |
| `axon tasks` | 自身とすべての祖先が浮上した未着手と、浮上を問わない着手中の Issue・Group |
| `axon list` | 非浮上・完了・取りやめも含む保存済み Entity を、必要な条件で絞り込む汎用一覧 |

`tasks` は依存先の完了待ちや親の着手待ちの未着手も含む。旧 `ready` の置き換えとして着手可能なものだけに限定しない。独立した `claims` 一覧は設けない。`list` と保存情報の `show` は外部条件コマンドを実行しない。専用一覧の取得は前述の評価契約に従う。

一覧と Group の子一覧は、状態ごとに区切らず作成日時の古い順へ統一する。作成日時そのものを通常の各行へ表示する必要はない。同時刻の安定した並べ方や絞り込み option の具体的な綴りは実装設計で揃える。

状況欄は保存された lifecycle だけの表示ではなく、保存状態と構造から導出する短い表現とする。再浮上条件の成立はこの欄での着手可能性の判定に使わない。

| 保存状態・前提 | 状況欄 |
| --- | --- |
| Undecided | 未判断 |
| NotStarted で親・dependency の着手前提を満たす | 着手可能 |
| NotStarted で着手前提が不足 | 依存待ち |
| InProgress で dependency が充足 | 着手中 |
| InProgress で未完了の依存先がある | 着手中・依存待ち |
| Completed | 完了 |
| Cancelled | 取りやめ |

親の着手待ちも「依存待ち」に含める。これは表示上のまとめ方で、保存する包含と明示 dependency の区別は維持する。Group の未終了の子は最終確認前の進捗として子一覧へ示し、明示 dependency と混ぜない。

```text
axon-screen   Group  着手中             画面Aを実装する
axon-api      Issue  着手中・依存待ち   検索APIを実装する
axon-form     Issue  着手可能           検索フォームを実装する
axon-results  Issue  依存待ち           検索結果を表示する
```

例は架空の表示内容であり、表の日本語は状況の意味を示す。CLIが生成するhelp・ラベル・診断は英語とし、利用者の原文は保持する。空白幅・グルーピング・色などは実装時に調整し、識別子・装飾・入出力の継続契約は [CLI契約](../docs/reference/lifecycle-cli.md) に従う。

### show と待ち理由

`axon show <id>` は、ID・種別・状況・タイトル、Note 件数、所属計画の ID・タイトル、本文を基本とする。本文は保存された内容を表示する。Note 本文、履歴、内部の causal 情報、不要な設定・件数の羅列、操作コマンドの案内は通常表示から外す。Note があれば `5 notes` のように存在を示し、0件ならその表示を省略する。

未充足の前提がある場合だけ、本文の前へ「着手に必要」または「完了に必要」を追加する。満たされていない直接の前提について、ID・種別・現在状態・タイトルと、必要な変化（親の着手または依存先の完了）を表示する。満たされた依存や依存先の先のツリーは常時展開しない。親の所属表示と待ち理由が同じ情報になる場合は、重複を避けて配置する。待ち理由の専用 `waits` コマンドは設けない。

Group の場合は、この共通表示の末尾へ全子孫のツリーと短い集計を加える。Completed・Cancelledも含めて全階層を展開する。各行は `ID / 種別 / 状況 / タイトル` とし、兄弟を作成日時順（同時刻は ID 順）で揃え、別の Dependencies 節へ同じ情報を再列挙しない。

集計は `2/4件終了（完了1・取りやめ1）` のように、全子孫の終了数と完了・取りやめの違いが読める形とし、見出しは `Descendants:` とする。集計は Group・Issue の両方を含み、対象自身を除く。全状態の内訳は羅列しない。進行中の Group で、子が全員終了し自身の依存先も完了していれば「最終確認待ち」と表示できる。これは導出される案内であり、保存状態やレビュー済み状態を追加しない。

保存条件や充足済みの依存、直接の逆依存を調べる場合は `show ID --details` を使う。通常の待ち理由節を保存情報の詳細へ置き換え、親・条件・全直接dependency・直接dependentを取得する。同じ関係を複数の節へ重複して列挙しない。通常表示と同様に条件コマンドは実行しない。

### Note と履歴

`list`・`tasks`・`proposals` の `--search` は現在のtitle・descriptionだけを検索する。Noteの内容を探す入口は `axon note search <語句>` とし、終了Entityを含む管理root内の全Note本文にcase-sensitiveなliteral部分一致を適用する。trim・Unicode正規化・条件実行・追加filterは行わない。空文字は構文エラー、非一致は空stdoutとstderrの案内で正常終了する。

Note検索は所属Entityの完全ID・完全な安定Note ID・日時・最初の一致と前後の抜粋を1 Noteにつき1行で表示する。Entityはlist順、Entity内はnote listの因果順（並行記録はID順）とし、見出しや分岐説明行は加えない。検索語は省略せず、文字境界を守り、`Excerpt:` と省略した側の `…` を示す。原文で検索と範囲決定をした後、改行・元のバックスラッシュ・端末制御文字を区別できる形で可視化する。保存本文は変えず、原文取得は `note show ID NOTE_ID` を使う。固定列や専用の機械向け形式は保証しない。具体的な抜粋文字数・escape・入力境界は [CLI契約](../docs/reference/lifecycle-cli.md) に従う。

`axon note list <id>` は指定 Entity の Note 本文を日時・記録者とともに表示し、`axon note add <id> …` は追記する。編集・削除は設けない。通常の逐次記録は保存順に古いものから読み、分岐した記録は因果関係を保持して表示する。分岐間の先後を時刻から捏造しない。記録者の詳細情報を取得できることは維持するが、通常は actor を中心に表示する。

`axon log <id>` は状態変更・統合の経緯を読む入口とする。変更前後の状態、日時、記録者、任意の理由を人が読める形で示す。統合では実際の操作と採用結果を区別し、内部の因果辺や記録 ID の羅列を通常表示へ出さない。

最終確認は Group・必要な子の `show` と、それぞれの Note を読む操作を組み合わせる。Note から成果・検証結果を自動抽出しない。配下の全Noteをまとめて読む入口は初回の必須機能にせず、運用後に必要性を再検討する。

### 登録・状態変更・編集

以下を公開コマンドの基本構成とする。`A` は操作対象、`B` は依存先、`G` は親 Group の ID を表す。

| 操作 | コマンド |
| --- | --- |
| 未判断の Issue / Group を登録 | `axon capture …` / `axon capture --kind group …` |
| 採用済みの Issue / Group を登録 | `axon capture --accept …` / `axon capture --kind group --accept …` |
| 採用 | `axon accept A` |
| 採用撤回 | `axon withdraw A` |
| 着手 | `axon start A` |
| 作業を解放 | `axon release A` |
| 完了 | `axon complete A` |
| 取りやめ | `axon cancel A` |
| 再検討 | `axon reconsider A` |
| タイトル・本文を編集 | `axon write A …` |
| 親 Group を設定・変更 / 解除 | `axon parent set A --parent G` / `axon parent unset A` |
| 依存先を追加 / 解除 | `axon dep add A --needs B` / `axon dep rm A --needs B` |
| 再浮上条件を設定 / 解除 | `axon condition set A --command '条件コマンド'` / `axon condition unset A` |

登録は一つのコマンドで行い、種別は `--kind issue|group`、採否は `--accept` の有無で指定する。登録と採否は直交し、作成後は同じ ID ベースの操作を使う。操作対象の ID は位置引数、関係先の ID は役割を明示する option とし、登録時の親・依存指定も `--parent`・`--needs` に揃える。`decide` の中間階層は設けない。

`write` はタイトル・本文を編集し、lifecycle を変えない。長い本文はファイルから渡せる入口を用意し、登録時と揃える。`--title`、`--file` は説明に用いた案であり、本文・Note の標準入力やファイル入力を含む細かな option 契約は実装設計で揃える。

Group への `complete` の実行自体を「計画全体の最終確認が通った」という明示入力とする。Axon は子・依存・状態を検査し、確認作業は呼び出す人・エージェントの skill と運用で担う。必須のレビュー確認フラグや独立したレビュー済み状態は設けない。どの変更コマンドも合意済みの状態・包含・依存制約を迂回しない。

表示・引数・並べ方・help の契約は実装側で検証し、Quint の保存状態を増やさない。`tasks` の集合と評価範囲は前述の一覧モデルで検証する。初期化・保存先・merge の公開 CLI は両 backend の契約と合わせて扱い、計画全体の取得と一括編集の公開 CLI は後述の「計画全体の取得と一括編集」で定める。ここで決めた通常操作の名前から推測して追加しない。

## SQLite と file backend の実装範囲

SQLite と file の両 backend を必要とする。SQLite だけを先に利用可能にする段階は許容するが、file backend を追加するときにコアや記録モデルの大幅な設計変更を必要とする進め方は採らない。保存方式から独立した状態操作・検証の境界と、分岐・統合に必要な記録の扱いを、初期の保存設計から検討する。

現行の [file backend の設計](../docs/design/file-backend.md)と[分岐履歴](../docs/development/branch-history.md)では、SQL からの状態操作の分離に加え、記録の安定 ID、現在値と分岐した記録の分離、因果関係と統合記録を共通モデルへ導入している。file backend の要件を保存 adapter の差し替えだけと捉えないための比較材料とする。旧 Revision・claim やその統合契約をそのまま再導入する判断ではない。

今回の情報モデルは単一の履歴列を対象としており、分岐した履歴の統合を検証していない。両 backend に共通する記録と統合の契約を新しい lifecycle に合わせて整理してから、初期の保存スキーマを固める。SQLite を先行させることを、分岐・統合の設計を後回しにする理由にはしない。既存データとの互換性や自動移行は要求せず、必要なデータは手動で移行する。

分岐の統合では、両側で実際に発生した状態変更履歴・Note を保持する。分岐した操作を一本の操作列へ並べ直さず、統合後の現在値を選んだことは、通常の lifecycle 遷移と区別した統合記録に残す。安定した記録 ID と先行関係を共通の保存設計へ織り込み、SQLite の連番や日時を記録の同一性・分岐間の先後関係の根拠にしない。

同じ進行中の Entity から、一方が完了、他方が作業を解放して未着手になった場合、統合時には未完了側を明示採用してよい。完了した側の履歴も残し、何を選んだかを統合記録から確認できるようにする。これは入力にある分岐の現在値を選ぶ操作であり、通常操作に Completed からの再開経路を追加するものではない。選択だけで包含・dependency・終了済み Group の構成に関する制約を免除しない。候補全体の整合性と、採用する終了状態に付随する構成の固定を検査する。

単一の履歴列を再生して現在値を説明する既存の情報モデルは、通常操作の検証として維持する。統合後の現在値は統合記録も根拠に含むため、分岐・統合の検証では通常遷移の列へ無理に還元しない。新しい相互作用に必要な検証を追加し、通常操作の完了固定・履歴追記専用性を緩めて対応しない。

### 保存先・初期化

保存先・探索・初期化は[現行の契約](../docs/reference/file-storage.md)を維持する。`axon init [prefix]` は新規作成専用で、既定は SQLite、`--backend file` で file を選ぶ。既存正本への再実行で修復・backend 切替・暗黙移行を行わない。

Git 内の SQLite は Git common directory の親の `.axon/axon.db` を linked worktree 間で共有し、file は現在の worktree の `.axon/state.jsonl` を使う。一つの repository で異種 backend を混在させない。file の変更は Git で取り込むまで他の worktree から見えず、同じ Issue に別々に着手できる。正本のない branch は未初期化であり、既存 store を使うには正本を取り込む。そこで init することは別 store の新規作成になる。

Git 内では現在の repository を探索境界とする。Git 外では最寄りの正本または初期化途中の marker を持つ祖先を管理 root とし、空の `.axon` や lock だけでは探索を止めない。Git 外の init は現在 directory を対象とし、既存管理 root 内の入れ子初期化を拒否する。backend は所定の正本から判別し、混在・破損・読取不能・初期化途中では停止する。別 backend や祖先への fallback はしない。

file の init は無関係な既存行を保って `.axon/.gitignore` と root の `.gitattributes` を生成・補完する。直接競合する設定を黙って上書きしない。SQLite の init はこれらを変更しない。Git merge driver の登録、stage・commit は利用者が行う。初期化の中断を自動修復や自動 rollback で隠さず、保存済み artifact と失敗箇所を示す。

### 自動統合の範囲と解決の入口

Entity の現在値を選ぶ自動統合は現行の慎重な単位を維持する。片側だけの変更は変更側、両側が同じ現在値ならその値を採用する。同じ Entity を両側で異なる現在値へ変更した場合は衝突とし、異なる項目の変更でも自動で混ぜない。たとえば本文変更と完了を合成して、変更後の仕事が完了したと推定しない。旧 Revision・claim はこの単位へ復活させない。

別 Entity の変更は組み合わせ、Note・実際の状態変更履歴は両側を保持する。その結果も候補全体で検証し、各入力が有効でも、依存の循環や終了した Group の構成に関する違反が生じれば適用しない。

`axon merge prepare` は入力を保全して衝突内容と解決用ファイルを用意し、正本を変更しない。`axon merge check` は明示的な選択・修正から候補を計算し、計画全体の整合性を検査する。`axon merge apply` は検査した入力と保存先が変わっていないことを再照合して適用する。未解決・不正な候補を部分適用せず、古い検査結果で変更後の入力や保存先へ適用しない。Git driver も同じ自動統合・全体検証を使い、解決できなければ明示的な解決へ渡す。

解決用ファイルの詳細形式は実装時に具体化する。通常操作と異なる現在値の選択、全記録の保持、通常の修正操作の制約を区別する。統合の検査・修正で外部条件コマンドを実行しない。これらの保存・CLI 契約だけを理由に Quint の状態を増やさない。

## 計画全体の取得と一括編集

### 目的と用途

計画全体を一つの declaration として取得し、登録・修正できるようにする。主な用途は二つある。新しく大きめの計画を一括で登録すること、登録後に不備が見つかったときに計画全体を取得して context に入れ、役割・完了条件・依存の矛盾を直してまとめて反映することである。Issue 単位の操作だけでは、エージェントが他の Issue の内容を見落とし、矛盾に気付けないことがあった。取得した declaration はファイルとして別のレビューにも渡せる。

維持する安全性は、計画全体と変更差分を保存前に検査できること、取得後に編集の前提が変わっていたら競合として止めること、部分適用を作らないこと、障害後の再実行で新規 Entity を重複作成しないことである。単一 lifecycle への再構築時に一度外した機能を、現行モデルに合わせて定め直す。旧 Revision・claim・observed の Control state・Disposition による編集固定は再導入しない。

この機能は既存の lifecycle・包含・dependency・終了構成の制約を迂回せず、通常操作と同じ共通コアの検査を通す。再浮上条件と Note は扱わない。既存 Entity の lifecycle 遷移も扱わず、`accept`・`start`・`done`・`cancel` などの通常コマンドに任せる。

### 対象範囲と編集集合

`axon export ID...` は一つ以上の ID を受け取り、Group ならその Group と全子孫、Issue ならその Issue を選ぶ。引数の ID は他のコマンドと同じく完全 ID または一意な suffix を受け付けるが、declaration 内の `id` と参照は完全 ID だけを使い、suffix は未解決として拒否する。複数指定は和集合とし、重複は除く。Completed・Cancelled の子孫も含める。lifecycle が読み取り専用で見えるため固定済みと分かり、除外すると「載せていないので触らない」と「終了済み」がファイル上で区別できないためである。保存先全体を一括で取得する selector は設けない。

declaration の `issues` と `groups` に載っている Entity だけが編集集合である。載っていない Entity は変更しない。編集集合を広げるには対象を足して再 export する。既存 Entity の record を手で書き足しても `base` は計算できず、`base` が null の record は新規 Entity とみなされて既存 ID との衝突が競合になる。ファイルから Entity を消すことは、削除、cancel、所属解除、依存解除のいずれも意味しない。Group から外すには record に `parent: null` を宣言し、作業自体をやめるには通常の `cancel` を使う。編集集合の各 Entity については、title、description、親、outgoing dependency を完全に宣言する。親の不在は `parent: null`、outgoing dependency の不在は `needs: []` で表す。

### declaration の形式

形式は strict YAML とし、schema label は `axon-declaration/v1` とする。次は既存 Group の subtree に新規 Issue を一件追加し、subtree 外の Issue へ依存を張る、`prepare` 前の例である。fingerprint は例示値である。

```yaml
schema: axon-declaration/v1
groups:
  - id: demo-k3m7pq
    key: null
    base: "blake3:1111111111111111111111111111111111111111111111111111111111111111"
    lifecycle: in-progress
    title: 検索画面を実装する
    description: |-
      ## 目的

      検索フォームと結果表示を揃える。
    parent: null
    needs: []
issues:
  - id: demo-8bxw2r
    key: null
    base: "blake3:2222222222222222222222222222222222222222222222222222222222222222"
    lifecycle: completed
    title: 検索 API を実装する
    description: レスポンス形式を固定した。
    parent: { id: demo-k3m7pq }
    needs: []
  - id: demo-c9d4ts
    key: null
    base: "blake3:3333333333333333333333333333333333333333333333333333333333333333"
    lifecycle: not-started
    title: 検索フォームを作る
    description: ""
    parent: { id: demo-k3m7pq }
    needs:
      - { id: demo-8bxw2r }
  - id: null
    key: results
    base: null
    lifecycle: not-started
    title: 検索結果を表示する
    description: フォームの送信結果を一覧に出す。
    parent: { id: demo-k3m7pq }
    needs:
      - { id: demo-8bxw2r }
      - { id: demo-zz01ab }
references:
  - id: demo-zz01ab
    kind: issue
    lifecycle: not-started
    title: 一覧の共通コンポーネントを作る
```

root は `schema`、`groups`、`issues`、`references` の 4 field だけをこの順で持つ mapping とする。すべて必須で、空の list も `[]` として書く。Group の下に子を入れ子で書く形は採らない。

`groups[]` と `issues[]` の要素は次の field をこの順で必ず持つ。kind は格納された list から決まるため、要素には書かない。

| field | 型 | 編集可否 | 契約 |
| --- | --- | --- | --- |
| `id` | string または null | identity | 既存 Entity と `prepare` 済みの新規 Entity では公開 ID。`prepare` 前の新規 Entity だけ null |
| `key` | string または null | file 内のみ | file-local の別名。`^[a-z][a-z0-9-]{0,63}$` に一致し、`groups`・`issues` を通じて file 内で一意。保存先には保存しない |
| `base` | fingerprint または null | 読み取り専用 | export 時点の値の fingerprint。未適用の新規 Entity だけ null |
| `lifecycle` | `undecided` / `not-started` / `in-progress` / `completed` / `cancelled` | 既存は読み取り専用 | 保存された lifecycle。新規 Entity は `undecided` か `not-started` のどちらかを書く |
| `title` | string | 編集可 | 空または空白だけを拒否する。それ以外は保存値をそのまま扱う |
| `description` | string | 編集可 | Markdown 本文。空文字は本文なし。trim・正規化をしない |
| `parent` | 参照 または null | 編集可 | 親 Group。Issue を親にする参照は拒否する |
| `needs` | 参照の list | 編集可 | outgoing dependency。空なら `[]` |

`base` が null の record を新規 Entity、non-null の record を既存 Entity とみなす。`id` が null なら `base` も null でなければならない。新規 Entity の record は `key` を持たなければならず、`prepare` 後に `id` を得ても `key` を消してはならない。既存 Entity に `key` を付けて別名で参照してもよい。同じ Entity を二度宣言してはならない。既存 Entity の kind は保存値で固定であり、record を `issues` と `groups` の間で移しても kind の変更にはならず、拒否する。lifecycle の綴りは `list --lifecycle` と同じにする。

参照は `{ id: demo-8bxw2r }` または `{ key: results }` のどちらか一方だけを持つ mapping とする。一つの参照 mapping への両方の併記、どちらもない mapping、未解決の ID・key はエラーである。素の文字列は使わない。解決後に同じ Entity を指す重複した `needs` はエラーとし、自己依存も拒否する。

親は child が、dependency は dependent が所有する。record に書く `parent` と `needs` はその Entity の所有物だけであり、外部から編集集合への incoming な包含・dependency は record に現れないため編集できない。初回の契約では incoming edge を declaration に表示しない。必要なら `show ID --details` で確認する。

### 外部参照

`references` には、編集集合の `parent`・`needs` が指す編集集合外の Entity を、`id`、`kind`（`issue` / `group`）、`lifecycle`、`title` の順で一件ずつ読み取り専用として載せる。description、key、関係は持たない。レビュー側が subtree 外の依存先を ID の突き合わせなしに読めるようにするための context である。

`references` は `prepare` と `apply` 後の canonical rewrite で保存先の現在値から再生成する。`check`・`apply` は `references` について、要素の集合が編集集合の `parent`・`needs` が指す編集集合外の Entity の集合と一致すること、および file に書かれた値どうしの形式と ID 順が canonical であることを検査し、kind は保存先の不変な値との一致を要求し、不一致は読み取り専用項目の書き換えとして拒否する。title や lifecycle の値が保存先の現在値と一致することは要求しない。過不足があれば `prepare` で再生成する。参照先の title や lifecycle が export 後に変わっても競合にせず、参照先が存在しない場合、kind が一致しない場合、および共通コアが拒否する状態（終了した Group を親に指定する、進行中の Entity の祖先に進行中でない Group を置く、外部を経由して循環を作るなど）だけを止める。Cancelled の Entity への依存は共通コアが通常操作で許すため、declaration でも拒否しない。編集者はそれらの値を見て編集したのではなく、旧契約のように全拒否にすると大きな計画ほど無関係な変更で止まるためである。

### canonical 形式

canonical serializer は次の規則で出力する。

1. mapping の key はこの節の表と例に示した順序で出す。alphabetical にしない。
2. `groups`・`issues` の要素は、`base` が non-null の要素を保存された作成日時順（同時刻は ID 順）、その後に `base` が null の要素を key 順で並べる。この節の ID 順・key 順はすべて bytewise UTF-8 昇順とし、locale を使わない。`list` と同じ並び方の規則であり、乱数 ID 順にしない。作成日時は declaration に書かず保存先から取るため、canonical 形は保存先の snapshot に対して定まる。`check`・`apply` は検証に使う同じ snapshot で canonical 形を判定し、`references` の要素は file 内の値の形式と順序だけを判定する。作成日時は保存後に変わらない。
3. `needs` は解決後の ID 順に並べ、`prepare` 前で ID を持たない参照先はその後に key 順で並べる。`references` は ID 順に並べる。canonical 規則は `prepare` 前後の file、`apply` 後の rewrite、`docs declaration --example` の出力のすべてに適用する。
4. duplicate な Entity、key、解決後の依存は許可せず、sort で潰さない。
5. block context の string のうち、改行を含み、改行が LF だけで、非空行が 1 行以上あり、末尾の改行が 0 個または 1 個、最初の非空行が空白で始まらず、空白文字だけからなる行や末尾空白を含まないものは literal block で出す。長さ 0 の空行は含んでよい。末尾改行がなければ `|-`、1 個なら `|` とする。flow context（参照 mapping の中）では literal block を使わない。改行を含まない string は plain scalar とし、出力する context（block か flow か）で plain scalar として同じ string に戻らないもの（空文字、null・`~`・真偽値・数値・日時に読める文字列、先頭・末尾の空白、YAML の指示子で始まるか `: ` や ` #` を含む文字列、flow 内では `,` や括弧を含む文字列など）は double quote する。上記のどちらでも無損失に表せない string（CR や制御文字、2 個以上の末尾改行、空白で始まる最初の非空行、flow context の改行など）は escape 付きの double quote で出す。保存値は共通コアが受け入れる任意の文字列であり、export と parse の往復で一文字も変えない。fingerprint は常に double quote する。
6. 参照 mapping は `{ id: demo-8bxw2r }` のように一行の flow style で出す。すべての field で null は `null`、空の list は `[]` と綴る。参照先の record が `key` を持てば `{ key: ... }`、持たなければ `{ id: ... }` で出す。
7. LF 改行、2 space indent、document marker なし、末尾 newline 一つとする。
8. comment は意味に含めず、canonical rewrite では保持しない。

parser は strict とし、unknown field、重複 key、anchor、alias、merge key、独自 tag、複数 document、mapping 以外の root、要求型と異なる scalar を拒否する。schema label が `axon-declaration/v1` 以外の file は、旧 `axon-plan/v3` を含めて変換せずに拒否し、旧モデルの形式であることを診断に示す。

### 競合検知

fingerprint は `blake3:` に続く lowercase 64 桁の hex string とする。各 Entity について、次の token 列を順に encode して BLAKE3 へ渡す。

1. schema label `axon-declaration/v1`
2. kind（`issue` / `group`）
3. ID
4. lifecycle の綴り
5. title
6. description
7. parent の presence（`none` / `some`）。`some` なら続けて解決済み parent ID
8. outgoing dependency の件数を符号なし 64 bit big-endian integer で表した 8 byte
9. prerequisite の解決済み ID を bytewise UTF-8 昇順に並べた各 token

一つの token は、UTF-8 byte 長を符号なし 64 bit big-endian integer で表した 8 byte と token 自体の UTF-8 byte を連結して encode する。件数だけは固定長整数として直接 encode する。`base` は export 時に計算し、`check`・`apply` は保存先の現在値から再計算して一致を要求する。不一致は、後述の再試行の適用済み判定に該当しない限り競合として file 全体を拒否し、対象 ID を列挙する。

fingerprint には declaration が見せる項目だけを含める。文面編集は状態記録を作らないため、記録 ID では本文の変更を検出できず、値から計算する。lifecycle は読み取り専用でも含める。Completed になっていれば編集自体が不可能で、それを競合として先に知らせる。再浮上条件、Note、記録者、作成日時、履歴、incoming edge、`references` の値は含めない。編集者が見ていない項目の変更は、その編集の意図と無関係である。export 後に変更され元の値へ戻った場合は競合にしない。編集者の意図はその値を見て作られており、現在値が同じなら適用して安全である。

### export

`axon export ID...` は一貫した snapshot から selector の和集合を編集集合として選び、canonical YAML を stdout へ出す。編集集合の各 Entity は現在値と `base` を持ち、`key` は null になる。fresh export では `key` を復元できないため、参照はすべて ID 参照になる。`references` は編集集合外の参照先から計算する。保存先を変更せず、再浮上条件を実行しない。

新規計画の雛形は `axon docs declaration --example` で取得する。新規 Group 一件、その子 Issue 二件、子 Issue 間の dependency 一本を含む完全な YAML だけを stdout に出し、`id` と `base` は null、`lifecycle` は `not-started` とする。`axon docs declaration` は field、新規と既存の違い、`prepare` → `check` → `apply` → 再 `check` の手順を説明する。両者は保存先を開かず、管理 root、ネットワーク、ソース checkout に依存しない。引数なしの `axon docs` は従来の説明を返し、declaration の説明へ案内する。

### prepare、check、apply

三つの段階を維持する。ID を apply 前に確定させることが、再試行で重複作成しない唯一の単純な方法であり、`merge prepare / check / apply` と段階構成を揃える。

`axon import prepare FILE` は保存先を変更せず、file だけを置き換える。strict YAML と局所規則（field、key、参照の形、重複、自己依存、親が Group であること）を検証し、既存 Entity の record の ID が保存先に存在し kind が一致すること、編集集合外の参照先が保存先に存在することを確認し、`id: null` の各 Entity に保存先と file の両方で衝突しない最終 ID を割り当てる。いずれかに失敗すれば file を変更しない。割り当て済みの ID が保存先に存在する場合は後述の適用済み判定を行い、該当すればその ID を保持し、該当しなければ別の writer が同じ ID を生成したものとして新しい ID を割り当て直す。key 参照は保持されるため、他の record の参照は変わらない。`key` はそのまま残し、`base` は null のままにする。`references` を現在値から再生成し、canonical 形式で元の場所へ atomic に置き換える。置き換えは file backend の正本と同じ手順に従い、temporary への書込と sync の後、file の bytes が読み取り時と同じであることを再照合してから rename し、directory を sync する。再照合前の失敗と bytes の変化は file を変更せず、rename 後の sync 失敗は file の結果不明として報告する。再照合後の非協調な editor 書込は、正本の保存契約と同じく対象外とする。すでに全 ID が割り当て済みなら新しい ID を発行せず、同じ入力への `prepare` は同じ内容を返す。競合は `prepare` の対象ではない。成功時は新規 record の `key -> 完全ID` の対応を一行ずつ表示する。

`axon import check FILE` は file と保存先を変更しない。全 Entity に ID があり、file が canonical 形式であることを要求し、違えば `prepare` が必要であることを報告して暗黙に書き換えない。検証は次の順に行う。

1. strict schema、identity、参照の局所検証と、既存 Entity の ID が保存先に存在し kind が一致すること。`references` に記載された外部 Entity の存在と kind の一致も、再試行の適用済み判定より前に検証する
2. `base` と保存先の現在値の照合、および新規 Entity の割り当て済み ID が保存先に存在しないことの確認。不一致または存在があれば後述の再試行の適用済み判定を行い、該当すれば以降の検証を省いて適用済みとして扱い、該当しなければ競合とする。`base` が一致するのに file の `lifecycle` が現在値と異なれば、読み取り専用項目の書き換えとして拒否する
3. 参照先の存在
4. 編集後の仮 snapshot を共通コアの通常操作で組み立て、包含・dependency・終了構成・固定された文面の制約を通常操作と同じ意味で検査
5. 作成、title の変更前後（改行を `\n`、制御文字を可視 escape とする list と同じ一行表示）、description の変更有無、parent の前後、`needs` の増減を Entity ごとに表示。差分がなければその旨を表示
6. 適用後の状況欄を保存情報から導出できる範囲で表示

description の全文差分は file 自体の git diff に任せ、CLI では変更の有無に留める。再浮上条件は実行しない。一件でも error があれば適用可能とは表示しない。

`axon import apply FILE` は保存先の書き込み lock を取得したあとに file の bytes を読んで digest を保持し、その内容と新しい snapshot に対して `check` と同じ検証を同じ優先順位で再実行し、差分を共通コアの通常操作の列に変換して、一つの保存境界（SQLite の一 transaction、file の一回の atomic replace）で反映する。一件でも拒否されれば全件適用せず、保存先を変えない。操作の適用順は実装が決め、有効な最終状態を中間状態の循環や前提不足で弾かないようにする。たとえば親の解除と依存の削除を追加より先に行う。新規 Entity は `lifecycle` が示す初期状態で作成し、架空の採用履歴を作らない。再浮上条件は既存 Entity では現在値を保持し、新規 Entity では未設定とする。同じ値の再指定は差分ではなく成功した no-op とする。

保存成功後に同じ file を canonical rewrite する。成功出力には、base の更新前に新規（`base: null`）だった record の `key -> 完全ID` の対応を一行ずつ含める。この apply が保存した snapshot から、新規 Entity と既存 Entity の `base` と `lifecycle` を置き換え、`references` を再生成し、record と `needs` の並びを含む canonical 形を file 全体に再適用する。`key` と key 参照は残す。lock 解放後に保存先を読み直して他者の変更を `base` に取り込まない。rewrite は `prepare` と同じ手順で元の場所へ置き換える。rename の直前に file の bytes が読み取り時の digest と一致することを再照合し、変わっていれば file を変更せず、保存先には適用済みで declaration は更新していないことを報告する。rename 後の sync 失敗は declaration の結果不明として報告する。保存先の失敗は既存の保存契約に従い、SQLite commit の失敗と正本の置換後の同期失敗は結果不明として扱う。保存成功後に declaration の更新だけが失敗または結果不明になった場合は、保存先が適用済みであることを明示する。

### 再試行

結果不明、または保存成功後の file 更新失敗のあとは、同じ file を再度 `apply` できる。`check` と `apply` は、`base` の不一致や新規 Entity の割り当て済み ID の存在を競合と判定する前に、この適用済み判定を行う。`base` が古くても、また新規 Entity の `base` が null のままその ID が保存先に存在していても、保存先の現在値が編集集合の全 Entity について declaration の最終値（title、description、parent、outgoing dependency、kind、lifecycle。新規 Entity は割り当て済み ID で存在し宣言した初期 lifecycle であること）に完全一致すれば、直前の適用済み内容と判断して保存先を no-op とし、file の rewrite だけを完了する。一部だけ一致する場合や、その後に別の変更がある場合は競合として拒否する。`prepare` で ID を確定しているため、再実行で同じ Entity が二度作られることはない。

### 拒否する入力と失敗の区別

次はいずれも file 全体を拒否し、部分適用しない。診断は原因の分類、対象 ID、固定されている項目名を示す。

- strict YAML 違反、schema label の不一致、unknown field、重複 key、anchor・alias・merge key・tag
- 一つの参照 mapping での id と key の併記、`id` が null で `base` が non-null の record、`base` が null の record の `key` 欠落、未解決の ID・key、同じ Entity の二重宣言、解決後の重複した `needs`、自己依存、Issue を親にする参照、新規 Entity の `lifecycle` が `undecided`・`not-started` 以外
- 既存 Entity の `lifecycle` の書き換え、既存 Entity の record を `issues` と `groups` の間で移す kind の変更
- `base` の不一致、または新規 Entity の割り当て済み ID が保存先に存在すること（再試行の適用済み判定に該当する場合を除く）。保存先の変更と入力側の `base` の改変・null 化は区別せず、いずれも競合として扱う
- Completed・Cancelled の title・description の差分、Completed の `needs` の差分、終了した Group の構成を変える所属変更、終了した Group を親にする作成・所属変更、進行中の Entity の祖先に進行中でない Group を置く変更、包含の循環、dependency の循環など、共通コアが通常操作でも拒否する変更。新規作成の親は終了していない Group であればよく、進行中である必要はない

終了した Entity を別の読み取り専用セクションに分けない。共通コアは Cancelled の dependency 編集を許し、Completed・Cancelled とも終了していない Group の間での所属変更を許すため、終了 Entity にも編集できる項目が残る。同じ list に置き、項目単位で拒否する。

保存境界の失敗は既存の CLI 契約と同じく Applied、Not applied、Result unknown を区別する。参照先の存在しない ID は保存先の不存在として、file 内で未解決の key とは別の診断にする。

### CLI と skill の責務

CLI は形式の検証、競合検知、共通コアによる制約検査、原子的な保存、再試行の判定を担う。skill は何を変えるかの判断、declaration の編集、check 結果の読み取り、apply の実行判断を担う。skill は保存 file を直接編集せず、一括操作を逐次の通常 CLI で代用しない。file 作成・レビューだけの依頼を保存済み Entity の変更へ広げず、`apply` の直前で同じ権限を再確認しない。個人 workflow の判断境界は[再設計資料](../docs/development/declaration-design-notes.md)に置く。

### 旧契約との対応

[旧契約](../docs/reference/declaration-file.md)は旧三軸モデル向けの形式であり、比較材料として残す。流用した部分、現行モデルに合わせて変えた部分、採らなかった案とその理由は[再設計資料](../docs/development/declaration-design-notes.md)に記す。

### 検証の分担

この機能のために Quint の状態や action を追加しない。declaration の apply は共通コアの通常操作の列であり、lifecycle・包含・dependency の意味を変えないためである。契約は Rust の独立 fixture で検証する。少なくとも次を対象にする。

- canonical example の parse と同値な serialize、strict parser の各拒否、CR・制御文字・YAML の型に読める文字列を含む文面の完全な往復
- export の selector（Group の全子孫、Issue 単体、和集合、重複除去）と `references` の計算
- `prepare` の ID 割り当て、`key` 保持、`base` null 保持、再実行の同一性
- `check` の検証順序、競合の列挙、構造差分の表示、file と保存先の不変
- `apply` の原子性（共通コアの拒否で保存先が変わらないこと）、両 backend での同じ結果、`base` と `references` の rewrite、`key` の保持
- 終了 Entity の固定項目、終了 Group の構成、循環、Issue 親、自己依存の拒否
- 結果不明・file 更新失敗後の再 `apply` が最終値一致で no-op になること、部分一致で競合になること
- 再浮上条件の保持と未実行、Note の不変
- `docs declaration` と `--example` が保存先を開かないこと
