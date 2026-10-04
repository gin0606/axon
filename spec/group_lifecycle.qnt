// Issue と Group の包含、所属変更、新規登録、種類の変換、dependency、
// 判断候補・着手候補・`axon tasks` の一覧の状況の導出を扱う統合モデル。
// Issue と Group は一つの ID 空間を共有し、各 Entity は種類（kind）を現在値として持つ。
// Group は保存される lifecycle（stored）として InProgress を持たず、実効 lifecycle を子から導出する。
// 固定 ID 5件の範囲で探索する。未登録の枠は探索領域を有限にする仕組みであって、新しい lifecycle ではない。
// 対象外: 文面・Note・状態変更履歴（lifecycle_information）、条件の種類・設定と評価失敗（candidate_evaluation）、CLI と表示、ID 発行、永続化。
// 基本遷移は lifecycle_rules を import し、到達しにくい経路は lifecycle_reachability が補う。
// 対応する契約: ../docs/reference/lifecycle.md
module group_lifecycle {
  import lifecycle_rules.* from "lifecycle_rules"

  // 所属は最大一つで、所属なしも許す。所属先は種類が Group の登録済み Entity に限る。
  type Id = int
  type Parent = Unassigned | InGroup(Id)
  type EntityRecord = { kind: Kind, stored: Lifecycle, parent: Parent, deps: Set[Id] }
  pure val IDS = Set(0, 1, 2, 3, 4)
  pure val PARENTS = Set(Unassigned).union(IDS.map(id => InGroup(id)))
  pure val KINDS = Set(Issue, Group)
  pure val REGISTRATION_LIFECYCLES = Set(Undecided, NotStarted)

  // 保存する状態は登録済みの枠と、各 Entity の種類・stored lifecycle・所属・dependency。
  // conditions は各 Entity の再浮上条件の成立・不成立を表す外部入力で、条件の種類や設定操作は抽象化する。
  type EntitySetState = { registered: Set[Id], entities: Id -> EntityRecord }
  type Conditions = Id -> bool
  type ConditionResult = NotEvaluated | Evaluated(bool)

  // observation は直前の状態と操作の検証専用の記録であり、製品の保存項目ではない。
  // reviewPassed は Group の最終確認が通ったという抽象入力。
  // history も検証専用で、Issue として Reopen した ID と、Issue から Group へ変換した ID を累積する。
  type EntitySetEvent = Initial
    | ConditionChanged(Id)
    | LifecycleOperation({ id: Id, kind: Kind, op: Operation })
    | Move({ id: Id, parent: Parent })
    | Register({ id: Id, kind: Kind, lifecycle: Lifecycle, parent: Parent })
    | ConvertedToGroup(Id)
    | ConvertedToIssue(Id)
    | AddDependency({ source: Id, target: Id })
    | RemoveDependency({ source: Id, target: Id })
  type EntitySetObservation = { before: EntitySetState, beforeConditions: Conditions, event: EntitySetEvent, reviewPassed: bool }
  type History = { reopenedIssues: Set[Id], convertedToGroup: Set[Id] }
  var entitySet: EntitySetState
  var conditions: Conditions
  var observation: EntitySetObservation
  var history: History
  pure def terminal(current: Lifecycle): bool = Set(Completed, Cancelled).contains(current)

  // 記録の読み出し。
  pure def kindOf(s: EntitySetState, id: Id): Kind = s.entities.get(id).kind
  pure def storedOf(s: EntitySetState, id: Id): Lifecycle = s.entities.get(id).stored
  pure def parentOf(s: EntitySetState, id: Id): Parent = s.entities.get(id).parent
  pure def depsOf(s: EntitySetState, id: Id): Set[Id] = s.entities.get(id).deps
  pure def groups(s: EntitySetState): Set[Id] = s.registered.filter(id => kindOf(s, id) == Group)
  pure def issues(s: EntitySetState): Set[Id] = s.registered.filter(id => kindOf(s, id) == Issue)

  // 包含の導出。所属から直属の子、祖先、子孫をたどる。
  // ancestors は所属先そのものを含み、所属先の祖先を Entity 数を上限にたどる。
  pure def children(s: EntitySetState, group: Id): Set[Id] =
    s.registered.filter(id => parentOf(s, id) == InGroup(group))
  pure def parentSet(parent: Parent): Set[Id] = match parent {
    | Unassigned => Set()
    | InGroup(id) => Set(id)
  }
  pure def ancestors(s: EntitySetState, parent: Parent): Set[Id] =
    IDS.fold(parentSet(parent), (seen, _) =>
      seen.union(seen.map(id => parentSet(parentOf(s, id))).flatten()))
  pure def ancestorsOf(s: EntitySetState, id: Id): Set[Id] = ancestors(s, parentOf(s, id))
  pure def descendants(s: EntitySetState, group: Id): Set[Id] =
    s.registered.filter(id => ancestorsOf(s, id).contains(group))
  pure def allChildrenTerminal(s: EntitySetState, group: Id): bool =
    children(s, group).forall(id => terminal(storedOf(s, id)))

  // Group の実効 lifecycle。stored が NotStarted で、直属の子のうち実効 lifecycle が
  // InProgress か Completed のものがあれば InProgress、それ以外は stored のまま。
  // 子 Group の実効値は再帰的に決まるため、進行中の Group の集合を空から Entity 数だけ広げて求める。
  // Issue と、stored が NotStarted 以外の Group は stored がそのまま実効値になる。
  pure def workingGroups(s: EntitySetState): Set[Id] =
    IDS.fold(Set(), (active, _) => groups(s).filter(g => storedOf(s, g) == NotStarted
      and children(s, g).exists(c => Set(InProgress, Completed).contains(storedOf(s, c)) or active.contains(c))))
  pure def effective(s: EntitySetState, id: Id): Lifecycle =
    if (workingGroups(s).contains(id)) InProgress else storedOf(s, id)

  // 親が終了していないかの判定。所属なしは通る。
  pure def parentOpen(s: EntitySetState, parent: Parent): bool = match parent {
    | Unassigned => true
    | InGroup(g) => not(terminal(storedOf(s, g)))
  }
  // 所属先として選べるか。所属なし、または登録済みで終了していない Group。
  pure def destinationOpen(s: EntitySetState, parent: Parent): bool = match parent {
    | Unassigned => true
    | InGroup(g) => s.registered.contains(g) and kindOf(s, g) == Group and not(terminal(storedOf(s, g)))
  }
  // 所属先とその全祖先が採用済みで未終了（stored が NotStarted）か。所属なしは通る。
  pure def destinationAdopted(s: EntitySetState, parent: Parent): bool =
    ancestors(s, parent).forall(a => storedOf(s, a) == NotStarted)

  pure def depsComplete(s: EntitySetState, id: Id): bool =
    depsOf(s, id).forall(target => storedOf(s, target) == Completed)
  pure def ancestorDepsComplete(s: EntitySetState, id: Id): bool =
    ancestorsOf(s, id).forall(a => depsComplete(s, a))
  pure def ancestorsAdopted(s: EntitySetState, id: Id): bool = destinationAdopted(s, parentOf(s, id))
  pure def hasStartedDescendant(s: EntitySetState, id: Id): bool =
    descendants(s, id).exists(d => Set(InProgress, Completed).contains(storedOf(s, d)))
  pure def completedDependents(s: EntitySetState, id: Id): Set[Id] =
    s.registered.filter(e => storedOf(s, e) == Completed and depsOf(s, e).contains(id))

  // 構造の循環検査には「全員が通常どおり完了する経路」を使う。各 Entity の完了の前提を
  // 「直属の子 ∪ 自身の依存先 ∪ 全祖先の依存先」とし、前提の残っていない節点を順に除去して
  // 全節点を除去できるかを検査する。取りやめによる回避はこの経路に加えない。
  pure def completionPredecessors(s: EntitySetState, id: Id): Set[Id] =
    children(s, id).union(depsOf(s, id)).union(ancestorsOf(s, id).map(a => depsOf(s, a)).flatten())
  pure def preconditionsAcyclic(s: EntitySetState): bool = {
    val nodes = s.registered
    val predecessors = nodes.mapBy(id => completionPredecessors(s, id))
    nodes.fold(nodes, (remaining, _) => remaining.filter(id =>
      predecessors.get(id).intersect(remaining).size() > 0)).size() == 0
  }

