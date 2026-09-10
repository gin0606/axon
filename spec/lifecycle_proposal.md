# Progress と採否を統合する検討用モデル

この literate specification は、Issue 1 件の Progress と採否を単一の lifecycle にまとめる案と、状態から独立した再浮上の導出を扱う。現行の仕様・既存モデルを置き換えるものではない。

説明と実行可能なモデルをこのファイルで管理する。[Quint の literate 形式](https://quint.sh/docs/literate)に従い、`lmt` で `target/literate/lifecycle_proposal.qnt` を生成する。生成ファイルは直接編集しない。

## 状態

登録直後の `Undecided`（未判断）から開始する。登録操作自体は扱わない。未判断は記録済みの提案であり、実施の約束を意味しない。採用によって `NotStarted`（未着手）へ進む。

採否を決めるためのまとまった調査は、別の Issue として採用・着手する。元の改善案は未判断のままにできる。この使い分けのために状態を増やさず、複数 Issue の関係は後の検討に残す。

```quint target/literate/lifecycle_proposal.qnt +=
module lifecycle_proposal {
  type Lifecycle = Undecided | NotStarted | InProgress | Completed | Cancelled
  type Operation = Accept | Withdraw | Start | Release | Complete | Cancel | Reconsider
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

```quint target/literate/lifecycle_proposal.qnt +=
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

## 対象外

claim、dependency、Group、declaration 編集、履歴保存、CLI、永続化、actor の権限判定は含めない。再浮上条件の種類・設定操作、一覧への表示、浮上と着手許可の接続もまだ扱わない。明示操作の許可条件は前段の lifecycle モデルを維持している。

条件定義を固定しているため、取りやめ時の定義の保存や読み戻しは検証しない。`NotEvaluated` は意味上の評価除外を表すもので、実際に外部コマンドが呼ばれないこと、読み取りコスト、実行失敗・副作用は実装側で別途検証する。

## 再現と結果

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
