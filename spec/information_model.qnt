// docs/reference/information-model.md の規範契約を検査可能にしたモデル。
//
// 検査対象は「どの情報を誰が所有し、どの操作がどこを変えてよいか」に限る。
// ready / blocked / orphaned / active scope などの導出規則、dependency の待ち graph の
// 非循環、Group の完了条件は spec/group_plan.qnt が持つ。ここで再定義すると導出規則が
// 2 箇所に散るため、このモデルは declaration の値としてのみ parent と dependency を扱い、
// parent の循環だけを最小限に拒否する。
//
// 対象外: claim、actor の本人性、DB 直接編集、複数利用者の並行編集の merge、redaction、
// 公開名称・コマンド構文・出力文言。AtDate は clock を持たない不透明な control 値として扱う。
module information_model {
  // ---- Identity ----
  type Id = int
  type EntityKind = Issue | Group

  // ---- Control state ----
  type Progress = NotStarted | InProgress | Ended
  type Disposition = Undecided | Accepted | Rejected
  type ResurfaceCondition = Always | AtDate(int) | AfterEntity(Id)

  type Control = {
    progress: Progress,
    disposition: Disposition,
    resurface: ResurfaceCondition,
  }

  // ---- Plan declaration ----
  // Entity 自身が所有する宣言。incoming dependency と子 Entity は関係の反対側が所有するため
  // ここには持たず、他 Entity の declaration から導出する。
  type Declaration = {
    title: int,
    description: int,
    parent: Id,
    outgoing: Set[Id],
  }

  type DeclarationField = TitleField | DescriptionField | ParentField | OutgoingField

  type Entity = {
    kind: EntityKind,
    declaration: Declaration,
    control: Control,
  }

  // ---- Typed history ----
  type DecisionChange =
    | DispositionDecided(Disposition)
    | ResurfaceChanged(ResurfaceCondition)

  type DecisionRecord = {
    target: Id,
    change: DecisionChange,
    snapshot: int,
    reason: int,
    actor: int,
  }

  type ProgressRecord = {
    target: Id,
    before: Progress,
    after: Progress,
    actor: int,
  }

  // ---- Supplemental information ----
  // 保存順を正とする追記専用の列。inputTime は入力時刻であり、順序の正ではない。
  type SupplementalRecord = {
    target: Id,
    ref: int,
    body: int,
    actor: int,
    inputTime: int,
  }

  type State = {
    entities: Id -> Entity,
    snapshots: Id -> List[Declaration],
    currentSnapshot: Id -> int,
    decisionHistory: List[DecisionRecord],
    progressHistory: List[ProgressRecord],
    supplemental: List[SupplementalRecord],
  }

  type RecordCounts = {
    decisions: int,
    progressions: int,
    snapshots: int,
    supplemental: int,
  }

  type Operation =
    | NoOperation
    | SetTitleOp
    | SetDescriptionOp
    | SetParentOp
    | UnsetParentOp
    | AddDependencyOp
    | RemoveDependencyOp
    | DecideOp
    | SetWhenOp
    | StartOp
    | DoneOp
    | ReleaseOp
    | AddSupplementalOp

  // 遷移そのものを検査するための ghost state。axon が保存する情報ではない。
  type Transition = {
    before: State,
    operation: Operation,
    target: Id,
  }

  // ---- 定数 ----
  pure val ISSUES: Set[Id] = Set(0, 1)
  pure val GROUPS: Set[Id] = Set(2, 3)
  pure val ENTITIES: Set[Id] = ISSUES.union(GROUPS)

  pure val NO_PARENT: Id = -1
  pure val NO_TARGET: Id = -1
  pure val NO_SNAPSHOT: int = -1

  // 文面は不透明な値として扱う。差分の有無だけが意味を持つ。
  pure val TEXTS = Set(0, 1)
  pure val BODIES = Set(0, 1)
  pure val ACTORS = Set(0, 1)
  pure val INPUT_TIMES = Set(0, 1, 2)
  pure val REASONS = Set(1, 2)
  pure val DATES = Set(1, 2)
  pure val DISPOSITIONS = Set(Undecided, Accepted, Rejected)
  pure val CONDITIONS: Set[ResurfaceCondition] =
    Set(Always)
      .union(DATES.map(d => AtDate(d)))
      .union(ENTITIES.map(i => AfterEntity(i)))

  pure val BASELINE_ENTITY: Id = 1

  // 全域な参照を保つための番兵。実在の値と一致しない。
  pure val MISSING_DECLARATION: Declaration = {
    title: -1,
    description: -1,
    parent: NO_PARENT,
    outgoing: Set(),
  }

  // ---- 初期状態 ----
  pure def initialKind(i: Id): EntityKind = if (ISSUES.contains(i)) Issue else Group

