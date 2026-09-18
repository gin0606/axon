// 計画（Group）と Issue の包含、Issue・計画間の dependency、再浮上による候補の選別を扱う統合モデル。
// 計画3件・Issue 用の固定 ID 3件の範囲で探索する。未登録の枠は探索領域を有限にする仕組みであって、
// 新しい lifecycle ではない。計画自身の登録操作と ID 発行は扱わない。
// 対象外: 文面・Note・状態変更履歴（lifecycle_information）、条件の種類・設定と評価失敗
// （candidate_evaluation）、CLI と表示、永続化。
// 基本遷移は lifecycle_rules を import し、到達しにくい経路は lifecycle_reachability が補う。
// 対応する契約: ../docs/reference/lifecycle.md
module group_lifecycle {
  import lifecycle_rules.* from "lifecycle_rules"

  // 所属は Issue・計画とも最大一つで、所属なしも許す。
  // 計画も最大一つの親計画を持ち、Issue と子計画を同じ階層に置ける。
  type Id = int
  type Parent = Unassigned | InGroup(Id)
  type Entity = IssueRef(Id) | GroupRef(Id)

  // 着手・完了・取りやめが要求する前提の種類。ChildEnded は完了・取りやめのどちらでも満たす。
  // 新規登録には、未判断の提案と採用済みの仕事の2経路がある。
  type Requirement = ParentInProgress(Id) | EntityCompleted(Entity) | ChildEnded(Entity)
  type Registration = Proposal | AdoptedWork
  pure val GROUPS = Set(0, 1, 2)
  pure val ISSUES = Set(0, 1, 2)
  pure val REFS = ISSUES.map(IssueRef).union(GROUPS.map(GroupRef))
  pure val PARENTS = Set(Unassigned, InGroup(0), InGroup(1), InGroup(2))
  pure val OPERATIONS = Set(Accept, Withdraw, Start, Release, Complete, Cancel, Reconsider)

  // 保存する状態は lifecycle、登録済みの枠、包含、dependency。
  // conditions は各計画・Issue の再浮上条件の成立・不成立を表す外部入力で、
  // 条件の種類や設定操作はこのモデルでも抽象化する。
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

  // observation は直前の状態と操作の検証専用の記録であり、製品の保存項目ではない。
  // reviewPassed は計画全体の最終確認が通ったという抽象入力。
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
  pure def terminal(current: Lifecycle): bool = Set(Completed, Cancelled).contains(current)

  // 包含の導出。所属から直属の子、祖先、子孫をたどり、
  // 計画の子がすべて終了しているか、進行中の子がいるかを判定する。
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

  // 親が終了していないか、進行中かの判定。所属なしはどちらも通る。
  pure def parentOpen(s: PlanState, parent: Parent): bool = match parent {
    | Unassigned => true
    | InGroup(group) => not(terminal(s.groups.get(group)))
  }
  pure def parentWorking(s: PlanState, parent: Parent): bool = match parent {
    | Unassigned => true
    | InGroup(group) => s.groups.get(group) == InProgress
  }

  // Issue と計画を Entity として同じ形で扱うための導出。
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

  // 操作ごとの前提。着手は親があれば現在進行中であること、着手と完了は全依存先の完了、
  // 完了と取りやめは計画なら直属の子がすべて終了していることを要求する。
  // 親の条件は過去の着手履歴ではなく現在の進行中を見る。
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

  // 構造の循環検査には「全員が通常どおり完了する経路」を使う。着手の節点を親へさかのぼって
  // 展開すると、各 Entity の完了の前提は「直属の子 ∪ 自身の依存先 ∪ 全祖先の依存先」へ縮約できる。
  // このグラフから前提の残っていない節点を順に除去し、全節点を除去できるかを検査する。
  // 取りやめによる回避はこの経路に加えない。保存する関係は包含と明示的な dependency のままで、
  // 実際の操作可否には現在の状態を見る requirements 側を使う。
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

  // 計画の操作。基本遷移に加えて、終了した親の配下を変更しないこと、前提の充足、
  // 進行中の子がいる間の Release の禁止、完了時の最終確認を要求する。
  // 最終確認は Complete の実行自体を明示入力として扱い、別のレビュー済み状態や必須フラグを持たない。
  // 空の計画でもこの確認を省略しない。
  pure def canGroup(s: PlanState, id: Id, op: Operation, passed: bool): bool = and {
    canPerform(s.groups.get(id), op),
    parentOpen(s, s.groupParents.get(id)),
    requirementsSatisfied(s, GroupRef(id), op),
    op != Release or not(hasWorkingChild(s, id)),
    op != Complete or passed,
  }
  pure def applyGroup(s: PlanState, id: Id, op: Operation): PlanState =
    { ...s, groups: s.groups.set(id, applyOperation(s.groups.get(id), op)) }

