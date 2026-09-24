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
  type PlanState = { registered: Set[Id], entities: Id -> EntityRecord }
  type Conditions = Id -> bool
  type ConditionResult = NotEvaluated | Evaluated(bool)

  // observation は直前の状態と操作の検証専用の記録であり、製品の保存項目ではない。
  // reviewPassed は Group の最終確認が通ったという抽象入力。
  // history も検証専用で、Issue として Reopen した ID と、Issue から Group へ変換した ID を累積する。
  type PlanEvent = Initial
    | ConditionChanged(Id)
    | LifecycleOperation({ id: Id, kind: Kind, op: Operation })
    | Move({ id: Id, parent: Parent })
    | Register({ id: Id, kind: Kind, lifecycle: Lifecycle, parent: Parent })
    | ConvertedToGroup(Id)
    | ConvertedToIssue(Id)
    | AddDependency({ source: Id, target: Id })
    | RemoveDependency({ source: Id, target: Id })
  type PlanObservation = { before: PlanState, beforeConditions: Conditions, event: PlanEvent, reviewPassed: bool }
  type History = { reopenedIssues: Set[Id], convertedToGroup: Set[Id] }
  var plan: PlanState
  var conditions: Conditions
  var observation: PlanObservation
  var history: History
  pure def terminal(current: Lifecycle): bool = Set(Completed, Cancelled).contains(current)

  // 記録の読み出し。
  pure def kindOf(s: PlanState, id: Id): Kind = s.entities.get(id).kind
  pure def storedOf(s: PlanState, id: Id): Lifecycle = s.entities.get(id).stored
  pure def parentOf(s: PlanState, id: Id): Parent = s.entities.get(id).parent
  pure def depsOf(s: PlanState, id: Id): Set[Id] = s.entities.get(id).deps
  pure def groups(s: PlanState): Set[Id] = s.registered.filter(id => kindOf(s, id) == Group)
  pure def issues(s: PlanState): Set[Id] = s.registered.filter(id => kindOf(s, id) == Issue)

  // 包含の導出。所属から直属の子、祖先、子孫をたどる。
  // ancestors は所属先そのものを含み、所属先の祖先を Entity 数を上限にたどる。
  pure def children(s: PlanState, group: Id): Set[Id] =
    s.registered.filter(id => parentOf(s, id) == InGroup(group))
  pure def parentSet(parent: Parent): Set[Id] = match parent {
    | Unassigned => Set()
    | InGroup(id) => Set(id)
  }
  pure def ancestors(s: PlanState, parent: Parent): Set[Id] =
    IDS.fold(parentSet(parent), (seen, _) =>
      seen.union(seen.map(id => parentSet(parentOf(s, id))).flatten()))
  pure def ancestorsOf(s: PlanState, id: Id): Set[Id] = ancestors(s, parentOf(s, id))
  pure def descendants(s: PlanState, group: Id): Set[Id] =
    s.registered.filter(id => ancestorsOf(s, id).contains(group))
  pure def allChildrenTerminal(s: PlanState, group: Id): bool =
    children(s, group).forall(id => terminal(storedOf(s, id)))

  // Group の実効 lifecycle。stored が NotStarted で、直属の子のうち実効 lifecycle が
  // InProgress か Completed のものがあれば InProgress、それ以外は stored のまま。
  // 子 Group の実効値は再帰的に決まるため、進行中の Group の集合を空から Entity 数だけ広げて求める。
  // Issue と、stored が NotStarted 以外の Group は stored がそのまま実効値になる。
  pure def workingGroups(s: PlanState): Set[Id] =
    IDS.fold(Set(), (active, _) => groups(s).filter(g => storedOf(s, g) == NotStarted
      and children(s, g).exists(c => Set(InProgress, Completed).contains(storedOf(s, c)) or active.contains(c))))
  pure def effective(s: PlanState, id: Id): Lifecycle =
    if (workingGroups(s).contains(id)) InProgress else storedOf(s, id)

  // 親が終了していないかの判定。所属なしは通る。
  pure def parentOpen(s: PlanState, parent: Parent): bool = match parent {
    | Unassigned => true
    | InGroup(g) => not(terminal(storedOf(s, g)))
  }
  // 所属先として選べるか。所属なし、または登録済みで終了していない Group。
  pure def destinationOpen(s: PlanState, parent: Parent): bool = match parent {
    | Unassigned => true
    | InGroup(g) => s.registered.contains(g) and kindOf(s, g) == Group and not(terminal(storedOf(s, g)))
  }
  // 所属先とその全祖先が採用済みで未終了（stored が NotStarted）か。所属なしは通る。
  pure def destinationAdopted(s: PlanState, parent: Parent): bool =
    ancestors(s, parent).forall(a => storedOf(s, a) == NotStarted)

  pure def depsComplete(s: PlanState, id: Id): bool =
    depsOf(s, id).forall(target => storedOf(s, target) == Completed)
  pure def ancestorDepsComplete(s: PlanState, id: Id): bool =
    ancestorsOf(s, id).forall(a => depsComplete(s, a))
  pure def ancestorsAdopted(s: PlanState, id: Id): bool = destinationAdopted(s, parentOf(s, id))
  pure def hasStartedDescendant(s: PlanState, id: Id): bool =
    descendants(s, id).exists(d => Set(InProgress, Completed).contains(storedOf(s, d)))
  pure def completedDependents(s: PlanState, id: Id): Set[Id] =
    s.registered.filter(e => storedOf(s, e) == Completed and depsOf(s, e).contains(id))

  // 構造の循環検査には「全員が通常どおり完了する経路」を使う。各 Entity の完了の前提を
  // 「直属の子 ∪ 自身の依存先 ∪ 全祖先の依存先」とし、前提の残っていない節点を順に除去して
  // 全節点を除去できるかを検査する。取りやめによる回避はこの経路に加えない。
  pure def completionPredecessors(s: PlanState, id: Id): Set[Id] =
    children(s, id).union(depsOf(s, id)).union(ancestorsOf(s, id).map(a => depsOf(s, a)).flatten())
  pure def preconditionsAcyclic(s: PlanState): bool = {
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
  pure def canOperate(s: PlanState, id: Id, op: Operation, passed: bool): bool = and {
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
  pure def applyLifecycle(s: PlanState, id: Id, op: Operation): PlanState = {
    ...s,
    entities: s.entities.set(id, { ...s.entities.get(id), stored: applyOperation(storedOf(s, id), op) }),
  }

  pure def observe(s: PlanState, c: Conditions, event: PlanEvent, passed: bool): PlanObservation =
    { before: s, beforeConditions: c, event: event, reviewPassed: passed }

  // 0 は Group、1 と 2 は Issue で 0 の子、3 は所属なしの Group、4 は未登録の枠。
  // 全 Entity は Undecided、条件入力はすべて不成立から始める。
  // これは探索の開始点であって、製品の再浮上条件の初期値を決めるものではない。
  // 未登録の枠の記録は使われない値で、登録時に上書きする。candidate_evaluation も同じ初期状態から始める。
  pure val initialPlan: PlanState = {
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
    plan' = initialPlan,
    conditions' = initialConditions,
    observation' = observe(initialPlan, initialConditions, Initial, false),
    history' = { reopenedIssues: Set(), convertedToGroup: Set() },
  }

  // 各操作は前提を満たすときだけ実行し、他の Entity を自動で更新しない。
  // Group の実効 lifecycle は保存せず、子の変化に応じて導出が変わるだけである。
  action performGroup(id: Id, op: Operation, passed: bool): bool = all {
    plan.registered.contains(id),
    kindOf(plan, id) == Group,
    canOperate(plan, id, op, passed),
    plan' = applyLifecycle(plan, id, op),
    conditions' = conditions,
    observation' = observe(plan, conditions, LifecycleOperation({ id: id, kind: Group, op: op }), passed),
    history' = history,
  }
  action performIssue(id: Id, op: Operation): bool = all {
    plan.registered.contains(id),
    kindOf(plan, id) == Issue,
    canOperate(plan, id, op, false),
    plan' = applyLifecycle(plan, id, op),
    conditions' = conditions,
    observation' = observe(plan, conditions, LifecycleOperation({ id: id, kind: Issue, op: op }), false),
    history' = if (op == Reopen) { ...history, reopenedIssues: history.reopenedIssues.union(Set(id)) } else history,
  }

  // 所属変更。移動元・移動先の親がどちらも終了しておらず、移動先は Group に限る。
  // 着手済み・完了済みの Entity（Issue の stored、Group の実効が InProgress か Completed）を移すときは、
  // 移動先の Group 自身を含む全祖先の stored が NotStarted であること。
  // 自己包含と子孫の下への移動、変更後の通常完了経路が循環する移動は拒否する。
  // 移動は lifecycle を変えず、Group は配下ごと部分木全体が移る。
  pure def applyMove(s: PlanState, id: Id, parent: Parent): PlanState =
    { ...s, entities: s.entities.set(id, { ...s.entities.get(id), parent: parent }) }
  pure def canMove(s: PlanState, id: Id, parent: Parent): bool = and {
    s.registered.contains(id),
    parentOf(s, id) != parent,
    parentOpen(s, parentOf(s, id)),
    destinationOpen(s, parent),
    not(Set(InProgress, Completed).contains(effective(s, id))) or destinationAdopted(s, parent),
    not(ancestors(s, parent).contains(id)),
    preconditionsAcyclic(applyMove(s, id, parent)),
  }
  action move(id: Id, parent: Parent): bool = all {
    canMove(plan, id, parent),
    plan' = applyMove(plan, id, parent),
    conditions' = conditions,
    observation' = observe(plan, conditions, Move({ id: id, parent: parent }), false),
    history' = history,
  }

  // 未登録の枠を Issue か Group として、Undecided か NotStarted で、所属先を指定して登録する。
  // 登録は与えられた採否を反映し、採用判断は代行せず、登録だけでは着手しない。
  pure def canRegister(s: PlanState, id: Id, lifecycle: Lifecycle, parent: Parent): bool = and {
    not(s.registered.contains(id)),
    REGISTRATION_LIFECYCLES.contains(lifecycle),
    destinationOpen(s, parent),
  }
  pure def applyRegister(s: PlanState, id: Id, kind: Kind, lifecycle: Lifecycle, parent: Parent): PlanState = {
    registered: s.registered.union(Set(id)),
    entities: s.entities.set(id, { kind: kind, stored: lifecycle, parent: parent, deps: Set() }),
  }
  action register(id: Id, kind: Kind, lifecycle: Lifecycle, parent: Parent): bool = all {
    canRegister(plan, id, lifecycle, parent),
    plan' = applyRegister(plan, id, kind, lifecycle, parent),
    conditions' = conditions,
    observation' = observe(plan, conditions,
      Register({ id: id, kind: kind, lifecycle: lifecycle, parent: parent }), false),
    history' = history,
  }

  // 種類の変換。lifecycle・所属・dependency を変えない。
  // Issue から Group へは stored が Undecided か NotStarted のとき（InProgress は先に Release が要る）。
  // Group から Issue へは stored が Undecided か NotStarted で、子がないとき。
  pure def canConvertToGroup(s: PlanState, id: Id): bool = and {
    s.registered.contains(id),
    kindOf(s, id) == Issue,
    REGISTRATION_LIFECYCLES.contains(storedOf(s, id)),
  }
  pure def canConvertToIssue(s: PlanState, id: Id): bool = and {
    s.registered.contains(id),
    kindOf(s, id) == Group,
    REGISTRATION_LIFECYCLES.contains(storedOf(s, id)),
    children(s, id).size() == 0,
  }
  pure def applyConvert(s: PlanState, id: Id, kind: Kind): PlanState =
    { ...s, entities: s.entities.set(id, { ...s.entities.get(id), kind: kind }) }
  action convertToGroup(id: Id): bool = all {
    canConvertToGroup(plan, id),
    plan' = applyConvert(plan, id, Group),
    conditions' = conditions,
    observation' = observe(plan, conditions, ConvertedToGroup(id), false),
    history' = { ...history, convertedToGroup: history.convertedToGroup.union(Set(id)) },
  }
  action convertToIssue(id: Id): bool = all {
    canConvertToIssue(plan, id),
    plan' = applyConvert(plan, id, Issue),
    conditions' = conditions,
    observation' = observe(plan, conditions, ConvertedToIssue(id), false),
    history' = history,
  }

  // dependency の編集。依存元が完了していればその dependency は固定し、Reopen 後は編集できる。
  // 自己依存と、包含を合わせた通常完了経路の循環は追加時に拒否する。祖先・子孫間の依存はこの循環に含まれる。
  pure def withDeps(s: PlanState, id: Id, deps: Set[Id]): PlanState =
    { ...s, entities: s.entities.set(id, { ...s.entities.get(id), deps: deps }) }
  pure def canAddDependency(s: PlanState, source: Id, target: Id): bool = and {
    s.registered.contains(source),
    s.registered.contains(target),
    storedOf(s, source) != Completed,
    source != target,
    not(depsOf(s, source).contains(target)),
    preconditionsAcyclic(withDeps(s, source, depsOf(s, source).union(Set(target)))),
  }
  action addDependency(source: Id, target: Id): bool = all {
    canAddDependency(plan, source, target),
    plan' = withDeps(plan, source, depsOf(plan, source).union(Set(target))),
    conditions' = conditions,
    observation' = observe(plan, conditions, AddDependency({ source: source, target: target }), false),
    history' = history,
  }
  pure def canRemoveDependency(s: PlanState, source: Id, target: Id): bool = and {
    s.registered.contains(source),
    storedOf(s, source) != Completed,
    depsOf(s, source).contains(target),
  }
  action removeDependency(source: Id, target: Id): bool = all {
    canRemoveDependency(plan, source, target),
    plan' = withDeps(plan, source, depsOf(plan, source).exclude(Set(target))),
    conditions' = conditions,
    observation' = observe(plan, conditions, RemoveDependency({ source: source, target: target }), false),
    history' = history,
  }

  // 条件入力は登録済みの Entity について両方向へ変化し、保存する状態を変えない。
  pure def canChangeCondition(s: PlanState, c: Conditions, id: Id, satisfied: bool): bool =
    s.registered.contains(id) and c.get(id) != satisfied
  action changeCondition(id: Id, satisfied: bool): bool = all {
    canChangeCondition(plan, conditions, id, satisfied),
    plan' = plan,
    conditions' = conditions.set(id, satisfied),
    observation' = observe(plan, conditions, ConditionChanged(id), false),
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
  pure def conditionResult(s: PlanState, c: Conditions, id: Id): ConditionResult =
    if (not(s.registered.contains(id)) or terminal(storedOf(s, id))) NotEvaluated else Evaluated(c.get(id))
  pure def ancestorsSurfaced(s: PlanState, c: Conditions, id: Id): bool =
    ancestorsOf(s, id).forall(a => conditionResult(s, c, a) == Evaluated(true))
  pure def surfaced(s: PlanState, c: Conditions, id: Id): bool =
    conditionResult(s, c, id) == Evaluated(true) and ancestorsSurfaced(s, c, id)

  // 判断候補（axon proposals）は未判断で、自身とすべての祖先が浮上しているもの。
  // 着手可能は Start の前提を満たす Issue、着手候補はそのうち自身と全祖先が浮上しているもの。
  // Group は Start を持たないため着手候補にならない。
  pure def judgments(s: PlanState, c: Conditions): Set[Id] =
    s.registered.filter(id => storedOf(s, id) == Undecided and surfaced(s, c, id))
  pure def startableIssues(s: PlanState): Set[Id] = issues(s).filter(id => canOperate(s, id, Start, false))
  pure def candidateIssues(s: PlanState, c: Conditions): Set[Id] =
    startableIssues(s).filter(id => surfaced(s, c, id))
  pure def workingIssues(s: PlanState): Set[Id] = issues(s).filter(id => storedOf(s, id) == InProgress)

  // axon tasks の一覧の行と状況。一覧は平らで、Issue と Group の行を含む。
  // Working は表示上の InProgress（lifecycle の InProgress と名前が衝突するため別名にする）。
  type TaskStatus = Confirmable | Ready | Working | Blocked | Empty
  // Issue の行は、未着手で自身と全祖先が浮上しているものと、浮上に関係なく進行中のもの。
  pure def issueRows(s: PlanState, c: Conditions): Set[Id] =
    issues(s).filter(id => (storedOf(s, id) == NotStarted and surfaced(s, c, id)) or storedOf(s, id) == InProgress)
  pure def issueStatus(s: PlanState, c: Conditions, id: Id): TaskStatus =
    if (candidateIssues(s, c).contains(id)) Ready
    else if (storedOf(s, id) == InProgress) Working
    else Blocked
  // Group の行は、stored が NotStarted で自身と全祖先が浮上しているものと、浮上に関係なく実効が InProgress のもの。
  pure def groupRows(s: PlanState, c: Conditions): Set[Id] =
    groups(s).filter(g => (storedOf(s, g) == NotStarted and surfaced(s, c, g)) or effective(s, g) == InProgress)
  pure def hasCandidateDescendant(s: PlanState, c: Conditions, g: Id): bool =
    descendants(s, g).exists(id => candidateIssues(s, c).contains(id))
  // 優先順位: Confirmable（最終確認が通れば完了できる）> Ready（着手候補の子孫がある）
  // > Working（実効が InProgress）> Blocked（子が一つ以上あり上の三つのどれでもない）> Empty（直属の子がない）。
  pure def groupStatus(s: PlanState, c: Conditions, g: Id): TaskStatus =
    if (canOperate(s, g, Complete, true)) Confirmable
    else if (hasCandidateDescendant(s, c, g)) Ready
    else if (effective(s, g) == InProgress) Working
    else if (children(s, g).size() > 0) Blocked
    else Empty

  // Blocked の理由の内訳。未完了の dependency は、自身・全祖先・未終了の子孫のいずれかの依存先が未完了であること。
  // 未判断の子、未終了の子 Group、浮上していない着手可能な子孫の Issue、stored が Undecided の祖先を区別する。
  type BlockReason = DependencyPending | UndecidedChild | OpenSubgroup | HiddenCandidate | UnadoptedAncestor
  pure def blockReasons(s: PlanState, c: Conditions, g: Id): Set[BlockReason] = {
    val dependency = not(depsComplete(s, g)) or not(ancestorDepsComplete(s, g))
      or descendants(s, g).exists(d => not(terminal(storedOf(s, d))) and not(depsComplete(s, d)))
    val undecided = children(s, g).exists(ch => storedOf(s, ch) == Undecided)
    val subgroup = children(s, g).exists(ch => kindOf(s, ch) == Group and not(terminal(storedOf(s, ch))))
    val hidden = descendants(s, g).exists(d => startableIssues(s).contains(d) and not(surfaced(s, c, d)))
    val unadopted = ancestorsOf(s, g).exists(a => storedOf(s, a) == Undecided)
    Set(DependencyPending, UndecidedChild, OpenSubgroup, HiddenCandidate, UnadoptedAncestor).filter(r => match r {
      | DependencyPending => dependency
      | UndecidedChild => undecided
      | OpenSubgroup => subgroup
      | HiddenCandidate => hidden
      | UnadoptedAncestor => unadopted
    })
  }

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
    groups(plan).forall(g => storedOf(plan, g) != InProgress),
    not(saw(Group, Start)),
    not(saw(Group, Release)),
  }
  val invEffectiveFixpoint = plan.registered.forall(id => effective(plan, id) ==
    (if (kindOf(plan, id) == Group and storedOf(plan, id) == NotStarted
        and children(plan, id).exists(c => Set(InProgress, Completed).contains(effective(plan, c))))
      InProgress else storedOf(plan, id)))
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invEffectiveHasStartedDescendant = groups(plan).forall(g => effective(plan, g) == InProgress implies
    descendants(plan, g).exists(d => Set(InProgress, Completed).contains(storedOf(plan, d))))

  // invariant: 着手中の Issue の全祖先の stored は NotStarted。実効が InProgress の Group も同様。
  val invWorkingAncestors = issues(plan).forall(id => storedOf(plan, id) == InProgress implies
    ancestorsOf(plan, id).forall(a => storedOf(plan, a) == NotStarted))
  val invEffectiveWorkingAncestors = groups(plan).forall(g => effective(plan, g) == InProgress implies
    ancestorsOf(plan, g).forall(a => storedOf(plan, a) == NotStarted))

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
  val invCompletionReviewed = plan.registered.forall(id =>
    (storedOf(plan, id) == Completed
      and not(observation.before.registered.contains(id) and storedOf(observation.before, id) == Completed))
    implies and {
      observation.event == LifecycleOperation({ id: id, kind: kindOf(plan, id), op: Complete }),
      kindOf(plan, id) == Group implies observation.reviewPassed,
    })

  // invariant: 完了から抜けるのは Reopen だけで、Reopen は完了から未着手へ戻し、他の Entity と
  // 自身の種類・所属・dependency を変えない。
  val invCompletedLeavesOnlyByReopen = observation.before.registered.forall(id =>
    (storedOf(observation.before, id) == Completed and storedOf(plan, id) != Completed) implies
      (observation.event == LifecycleOperation({ id: id, kind: kindOf(plan, id), op: Reopen })
        and storedOf(plan, id) == NotStarted))
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invReopenTransition = match observation.event {
    | LifecycleOperation(change) => change.op == Reopen implies and {
        storedOf(observation.before, change.id) == Completed,
        storedOf(plan, change.id) == NotStarted,
        completedDependents(observation.before, change.id).size() == 0,
        plan.registered == observation.before.registered,
        IDS.exclude(Set(change.id)).forall(id => plan.entities.get(id) == observation.before.entities.get(id)),
        plan.entities.get(change.id) == { ...observation.before.entities.get(change.id), stored: NotStarted },
      }
    | _ => true
  }

  // ---- 包含 ----

  // invariant: 所属は登録済みの Group を指し、未登録の枠は所属も dependency も持たない。Issue は子を持たない。
  // 包含は循環しない。終了した Group の直属の子と全子孫は終了しており、構成は操作の前後で変わらない。
  // 終了した Group への追加・取り外しはどれも許可されず、自己包含と子孫の下への移動は拒否される。
  val invMembershipValid = and {
    plan.registered.subseteq(IDS),
    plan.registered.forall(id => match parentOf(plan, id) {
      | Unassigned => true
      | InGroup(g) => plan.registered.contains(g) and kindOf(plan, g) == Group
    }),
  }
  val invIssueHasNoChildren = issues(plan).forall(id => children(plan, id).size() == 0)
  val invContainmentAcyclic = plan.registered.forall(id => not(ancestorsOf(plan, id).contains(id)))
  val invClosedChildrenTerminal = groups(plan).forall(g => terminal(storedOf(plan, g)) implies allChildrenTerminal(plan, g))
  val invClosedDescendantsTerminal = groups(plan).forall(g => terminal(storedOf(plan, g)) implies
    descendants(plan, g).forall(d => terminal(storedOf(plan, d))))
  val invClosedStructureFrozen = groups(observation.before).forall(g => terminal(storedOf(observation.before, g))
    implies (children(plan, g) == children(observation.before, g)
      and descendants(plan, g) == descendants(observation.before, g)))
  val invClosedEditsDisabled = groups(plan).forall(g => terminal(storedOf(plan, g)) implies and {
    IDS.forall(id => not(canMove(plan, id, InGroup(g)))
      and REGISTRATION_LIFECYCLES.forall(lc => not(canRegister(plan, id, lc, InGroup(g))))),
    descendants(plan, g).forall(d => PARENTS.forall(p => not(canMove(plan, d, p)))
      and OPERATIONS.forall(op => not(canOperate(plan, d, op, true)))
      and not(canConvertToGroup(plan, d)) and not(canConvertToIssue(plan, d))),
  })
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invContainmentCycleEditsDisabled = plan.registered.forall(id =>
    not(canMove(plan, id, InGroup(id))) and descendants(plan, id).forall(d => not(canMove(plan, id, InGroup(d)))))

  // invariant: 各操作は対象以外の Entity を変えない。移動は所属だけ、登録は枠一つだけ、
  // dependency の編集は依存元の dependency だけ、条件入力の変化は保存する状態を変えない。
  pure def othersUnchanged(s: PlanState, before: PlanState, id: Id): bool =
    s.registered == before.registered
      and IDS.exclude(Set(id)).forall(e => s.entities.get(e) == before.entities.get(e))
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invOperationScope = match observation.event {
    | Initial => plan == observation.before
    | ConditionChanged(_) => plan == observation.before
    | LifecycleOperation(change) => othersUnchanged(plan, observation.before, change.id)
        and plan.entities.get(change.id) == { ...observation.before.entities.get(change.id),
          stored: applyOperation(storedOf(observation.before, change.id), change.op) }
    | Move(change) => othersUnchanged(plan, observation.before, change.id)
        and plan.entities.get(change.id) == { ...observation.before.entities.get(change.id), parent: change.parent }
    | Register(change) => plan.registered == observation.before.registered.union(Set(change.id))
        and not(observation.before.registered.contains(change.id))
        and IDS.exclude(Set(change.id)).forall(e => plan.entities.get(e) == observation.before.entities.get(e))
        and plan.entities.get(change.id) ==
          { kind: change.kind, stored: change.lifecycle, parent: change.parent, deps: Set() }
    | ConvertedToGroup(id) => othersUnchanged(plan, observation.before, id)
    | ConvertedToIssue(id) => othersUnchanged(plan, observation.before, id)
    | AddDependency(change) => othersUnchanged(plan, observation.before, change.source)
        and plan.entities.get(change.source) == { ...observation.before.entities.get(change.source),
          deps: depsOf(observation.before, change.source).union(Set(change.target)) }
    | RemoveDependency(change) => othersUnchanged(plan, observation.before, change.source)
        and plan.entities.get(change.source) == { ...observation.before.entities.get(change.source),
          deps: depsOf(observation.before, change.source).exclude(Set(change.target)) }
  }

  // invariant: 変換は種類だけを変え、lifecycle・所属・dependency・他の Entity を変えない。
  // Issue への変換は子のない Group に限り、どちらの変換も進行中・終了した Entity には起きない。
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invConversionScope = match observation.event {
    | ConvertedToGroup(id) => and {
        othersUnchanged(plan, observation.before, id),
        kindOf(observation.before, id) == Issue,
        plan.entities.get(id) == { ...observation.before.entities.get(id), kind: Group },
        REGISTRATION_LIFECYCLES.contains(storedOf(plan, id)),
      }
    | ConvertedToIssue(id) => and {
        othersUnchanged(plan, observation.before, id),
        kindOf(observation.before, id) == Group,
        children(observation.before, id).size() == 0,
        plan.entities.get(id) == { ...observation.before.entities.get(id), kind: Issue },
        REGISTRATION_LIFECYCLES.contains(storedOf(plan, id)),
      }
    | _ => true
  }

  // invariant: 新規登録は与えられた採否に対応する初期状態になり、自動で着手しない。
  val invRegistrationMatchesDecision = match observation.event {
    | Register(change) => and {
        storedOf(plan, change.id) == change.lifecycle,
        REGISTRATION_LIFECYCLES.contains(change.lifecycle),
        effective(plan, change.id) != InProgress,
        destinationOpen(observation.before, change.parent),
      }
    | _ => true
  }

  // ---- dependency ----

  // invariant: dependency は登録済みの Entity だけを参照し、包含と合わせた通常完了経路が循環しない。
  // 完了した Entity の依存先はすべて完了しており（Reopen の前提がこれを守る）、
  // 完了した Entity 自身の dependency は固定される。自己依存は許可されず、祖先・子孫間の依存は持たない。
  val invDependenciesValid = IDS.forall(id =>
    (plan.registered.contains(id) implies depsOf(plan, id).subseteq(plan.registered))
      and (not(plan.registered.contains(id)) implies depsOf(plan, id).size() == 0))
  val invPreconditionsAcyclic = preconditionsAcyclic(plan)
  val invCompletedPrerequisites = plan.registered.forall(id =>
    storedOf(plan, id) == Completed implies depsComplete(plan, id))
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invCompletedDependenciesFrozen = observation.before.registered.forall(id =>
    storedOf(observation.before, id) == Completed implies depsOf(plan, id) == depsOf(observation.before, id))
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invSelfDependencyRejected = plan.registered.forall(id =>
    not(canAddDependency(plan, id, id)) and not(depsOf(plan, id).contains(id)))
  val invNoAncestorDependencies = plan.registered.forall(id => ancestorsOf(plan, id).forall(a =>
    not(depsOf(plan, id).contains(a)) and not(depsOf(plan, a).contains(id))
      and not(canAddDependency(plan, id, a)) and not(canAddDependency(plan, a, id))))

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
    (not(plan.registered.contains(id)) or terminal(storedOf(plan, id))) implies conditionResult(plan, conditions, id) == NotEvaluated)

  // invariant: 判断候補・着手候補の定義。着手候補は着手可能な Issue に限り、Group は着手候補にならない。
  // 判断候補は着手候補とも着手中とも重ならず、条件入力の変化は着手可否と着手中の集合を変えない。
  val invCandidateDefinition = candidateIssues(plan, conditions) == plan.registered.filter(id => and {
    kindOf(plan, id) == Issue,
    storedOf(plan, id) == NotStarted,
    parentOpen(plan, parentOf(plan, id)),
    ancestorsOf(plan, id).forall(a => storedOf(plan, a) == NotStarted and depsComplete(plan, a) and conditions.get(a)),
    depsComplete(plan, id),
    conditions.get(id),
  })
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invGroupsNotCandidates = groups(plan).forall(g => not(canOperate(plan, g, Start, false)))
    and candidateIssues(plan, conditions).subseteq(startableIssues(plan))
  val invJudgmentDefinition = judgments(plan, conditions) == plan.registered.filter(id => and {
    storedOf(plan, id) == Undecided,
    conditions.get(id),
    ancestorsOf(plan, id).forall(a => not(terminal(storedOf(plan, a))) and conditions.get(a)),
  })
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invJudgmentSeparateFromWork = and {
    judgments(plan, conditions).intersect(candidateIssues(plan, conditions)).size() == 0,
    judgments(plan, conditions).intersect(workingIssues(plan)).size() == 0,
    judgments(plan, conditions).forall(id => effective(plan, id) != InProgress),
  }
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invDependencyEditsPreserveJudgment = match observation.event {
    | AddDependency(_) => judgments(plan, conditions) == judgments(observation.before, observation.beforeConditions)
    | RemoveDependency(_) => judgments(plan, conditions) == judgments(observation.before, observation.beforeConditions)
    | _ => true
  }
  // 構成の確認: 前提・更新の定義から直接従う性質で、定義の書き誤りを検出するために置く。
  val invConditionChangesPreserveEligibility = match observation.event {
    | ConditionChanged(_) => startableIssues(plan) == startableIssues(observation.before)
        and workingIssues(plan) == workingIssues(observation.before)
        and workingGroups(plan) == workingGroups(observation.before)
    | _ => true
  }

  // invariant: 一覧の状況は定義どおりに一つへ決まる。Issue の行は Ready・Blocked・Working のどれかで、
  // Blocked は未着手で浮上しているが着手可能でないもの。Group の行の状況の導出は優先順位の定義と一致し、
  // Confirmable の Group はそのとき（最終確認が通れば）完了でき、Ready の Group には着手候補の子孫がある。
  // 構成の確認: issueStatus・groupStatus の定義を優先順位ごとに書き下したもので、定義の書き誤りを検出するために置く。
  val invIssueStatusDefinition = issueRows(plan, conditions).forall(id => and {
    issueStatus(plan, conditions, id) == Ready iff candidateIssues(plan, conditions).contains(id),
    issueStatus(plan, conditions, id) == Working iff storedOf(plan, id) == InProgress,
    issueStatus(plan, conditions, id) == Blocked iff (storedOf(plan, id) == NotStarted
      and surfaced(plan, conditions, id) and not(startableIssues(plan).contains(id))),
  })
  val invGroupStatusDefinition = groupRows(plan, conditions).forall(g => and {
    groupStatus(plan, conditions, g) == Confirmable iff canOperate(plan, g, Complete, true),
    groupStatus(plan, conditions, g) == Ready iff (not(canOperate(plan, g, Complete, true))
      and hasCandidateDescendant(plan, conditions, g)),
    groupStatus(plan, conditions, g) == Working iff (not(canOperate(plan, g, Complete, true))
      and not(hasCandidateDescendant(plan, conditions, g)) and effective(plan, g) == InProgress),
    groupStatus(plan, conditions, g) == Blocked iff (not(canOperate(plan, g, Complete, true))
      and not(hasCandidateDescendant(plan, conditions, g)) and effective(plan, g) != InProgress
      and children(plan, g).size() > 0),
    groupStatus(plan, conditions, g) == Empty iff (not(canOperate(plan, g, Complete, true))
      and not(hasCandidateDescendant(plan, conditions, g)) and effective(plan, g) != InProgress
      and children(plan, g).size() == 0),
  })
  // Blocked の Group には少なくとも一つ理由がある。
  val invBlockedHasReason = groupRows(plan, conditions).forall(g =>
    groupStatus(plan, conditions, g) == Blocked implies blockReasons(plan, conditions, g).size() > 0)

  // invariant: 未終了（Completed でない登録済み）の各 Entity について、その近傍のいずれかに
  // 実行可能な lifecycle 操作（最終確認は通るとする）か移動か dependency の編集がある。
  // 近傍は自身・祖先・子孫・依存先に、各要素の完了済みの依存元とその祖先を Entity 数だけ繰り返し加えた閉包とする。
  // 完了した祖先の Reopen が完了済みの依存元に止められ、その依存元も祖先の採用を待つように、
  // 完了済みの依存元とその祖先を順に Reopen しないと再開できない場合があるため。
  // 整理・再判断・Reopen も数えるため、当初の計画どおり完了できる意味ではない。
  pure def canActOn(s: PlanState, x: Id): bool = or {
    OPERATIONS.exists(op => canOperate(s, x, op, true)),
    PARENTS.exists(p => canMove(s, x, p)),
    s.registered.exists(t => canAddDependency(s, x, t) or canRemoveDependency(s, x, t)),
  }
  pure def neighborhood(s: PlanState, id: Id): Set[Id] =
    IDS.fold(Set(id).union(ancestorsOf(s, id)).union(descendants(s, id)).union(depsOf(s, id)), (seen, _) =>
      seen.union(seen.map(x => completedDependents(s, x)
        .union(completedDependents(s, x).map(d => ancestorsOf(s, d)).flatten())).flatten()))
  val invNoUnexpectedDeadEnd = plan.registered.forall(id => storedOf(plan, id) != Completed implies
    neighborhood(plan, id).exists(x => canActOn(plan, x)))

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
        and children(plan, change.id).exists(c => storedOf(plan, c) == Completed)
        and effective(plan, change.id) == InProgress
    | _ => false
  }
  val wReopenRejectedByDependent = plan.registered.exists(id => storedOf(plan, id) == Completed
    and parentOpen(plan, parentOf(plan, id)) and completedDependents(plan, id).size() > 0
    and not(canOperate(plan, id, Reopen, true)))
  val wDependencyEditAfterReopen = match observation.event {
    | AddDependency(change) => history.reopenedIssues.contains(change.source)
    | RemoveDependency(change) => history.reopenedIssues.contains(change.source)
    | _ => false
  }

  // witness: Group の完了。空の Group を NotStarted から完了、子がすべて取りやめの Group の完了、
  // 子が完了済みの Group の完了、最終確認待ちとその不合格、子 Group の完了を経た完了。
  pure def completedGroupEvent(o: PlanObservation): Set[Id] = match o.event {
    | LifecycleOperation(change) => if (change.kind == Group and change.op == Complete) Set(change.id) else Set()
    | _ => Set()
  }
  val wEmptyGroupComplete = completedGroupEvent(observation).exists(g =>
    children(observation.before, g).size() == 0 and storedOf(observation.before, g) == NotStarted)
  val wAllCancelledGroupComplete = completedGroupEvent(observation).exists(g =>
    children(plan, g).size() > 0 and children(plan, g).forall(c => storedOf(plan, c) == Cancelled))
  val wWorkingGroupComplete = completedGroupEvent(observation).exists(g => effective(observation.before, g) == InProgress)
  val wParentCompletesAfterGroup = completedGroupEvent(observation).exists(g =>
    children(plan, g).exists(c => kindOf(plan, c) == Group and storedOf(plan, c) == Completed))
  val wAwaitingFinalCheck = groups(plan).exists(g => children(plan, g).size() > 0
    and canOperate(plan, g, Complete, true) and not(canOperate(plan, g, Complete, false)))

  // witness: 着手の前提。祖先の依存先が未完了で子 Issue の着手が拒否され、その依存先の完了で着手可能になる。
  // 親 Group が Undecided のため子 Issue が着手できない。孫 Issue の着手で祖父 Group の実効が InProgress になる。
  // 子が完了済みのため Group の Withdraw が拒否される。
  val wAncestorDependencyBlocksStart = issues(plan).exists(id => storedOf(plan, id) == NotStarted
    and ancestorsAdopted(plan, id) and depsComplete(plan, id)
    and not(ancestorDepsComplete(plan, id)) and not(canOperate(plan, id, Start, false)))
  val wAncestorDependencyCompletionUnblocks = match observation.event {
    | LifecycleOperation(change) => change.op == Complete and issues(plan).exists(id =>
        ancestorsOf(plan, id).exists(a => depsOf(plan, a).contains(change.id))
        and not(canOperate(observation.before, id, Start, false)) and canOperate(plan, id, Start, false))
    | _ => false
  }
  val wUndecidedParentBlocksStart = issues(plan).exists(id => storedOf(plan, id) == NotStarted
    and depsComplete(plan, id)
    and (match parentOf(plan, id) {
      | Unassigned => false
      | InGroup(g) => storedOf(plan, g) == Undecided
    })
    and not(canOperate(plan, id, Start, false)))
  val wGrandchildStartActivatesGrandparent = match observation.event {
    | LifecycleOperation(change) => change.kind == Issue and change.op == Start
        and ancestorsOf(plan, change.id).exists(a => ancestorsOf(plan, a).size() == 0
          and ancestorsOf(plan, change.id).size() >= 2
          and effective(observation.before, a) == NotStarted and effective(plan, a) == InProgress)
    | _ => false
  }
  val wWithdrawRejectedByCompletedChild = groups(plan).exists(g => storedOf(plan, g) == NotStarted
    and parentOpen(plan, parentOf(plan, g))
    and children(plan, g).exists(c => storedOf(plan, c) == Completed)
    and not(canOperate(plan, g, Withdraw, false)))
  val wWithdrawRejectedByWorkingGrandchild = groups(plan).exists(g => storedOf(plan, g) == NotStarted
    and parentOpen(plan, parentOf(plan, g))
    and children(plan, g).exists(c => kindOf(plan, c) == Group and storedOf(plan, c) == NotStarted
      and children(plan, c).exists(i => kindOf(plan, i) == Issue and storedOf(plan, i) == InProgress))
    and not(canOperate(plan, g, Withdraw, false)))
  val wPrerequisiteCompletionUnblocksStart = match observation.event {
    | LifecycleOperation(change) => change.op == Complete and issues(plan).exists(id =>
        depsOf(plan, id).contains(change.id)
        and not(canOperate(observation.before, id, Start, false)) and canOperate(plan, id, Start, false))
    | _ => false
  }
  val wStartUnderUndecidedGrandparentRejected = issues(plan).exists(id => storedOf(plan, id) == NotStarted
    and ancestorsOf(plan, id).exists(a => storedOf(plan, a) == Undecided)
    and (match parentOf(plan, id) { | Unassigned => false | InGroup(g) => storedOf(plan, g) == NotStarted })
    and not(canOperate(plan, id, Start, false)))

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
      | InGroup(g) => history.convertedToGroup.contains(g) and kindOf(plan, g) == Group
    }
    | _ => false
  }
  val wWorkingIssueNotConvertible = match observation.event {
    | LifecycleOperation(change) => change.kind == Issue and change.op == Start
        and canConvertToGroup(observation.before, change.id) and not(canConvertToGroup(plan, change.id))
    | _ => false
  }
  val wConvertAdoptedWithDependency = match observation.event {
    | ConvertedToGroup(id) => storedOf(plan, id) == NotStarted and depsOf(plan, id).size() > 0
    | ConvertedToIssue(id) => storedOf(plan, id) == NotStarted and depsOf(plan, id).size() > 0
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
    | Move(change) => kindOf(plan, change.id) == Issue and storedOf(plan, change.id) == InProgress
        and change.parent != Unassigned
    | _ => false
  }
  val wWorkingGroupMove = match observation.event {
    | Move(change) => kindOf(plan, change.id) == Group and effective(plan, change.id) == InProgress
        and change.parent != Unassigned
    | _ => false
  }
  val wWorkingMoveRejectedByUndecided = plan.registered.exists(id => effective(plan, id) == InProgress
    and groups(plan).exists(g => parentOf(plan, id) != InGroup(g) and destinationOpen(plan, InGroup(g))
      and not(destinationAdopted(plan, InGroup(g))) and not(canMove(plan, id, InGroup(g)))))
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
  val wRegisterIntoClosedRejected = groups(plan).exists(g => terminal(storedOf(plan, g))
    and IDS.exists(id => not(plan.registered.contains(id))
      and REGISTRATION_LIFECYCLES.forall(lc => not(canRegister(plan, id, lc, InGroup(g))))))
  val wMoveIntoClosedRejected = groups(plan).exists(g => terminal(storedOf(plan, g))
    and plan.registered.exists(id => id != g and parentOf(plan, id) != InGroup(g)
      and parentOpen(plan, parentOf(plan, id)) and not(ancestorsOf(plan, g).contains(id))
      and preconditionsAcyclic(applyMove(plan, id, InGroup(g)))
      and not(canMove(plan, id, InGroup(g)))))
  val wReconsiderAllowsMembership = match observation.event {
    | LifecycleOperation(change) => change.kind == Group and change.op == Reconsider and IDS.exists(id => or {
        not(canMove(observation.before, id, InGroup(change.id))) and canMove(plan, id, InGroup(change.id)),
        REGISTRATION_LIFECYCLES.exists(lc => not(canRegister(observation.before, id, lc, InGroup(change.id)))
          and canRegister(plan, id, lc, InGroup(change.id))),
      })
    | _ => false
  }
  val wReopenAllowsMembership = match observation.event {
    | LifecycleOperation(change) => change.kind == Group and change.op == Reopen
        and plan.registered.exists(id => canMove(plan, id, InGroup(change.id))
          or (parentOf(plan, id) == InGroup(change.id) and canMove(plan, id, Unassigned)))
    | _ => false
  }
  val wThreeLevels = plan.registered.exists(id => ancestorsOf(plan, id).size() >= 2)
  val wMixedChildren = groups(plan).exists(g => children(plan, g).exists(c => kindOf(plan, c) == Issue)
    and children(plan, g).exists(c => kindOf(plan, c) == Group))
  val wIndirectContainmentCycleRejected = groups(plan).exists(g => descendants(plan, g).exists(d =>
    parentOf(plan, d) != InGroup(g) and kindOf(plan, d) == Group and not(canMove(plan, g, InGroup(d)))))

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
    | AddDependency(change) => kindOf(plan, change.source) == Group and kindOf(plan, change.target) == Issue
    | _ => false
  }
  val wIssueDependsOnGroup = match observation.event {
    | AddDependency(change) => kindOf(plan, change.source) == Issue and kindOf(plan, change.target) == Group
    | _ => false
  }
  val wDirectCycleRejected = plan.registered.exists(source => plan.registered.exists(target =>
    storedOf(plan, source) != Completed and depsOf(plan, target).contains(source)
      and not(canAddDependency(plan, source, target))))
  val wAncestorDependencyRejected = issues(plan).exists(id => storedOf(plan, id) == NotStarted
    and parentOf(plan, id) != Unassigned
    and ancestorsOf(plan, id).exists(a => not(depsOf(plan, id).contains(a)) and not(canAddDependency(plan, id, a))))
  val wIndirectCycleRejected = plan.registered.exists(source => plan.registered.exists(mid =>
    plan.registered.exists(target => and {
      source != mid, mid != target, source != target,
      depsOf(plan, mid).contains(source), depsOf(plan, target).contains(mid),
      storedOf(plan, source) != Completed, not(depsOf(plan, source).contains(target)),
      not(canAddDependency(plan, source, target)),
    })))
  val wAddDuringWorkBlocksCompletion = match observation.event {
    | AddDependency(change) => kindOf(plan, change.source) == Issue and storedOf(plan, change.source) == InProgress
        and canOperate(observation.before, change.source, Complete, false)
        and not(canOperate(plan, change.source, Complete, false))
    | _ => false
  }

  // witness: 候補と一覧。判断候補と着手候補、浮上していない Issue への ID を明示した着手、Group の五つの状況、
  // Ready と Working が同時に成り立つ Group で Ready が選ばれること、Issue の三つの状況。
  val wJudgment = judgments(plan, conditions).size() > 0
  val wCandidateIssue = candidateIssues(plan, conditions).size() > 0
  val wStartHiddenIssue = match observation.event {
    | LifecycleOperation(change) => change.kind == Issue and change.op == Start
        and not(surfaced(observation.before, observation.beforeConditions, change.id))
    | _ => false
  }
  def sawGroupStatus(status: TaskStatus): bool =
    groupRows(plan, conditions).exists(g => groupStatus(plan, conditions, g) == status)
  val wGroupConfirmable = sawGroupStatus(Confirmable)
  val wGroupReady = sawGroupStatus(Ready)
  val wGroupWorking = sawGroupStatus(Working)
  val wGroupBlocked = sawGroupStatus(Blocked)
  val wGroupEmpty = sawGroupStatus(Empty)
  val wReadyOverWorking = groupRows(plan, conditions).exists(g =>
    groupStatus(plan, conditions, g) == Ready and effective(plan, g) == InProgress)
  val wConfirmableOverWorking = groupRows(plan, conditions).exists(g =>
    groupStatus(plan, conditions, g) == Confirmable and effective(plan, g) == InProgress)
  val wWorkingGroupRowHidden = groupRows(plan, conditions).exists(g =>
    effective(plan, g) == InProgress and not(surfaced(plan, conditions, g)))
  def sawIssueStatus(status: TaskStatus): bool =
    issueRows(plan, conditions).exists(id => issueStatus(plan, conditions, id) == status)
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

  // witness: Blocked の理由の内訳ごとの観測。
  def sawBlockedBy(reason: BlockReason): bool = groupRows(plan, conditions).exists(g =>
    groupStatus(plan, conditions, g) == Blocked and blockReasons(plan, conditions, g).contains(reason))
  val wBlockedByUndecidedChildren = sawBlockedBy(UndecidedChild)
  val wBlockedByDependency = sawBlockedBy(DependencyPending)
  val wBlockedBySubgroup = sawBlockedBy(OpenSubgroup)
  val wBlockedByHiddenCandidate = sawBlockedBy(HiddenCandidate)
  val wBlockedByUnadoptedAncestor = sawBlockedBy(UnadoptedAncestor)
  // 理由のどれもない Blocked の Group（invBlockedHasReason の反例）。観測されないことを確かめる。
  val wBlockedWithoutListedReason = groupRows(plan, conditions).exists(g =>
    groupStatus(plan, conditions, g) == Blocked and blockReasons(plan, conditions, g).size() == 0)

  // witness: 実効が InProgress の Group が、stored が NotStarted でない祖先を持つ状態
  // （invEffectiveWorkingAncestors の反例）と、そこへ至る経路。Group の完了、完了済みの Entity の移動、
  // 着手・完了した子孫を持つ Group の採用・Reopen はどれも前提で塞いでおり、観測されないことを確かめる。
  pure def effectiveUnderUnadopted(s: PlanState): Set[Id] = groups(s).filter(g =>
    effective(s, g) == InProgress and not(ancestorsAdopted(s, g)))
  val wEffectiveWorkingUnderUnadopted = effectiveUnderUnadopted(plan).size() > 0
  val wUnadoptedViaComplete = effectiveUnderUnadopted(plan).size() > 0
    and effectiveUnderUnadopted(observation.before).size() == 0
    and (match observation.event {
      | LifecycleOperation(change) => change.op == Complete
      | _ => false
    })
  val wUnadoptedViaMove = effectiveUnderUnadopted(plan).size() > 0
    and effectiveUnderUnadopted(observation.before).size() == 0
    and (match observation.event {
      | Move(change) => storedOf(plan, change.id) == Completed or effective(plan, change.id) == InProgress
      | _ => false
    })
  val wUnadoptedViaAcceptOrReopen = effectiveUnderUnadopted(plan).size() > 0
    and effectiveUnderUnadopted(observation.before).size() == 0
    and (match observation.event {
      | LifecycleOperation(change) => change.op == Accept or change.op == Reopen
      | _ => false
    })
}