  pure def initialParent(i: Id): Id = if (i == 0) 2 else NO_PARENT

  pure def initialDeclaration(i: Id): Declaration = {
    title: 0,
    description: 0,
    parent: initialParent(i),
    outgoing: Set(),
  }

  pure def initialControl(i: Id): Control = {
    progress: NotStarted,
    disposition: if (i == BASELINE_ENTITY) Accepted else Undecided,
    resurface: Always,
  }

  pure def initialEntity(i: Id): Entity = {
    kind: initialKind(i),
    declaration: initialDeclaration(i),
    control: initialControl(i),
  }

  // migration 前の判断を捏造せず、決定済み Entity の現在値だけを baseline snapshot とする。
  pure val INITIAL_STATE: State = {
    entities: ENTITIES.mapBy(i => initialEntity(i)),
    snapshots: ENTITIES.mapBy(i =>
      if (i == BASELINE_ENTITY) List(initialDeclaration(i)) else List()),
    currentSnapshot: ENTITIES.mapBy(i => if (i == BASELINE_ENTITY) 0 else NO_SNAPSHOT),
    decisionHistory: List(),
    progressHistory: List(),
    supplemental: List(),
  }

  // ---- Accessor ----
  pure def entity(s: State, i: Id): Entity = s.entities.get(i)
  pure def decl(s: State, i: Id): Declaration = entity(s, i).declaration
  pure def control(s: State, i: Id): Control = entity(s, i).control
  pure def isGroup(s: State, i: Id): bool = entity(s, i).kind == Group
  pure def isDecided(d: Disposition): bool = d != Undecided

  // lastTransition.target は NoOperation のとき NO_TARGET を指す。参照を全域にする。
  pure def targetDeclaration(s: State, i: Id): Declaration =
    if (ENTITIES.contains(i)) decl(s, i) else MISSING_DECLARATION

  pure def targetDisposition(s: State, i: Id): Disposition =
    if (ENTITIES.contains(i)) control(s, i).disposition else Undecided

  pure def targetProgress(s: State, i: Id): Progress =
    if (ENTITIES.contains(i)) control(s, i).progress else NotStarted

  // ---- 所有範囲の反対側 (導出) ----
  // 子と incoming dependency は保存せず、他 Entity の declaration から計算する。
  pure def children(s: State, group: Id): Set[Id] =
    ENTITIES.filter(i => decl(s, i).parent == group)

  pure def incoming(s: State, target: Id): Set[Id] =
    ENTITIES.filter(i => decl(s, i).outgoing.contains(target))

  // ---- parent の非循環 ----
  pure def parentEdges(s: State): Set[(Id, Id)] =
    ENTITIES
      .filter(i => decl(s, i).parent != NO_PARENT)
      .map(i => (i, decl(s, i).parent))

  pure def directTargets(edges: Set[(Id, Id)], source: Id): Set[Id] =
    ENTITIES.filter(target => edges.contains((source, target)))

  pure def reach1(edges: Set[(Id, Id)], source: Id): Set[Id] =
    directTargets(edges, source)
  pure def reach2(edges: Set[(Id, Id)], source: Id): Set[Id] =
    reach1(edges, source).union(reach1(edges, source).map(i => reach1(edges, i)).flatten())
  pure def reach3(edges: Set[(Id, Id)], source: Id): Set[Id] =
    reach2(edges, source).union(reach2(edges, source).map(i => reach1(edges, i)).flatten())
  pure def reach4(edges: Set[(Id, Id)], source: Id): Set[Id] =
    reach3(edges, source).union(reach3(edges, source).map(i => reach1(edges, i)).flatten())

  pure def acyclic(edges: Set[(Id, Id)]): bool =
    ENTITIES.forall(i => not(reach4(edges, i).contains(i)))

  // ---- 決定済み declaration の固定 ----
  pure def declarationMutable(s: State, i: Id): bool =
    control(s, i).disposition == Undecided

  // declaration の全変更経路がこの述語を通る。一部の経路から固定を迂回させない。
  // 現在値と同じ値の再指定は declaration の変更ではないため、固定中も成功してよい。
  pure def canReplaceDeclaration(s: State, i: Id, updated: Declaration): bool = or {
    updated == decl(s, i),
    declarationMutable(s, i),
  }

  pure def applyDeclaration(s: State, i: Id, updated: Declaration): State =
    {...s, entities: s.entities.set(i, {...entity(s, i), declaration: updated})}

  pure def withTitle(s: State, i: Id, value: int): Declaration =
    {...decl(s, i), title: value}

  pure def withDescription(s: State, i: Id, value: int): Declaration =
    {...decl(s, i), description: value}