  // Issue の操作。登録済みであることに加えて、計画と同じ前提を要求する。
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

  // 2 Issue が計画0に所属し、1枠を未登録として残す。計画はすべて所属なしで、
  // 全 Entity は Undecided、条件入力はすべて不成立から始める。
  // これは探索の開始点であって、製品の再浮上条件の初期値を決めるものではない。
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

  // 各操作は前提を満たすときだけ実行し、他の Entity を自動で更新しない。
  // 子への着手で親を進行中にすることも、子の終了で親を終了させることもしない。
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

  // 所属変更。移動元・移動先の計画がどちらも終了していないことを求め、進行中の Entity は
  // 所属なしか進行中の計画へだけ移せる。移動は lifecycle を変えず、
  // 変更後の通常完了経路が循環する移動は拒否する。
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

  // 計画の移動はその計画の親だけを付け替え、配下ごと部分木全体が移る。
  // 自己包含と、子孫の下への移動による循環を禁じる。
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

  // 未登録の枠を計画内または計画外へ登録する。提案は Undecided、採用済みの仕事は NotStarted とし、
  // どちらも登録だけでは進行中にならない。登録は与えられた採否を反映し、採用判断は代行しない。
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

  // dependency の編集。依存元が完了していればその dependency は固定する。
  // 自己依存と、包含を合わせた通常完了経路の循環は追加時に拒否する。
  // 進行中でも未完了の依存先を追加でき、lifecycle と所属は変わらない。
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

  // 再浮上条件の評価。取りやめ・完了の Entity と未登録の枠は評価せず、浮上しない。
  // 評価結果は保存せず、入力と状態から導出する。
  pure def groupConditionResult(s: PlanState, c: Conditions, id: Id): ConditionResult =
    if (terminal(s.groups.get(id))) NotEvaluated else Evaluated(c.groups.get(id))
  pure def issueConditionResult(s: PlanState, c: Conditions, id: Id): ConditionResult =
    if (not(s.registered.contains(id)) or terminal(s.issues.get(id))) NotEvaluated
    else Evaluated(c.issues.get(id))
  pure def parentSurfaced(s: PlanState, c: Conditions, parent: Parent): bool =
    ancestorGroups(s, parent).forall(id => groupConditionResult(s, c, id) == Evaluated(true))

  // 判断候補は未判断で、自身とすべての祖先が浮上しているもの。
  // 採否を検討するための集合なので、親の着手や依存先の完了は要求しない。
  pure def judgmentIssues(s: PlanState, c: Conditions): Set[Id] =
    s.registered.filter(id => s.issues.get(id) == Undecided
      and issueConditionResult(s, c, id) == Evaluated(true)
      and parentSurfaced(s, c, s.parents.get(id)))
  pure def judgmentGroups(s: PlanState, c: Conditions): Set[Id] =
    GROUPS.filter(id => s.groups.get(id) == Undecided
      and groupConditionResult(s, c, id) == Evaluated(true)
      and parentSurfaced(s, c, s.groupParents.get(id)))

  // 着手可能は前提を満たすもの、着手候補はそのうち自身と全祖先が浮上しているもの、
  // 着手中は浮上に関係なく進行中のもの。再浮上は候補の表示だけを制御し、
  // 明示的な採否判断・着手・完了・所属や依存の変更の可否には加えない。
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

  // 条件入力は登録済みの Issue と計画について両方向へ変化し、保存する状態を変えない。
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

  // 基本遷移、所属変更、新規登録、dependency の編集、条件入力の変化を混ぜて探索する。
  // 計画の lifecycle、Issue の lifecycle、それ以外の三枝に分け、
  // 前提を満たして着手・完了へ進む経路も探索しやすくする。
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

  // witness: 計画・Issue の7操作、同じ計画での採否の混在、親の着手待ち、
  // 進行中の子による解放の阻害、最終確認待ちと不合格、取りやめた子を含む完了、
  // 子が全員完了した完了、未判断の子による終了の阻害。
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

  // witness: 所属の付け外しと計画間の移動、進行中のままの移動、2経路の新規登録、
  // 採用済み登録の着手待ちと着手可能、最終確認待ちの計画への追加、
  // 再検討による構成変更の再開、未判断を計画外へ出して完了可能になる場合。
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

  // invariant: 進行中の子の親は進行中で、終了した計画の子はすべて終了している。
  // 完了は戻らず、終了した計画の構成は固定され、計画の完了は最終確認を経た明示操作でのみ起きる。
  // 各操作は対象以外の Entity を自動更新しない。
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