  // lifecycle 操作の前提。基本遷移（種類ごと）と、親が終了していないことに加えて、操作ごとに次を求める。
  // Start（Issue のみ）: 全祖先の stored が NotStarted、自身と全祖先の依存先がすべて完了。親の進行中は求めない。
  // Complete: 自身の依存先がすべて完了し、直属の子がすべて終了し、全祖先の stored が NotStarted であること
  // （完了した仕事は採用済みの計画の下にしか置かない）。Group は最終確認の通過も求める。
  // Cancel: 直属の子がすべて終了。完了済みの子を持つ Group の取りやめは許す。
  // Withdraw: 実効 lifecycle が NotStarted。実効は NotStarted の子 Group を通じて上へ伝わるので、
  // 着手・完了した子孫が NotStarted の Group の連なりの下にある限り採用を取り消せない。
  // 取りやめた子 Group の下の完了済みの子孫は止めない（Cancel→Reconsider で同じ状態に到達できるため）。
  // Accept: Group の子孫に着手・完了した Entity があるときだけ、全祖先の stored が NotStarted であること。
  // Issue の Accept と、そうでない Group の Accept は祖先を問わない（未判断の Group の下で子を先に採用できる）。
  // Reopen: 全祖先の stored が NotStarted で、自身を依存先に持つ完了済みの Entity がないこと。子の lifecycle は変えない。
  pure def canOperate(s: EntitySetState, id: Id, op: Operation, passed: bool): bool = and {
    s.registered.contains(id),
    canPerformAs(kindOf(s, id), storedOf(s, id), op),
    parentOpen(s, parentOf(s, id)),
    match op {
      | Start => ancestorsAdopted(s, id) and depsComplete(s, id) and ancestorDepsComplete(s, id)
      | Complete => depsComplete(s, id) and allChildrenTerminal(s, id) and ancestorsAdopted(s, id)
          and (kindOf(s, id) == Issue or passed)
      | Cancel => allChildrenTerminal(s, id)
      | Withdraw => effective(s, id) == NotStarted
      | Accept => (kindOf(s, id) == Group and hasStartedDescendant(s, id)) implies ancestorsAdopted(s, id)
      | Reopen => ancestorsAdopted(s, id) and completedDependents(s, id).size() == 0
      | _ => true
    },
  }
  pure def applyLifecycle(s: EntitySetState, id: Id, op: Operation): EntitySetState = {
    ...s,
    entities: s.entities.set(id, { ...s.entities.get(id), stored: applyOperation(storedOf(s, id), op) }),
  }

  pure def observe(s: EntitySetState, c: Conditions, event: EntitySetEvent, passed: bool): EntitySetObservation =
    { before: s, beforeConditions: c, event: event, reviewPassed: passed }

  // 0 は Group、1 と 2 は Issue で 0 の子、3 は所属なしの Group、4 は未登録の枠。
  // 全 Entity は Undecided、条件入力はすべて不成立から始める。
  // これは探索の開始点であって、製品の再浮上条件の初期値を決めるものではない。
  // 未登録の枠の記録は使われない値で、登録時に上書きする。candidate_evaluation も同じ初期状態から始める。
  pure val initialEntitySet: EntitySetState = {
    registered: Set(0, 1, 2, 3),
    entities: IDS.mapBy(id => {
      kind: if (id == 0 or id == 3) Group else Issue,
      stored: Undecided,
      parent: if (id == 1 or id == 2) InGroup(0) else Unassigned,
      deps: Set(),
    }),
  }
  pure val initialConditions: Conditions = IDS.mapBy(_ => false)
  action init = all {
    entitySet' = initialEntitySet,
    conditions' = initialConditions,
    observation' = observe(initialEntitySet, initialConditions, Initial, false),
    history' = { reopenedIssues: Set(), convertedToGroup: Set() },
  }

  // 各操作は前提を満たすときだけ実行し、他の Entity を自動で更新しない。
  // Group の実効 lifecycle は保存せず、子の変化に応じて導出が変わるだけである。
  action performGroup(id: Id, op: Operation, passed: bool): bool = all {
    entitySet.registered.contains(id),
    kindOf(entitySet, id) == Group,
    canOperate(entitySet, id, op, passed),
    entitySet' = applyLifecycle(entitySet, id, op),
    conditions' = conditions,
    observation' = observe(entitySet, conditions, LifecycleOperation({ id: id, kind: Group, op: op }), passed),
    history' = history,
  }
  action performIssue(id: Id, op: Operation): bool = all {
    entitySet.registered.contains(id),
    kindOf(entitySet, id) == Issue,
    canOperate(entitySet, id, op, false),
    entitySet' = applyLifecycle(entitySet, id, op),
    conditions' = conditions,
    observation' = observe(entitySet, conditions, LifecycleOperation({ id: id, kind: Issue, op: op }), false),
    history' = if (op == Reopen) { ...history, reopenedIssues: history.reopenedIssues.union(Set(id)) } else history,
  }

  // 所属変更。移動元・移動先の親がどちらも終了しておらず、移動先は Group に限る。
  // 着手済み・完了済みの Entity（Issue の stored、Group の実効が InProgress か Completed）を移すときは、
  // 移動先の Group 自身を含む全祖先の stored が NotStarted であること。
  // 自己包含と子孫の下への移動、変更後の通常完了経路が循環する移動は拒否する。
  // 移動は lifecycle を変えず、Group は配下ごと部分木全体が移る。
  pure def applyMove(s: EntitySetState, id: Id, parent: Parent): EntitySetState =
    { ...s, entities: s.entities.set(id, { ...s.entities.get(id), parent: parent }) }
  pure def canMove(s: EntitySetState, id: Id, parent: Parent): bool = and {
    s.registered.contains(id),
    parentOf(s, id) != parent,
    parentOpen(s, parentOf(s, id)),
    destinationOpen(s, parent),
    not(Set(InProgress, Completed).contains(effective(s, id))) or destinationAdopted(s, parent),
    not(ancestors(s, parent).contains(id)),
    preconditionsAcyclic(applyMove(s, id, parent)),
  }
  action move(id: Id, parent: Parent): bool = all {
    canMove(entitySet, id, parent),
    entitySet' = applyMove(entitySet, id, parent),
    conditions' = conditions,
    observation' = observe(entitySet, conditions, Move({ id: id, parent: parent }), false),
    history' = history,
  }