  pure def withParent(s: State, i: Id, group: Id): Declaration =
    {...decl(s, i), parent: group}

  pure def withDependency(s: State, i: Id, target: Id): Declaration =
    {...decl(s, i), outgoing: decl(s, i).outgoing.union(Set(target))}

  pure def withoutDependency(s: State, i: Id, target: Id): Declaration =
    {...decl(s, i), outgoing: decl(s, i).outgoing.exclude(Set(target))}

  pure def canSetTitle(s: State, i: Id, value: int): bool =
    canReplaceDeclaration(s, i, withTitle(s, i, value))

  pure def canSetDescription(s: State, i: Id, value: int): bool =
    canReplaceDeclaration(s, i, withDescription(s, i, value))

  pure def canSetParent(s: State, i: Id, group: Id): bool = {
    val updated = withParent(s, i, group)
    and {
      isGroup(s, group),
      group != i,
      canReplaceDeclaration(s, i, updated),
      acyclic(parentEdges(applyDeclaration(s, i, updated))),
    }
  }

  pure def canUnsetParent(s: State, i: Id): bool =
    canReplaceDeclaration(s, i, withParent(s, i, NO_PARENT))

  // 既存の dependency 契約として自己依存を許さない。
  pure def canAddDependency(s: State, source: Id, target: Id): bool = and {
    source != target,
    canReplaceDeclaration(s, source, withDependency(s, source, target)),
  }

  pure def canRemoveDependency(s: State, source: Id, target: Id): bool =
    canReplaceDeclaration(s, source, withoutDependency(s, source, target))

  // ---- 決定済み declaration の記録 ----
  pure def snapshotsOf(s: State, i: Id): List[Declaration] = s.snapshots.get(i)
  pure def currentSnapshotOf(s: State, i: Id): int = s.currentSnapshot.get(i)

  pure def validSnapshotIndex(s: State, i: Id, k: int): bool =
    k >= 0 and k < snapshotsOf(s, i).length()

  pure def snapshotAt(s: State, i: Id, k: int): Declaration =
    if (validSnapshotIndex(s, i, k)) snapshotsOf(s, i).nth(k) else MISSING_DECLARATION

  pure def isBaselineSnapshot(i: Id, k: int): bool = i == BASELINE_ENTITY and k == 0

  pure def latestSnapshotMatchesCurrent(s: State, i: Id): bool = {
    val snaps = snapshotsOf(s, i)
    if (snaps.length() == 0) false else snaps.nth(snaps.length() - 1) == decl(s, i)
  }

  // declaration が前回の決定済み snapshot と同じなら、同じ snapshot に別の判断を結び付ける。
  pure def snapshotIndexOnDecide(s: State, i: Id): int = {
    val snaps = snapshotsOf(s, i)
    if (latestSnapshotMatchesCurrent(s, i)) snaps.length() - 1 else snaps.length()
  }

  pure def declarationDiff(a: Declaration, b: Declaration): Set[DeclarationField] =
    (if (a.title != b.title) Set(TitleField) else Set())
      .union(if (a.description != b.description) Set(DescriptionField) else Set())
      .union(if (a.parent != b.parent) Set(ParentField) else Set())
      .union(if (a.outgoing != b.outgoing) Set(OutgoingField) else Set())

  pure def latestSnapshotDiff(s: State, i: Id): Set[DeclarationField] = {
    val snaps = snapshotsOf(s, i)
    if (snaps.length() < 2) Set()
    else declarationDiff(snaps.nth(snaps.length() - 2), snaps.nth(snaps.length() - 1))
  }

  // ---- 記録の絞り込み ----
  pure def isDispositionDecision(r: DecisionRecord): bool =
    match r.change {
    | DispositionDecided(_) => true
    | ResurfaceChanged(_) => false
    }

  pure def decisionRecordsOf(s: State, i: Id): List[DecisionRecord] =
    s.decisionHistory.select(r => r.target == i)

  pure def dispositionDecisions(s: State, i: Id): List[DecisionRecord] =
    s.decisionHistory.select(r => r.target == i and isDispositionDecision(r))

  pure def progressRecordsOf(s: State, i: Id): List[ProgressRecord] =
    s.progressHistory.select(r => r.target == i)

  pure def supplementalOf(s: State, i: Id): List[SupplementalRecord] =
    s.supplemental.select(r => r.target == i)

  // Entity の詳細表示から観測できる各種記録の件数。
  pure def recordCounts(s: State, i: Id): RecordCounts = {
    decisions: decisionRecordsOf(s, i).length(),
    progressions: progressRecordsOf(s, i).length(),
    snapshots: snapshotsOf(s, i).length(),
    supplemental: supplementalOf(s, i).length(),
  }