  // invariant: dependency は登録済みの Issue・計画だけを参照し、包含と合わせた通常完了経路が
  // 循環しない。着手・完了の時点で全依存先が完了しており、完了後もその前提は満たされたまま、
  // 完了した Entity 自身の dependency は固定される。
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

  // witness: 追加と削除、着手後の追加による完了の阻害、計画をまたぐ依存、完了済みへの依存、
  // 移動後の依存の保持、取りやめた依存先による阻害、依存先の完了による着手・完了の解禁、
  // 直接・間接の循環の拒否。
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

  // invariant: 自己依存は許可されず、親・祖先と子孫の間の明示的な dependency は
  // どちら向きも持たない。
  val invSelfDependencyRejected = entities(plan).forall(entity => not(canAddDependency(plan, entity, entity)))

  val invNoAncestorDependencies = entities(plan).forall(entity =>
    ancestorGroups(plan, entityParent(plan, entity)).forall(g =>
      not(plan.dependencies.get(entity).contains(GroupRef(g)))
        and not(plan.dependencies.get(GroupRef(g)).contains(entity))))

  // witness: 計画を含む依存の3組み合わせ、依存先の計画の最終確認待ち、
  // 計画の完了による後続の着手の解禁、着手後の前提追加、依存に反する所属変更の拒否、
  // 取りやめても残る循環と、別の枝を経由する循環の拒否。
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

  // invariant: 条件入力の変化は対象の Entity の入力だけを変え、明示操作は条件入力を変えない。
  val invConditionInputScope = match observation.event {
    | IssueConditionChanged(changed) => conditions.groups == observation.beforeConditions.groups
        and ISSUES.exclude(Set(changed)).forall(id => conditions.issues.get(id) == observation.beforeConditions.issues.get(id))
    | GroupConditionChanged(changed) => conditions.issues == observation.beforeConditions.issues
        and GROUPS.exclude(Set(changed)).forall(id => conditions.groups.get(id) == observation.beforeConditions.groups.get(id))
    | _ => conditions == observation.beforeConditions
  }

  // witness: 計画・Issue の条件の成立と不成立、両方向の変化。
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

  // invariant: 未登録・取りやめ・完了は条件を評価しない。着手候補は着手可能なものに限り、
  // 判断候補と着手候補の定義が、状態・親の着手・依存先の完了と、
  // 自身および全祖先の浮上条件の組み合わせに一致する。
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

  // invariant: 判断候補は着手候補とも着手中とも重ならず、dependency の編集は判断候補を変えない。
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

  // witness: 親が未着手の判断候補、依存先が未完了の判断候補、親の浮上による除外と復帰、
  // 親が非浮上でも明示的に採用できること、依存先の完了前にも採用できること。
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

  // invariant: 着手中は進行中の Entity と一致し、条件入力の変化は着手可否と着手中の集合を変えない。
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

  // witness: 着手候補の成立、非浮上のままの明示的な着手、親の浮上による候補の除外と復帰、
  // 親や自身が非浮上でも進行中の仕事が残ること、完了・取りやめの条件が評価されないこと。
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

  // invariant: 未登録の枠が計画に混入せず、所属は高々一つで包含は循環しない。
  // 新規登録は与えられた採否に対応する初期状態になり、自動で着手しない。
  // 終了した計画への追加と取り外しはどちらも許可されず、進行中の Entity の祖先はすべて進行中、
  // 終了した計画の子孫はすべて終了している。計画の移動はその子孫の集合と内部の所属・状態を変えない。
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

  // witness: 計画の付け外しと計画間の移動、進行中の部分木の移動、三階層の包含、
  // Issue と子計画の混在、入れ子の Issue の着手、子計画が親の着手・解放・終了に与える制約、
  // 入れ子の計画の取りやめ、間接的な包含循環の拒否。
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

  // invariant: 未終了の仕事や再検討可能な計画が残っている間、全 lifecycle 操作が行き止まりにならない。
  // 整理・再判断の操作も数えるため、当初の計画どおり必ず完了できるという意味ではない。
  val invNoUnexpectedDeadEnd =
    (GROUPS.exists(g => plan.groups.get(g) != Completed
      and ancestorGroups(plan, plan.groupParents.get(g)).forall(a => plan.groups.get(a) != Completed))
      or plan.registered.exists(id => not(terminal(plan.issues.get(id))))) implies or {
        GROUPS.exists(g => OPERATIONS.exists(op => canGroup(plan, g, op, true))),
        plan.registered.exists(id => OPERATIONS.exists(op => canIssue(plan, id, op))),
      }
}