  // 未登録の枠を Issue か Group として、Undecided か NotStarted で、所属先を指定して登録する。
  // 登録は与えられた採否を反映し、採用判断は代行せず、登録だけでは着手しない。
  pure def canRegister(s: EntitySetState, id: Id, lifecycle: Lifecycle, parent: Parent): bool = and {
    not(s.registered.contains(id)),
    REGISTRATION_LIFECYCLES.contains(lifecycle),
    destinationOpen(s, parent),
  }
  pure def applyRegister(s: EntitySetState, id: Id, kind: Kind, lifecycle: Lifecycle, parent: Parent): EntitySetState = {
    registered: s.registered.union(Set(id)),
    entities: s.entities.set(id, { kind: kind, stored: lifecycle, parent: parent, deps: Set() }),
  }
  action register(id: Id, kind: Kind, lifecycle: Lifecycle, parent: Parent): bool = all {
    canRegister(entitySet, id, lifecycle, parent),
    entitySet' = applyRegister(entitySet, id, kind, lifecycle, parent),
    conditions' = conditions,
    observation' = observe(entitySet, conditions,
      Register({ id: id, kind: kind, lifecycle: lifecycle, parent: parent }), false),
    history' = history,
  }

  // 種類の変換。lifecycle・所属・dependency を変えない。
  // Issue から Group へは stored が Undecided か NotStarted のとき（InProgress は先に Release が要る）。
  // Group から Issue へは stored が Undecided か NotStarted で、子がないとき。
  pure def canConvertToGroup(s: EntitySetState, id: Id): bool = and {
    s.registered.contains(id),
    kindOf(s, id) == Issue,
    REGISTRATION_LIFECYCLES.contains(storedOf(s, id)),
  }
  pure def canConvertToIssue(s: EntitySetState, id: Id): bool = and {
    s.registered.contains(id),
    kindOf(s, id) == Group,
    REGISTRATION_LIFECYCLES.contains(storedOf(s, id)),
    children(s, id).size() == 0,
  }
  pure def applyConvert(s: EntitySetState, id: Id, kind: Kind): EntitySetState =
    { ...s, entities: s.entities.set(id, { ...s.entities.get(id), kind: kind }) }
  action convertToGroup(id: Id): bool = all {
    canConvertToGroup(entitySet, id),
    entitySet' = applyConvert(entitySet, id, Group),
    conditions' = conditions,
    observation' = observe(entitySet, conditions, ConvertedToGroup(id), false),
    history' = { ...history, convertedToGroup: history.convertedToGroup.union(Set(id)) },
  }
  action convertToIssue(id: Id): bool = all {
    canConvertToIssue(entitySet, id),
    entitySet' = applyConvert(entitySet, id, Issue),
    conditions' = conditions,
    observation' = observe(entitySet, conditions, ConvertedToIssue(id), false),
    history' = history,
  }

  // dependency の編集。依存元が完了していればその dependency は固定し、Reopen 後は編集できる。
  // 自己依存と、包含を合わせた通常完了経路の循環は追加時に拒否する。祖先・子孫間の依存はこの循環に含まれる。
  pure def withDeps(s: EntitySetState, id: Id, deps: Set[Id]): EntitySetState =
    { ...s, entities: s.entities.set(id, { ...s.entities.get(id), deps: deps }) }
  pure def canAddDependency(s: EntitySetState, source: Id, target: Id): bool = and {
    s.registered.contains(source),
    s.registered.contains(target),
    storedOf(s, source) != Completed,
    source != target,
    not(depsOf(s, source).contains(target)),
    preconditionsAcyclic(withDeps(s, source, depsOf(s, source).union(Set(target)))),
  }
  action addDependency(source: Id, target: Id): bool = all {
    canAddDependency(entitySet, source, target),
    entitySet' = withDeps(entitySet, source, depsOf(entitySet, source).union(Set(target))),
    conditions' = conditions,
    observation' = observe(entitySet, conditions, AddDependency({ source: source, target: target }), false),
    history' = history,
  }
  pure def canRemoveDependency(s: EntitySetState, source: Id, target: Id): bool = and {
    s.registered.contains(source),
    storedOf(s, source) != Completed,
    depsOf(s, source).contains(target),
  }
  action removeDependency(source: Id, target: Id): bool = all {
    canRemoveDependency(entitySet, source, target),
    entitySet' = withDeps(entitySet, source, depsOf(entitySet, source).exclude(Set(target))),
    conditions' = conditions,
    observation' = observe(entitySet, conditions, RemoveDependency({ source: source, target: target }), false),
    history' = history,
  }

  // 条件入力は登録済みの Entity について両方向へ変化し、保存する状態を変えない。
  pure def canChangeCondition(s: EntitySetState, c: Conditions, id: Id, satisfied: bool): bool =
    s.registered.contains(id) and c.get(id) != satisfied
  action changeCondition(id: Id, satisfied: bool): bool = all {
    canChangeCondition(entitySet, conditions, id, satisfied),
    entitySet' = entitySet,
    conditions' = conditions.set(id, satisfied),
    observation' = observe(entitySet, conditions, ConditionChanged(id), false),
    history' = history,
  }

  // Group の lifecycle、Issue の lifecycle、それ以外の操作の三枝に分けて探索する。
  action step = {
    nondet id = IDS.oneOf()
    nondet target = IDS.oneOf()
    nondet op = OPERATIONS.oneOf()
    nondet passed = Set(false, true).oneOf()
    nondet parent = PARENTS.oneOf()
    nondet kind = KINDS.oneOf()
    nondet lifecycle = REGISTRATION_LIFECYCLES.oneOf()
    any {
      performGroup(id, op, passed),
      performIssue(id, op),
      any {
        move(id, parent),
        register(id, kind, lifecycle, parent),
        convertToGroup(id),
        convertToIssue(id),
        addDependency(id, target),
        removeDependency(id, target),
        changeCondition(id, passed),
      },
    }
  }

  // 再浮上条件の評価。取りやめ・完了の Entity と未登録の枠は評価せず、浮上しない。
  // 評価結果は保存せず、入力と状態から導出する。
  pure def conditionResult(s: EntitySetState, c: Conditions, id: Id): ConditionResult =
    if (not(s.registered.contains(id)) or terminal(storedOf(s, id))) NotEvaluated else Evaluated(c.get(id))
  pure def ancestorsSurfaced(s: EntitySetState, c: Conditions, id: Id): bool =
    ancestorsOf(s, id).forall(a => conditionResult(s, c, a) == Evaluated(true))
  pure def surfaced(s: EntitySetState, c: Conditions, id: Id): bool =
    conditionResult(s, c, id) == Evaluated(true) and ancestorsSurfaced(s, c, id)

  // 判断候補（axon proposals）は未判断で、自身とすべての祖先が浮上しているもの。
  // 着手可能は Start の前提を満たす Issue、着手候補はそのうち自身と全祖先が浮上しているもの。
  // Group は Start を持たないため着手候補にならない。
  pure def judgments(s: EntitySetState, c: Conditions): Set[Id] =
    s.registered.filter(id => storedOf(s, id) == Undecided and surfaced(s, c, id))
  pure def startableIssues(s: EntitySetState): Set[Id] = issues(s).filter(id => canOperate(s, id, Start, false))
  pure def candidateIssues(s: EntitySetState, c: Conditions): Set[Id] =
    startableIssues(s).filter(id => surfaced(s, c, id))
  pure def workingIssues(s: EntitySetState): Set[Id] = issues(s).filter(id => storedOf(s, id) == InProgress)

  // axon tasks の一覧の行と状況。一覧は平らで、Issue と Group の行を含む。
  // Working は表示上の InProgress（lifecycle の InProgress と名前が衝突するため別名にする）。
  type TaskStatus = Confirmable | Ready | Working | Blocked | Empty
  // Issue の行は、未着手で自身と全祖先が浮上しているものと、浮上に関係なく進行中のもの。
  pure def issueRows(s: EntitySetState, c: Conditions): Set[Id] =
    issues(s).filter(id => (storedOf(s, id) == NotStarted and surfaced(s, c, id)) or storedOf(s, id) == InProgress)
  pure def issueStatus(s: EntitySetState, c: Conditions, id: Id): TaskStatus =
    if (candidateIssues(s, c).contains(id)) Ready
    else if (storedOf(s, id) == InProgress) Working
    else Blocked
  // Group の行は、stored が NotStarted で自身と全祖先が浮上しているものと、浮上に関係なく実効が InProgress のもの。
  pure def groupRows(s: EntitySetState, c: Conditions): Set[Id] =
    groups(s).filter(g => (storedOf(s, g) == NotStarted and surfaced(s, c, g)) or effective(s, g) == InProgress)
  pure def hasCandidateDescendant(s: EntitySetState, c: Conditions, g: Id): bool =
    descendants(s, g).exists(id => candidateIssues(s, c).contains(id))
  // 優先順位: Empty（直属の子がない。完了できても計画を書くのが次の一手）> Confirmable（最終確認が通れば完了できる）
  // > Ready（着手候補の子孫がある）> Working（実効が InProgress）> Blocked（子があり上のどれでもない）。
  pure def groupStatus(s: EntitySetState, c: Conditions, g: Id): TaskStatus =
    if (children(s, g).size() == 0) Empty
    else if (canOperate(s, g, Complete, true)) Confirmable
    else if (hasCandidateDescendant(s, c, g)) Ready
    else if (effective(s, g) == InProgress) Working
    else Blocked

  // 詰まっている Group: 完了できず、着手候補の子孫も着手中（stored が InProgress の Issue）の子孫もない。
  // 状況が Blocked の行のほか、完了済みの子孫はあるが着手中の子孫がない Working の行と、完了できない Empty の行が該当する。
  // Confirmable の行と完了できる Empty の行は次の一手が状況から分かるため含めない。
  // 着手中の Issue に後から足した未完了の dependency は、その Issue 自身の行の問題で、Group を詰まらせない。
  pure def hasWorkingDescendant(s: EntitySetState, g: Id): bool =
    workingIssues(s).exists(d => descendants(s, g).contains(d))
  pure def stalled(s: EntitySetState, c: Conditions, g: Id): bool =
    not(canOperate(s, g, Complete, true)) and not(hasCandidateDescendant(s, c, g)) and not(hasWorkingDescendant(s, g))

  // 詰まっている理由の内訳。未完了の dependency は、自身・全祖先・未終了の子孫のいずれかの依存先が未完了であること。
  // 未判断の子、未終了の子 Group、浮上していない着手可能な子孫の Issue、stored が Undecided の祖先、
  // 浮上していない祖先（祖先のいずれかの条件が未成立）、自身の条件が未成立を区別する。
  // 自身の条件は全祖先が浮上しているときだけ評価される（axon show は未成立の祖先より下を評価しない）ため、
  // 浮上していない祖先がある Group には自身の条件が未成立という理由は付かない。
  // 理由は詰まっている行に示す。関数はどの Group にも定義できるが、詰まっていない行（Confirmable、Ready、
  // 完了できる Empty）は次の一手が状況から分かるため理由を示さない。
  // axon show は tasks の行でない stored が NotStarted の Group にも同じ導出を使うため、
  // 詰まっている Group には行に出るかに関係なく少なくとも一つ付く（invStalledHasReason）。
  // 浮上していない祖先と自身の条件が未成立は、tasks の行になる Group では実効 InProgress の行にだけ当たり
  // （浮上して行になる Group は自身と全祖先が浮上している）、行に出ない Group にも show が示す。
  type StallReason = DependencyPending | UndecidedChild | OpenSubgroup | HiddenCandidate | UnadoptedAncestor
    | UnsurfacedAncestor | OwnConditionUnsatisfied
  pure def stallReasons(s: EntitySetState, c: Conditions, g: Id): Set[StallReason] = {
    val dependency = not(depsComplete(s, g)) or not(ancestorDepsComplete(s, g))
      or descendants(s, g).exists(d => not(terminal(storedOf(s, d))) and not(depsComplete(s, d)))
    val undecided = children(s, g).exists(ch => storedOf(s, ch) == Undecided)
    val subgroup = children(s, g).exists(ch => kindOf(s, ch) == Group and not(terminal(storedOf(s, ch))))
    val hidden = descendants(s, g).exists(d => startableIssues(s).contains(d) and not(surfaced(s, c, d)))
    val unadopted = ancestorsOf(s, g).exists(a => storedOf(s, a) == Undecided)
    val unsurfacedAncestor = not(ancestorsSurfaced(s, c, g))
    val ownCondition = storedOf(s, g) == NotStarted and ancestorsSurfaced(s, c, g)
      and conditionResult(s, c, g) != Evaluated(true)
    Set(DependencyPending, UndecidedChild, OpenSubgroup, HiddenCandidate, UnadoptedAncestor, UnsurfacedAncestor,
        OwnConditionUnsatisfied)
      .filter(r => match r {
        | DependencyPending => dependency
        | UndecidedChild => undecided
        | OpenSubgroup => subgroup
        | HiddenCandidate => hidden
        | UnadoptedAncestor => unadopted
        | UnsurfacedAncestor => unsurfacedAncestor
        | OwnConditionUnsatisfied => ownCondition
      })
  }
  // axon show が対象にする Group: stored が NotStarted のもの（tasks の行を含む）。
  pure def shownGroups(s: EntitySetState): Set[Id] = groups(s).filter(g => storedOf(s, g) == NotStarted)

  // ---- lifecycle 操作 ----

  // witness: Issue の8操作と Group の6操作それぞれの到達。
  def saw(kind: Kind, op: Operation): bool = match observation.event {
    | LifecycleOperation(change) => change.kind == kind and change.op == op
    | _ => false
  }
  val wIssueAccept = saw(Issue, Accept)
  val wIssueWithdraw = saw(Issue, Withdraw)
  val wIssueStart = saw(Issue, Start)
  val wIssueRelease = saw(Issue, Release)
  val wIssueComplete = saw(Issue, Complete)
  val wIssueCancel = saw(Issue, Cancel)
  val wIssueReconsider = saw(Issue, Reconsider)
  val wIssueReopen = saw(Issue, Reopen)
  val wGroupAccept = saw(Group, Accept)
  val wGroupWithdraw = saw(Group, Withdraw)
  val wGroupComplete = saw(Group, Complete)
  val wGroupCancel = saw(Group, Cancel)
  val wGroupReconsider = saw(Group, Reconsider)
  val wGroupReopen = saw(Group, Reopen)

  // invariant: Group の stored は InProgress にならず、Group への Start・Release は起きない。
  // 実効 lifecycle は導出の不動点になっており、Group の実効が InProgress なら着手中か完了済みの子孫がいる。
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invGroupNeverStoredInProgress = and {
    groups(entitySet).forall(g => storedOf(entitySet, g) != InProgress),
    not(saw(Group, Start)),
    not(saw(Group, Release)),
  }
  val invEffectiveFixpoint = entitySet.registered.forall(id => effective(entitySet, id) ==
    (if (kindOf(entitySet, id) == Group and storedOf(entitySet, id) == NotStarted
        and children(entitySet, id).exists(c => Set(InProgress, Completed).contains(effective(entitySet, c))))
      InProgress else storedOf(entitySet, id)))
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invEffectiveHasStartedDescendant = groups(entitySet).forall(g => effective(entitySet, g) == InProgress implies
    descendants(entitySet, g).exists(d => Set(InProgress, Completed).contains(storedOf(entitySet, d))))

  // invariant: 着手中の Issue の全祖先の stored は NotStarted。実効が InProgress の Group も同様。
  val invWorkingAncestors = issues(entitySet).forall(id => storedOf(entitySet, id) == InProgress implies
    ancestorsOf(entitySet, id).forall(a => storedOf(entitySet, a) == NotStarted))
  val invEffectiveWorkingAncestors = groups(entitySet).forall(g => effective(entitySet, g) == InProgress implies
    ancestorsOf(entitySet, g).forall(a => storedOf(entitySet, a) == NotStarted))

  // invariant: 着手・完了の前提。Start は Issue だけで、全祖先の採用と自身・全祖先の依存先の完了を、
  // Complete は自身の依存先の完了と直属の子の終了を、Group では最終確認の通過も伴う。
  // Group の Completed への到達は最終確認を伴う Complete だけで、Issue の Completed も Complete だけで起きる。
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invStartRequirements = match observation.event {
    | LifecycleOperation(change) => change.op == Start implies and {
        change.kind == Issue,
        ancestorsAdopted(observation.before, change.id),
        depsComplete(observation.before, change.id),
        ancestorDepsComplete(observation.before, change.id),
      }
    | _ => true
  }
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invCompleteRequirements = match observation.event {
    | LifecycleOperation(change) => change.op == Complete implies and {
        depsComplete(observation.before, change.id),
        allChildrenTerminal(observation.before, change.id),
        ancestorsAdopted(observation.before, change.id),
        change.kind == Group implies (observation.reviewPassed and storedOf(observation.before, change.id) == NotStarted),
        change.kind == Issue implies storedOf(observation.before, change.id) == InProgress,
      }
    | _ => true
  }
  val invCompletionReviewed = entitySet.registered.forall(id =>
    (storedOf(entitySet, id) == Completed
      and not(observation.before.registered.contains(id) and storedOf(observation.before, id) == Completed))
    implies and {
      observation.event == LifecycleOperation({ id: id, kind: kindOf(entitySet, id), op: Complete }),
      kindOf(entitySet, id) == Group implies observation.reviewPassed,
    })

  // invariant: 完了から抜けるのは Reopen だけで、Reopen は完了から未着手へ戻し、他の Entity と
  // 自身の種類・所属・dependency を変えない。
  val invCompletedLeavesOnlyByReopen = observation.before.registered.forall(id =>
    (storedOf(observation.before, id) == Completed and storedOf(entitySet, id) != Completed) implies
      (observation.event == LifecycleOperation({ id: id, kind: kindOf(entitySet, id), op: Reopen })
        and storedOf(entitySet, id) == NotStarted))
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invReopenTransition = match observation.event {
    | LifecycleOperation(change) => change.op == Reopen implies and {
        storedOf(observation.before, change.id) == Completed,
        storedOf(entitySet, change.id) == NotStarted,
        completedDependents(observation.before, change.id).size() == 0,
        entitySet.registered == observation.before.registered,
        IDS.exclude(Set(change.id)).forall(id => entitySet.entities.get(id) == observation.before.entities.get(id)),
        entitySet.entities.get(change.id) == { ...observation.before.entities.get(change.id), stored: NotStarted },
      }
    | _ => true
  }

  // ---- 包含 ----

  // invariant: 所属は登録済みの Group を指し、未登録の枠は所属も dependency も持たない。Issue は子を持たない。
  // 包含は循環しない。終了した Group の直属の子と全子孫は終了しており、構成は操作の前後で変わらない。
  // 終了した Group への追加・取り外しはどれも許可されず、自己包含と子孫の下への移動は拒否される。
  val invMembershipValid = and {
    entitySet.registered.subseteq(IDS),
    entitySet.registered.forall(id => match parentOf(entitySet, id) {
      | Unassigned => true
      | InGroup(g) => entitySet.registered.contains(g) and kindOf(entitySet, g) == Group
    }),
  }
  val invIssueHasNoChildren = issues(entitySet).forall(id => children(entitySet, id).size() == 0)
  val invContainmentAcyclic = entitySet.registered.forall(id => not(ancestorsOf(entitySet, id).contains(id)))
  val invClosedChildrenTerminal = groups(entitySet).forall(g => terminal(storedOf(entitySet, g)) implies allChildrenTerminal(entitySet, g))
  val invClosedDescendantsTerminal = groups(entitySet).forall(g => terminal(storedOf(entitySet, g)) implies
    descendants(entitySet, g).forall(d => terminal(storedOf(entitySet, d))))
  val invClosedStructureFrozen = groups(observation.before).forall(g => terminal(storedOf(observation.before, g))
    implies (children(entitySet, g) == children(observation.before, g)
      and descendants(entitySet, g) == descendants(observation.before, g)))
  val invClosedEditsDisabled = groups(entitySet).forall(g => terminal(storedOf(entitySet, g)) implies and {
    IDS.forall(id => not(canMove(entitySet, id, InGroup(g)))
      and REGISTRATION_LIFECYCLES.forall(lc => not(canRegister(entitySet, id, lc, InGroup(g))))),
    descendants(entitySet, g).forall(d => PARENTS.forall(p => not(canMove(entitySet, d, p)))
      and OPERATIONS.forall(op => not(canOperate(entitySet, d, op, true)))
      and not(canConvertToGroup(entitySet, d)) and not(canConvertToIssue(entitySet, d))),
  })
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invContainmentCycleEditsDisabled = entitySet.registered.forall(id =>
    not(canMove(entitySet, id, InGroup(id))) and descendants(entitySet, id).forall(d => not(canMove(entitySet, id, InGroup(d)))))

  // invariant: 各操作は対象以外の Entity を変えない。移動は所属だけ、登録は枠一つだけ、
  // dependency の編集は依存元の dependency だけ、条件入力の変化は保存する状態を変えない。
  pure def othersUnchanged(s: EntitySetState, before: EntitySetState, id: Id): bool =
    s.registered == before.registered
      and IDS.exclude(Set(id)).forall(e => s.entities.get(e) == before.entities.get(e))
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invOperationScope = match observation.event {
    | Initial => entitySet == observation.before
    | ConditionChanged(_) => entitySet == observation.before
    | LifecycleOperation(change) => othersUnchanged(entitySet, observation.before, change.id)
        and entitySet.entities.get(change.id) == { ...observation.before.entities.get(change.id),
          stored: applyOperation(storedOf(observation.before, change.id), change.op) }
    | Move(change) => othersUnchanged(entitySet, observation.before, change.id)
        and entitySet.entities.get(change.id) == { ...observation.before.entities.get(change.id), parent: change.parent }
    | Register(change) => entitySet.registered == observation.before.registered.union(Set(change.id))
        and not(observation.before.registered.contains(change.id))
        and IDS.exclude(Set(change.id)).forall(e => entitySet.entities.get(e) == observation.before.entities.get(e))
        and entitySet.entities.get(change.id) ==
          { kind: change.kind, stored: change.lifecycle, parent: change.parent, deps: Set() }
    | ConvertedToGroup(id) => othersUnchanged(entitySet, observation.before, id)
    | ConvertedToIssue(id) => othersUnchanged(entitySet, observation.before, id)
    | AddDependency(change) => othersUnchanged(entitySet, observation.before, change.source)
        and entitySet.entities.get(change.source) == { ...observation.before.entities.get(change.source),
          deps: depsOf(observation.before, change.source).union(Set(change.target)) }
    | RemoveDependency(change) => othersUnchanged(entitySet, observation.before, change.source)
        and entitySet.entities.get(change.source) == { ...observation.before.entities.get(change.source),
          deps: depsOf(observation.before, change.source).exclude(Set(change.target)) }
  }

  // invariant: 変換は種類だけを変え、lifecycle・所属・dependency・他の Entity を変えない。
  // Issue への変換は子のない Group に限り、どちらの変換も進行中・終了した Entity には起きない。
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invConversionScope = match observation.event {
    | ConvertedToGroup(id) => and {
        othersUnchanged(entitySet, observation.before, id),
        kindOf(observation.before, id) == Issue,
        entitySet.entities.get(id) == { ...observation.before.entities.get(id), kind: Group },
        REGISTRATION_LIFECYCLES.contains(storedOf(entitySet, id)),
      }
    | ConvertedToIssue(id) => and {
        othersUnchanged(entitySet, observation.before, id),
        kindOf(observation.before, id) == Group,
        children(observation.before, id).size() == 0,
        entitySet.entities.get(id) == { ...observation.before.entities.get(id), kind: Issue },
        REGISTRATION_LIFECYCLES.contains(storedOf(entitySet, id)),
      }
    | _ => true
  }

  // invariant: 新規登録は与えられた採否に対応する初期状態になり、自動で着手しない。
  val invRegistrationMatchesDecision = match observation.event {
    | Register(change) => and {
        storedOf(entitySet, change.id) == change.lifecycle,
        REGISTRATION_LIFECYCLES.contains(change.lifecycle),
        effective(entitySet, change.id) != InProgress,
        destinationOpen(observation.before, change.parent),
      }
    | _ => true
  }

  // ---- dependency ----

  // invariant: dependency は登録済みの Entity だけを参照し、包含と合わせた通常完了経路が循環しない。
  // 完了した Entity の依存先はすべて完了しており（Reopen の前提がこれを守る）、
  // 完了した Entity 自身の dependency は固定される。自己依存は許可されず、祖先・子孫間の依存は持たない。
  val invDependenciesValid = IDS.forall(id =>
    (entitySet.registered.contains(id) implies depsOf(entitySet, id).subseteq(entitySet.registered))
      and (not(entitySet.registered.contains(id)) implies depsOf(entitySet, id).size() == 0))
  val invPreconditionsAcyclic = preconditionsAcyclic(entitySet)
  val invCompletedPrerequisites = entitySet.registered.forall(id =>
    storedOf(entitySet, id) == Completed implies depsComplete(entitySet, id))
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invCompletedDependenciesFrozen = observation.before.registered.forall(id =>
    storedOf(observation.before, id) == Completed implies depsOf(entitySet, id) == depsOf(observation.before, id))
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invSelfDependencyRejected = entitySet.registered.forall(id =>
    not(canAddDependency(entitySet, id, id)) and not(depsOf(entitySet, id).contains(id)))
  val invNoAncestorDependencies = entitySet.registered.forall(id => ancestorsOf(entitySet, id).forall(a =>
    not(depsOf(entitySet, id).contains(a)) and not(depsOf(entitySet, a).contains(id))
      and not(canAddDependency(entitySet, id, a)) and not(canAddDependency(entitySet, a, id))))

  // ---- 候補と一覧 ----

  // invariant: 条件入力の変化は対象の入力だけを変え、他の操作は条件入力を変えない。
  // 未登録・取りやめ・完了は条件を評価しない。
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invConditionInputScope = match observation.event {
    | ConditionChanged(changed) => IDS.exclude(Set(changed)).forall(id =>
        conditions.get(id) == observation.beforeConditions.get(id))
    | _ => conditions == observation.beforeConditions
  }
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invTerminalConditionsNotEvaluated = IDS.forall(id =>
    (not(entitySet.registered.contains(id)) or terminal(storedOf(entitySet, id))) implies conditionResult(entitySet, conditions, id) == NotEvaluated)

  // invariant: 判断候補・着手候補の定義。着手候補は着手可能な Issue に限り、Group は着手候補にならない。
  // 判断候補は着手候補とも着手中とも重ならず、条件入力の変化は着手可否と着手中の集合を変えない。
  val invCandidateDefinition = candidateIssues(entitySet, conditions) == entitySet.registered.filter(id => and {
    kindOf(entitySet, id) == Issue,
    storedOf(entitySet, id) == NotStarted,
    parentOpen(entitySet, parentOf(entitySet, id)),
    ancestorsOf(entitySet, id).forall(a => storedOf(entitySet, a) == NotStarted and depsComplete(entitySet, a) and conditions.get(a)),
    depsComplete(entitySet, id),
    conditions.get(id),
  })
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invGroupsNotCandidates = groups(entitySet).forall(g => not(canOperate(entitySet, g, Start, false)))
    and candidateIssues(entitySet, conditions).subseteq(startableIssues(entitySet))
  val invJudgmentDefinition = judgments(entitySet, conditions) == entitySet.registered.filter(id => and {
    storedOf(entitySet, id) == Undecided,
    conditions.get(id),
    ancestorsOf(entitySet, id).forall(a => not(terminal(storedOf(entitySet, a))) and conditions.get(a)),
  })
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invJudgmentSeparateFromWork = and {
    judgments(entitySet, conditions).intersect(candidateIssues(entitySet, conditions)).size() == 0,
    judgments(entitySet, conditions).intersect(workingIssues(entitySet)).size() == 0,
    judgments(entitySet, conditions).forall(id => effective(entitySet, id) != InProgress),
  }
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invDependencyEditsPreserveJudgment = match observation.event {
    | AddDependency(_) => judgments(entitySet, conditions) == judgments(observation.before, observation.beforeConditions)
    | RemoveDependency(_) => judgments(entitySet, conditions) == judgments(observation.before, observation.beforeConditions)
    | _ => true
  }
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invConditionChangesPreserveEligibility = match observation.event {
    | ConditionChanged(_) => startableIssues(entitySet) == startableIssues(observation.before)
        and workingIssues(entitySet) == workingIssues(observation.before)
        and workingGroups(entitySet) == workingGroups(observation.before)
    | _ => true
  }

  // invariant: 一覧の状況は定義どおりに一つへ決まる。Issue の行は Ready・Blocked・Working のどれかで、
  // Blocked は未着手で浮上しているが着手可能でないもの。Group の行の状況の導出は優先順位の定義と一致し、
  // Confirmable の Group はそのとき（最終確認が通れば）完了でき、Ready の Group には着手候補の子孫がある。
  // 構成の確認: issueStatus・groupStatus の定義を優先順位ごとに書き下したもので、定義の書き誤りを検出するために置く。
  val invIssueStatusDefinition = issueRows(entitySet, conditions).forall(id => and {
    issueStatus(entitySet, conditions, id) == Ready iff candidateIssues(entitySet, conditions).contains(id),
    issueStatus(entitySet, conditions, id) == Working iff storedOf(entitySet, id) == InProgress,
    issueStatus(entitySet, conditions, id) == Blocked iff (storedOf(entitySet, id) == NotStarted
      and surfaced(entitySet, conditions, id) and not(startableIssues(entitySet).contains(id))),
  })
  val invGroupStatusDefinition = groupRows(entitySet, conditions).forall(g => and {
    groupStatus(entitySet, conditions, g) == Empty iff children(entitySet, g).size() == 0,
    groupStatus(entitySet, conditions, g) == Confirmable iff (children(entitySet, g).size() > 0
      and canOperate(entitySet, g, Complete, true)),
    groupStatus(entitySet, conditions, g) == Ready iff (children(entitySet, g).size() > 0
      and not(canOperate(entitySet, g, Complete, true)) and hasCandidateDescendant(entitySet, conditions, g)),
    groupStatus(entitySet, conditions, g) == Working iff (children(entitySet, g).size() > 0
      and not(canOperate(entitySet, g, Complete, true)) and not(hasCandidateDescendant(entitySet, conditions, g))
      and effective(entitySet, g) == InProgress),
    groupStatus(entitySet, conditions, g) == Blocked iff (children(entitySet, g).size() > 0
      and not(canOperate(entitySet, g, Complete, true)) and not(hasCandidateDescendant(entitySet, conditions, g))
      and effective(entitySet, g) != InProgress),
  })
  // 詰まっている Group には少なくとも一つ理由がある。tasks の行に限らず、axon show が対象にする
  // stored が NotStarted の Group すべてについて成り立つ。Blocked の行はすべて詰まっており、
  // Working の行は着手中の子孫がないとき、Empty の行は完了できないときに詰まっている。
  val invStalledHasReason = shownGroups(entitySet).forall(g =>
    stalled(entitySet, conditions, g) implies stallReasons(entitySet, conditions, g).size() > 0)
  // 条件に由来する理由（浮上していない祖先、自身の条件が未成立）は条件入力が不成立ならほぼ常に付くため、
  // それらを除いても、条件に依らない 5 種類のいずれかが詰まっている Group に付くことを別に検査する。
  pure val CONDITION_REASONS = Set(UnsurfacedAncestor, OwnConditionUnsatisfied)
  val invStalledHasStructuralReason = shownGroups(entitySet).forall(g =>
    stalled(entitySet, conditions, g) implies stallReasons(entitySet, conditions, g).exclude(CONDITION_REASONS).size() > 0)
  // 構成の確認: 浮上していない祖先の理由は tasks の行には現れない。行になる Group は自身と全祖先が浮上しているか、
  // 実効 InProgress で祖先が採用済みだが、後者は祖先の条件を問わないので、実効 InProgress の行にだけ現れうる。
  val invUnsurfacedAncestorOnlyOnWorkingRows = groupRows(entitySet, conditions).forall(g =>
    stallReasons(entitySet, conditions, g).contains(UnsurfacedAncestor) implies effective(entitySet, g) == InProgress)
  // 構成の確認: 自身の条件が未成立の理由も同じく、tasks の行では実効 InProgress の行にだけ現れる。
  val invOwnConditionOnlyOnWorkingRows = groupRows(entitySet, conditions).forall(g =>
    stallReasons(entitySet, conditions, g).contains(OwnConditionUnsatisfied) implies effective(entitySet, g) == InProgress)
  // 構成の確認: Blocked の行は詰まっており、Working の行は着手中の子孫がなければ、Empty の行は完了できなければ詰まっている。
  // Blocked の節は「着手中の Issue の祖先は採用済み」（invWorkingAncestors）に依存し、残る二つは定義から従う。
  val invStalledRowStatuses = groupRows(entitySet, conditions).forall(g => and {
    groupStatus(entitySet, conditions, g) == Blocked implies stalled(entitySet, conditions, g),
    (groupStatus(entitySet, conditions, g) == Working and not(hasWorkingDescendant(entitySet, g)))
      implies stalled(entitySet, conditions, g),
    (groupStatus(entitySet, conditions, g) == Empty and not(canOperate(entitySet, g, Complete, true)))
      implies stalled(entitySet, conditions, g),
  })

  // invariant: 未終了（Completed でない登録済み）の各 Entity について、その近傍のいずれかに
  // 実行可能な lifecycle 操作（最終確認は通るとする）か移動か dependency の編集がある。
  // 近傍は自身・祖先・子孫・依存先に、各要素の完了済みの依存元とその祖先を Entity 数だけ繰り返し加えた閉包とする。
  // 完了した祖先の Reopen が完了済みの依存元に止められ、その依存元も祖先の採用を待つように、
  // 完了済みの依存元とその祖先を順に Reopen しないと再開できない場合があるため。
  // 整理・再判断・Reopen も数えるため、当初の計画どおり完了できる意味ではない。
  pure def canActOn(s: EntitySetState, x: Id): bool = or {
    OPERATIONS.exists(op => canOperate(s, x, op, true)),
    PARENTS.exists(p => canMove(s, x, p)),
    s.registered.exists(t => canAddDependency(s, x, t) or canRemoveDependency(s, x, t)),
  }
  pure def neighborhood(s: EntitySetState, id: Id): Set[Id] =
    IDS.fold(Set(id).union(ancestorsOf(s, id)).union(descendants(s, id)).union(depsOf(s, id)), (seen, _) =>
      seen.union(seen.map(x => completedDependents(s, x)
        .union(completedDependents(s, x).map(d => ancestorsOf(s, d)).flatten())).flatten()))
  val invNoUnexpectedDeadEnd = entitySet.registered.forall(id => storedOf(entitySet, id) != Completed implies
    neighborhood(entitySet, id).exists(x => canActOn(entitySet, x)))

  // ---- witness ----

  // witness: Reopen の経路。Issue を Reopen して再び着手する、Completed の子を持つ Group を Reopen して
  // 実効が InProgress になる、完了済みの依存元があるため Reopen できない、Reopen 後の dependency の編集。
  val wReopenThenStart = match observation.event {
    | LifecycleOperation(change) => change.kind == Issue and change.op == Start
        and history.reopenedIssues.contains(change.id)
    | _ => false
  }
  val wGroupReopenWorking = match observation.event {
    | LifecycleOperation(change) => change.kind == Group and change.op == Reopen
        and children(entitySet, change.id).exists(c => storedOf(entitySet, c) == Completed)
        and effective(entitySet, change.id) == InProgress
    | _ => false
  }
  val wReopenRejectedByDependent = entitySet.registered.exists(id => storedOf(entitySet, id) == Completed
    and parentOpen(entitySet, parentOf(entitySet, id)) and completedDependents(entitySet, id).size() > 0
    and not(canOperate(entitySet, id, Reopen, true)))
  val wDependencyEditAfterReopen = match observation.event {
    | AddDependency(change) => history.reopenedIssues.contains(change.source)
    | RemoveDependency(change) => history.reopenedIssues.contains(change.source)
    | _ => false
  }

  // witness: Group の完了。空の Group を NotStarted から完了、子がすべて取りやめの Group の完了、
  // 子が完了済みの Group の完了、最終確認待ちとその不合格、子 Group の完了を経た完了。
  pure def completedGroupEvent(o: EntitySetObservation): Set[Id] = match o.event {
    | LifecycleOperation(change) => if (change.kind == Group and change.op == Complete) Set(change.id) else Set()
    | _ => Set()
  }
  val wEmptyGroupComplete = completedGroupEvent(observation).exists(g =>
    children(observation.before, g).size() == 0 and storedOf(observation.before, g) == NotStarted)
  val wAllCancelledGroupComplete = completedGroupEvent(observation).exists(g =>
    children(entitySet, g).size() > 0 and children(entitySet, g).forall(c => storedOf(entitySet, c) == Cancelled))
  val wWorkingGroupComplete = completedGroupEvent(observation).exists(g => effective(observation.before, g) == InProgress)
  val wParentCompletesAfterGroup = completedGroupEvent(observation).exists(g =>
    children(entitySet, g).exists(c => kindOf(entitySet, c) == Group and storedOf(entitySet, c) == Completed))
  val wAwaitingFinalCheck = groups(entitySet).exists(g => children(entitySet, g).size() > 0
    and canOperate(entitySet, g, Complete, true) and not(canOperate(entitySet, g, Complete, false)))

  // witness: 着手の前提。祖先の依存先が未完了で子 Issue の着手が拒否され、その依存先の完了で着手可能になる。
  // 親 Group が Undecided のため子 Issue が着手できない。孫 Issue の着手で祖父 Group の実効が InProgress になる。
  // 子が完了済みのため Group の Withdraw が拒否される。
  val wAncestorDependencyBlocksStart = issues(entitySet).exists(id => storedOf(entitySet, id) == NotStarted
    and ancestorsAdopted(entitySet, id) and depsComplete(entitySet, id)
    and not(ancestorDepsComplete(entitySet, id)) and not(canOperate(entitySet, id, Start, false)))
  val wAncestorDependencyCompletionUnblocks = match observation.event {
    | LifecycleOperation(change) => change.op == Complete and issues(entitySet).exists(id =>
        ancestorsOf(entitySet, id).exists(a => depsOf(entitySet, a).contains(change.id))
        and not(canOperate(observation.before, id, Start, false)) and canOperate(entitySet, id, Start, false))
    | _ => false
  }
  val wUndecidedParentBlocksStart = issues(entitySet).exists(id => storedOf(entitySet, id) == NotStarted
    and depsComplete(entitySet, id)
    and (match parentOf(entitySet, id) {
      | Unassigned => false
      | InGroup(g) => storedOf(entitySet, g) == Undecided
    })
    and not(canOperate(entitySet, id, Start, false)))
  val wGrandchildStartActivatesGrandparent = match observation.event {
    | LifecycleOperation(change) => change.kind == Issue and change.op == Start
        and ancestorsOf(entitySet, change.id).exists(a => ancestorsOf(entitySet, a).size() == 0
          and ancestorsOf(entitySet, change.id).size() >= 2
          and effective(observation.before, a) == NotStarted and effective(entitySet, a) == InProgress)
    | _ => false
  }
  val wWithdrawRejectedByCompletedChild = groups(entitySet).exists(g => storedOf(entitySet, g) == NotStarted
    and parentOpen(entitySet, parentOf(entitySet, g))
    and children(entitySet, g).exists(c => storedOf(entitySet, c) == Completed)
    and not(canOperate(entitySet, g, Withdraw, false)))
  val wWithdrawRejectedByWorkingGrandchild = groups(entitySet).exists(g => storedOf(entitySet, g) == NotStarted
    and parentOpen(entitySet, parentOf(entitySet, g))
    and children(entitySet, g).exists(c => kindOf(entitySet, c) == Group and storedOf(entitySet, c) == NotStarted
      and children(entitySet, c).exists(i => kindOf(entitySet, i) == Issue and storedOf(entitySet, i) == InProgress))
    and not(canOperate(entitySet, g, Withdraw, false)))
  val wPrerequisiteCompletionUnblocksStart = match observation.event {
    | LifecycleOperation(change) => change.op == Complete and issues(entitySet).exists(id =>
        depsOf(entitySet, id).contains(change.id)
        and not(canOperate(observation.before, id, Start, false)) and canOperate(entitySet, id, Start, false))
    | _ => false
  }
  val wStartUnderUndecidedGrandparentRejected = issues(entitySet).exists(id => storedOf(entitySet, id) == NotStarted
    and ancestorsOf(entitySet, id).exists(a => storedOf(entitySet, a) == Undecided)
    and (match parentOf(entitySet, id) { | Unassigned => false | InGroup(g) => storedOf(entitySet, g) == NotStarted })
    and not(canOperate(entitySet, id, Start, false)))

  // witness: 変換。Issue を Group に変換して子を登録する、Group を Issue に変換する、
  // 進行中の Issue は変換できない、変換した Issue の lifecycle と dependency の保持。
  val wConvertToGroup = match observation.event {
    | ConvertedToGroup(_) => true
    | _ => false
  }
  val wConvertToIssue = match observation.event {
    | ConvertedToIssue(_) => true
    | _ => false
  }
  val wRegisterUnderConvertedGroup = match observation.event {
    | Register(change) => match change.parent {
      | Unassigned => false
      | InGroup(g) => history.convertedToGroup.contains(g) and kindOf(entitySet, g) == Group
    }
    | _ => false
  }
  val wWorkingIssueNotConvertible = match observation.event {
    | LifecycleOperation(change) => change.kind == Issue and change.op == Start
        and canConvertToGroup(observation.before, change.id) and not(canConvertToGroup(entitySet, change.id))
    | _ => false
  }
  val wConvertAdoptedWithDependency = match observation.event {
    | ConvertedToGroup(id) => storedOf(entitySet, id) == NotStarted and depsOf(entitySet, id).size() > 0
    | ConvertedToIssue(id) => storedOf(entitySet, id) == NotStarted and depsOf(entitySet, id).size() > 0
    | _ => false
  }

  // witness: 所属と登録。付け外しと Group 間の移動、実効が InProgress の Entity の移動、
  // 2種類 × 2経路の登録、終了した Group への登録・移動の拒否、再検討・Reopen による構成変更の再開、三階層の包含。
  val wAttach = match observation.event {
    | Move(change) => parentOf(observation.before, change.id) == Unassigned and change.parent != Unassigned
    | _ => false
  }
  val wDetach = match observation.event {
    | Move(change) => change.parent == Unassigned
    | _ => false
  }
  val wMoveBetweenGroups = match observation.event {
    | Move(change) => parentOf(observation.before, change.id) != Unassigned and change.parent != Unassigned
    | _ => false
  }
  val wWorkingIssueMove = match observation.event {
    | Move(change) => kindOf(entitySet, change.id) == Issue and storedOf(entitySet, change.id) == InProgress
        and change.parent != Unassigned
    | _ => false
  }
  val wWorkingGroupMove = match observation.event {
    | Move(change) => kindOf(entitySet, change.id) == Group and effective(entitySet, change.id) == InProgress
        and change.parent != Unassigned
    | _ => false
  }
  val wWorkingMoveRejectedByUndecided = entitySet.registered.exists(id => effective(entitySet, id) == InProgress
    and groups(entitySet).exists(g => parentOf(entitySet, id) != InGroup(g) and destinationOpen(entitySet, InGroup(g))
      and not(destinationAdopted(entitySet, InGroup(g))) and not(canMove(entitySet, id, InGroup(g)))))
  def sawRegister(kind: Kind, lifecycle: Lifecycle): bool = match observation.event {
    | Register(change) => change.kind == kind and change.lifecycle == lifecycle
    | _ => false
  }
  val wRegisterIssueProposal = sawRegister(Issue, Undecided)
  val wRegisterIssueAdopted = sawRegister(Issue, NotStarted)
  val wRegisterGroupProposal = sawRegister(Group, Undecided)
  val wRegisterGroupAdopted = sawRegister(Group, NotStarted)
  val wRegisterOutside = match observation.event {
    | Register(change) => change.parent == Unassigned
    | _ => false
  }
  val wRegisterIntoClosedRejected = groups(entitySet).exists(g => terminal(storedOf(entitySet, g))
    and IDS.exists(id => not(entitySet.registered.contains(id))
      and REGISTRATION_LIFECYCLES.forall(lc => not(canRegister(entitySet, id, lc, InGroup(g))))))
  val wMoveIntoClosedRejected = groups(entitySet).exists(g => terminal(storedOf(entitySet, g))
    and entitySet.registered.exists(id => id != g and parentOf(entitySet, id) != InGroup(g)
      and parentOpen(entitySet, parentOf(entitySet, id)) and not(ancestorsOf(entitySet, g).contains(id))
      and preconditionsAcyclic(applyMove(entitySet, id, InGroup(g)))
      and not(canMove(entitySet, id, InGroup(g)))))
  val wReconsiderAllowsMembership = match observation.event {
    | LifecycleOperation(change) => change.kind == Group and change.op == Reconsider and IDS.exists(id => or {
        not(canMove(observation.before, id, InGroup(change.id))) and canMove(entitySet, id, InGroup(change.id)),
        REGISTRATION_LIFECYCLES.exists(lc => not(canRegister(observation.before, id, lc, InGroup(change.id)))
          and canRegister(entitySet, id, lc, InGroup(change.id))),
      })
    | _ => false
  }
  val wReopenAllowsMembership = match observation.event {
    | LifecycleOperation(change) => change.kind == Group and change.op == Reopen
        and entitySet.registered.exists(id => canMove(entitySet, id, InGroup(change.id))
          or (parentOf(entitySet, id) == InGroup(change.id) and canMove(entitySet, id, Unassigned)))
    | _ => false
  }
  val wThreeLevels = entitySet.registered.exists(id => ancestorsOf(entitySet, id).size() >= 2)
  val wMixedChildren = groups(entitySet).exists(g => children(entitySet, g).exists(c => kindOf(entitySet, c) == Issue)
    and children(entitySet, g).exists(c => kindOf(entitySet, c) == Group))
  val wIndirectContainmentCycleRejected = groups(entitySet).exists(g => descendants(entitySet, g).exists(d =>
    parentOf(entitySet, d) != InGroup(g) and kindOf(entitySet, d) == Group and not(canMove(entitySet, g, InGroup(d)))))

  // witness: dependency。追加と削除、Group を含む依存、直接・間接の循環の拒否、祖先・子孫間の依存の拒否、
  // 着手中に足した未完了の dependency による完了の阻止。
  val wAddDependency = match observation.event {
    | AddDependency(_) => true
    | _ => false
  }
  val wRemoveDependency = match observation.event {
    | RemoveDependency(_) => true
    | _ => false
  }
  val wGroupDependsOnIssue = match observation.event {
    | AddDependency(change) => kindOf(entitySet, change.source) == Group and kindOf(entitySet, change.target) == Issue
    | _ => false
  }
  val wIssueDependsOnGroup = match observation.event {
    | AddDependency(change) => kindOf(entitySet, change.source) == Issue and kindOf(entitySet, change.target) == Group
    | _ => false
  }
  val wDirectCycleRejected = entitySet.registered.exists(source => entitySet.registered.exists(target =>
    storedOf(entitySet, source) != Completed and depsOf(entitySet, target).contains(source)
      and not(canAddDependency(entitySet, source, target))))
  val wAncestorDependencyRejected = issues(entitySet).exists(id => storedOf(entitySet, id) == NotStarted
    and parentOf(entitySet, id) != Unassigned
    and ancestorsOf(entitySet, id).exists(a => not(depsOf(entitySet, id).contains(a)) and not(canAddDependency(entitySet, id, a))))
  val wIndirectCycleRejected = entitySet.registered.exists(source => entitySet.registered.exists(mid =>
    entitySet.registered.exists(target => and {
      source != mid, mid != target, source != target,
      depsOf(entitySet, mid).contains(source), depsOf(entitySet, target).contains(mid),
      storedOf(entitySet, source) != Completed, not(depsOf(entitySet, source).contains(target)),
      not(canAddDependency(entitySet, source, target)),
    })))
  val wAddDuringWorkBlocksCompletion = match observation.event {
    | AddDependency(change) => kindOf(entitySet, change.source) == Issue and storedOf(entitySet, change.source) == InProgress
        and canOperate(observation.before, change.source, Complete, false)
        and not(canOperate(entitySet, change.source, Complete, false))
    | _ => false
  }

  // witness: 候補と一覧。判断候補と着手候補、浮上していない Issue への ID を明示した着手、Group の五つの状況、
  // Ready と Working が同時に成り立つ Group で Ready が選ばれること、Issue の三つの状況。
  val wJudgment = judgments(entitySet, conditions).size() > 0
  val wCandidateIssue = candidateIssues(entitySet, conditions).size() > 0
  val wStartHiddenIssue = match observation.event {
    | LifecycleOperation(change) => change.kind == Issue and change.op == Start
        and not(surfaced(observation.before, observation.beforeConditions, change.id))
    | _ => false
  }
  def sawGroupStatus(status: TaskStatus): bool =
    groupRows(entitySet, conditions).exists(g => groupStatus(entitySet, conditions, g) == status)
  val wGroupConfirmable = sawGroupStatus(Confirmable)
  val wGroupReady = sawGroupStatus(Ready)
  val wGroupWorking = sawGroupStatus(Working)
  val wGroupBlocked = sawGroupStatus(Blocked)
  val wGroupEmpty = sawGroupStatus(Empty)
  // 空の採用済み Group は完了できても Empty になる。
  val wEmptyOverConfirmable = groupRows(entitySet, conditions).exists(g =>
    groupStatus(entitySet, conditions, g) == Empty and canOperate(entitySet, g, Complete, true))
  val wReadyOverWorking = groupRows(entitySet, conditions).exists(g =>
    groupStatus(entitySet, conditions, g) == Ready and effective(entitySet, g) == InProgress)
  val wConfirmableOverWorking = groupRows(entitySet, conditions).exists(g =>
    groupStatus(entitySet, conditions, g) == Confirmable and effective(entitySet, g) == InProgress)
  val wWorkingGroupRowHidden = groupRows(entitySet, conditions).exists(g =>
    effective(entitySet, g) == InProgress and not(surfaced(entitySet, conditions, g)))
  def sawIssueStatus(status: TaskStatus): bool =
    issueRows(entitySet, conditions).exists(id => issueStatus(entitySet, conditions, id) == status)
  val wIssueReady = sawIssueStatus(Ready)
  val wIssueBlocked = sawIssueStatus(Blocked)
  val wIssueWorking = sawIssueStatus(Working)
  val wConditionRose = match observation.event {
    | ConditionChanged(id) => conditions.get(id)
    | _ => false
  }
  val wConditionFell = match observation.event {
    | ConditionChanged(id) => not(conditions.get(id))
    | _ => false
  }

  // witness: 詰まっている理由の内訳ごとの観測と、理由が付く行の状況ごとの観測。
  // 理由の内訳は子のある行（Blocked と Working）で観測する。Empty の行は理由が 2 種類に限られ、
  // 到達しやすいため、含めると子のある行での各理由の到達を隠す。
  def sawStalledBy(reason: StallReason): bool = groupRows(entitySet, conditions).exists(g =>
    stalled(entitySet, conditions, g) and children(entitySet, g).size() > 0
      and stallReasons(entitySet, conditions, g).contains(reason))
  val wStalledByUndecidedChildren = sawStalledBy(UndecidedChild)
  val wStalledByDependency = sawStalledBy(DependencyPending)
  val wStalledBySubgroup = sawStalledBy(OpenSubgroup)
  val wStalledByHiddenCandidate = sawStalledBy(HiddenCandidate)
  val wStalledByUnadoptedAncestor = sawStalledBy(UnadoptedAncestor)
  // 浮上していない祖先の理由は、axon show の対象（stored が NotStarted の Group）で観測する。
  // 行に出ない Group と、行に出る実効 InProgress の Group の両方を観測する。
  val wStalledByUnsurfacedAncestorOffRows = shownGroups(entitySet).exists(g =>
    stalled(entitySet, conditions, g) and children(entitySet, g).size() > 0
      and stallReasons(entitySet, conditions, g).contains(UnsurfacedAncestor)
      and not(groupRows(entitySet, conditions).contains(g)))
  val wStalledByUnsurfacedAncestorOnWorkingRow = groupRows(entitySet, conditions).exists(g =>
    stalled(entitySet, conditions, g) and stallReasons(entitySet, conditions, g).contains(UnsurfacedAncestor))
  // 自身の条件が未成立の理由も、行に出ない Group と、行に出る実効 InProgress の Group の両方で観測する。
  // 行に出ない Group では、着手可能な子孫が隠れている場合（Unsurfaced candidate と重なる）も観測する。
  val wStalledByOwnConditionOffRows = shownGroups(entitySet).exists(g =>
    stalled(entitySet, conditions, g) and children(entitySet, g).size() > 0
      and stallReasons(entitySet, conditions, g).contains(OwnConditionUnsatisfied)
      and not(groupRows(entitySet, conditions).contains(g)))
  val wStalledByOwnConditionHidingCandidates = shownGroups(entitySet).exists(g =>
    stalled(entitySet, conditions, g)
      and stallReasons(entitySet, conditions, g).contains(OwnConditionUnsatisfied)
      and stallReasons(entitySet, conditions, g).contains(HiddenCandidate)
      and not(groupRows(entitySet, conditions).contains(g)))
  val wStalledByOwnConditionOnWorkingRow = groupRows(entitySet, conditions).exists(g =>
    stalled(entitySet, conditions, g) and stallReasons(entitySet, conditions, g).contains(OwnConditionUnsatisfied))
  def sawStalledStatus(status: TaskStatus): bool = groupRows(entitySet, conditions).exists(g =>
    stalled(entitySet, conditions, g) and groupStatus(entitySet, conditions, g) == status)
  // 完了済みの子孫はあるが着手中の子孫がない実効 InProgress の行に理由が付く。
  val wStalledWorking = sawStalledStatus(Working)
  val wStalledBlocked = sawStalledStatus(Blocked)
  val wStalledEmpty = sawStalledStatus(Empty)
  // 条件に依らない理由のどれもない詰まっている Group（invStalledHasStructuralReason の反例）。
  // axon show の対象全体で観測されないことを確かめる。
  val wStalledWithoutListedReason = shownGroups(entitySet).exists(g =>
    stalled(entitySet, conditions, g) and stallReasons(entitySet, conditions, g).exclude(CONDITION_REASONS).size() == 0)

  // witness: 実効が InProgress の Group が、stored が NotStarted でない祖先を持つ状態
  // （invEffectiveWorkingAncestors の反例）と、そこへ至る経路。Group の完了、完了済みの Entity の移動、
  // 着手・完了した子孫を持つ Group の採用・Reopen はどれも前提で塞いでおり、観測されないことを確かめる。
  pure def effectiveUnderUnadopted(s: EntitySetState): Set[Id] = groups(s).filter(g =>
    effective(s, g) == InProgress and not(ancestorsAdopted(s, g)))
  val wEffectiveWorkingUnderUnadopted = effectiveUnderUnadopted(entitySet).size() > 0
  val wUnadoptedViaComplete = effectiveUnderUnadopted(entitySet).size() > 0
    and effectiveUnderUnadopted(observation.before).size() == 0
    and (match observation.event {
      | LifecycleOperation(change) => change.op == Complete
      | _ => false
    })
  val wUnadoptedViaMove = effectiveUnderUnadopted(entitySet).size() > 0
    and effectiveUnderUnadopted(observation.before).size() == 0
    and (match observation.event {
      | Move(change) => storedOf(entitySet, change.id) == Completed or effective(entitySet, change.id) == InProgress
      | _ => false
    })
  val wUnadoptedViaAcceptOrReopen = effectiveUnderUnadopted(entitySet).size() > 0
    and effectiveUnderUnadopted(observation.before).size() == 0
    and (match observation.event {
      | LifecycleOperation(change) => change.op == Accept or change.op == Reopen
      | _ => false
    })
}