  // ---- 状態を変える操作 ----
  pure def canDecide(s: State, i: Id, choice: Disposition): bool =
    control(s, i).disposition != choice

  pure def applyDecide(s: State, i: Id, choice: Disposition, reason: int, actor: int): State = {
    val decided = isDecided(choice)
    val nextSnapshot = if (decided) snapshotIndexOnDecide(s, i) else NO_SNAPSHOT
    val nextSnapshots =
      if (decided and not(latestSnapshotMatchesCurrent(s, i)))
        s.snapshots.set(i, snapshotsOf(s, i).append(decl(s, i)))
      else s.snapshots
    val record = {
      target: i,
      change: DispositionDecided(choice),
      snapshot: nextSnapshot,
      reason: reason,
      actor: actor,
    }
    {...s,
      entities: s.entities.set(i, {...entity(s, i), control: {...control(s, i), disposition: choice}}),
      snapshots: nextSnapshots,
      currentSnapshot: s.currentSnapshot.set(i, nextSnapshot),
      decisionHistory: s.decisionHistory.append(record),
    }
  }

  pure def canSetWhen(s: State, i: Id, condition: ResurfaceCondition): bool =
    control(s, i).resurface != condition

  pure def applySetWhen(s: State, i: Id, condition: ResurfaceCondition, reason: int, actor: int): State = {
    val record = {
      target: i,
      change: ResurfaceChanged(condition),
      snapshot: NO_SNAPSHOT,
      reason: reason,
      actor: actor,
    }
    {...s,
      entities: s.entities.set(i, {...entity(s, i), control: {...control(s, i), resurface: condition}}),
      decisionHistory: s.decisionHistory.append(record),
    }
  }

  // 進行の可否は Progress のみで決める。ready の導出規則は spec/group_plan.qnt が持つ。
  pure def canStart(s: State, i: Id): bool = control(s, i).progress == NotStarted
  pure def canDone(s: State, i: Id): bool = control(s, i).progress == InProgress
  pure def canRelease(s: State, i: Id): bool = control(s, i).progress == InProgress

  pure def applyProgress(s: State, i: Id, updated: Progress, actor: int): State = {
    val record = {
      target: i,
      before: control(s, i).progress,
      after: updated,
      actor: actor,
    }
    {...s,
      entities: s.entities.set(i, {...entity(s, i), control: {...control(s, i), progress: updated}}),
      progressHistory: s.progressHistory.append(record),
    }
  }

  // Progress、Disposition、Resurface condition によって追加を制限しない。
  // ここに条件を足すと invSupplementalNeverRestricted が壊れる。
  pure def canAddSupplemental(s: State, i: Id): bool = ENTITIES.contains(i)

  // 対象内で安定した識別子を採番する。既存記録は書き換えない。
  pure def nextRef(s: State, i: Id): int = supplementalOf(s, i).length()

  pure def applyAddSupplemental(s: State, i: Id, body: int, actor: int, inputTime: int): State = {
    val record = {
      target: i,
      ref: nextRef(s, i),
      body: body,
      actor: actor,
      inputTime: inputTime,
    }
    {...s, supplemental: s.supplemental.append(record)}
  }

  // ---- State machine ----
  var state: State
  var lastTransition: Transition

  action init: bool = all {
    state' = INITIAL_STATE,
    lastTransition' = { before: INITIAL_STATE, operation: NoOperation, target: NO_TARGET },
  }

  action commit(updated: State, operation: Operation, target: Id): bool = all {
    state' = updated,
    lastTransition' = { before: state, operation: operation, target: target },
  }

  action doSetTitle: bool = {
    nondet i = ENTITIES.oneOf()
    nondet value = TEXTS.oneOf()
    all {
      canSetTitle(state, i, value),
      commit(applyDeclaration(state, i, withTitle(state, i, value)), SetTitleOp, i),
    }
  }

  action doSetDescription: bool = {
    nondet i = ENTITIES.oneOf()
    nondet value = TEXTS.oneOf()
    all {
      canSetDescription(state, i, value),
      commit(applyDeclaration(state, i, withDescription(state, i, value)), SetDescriptionOp, i),
    }
  }

  action doSetParent: bool = {
    nondet i = ENTITIES.oneOf()
    nondet group = GROUPS.oneOf()
    all {
      canSetParent(state, i, group),
      commit(applyDeclaration(state, i, withParent(state, i, group)), SetParentOp, i),
    }
  }

  action doUnsetParent: bool = {
    nondet i = ENTITIES.oneOf()
    all {
      canUnsetParent(state, i),
      commit(applyDeclaration(state, i, withParent(state, i, NO_PARENT)), UnsetParentOp, i),
    }
  }

  action doAddDependency: bool = {
    nondet source = ENTITIES.oneOf()
    nondet target = ENTITIES.oneOf()
    all {
      canAddDependency(state, source, target),
      commit(applyDeclaration(state, source, withDependency(state, source, target)),
             AddDependencyOp, source),
    }
  }

  action doRemoveDependency: bool = {
    nondet source = ENTITIES.oneOf()
    nondet target = ENTITIES.oneOf()
    all {
      canRemoveDependency(state, source, target),
      commit(applyDeclaration(state, source, withoutDependency(state, source, target)),
             RemoveDependencyOp, source),
    }
  }

  action doDecide: bool = {
    nondet i = ENTITIES.oneOf()
    nondet choice = DISPOSITIONS.oneOf()
    nondet reason = REASONS.oneOf()
    nondet actor = ACTORS.oneOf()
    all {
      canDecide(state, i, choice),
      commit(applyDecide(state, i, choice, reason, actor), DecideOp, i),
    }
  }

  action doSetWhen: bool = {
    nondet i = ENTITIES.oneOf()
    nondet condition = CONDITIONS.oneOf()
    nondet reason = REASONS.oneOf()
    nondet actor = ACTORS.oneOf()
    all {
      canSetWhen(state, i, condition),
      commit(applySetWhen(state, i, condition, reason, actor), SetWhenOp, i),
    }
  }

  action doStart: bool = {
    nondet i = ENTITIES.oneOf()
    nondet actor = ACTORS.oneOf()
    all {
      canStart(state, i),
      commit(applyProgress(state, i, InProgress, actor), StartOp, i),
    }
  }

  action doDone: bool = {
    nondet i = ENTITIES.oneOf()
    nondet actor = ACTORS.oneOf()
    all {
      canDone(state, i),
      commit(applyProgress(state, i, Ended, actor), DoneOp, i),
    }
  }

  action doRelease: bool = {
    nondet i = ENTITIES.oneOf()
    nondet actor = ACTORS.oneOf()
    all {
      canRelease(state, i),
      commit(applyProgress(state, i, NotStarted, actor), ReleaseOp, i),
    }
  }

  action doAddSupplemental: bool = {
    nondet i = ENTITIES.oneOf()
    nondet body = BODIES.oneOf()
    nondet actor = ACTORS.oneOf()
    nondet inputTime = INPUT_TIMES.oneOf()
    all {
      canAddSupplemental(state, i),
      commit(applyAddSupplemental(state, i, body, actor, inputTime), AddSupplementalOp, i),
    }
  }

  action step: bool = any {
    doSetTitle,
    doSetDescription,
    doSetParent,
    doUnsetParent,
    doAddDependency,
    doRemoveDependency,
    doDecide,
    doSetWhen,
    doStart,
    doDone,
    doRelease,
    doAddSupplemental,
  }

  // ---- 検査の補助 ----
  pure def appendOnly(before: List[a], after: List[a]): bool =
    if (before.length() > after.length()) false
    else before.indices().forall(k => after.nth(k) == before.nth(k))

  pure def isDeclarationOp(operation: Operation): bool = or {
    operation == SetTitleOp,
    operation == SetDescriptionOp,
    operation == SetParentOp,
    operation == UnsetParentOp,
    operation == AddDependencyOp,
    operation == RemoveDependencyOp,
  }

  pure def isProgressOp(operation: Operation): bool = or {
    operation == StartOp,
    operation == DoneOp,
    operation == ReleaseOp,
  }

  pure def declarationsUnchanged(before: State, after: State): bool =
    ENTITIES.forall(i => decl(after, i) == decl(before, i))

  pure def controlsUnchanged(before: State, after: State): bool =
    ENTITIES.forall(i => control(after, i) == control(before, i))

  pure def progressesUnchanged(before: State, after: State): bool =
    ENTITIES.forall(i => control(after, i).progress == control(before, i).progress)

  pure def dispositionsUnchanged(before: State, after: State): bool =
    ENTITIES.forall(i => control(after, i).disposition == control(before, i).disposition)

  pure def resurfacesUnchanged(before: State, after: State): bool =
    ENTITIES.forall(i => control(after, i).resurface == control(before, i).resurface)

  val before: State = lastTransition.before
  val operation: Operation = lastTransition.operation
  val target: Id = lastTransition.target

  // ---- 決定済み declaration の固定 ----

  // 直前状態で決定済みだった Entity の declaration は、どの経路からも変わらない。
  val invDecidedDeclarationFrozen: bool =
    ENTITIES.forall(i =>
      isDecided(control(before, i).disposition)
        implies decl(state, i) == decl(before, i))

  // ---- Plan declaration の所有範囲 ----

  // 1 ステップで変わる declaration は、その操作の対象 1 件だけ。
  val invOnlyOwnerDeclarationChanges: bool =
    ENTITIES.forall(i => decl(state, i) != decl(before, i) implies i == target)

  // Group の子集合が変わるとき、変更される declaration は子側であり Group 自身ではない。
  val invChildSetOwnedByChild: bool =
    GROUPS.forall(group => {
      val childrenBefore = children(before, group)
      val childrenAfter = children(state, group)
      (childrenBefore != childrenAfter) implies and {
        decl(state, group) == decl(before, group),
        childrenAfter
          .exclude(childrenBefore)
          .union(childrenBefore.exclude(childrenAfter))
          .forall(child => decl(state, child) != decl(before, child)),
      }
    })

  // incoming dependency が変わるとき、変更される declaration は依存元であり依存先ではない。
  val invIncomingDependencyOwnedBySource: bool =
    ENTITIES.forall(i =>
      incoming(state, i) != incoming(before, i) implies decl(state, i) == decl(before, i))

  val invParentsAcyclic: bool = acyclic(parentEdges(state))

  // ---- 決定済み declaration の記録 ----

  // 決定済みだけが現在 snapshot を参照し、Undecided は参照しない。
  val invCurrentSnapshotByDisposition: bool =
    ENTITIES.forall(i => {
      val current = currentSnapshotOf(state, i)
      if (isDecided(control(state, i).disposition)) validSnapshotIndex(state, i, current)
      else current == NO_SNAPSHOT
    })

  // 決定済み Entity の現在 declaration は、明示的な現在 snapshot と一致する。
  val invDecidedMatchesSnapshot: bool =
    ENTITIES.forall(i =>
      isDecided(control(state, i).disposition) implies
        snapshotAt(state, i, currentSnapshotOf(state, i)) == decl(state, i))

  // 採否判断履歴があれば、最新の判断と現在参照が同じ disposition / snapshot を指す。
  val invLatestDecisionMatchesCurrentSnapshot: bool =
    ENTITIES.forall(i => {
      val decisions = dispositionDecisions(state, i)
      if (decisions.length() == 0) true else {
        val latest = decisions.nth(decisions.length() - 1)
        and {
          latest.change == DispositionDecided(control(state, i).disposition),
          latest.snapshot == currentSnapshotOf(state, i),
        }
      }
    })

  // 判断履歴で snapshot を持つのは Accepted / Rejected だけ。
  val invSnapshotOnlyForDecided: bool =
    state.decisionHistory.indices().forall(k => {
      val record = state.decisionHistory.nth(k)
      match record.change {
      | DispositionDecided(choice) =>
          if (isDecided(choice)) validSnapshotIndex(state, record.target, record.snapshot)
          else record.snapshot == NO_SNAPSHOT
      | ResurfaceChanged(_) => record.snapshot == NO_SNAPSHOT
      }
    })

  // snapshot は移行時の baseline か、切り替え後の採否判断に対応する。
  val invEverySnapshotHasOrigin: bool =
    ENTITIES.forall(i => {
      val decisions = dispositionDecisions(state, i)
      snapshotsOf(state, i).indices().forall(k =>
        isBaselineSnapshot(i, k)
          or decisions.indices().exists(m => decisions.nth(m).snapshot == k))
    })

  // 判断は過去の同内容 snapshot へ巻き戻らず、直前の決定済み snapshot だけを再利用する。
  val invDecisionSnapshotIndexesMonotonic: bool =
    ENTITIES.forall(i => {
      val decisions = dispositionDecisions(state, i).select(r => r.snapshot != NO_SNAPSHOT)
      decisions.indices().forall(a =>
        decisions.indices().forall(b =>
          (a < b) implies decisions.nth(a).snapshot <= decisions.nth(b).snapshot))
    })

  // 連続する決定済み snapshot は必ず差分を持つ (同一なら同じ snapshot を再利用するため)。
  val invConsecutiveSnapshotsDiffer: bool =
    ENTITIES.forall(i => {
      val snaps = snapshotsOf(state, i)
      snaps.indices().forall(k =>
        (k + 1 < snaps.length())
          implies declarationDiff(snaps.nth(k), snaps.nth(k + 1)).size() > 0)
    })

  // ---- 記録は追記のみ ----

  // 一度保存した記録は、通常操作で書き換わらず前方一致のまま伸びる。
  val invRecordsAppendOnly: bool = and {
    appendOnly(before.decisionHistory, state.decisionHistory),
    appendOnly(before.progressHistory, state.progressHistory),
    appendOnly(before.supplemental, state.supplemental),
    ENTITIES.forall(i => appendOnly(snapshotsOf(before, i), snapshotsOf(state, i))),
  }

  // ---- 記録の意味を混ぜない ----

  val invDeclarationOpScope: bool =
    isDeclarationOp(operation) implies and {
      controlsUnchanged(before, state),
      state.snapshots == before.snapshots,
      state.currentSnapshot == before.currentSnapshot,
      state.decisionHistory == before.decisionHistory,
      state.progressHistory == before.progressHistory,
      state.supplemental == before.supplemental,
    }

  // 採否は進行を変えず、判断履歴だけに残る。
  val invDecideOpScope: bool =
    (operation == DecideOp) implies and {
      declarationsUnchanged(before, state),
      progressesUnchanged(before, state),
      resurfacesUnchanged(before, state),
      ENTITIES.forall(i =>
        (i != target) implies currentSnapshotOf(state, i) == currentSnapshotOf(before, i)),
      state.progressHistory == before.progressHistory,
      state.supplemental == before.supplemental,
    }

  val invSetWhenOpScope: bool =
    (operation == SetWhenOp) implies and {
      declarationsUnchanged(before, state),
      progressesUnchanged(before, state),
      dispositionsUnchanged(before, state),
      state.snapshots == before.snapshots,
      state.currentSnapshot == before.currentSnapshot,
      state.progressHistory == before.progressHistory,
      state.supplemental == before.supplemental,
    }

  // 進行は採否を変えず、進行履歴だけに残る。
  val invProgressOpScope: bool =
    isProgressOp(operation) implies and {
      declarationsUnchanged(before, state),
      dispositionsUnchanged(before, state),
      resurfacesUnchanged(before, state),
      state.snapshots == before.snapshots,
      state.currentSnapshot == before.currentSnapshot,
      state.decisionHistory == before.decisionHistory,
      state.supplemental == before.supplemental,
    }

  // supplemental の追加は declaration、Control state、関係、既存履歴を変えない。
  val invSupplementalOpScope: bool =
    (operation == AddSupplementalOp) implies and {
      state.entities == before.entities,
      state.snapshots == before.snapshots,
      state.currentSnapshot == before.currentSnapshot,
      state.decisionHistory == before.decisionHistory,
      state.progressHistory == before.progressHistory,
    }

  // ---- Supplemental information ----

  // Progress、Disposition、Resurface condition によって追加を制限しない。
  val invSupplementalNeverRestricted: bool =
    ENTITIES.forall(i => canAddSupplemental(state, i))

  // 各記録は対象内で安定して参照できる識別子を持つ。
  val invSupplementalRefUniquePerTarget: bool =
    ENTITIES.forall(i => {
      val records = supplementalOf(state, i)
      records.indices().forall(a =>
        records.indices().forall(b =>
          (a != b) implies records.nth(a).ref != records.nth(b).ref))
    })

  val invSupplementalTargetsExist: bool =
    state.supplemental.indices().forall(k =>
      ENTITIES.contains(state.supplemental.nth(k).target))

  // 保存済みの記録は省略されず、必ずどれかの Entity の観測結果に現れる。
  val invSupplementalFullyObservable: bool =
    state.supplemental.length() ==
      ENTITIES.fold(0, (acc, i) => acc + supplementalOf(state, i).length())

  // ---- Reachability witnesses ----

  val wSetTitle: bool = operation == SetTitleOp
  val wSetDescription: bool = operation == SetDescriptionOp
  val wSetParent: bool = operation == SetParentOp
  val wUnsetParent: bool = operation == UnsetParentOp
  val wAddDependency: bool = operation == AddDependencyOp
  val wRemoveDependency: bool = operation == RemoveDependencyOp
  val wDecide: bool = operation == DecideOp
  val wSetWhen: bool = operation == SetWhenOp
  val wStart: bool = operation == StartOp
  val wDone: bool = operation == DoneOp
  val wRelease: bool = operation == ReleaseOp
  val wAddSupplemental: bool = operation == AddSupplementalOp

  // 決定済み Entity への declaration 操作は、現在値と同じ値の再指定なら成功する。
  val wFrozenNoOpAccepted: bool = and {
    isDeclarationOp(operation),
    isDecided(targetDisposition(before, target)),
    targetDeclaration(state, target) == targetDeclaration(before, target),
  }

  // 固定は Progress と直交する: Ended でも Undecided なら declaration を編集できる。
  val wEndedUndecidedDeclarationEdited: bool = and {
    isDeclarationOp(operation),
    targetProgress(state, target) == Ended,
    targetDisposition(before, target) == Undecided,
    targetDeclaration(state, target) != targetDeclaration(before, target),
  }

  // 逆側: NotStarted でも決定済みなら固定される。
  val wNotStartedAndFrozen: bool =
    ENTITIES.exists(i => and {
      i != BASELINE_ENTITY,
      control(state, i).progress == NotStarted,
      isDecided(control(state, i).disposition),
      not(declarationMutable(state, i)),
    })

  // 固定中の Group が子を得る (変わるのは子側の declaration)。
  val wFrozenGroupGainsChild: bool =
    GROUPS.exists(group => and {
      isDecided(control(state, group).disposition),
      children(before, group).subseteq(children(state, group)),
      children(before, group) != children(state, group),
    })

  // 固定中の Entity が incoming dependency を得る (変わるのは依存元の declaration)。
  val wFrozenTargetGainsIncomingDependency: bool =
    ENTITIES.exists(i => and {
      isDecided(control(state, i).disposition),
      incoming(before, i).subseteq(incoming(state, i)),
      incoming(before, i) != incoming(state, i),
    })

  // 採否を戻して編集し、改めて判断すると別 snapshot が残り、差分を観測できる。
  val wRedecidedWithNewSnapshot: bool =
    ENTITIES.exists(i => and {
      isDecided(control(state, i).disposition),
      snapshotsOf(state, i).length() >= 2,
      latestSnapshotDiff(state, i).size() > 0,
    })

  // declaration が変わらず採否だけが変わった場合は、同じ snapshot に別の判断が結び付く。
  val wSameSnapshotTwoDecisions: bool =
    ENTITIES.exists(i => {
      val decisions = dispositionDecisions(state, i).select(r => r.snapshot != NO_SNAPSHOT)
      decisions.indices().exists(a =>
        decisions.indices().exists(b => and {
          a != b,
          decisions.nth(a).snapshot == decisions.nth(b).snapshot,
        }))
    })

  val wUndecideClearsCurrentSnapshot: bool = and {
    operation == DecideOp,
    isDecided(targetDisposition(before, target)),
    targetDisposition(state, target) == Undecided,
    currentSnapshotOf(before, target) != NO_SNAPSHOT,
    currentSnapshotOf(state, target) == NO_SNAPSHOT,
    snapshotsOf(state, target) == snapshotsOf(before, target),
  }

  // A -> B -> A は過去の A へ戻らず、同内容の新しい snapshot を末尾に作る。
  val wHistoricalSnapshotReappearsAsNew: bool =
    ENTITIES.exists(i => {
      val snaps = snapshotsOf(state, i)
      snaps.indices().exists(a =>
        snaps.indices().exists(b =>
          snaps.indices().exists(c => and {
            a < b,
            b < c,
            snaps.nth(a) == snaps.nth(c),
            snaps.nth(a) != snaps.nth(b),
          })))
    })

  // migration 直後は baseline snapshot と現在参照だけがあり、判断履歴を捏造しない。
  val wBaselineWithoutDecisionHistory: bool = and {
    control(state, BASELINE_ENTITY).disposition == Accepted,
    currentSnapshotOf(state, BASELINE_ENTITY) == 0,
    snapshotAt(state, BASELINE_ENTITY, 0) == decl(state, BASELINE_ENTITY),
    decisionRecordsOf(state, BASELINE_ENTITY).length() == 0,
  }

  // 決定済みかつ完了済みの Entity にも supplemental を追加できる。
  val wSupplementalOnDecidedAndEnded: bool =
    ENTITIES.exists(i => and {
      isDecided(control(state, i).disposition),
      control(state, i).progress == Ended,
      supplementalOf(state, i).length() > 0,
    })

  // 繰り返し実行は同じ記録への no-op ではなく、新しい記録の追加になる。
  val wRepeatedSupplementalCreatesNewRecord: bool =
    ENTITIES.exists(i => {
      val records = supplementalOf(state, i)
      records.indices().exists(a =>
        records.indices().exists(b => and {
          a != b,
          records.nth(a).body == records.nth(b).body,
          records.nth(a).actor == records.nth(b).actor,
          records.nth(a).ref != records.nth(b).ref,
        }))
    })

  // 保存順と入力時刻順は一致しないことがある。入力時刻から順序を推測しない根拠。
  val wStorageOrderDiffersFromInputTime: bool =
    state.supplemental.indices().exists(a =>
      state.supplemental.indices().exists(b => and {
        a < b,
        state.supplemental.nth(a).inputTime > state.supplemental.nth(b).inputTime,
      }))

  // 1 つの Entity に 4 種類の記録が同時に付いても、意味は混ざらない。
  val wAllRecordKindsPresent: bool =
    ENTITIES.exists(i => {
      val counts = recordCounts(state, i)
      and {
        counts.decisions > 0,
        counts.progressions > 0,
        counts.snapshots > 0,
        counts.supplemental > 0,
      }
    })
}
