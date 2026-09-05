// docs/reference/state-model.md で確定した、明示的な計画 group の拡張モデル。
// ID 文字列、文面、永続化、履歴、claim、CLI 表示はモデルの対象外とする。
// コア module を拡張せず、A / B / C の型をこの model 内で再掲する。
module group_plan {
  type EntityKind = Issue | Group
  type Progress = NotStarted | InProgress | Ended
  type Disposition = Undecided | Accepted | Rejected

  type Id = int
  type ResurfaceCondition =
    | Always
    | AtDate(int)
    | AfterEntity(Id)
    | Command
    | Manual

  type Entity = {
    kind: EntityKind,
    progress: Progress,
    disposition: Disposition,
    resurface: ResurfaceCondition,
  }

  type State = {
    entities: Id -> Entity,
    parent: Id -> Id,
    dependencies: Set[(Id, Id)],
    clock: int,
    // 外部観測入力。Entity の Control state ではない。
    commandSatisfied: Id -> bool,
  }

  // Witness の action coverage だけに使う ghost state。axon が保存する状態ではない。
  type Operation =
    | DecideOp
    | StartOp
    | DoneIssueOp
    | DoneGroupOp
    | ReleaseIssueOp
    | ReleaseGroupOp
    | SetDateOp
    | SetAfterOp
    | ClearWhenOp
    | SetParentOp
    | UnsetParentOp
    | AddDependencyOp
    | RemoveDependencyOp
    | TickOp
    | SetManualOp
    | SetCommandOp
    | ObserveCommandOp
    | CommandGateClosedOp

  pure val ISSUES: Set[Id] = Set(0, 1, 2)
  pure val GROUPS: Set[Id] = Set(3, 4, 5)
  pure val ENTITIES: Set[Id] = ISSUES.union(GROUPS)
  pure val NO_PARENT: Id = -1
  pure val CLOCK_MAX = 3
  pure val DATES = Set(1, 2, 3)
  pure val DISPOSITIONS = Set(Undecided, Accepted, Rejected)

  pure def initialKind(i: Id): EntityKind = if (ISSUES.contains(i)) Issue else Group

  pure def initialEntity(i: Id): Entity = {
    kind: initialKind(i),
    progress: NotStarted,
    disposition: Accepted,
    resurface: Always,
  }

  // Group 3 は issue 2 件と子 group 4 を持つ。group 4 は issue 1 件を持つ。
  // Group 5 は空 group の明示完了・release を実行できるよう root に置く。
  pure val INITIAL_PARENT: Id -> Id = Map(
    0 -> 3,
    1 -> 3,
    2 -> 4,
    3 -> NO_PARENT,
    4 -> 3,
    5 -> NO_PARENT,
  )

  pure val INITIAL_STATE: State = {
    entities: ENTITIES.mapBy(i => initialEntity(i)),
    parent: INITIAL_PARENT,
    dependencies: Set(),
    clock: 0,
    commandSatisfied: ENTITIES.mapBy(_ => false),
  }

  // ---- Pure domain logic ----

  pure def entity(s: State, i: Id): Entity = s.entities.get(i)
  pure def isIssue(s: State, i: Id): bool = entity(s, i).kind == Issue
  pure def isGroup(s: State, i: Id): bool = entity(s, i).kind == Group
  pure def isEnded(s: State, i: Id): bool = entity(s, i).progress == Ended
  pure def isRejected(s: State, i: Id): bool = entity(s, i).disposition == Rejected
  pure def terminal(s: State, i: Id): bool = isEnded(s, i) or isRejected(s, i)

  pure def directTargets(edges: Set[(Id, Id)], source: Id): Set[Id] =
    ENTITIES.filter(target => edges.contains((source, target)))

  // 固定した Entity 数ぶん到達集合を展開する。
  pure def reach1(edges: Set[(Id, Id)], source: Id): Set[Id] =
    directTargets(edges, source)
  pure def reach2(edges: Set[(Id, Id)], source: Id): Set[Id] =
    reach1(edges, source)
      .union(reach1(edges, source).map(i => reach1(edges, i)).flatten())
  pure def reach3(edges: Set[(Id, Id)], source: Id): Set[Id] =
    reach2(edges, source)
      .union(reach2(edges, source).map(i => reach1(edges, i)).flatten())
  pure def reach4(edges: Set[(Id, Id)], source: Id): Set[Id] =
    reach3(edges, source)
      .union(reach3(edges, source).map(i => reach1(edges, i)).flatten())
  pure def reach5(edges: Set[(Id, Id)], source: Id): Set[Id] =
    reach4(edges, source)
      .union(reach4(edges, source).map(i => reach1(edges, i)).flatten())
  pure def reach6(edges: Set[(Id, Id)], source: Id): Set[Id] =
    reach5(edges, source)
      .union(reach5(edges, source).map(i => reach1(edges, i)).flatten())

  pure def acyclic(edges: Set[(Id, Id)]): bool =
    ENTITIES.forall(i => not(reach6(edges, i).contains(i)))

  pure def parentEdges(s: State): Set[(Id, Id)] =
    ENTITIES
      .filter(child => s.parent.get(child) != NO_PARENT)
      .map(child => (child, s.parent.get(child)))

  pure def childEdges(s: State): Set[(Id, Id)] =
    parentEdges(s).map(edge => (edge._2, edge._1))

  pure def ancestors(s: State, i: Id): Set[Id] = reach6(parentEdges(s), i)
  pure def descendants(s: State, i: Id): Set[Id] = reach6(childEdges(s), i)

  pure def directDependencyTargets(s: State, source: Id): Set[Id] =
    directTargets(s.dependencies, source)

  // Group を依存元にした関係は、その group 自身と全子孫へ効く。
  pure def waitingScope(s: State, source: Id): Set[Id] =
    if (isGroup(s, source)) Set(source).union(descendants(s, source)) else Set(source)

  pure def waitingSources(s: State, i: Id): Set[Id] = Set(i).union(ancestors(s, i))

  pure def dependencyTargets(s: State, i: Id): Set[Id] =
    waitingSources(s, i)
      .map(source => directDependencyTargets(s, source))
      .flatten()

  pure def orphaned(s: State, i: Id): bool =
    dependencyTargets(s, i).exists(target => isRejected(s, target))

  pure def blocked(s: State, i: Id): bool =
    dependencyTargets(s, i).exists(target => not(terminal(s, target)))

  pure def surfaced(s: State, i: Id): bool =
    match entity(s, i).resurface {
    | Always => true
    | Manual => false
    | Command => s.commandSatisfied.get(i)
    | AtDate(date) => date <= s.clock
    | AfterEntity(target) => terminal(s, target)
    }

  // Group はこの条件を満たす間だけ、子孫を active scope に入れる。
  pure def opensDescendants(s: State, group: Id): bool = and {
    entity(s, group).progress == InProgress,
    entity(s, group).disposition == Accepted,
    surfaced(s, group),
    not(blocked(s, group)),
    not(orphaned(s, group)),
  }

  // Root Entity は常に active scope 内。Entity 自身の状態は ready / triage が別に判定する。
  pure def withinActiveScope(s: State, i: Id): bool =
    ancestors(s, i).forall(group => opensDescendants(s, group))

  pure def ready(s: State, i: Id): bool = and {
    entity(s, i).progress == NotStarted,
    entity(s, i).disposition == Accepted,
    surfaced(s, i),
    withinActiveScope(s, i),
    not(blocked(s, i)),
    not(orphaned(s, i)),
  }

  pure def triage(s: State, i: Id): bool = and {
    not(terminal(s, i)),
    surfaced(s, i),
    withinActiveScope(s, i),
    entity(s, i).disposition == Undecided or orphaned(s, i),
  }

  // Dependency の鎖だけを遡る。包含による非 active の理由は別の導出値として扱い、
  // active scope 外の Entity に到達したら、その Entity 自身を blocking cause とする。
  pure def unresolvedTargets(s: State, i: Id): Set[Id] =
    dependencyTargets(s, i).filter(target => not(terminal(s, target)))

  pure def isBlockingRootCause(s: State, i: Id): bool = or {
    orphaned(s, i),
    not(surfaced(s, i)),
    not(withinActiveScope(s, i)),
    unresolvedTargets(s, i) == Set(),
  }

  pure def unresolvedDependencyEdges(s: State): Set[(Id, Id)] =
    ENTITIES
      .map(i => unresolvedTargets(s, i).map(target => (i, target)))
      .flatten()

  pure def blockingCauses(s: State, i: Id): Set[Id] =
    reach6(unresolvedDependencyEdges(s), i)
      .filter(target => isBlockingRootCause(s, target))

  pure def groupCompletionSatisfied(s: State, group: Id): bool =
    descendants(s, group).forall(i => terminal(s, i))

  pure def canCompleteGroup(s: State, group: Id): bool = and {
    isGroup(s, group),
    entity(s, group).progress == InProgress,
    groupCompletionSatisfied(s, group),
  }

  pure def hasAfterEntity(s: State, i: Id): bool =
    match entity(s, i).resurface {
    | AfterEntity(_) => true
    | _ => false
    }

  pure def afterTarget(s: State, i: Id): Id =
    match entity(s, i).resurface {
    | AfterEntity(target) => target
    | _ => i
    }

  pure def expandedDependencyEdges(s: State): Set[(Id, Id)] =
    s.dependencies
      .map(edge => waitingScope(s, edge._1).map(i => (i, edge._2)))
      .flatten()

  pure def expandedAfterEdges(s: State): Set[(Id, Id)] =
    ENTITIES
      .filter(i => hasAfterEntity(s, i))
      .map(i => waitingScope(s, i).map(member => (member, afterTarget(s, i))))
      .flatten()

  pure def liveEdges(s: State, edges: Set[(Id, Id)]): Set[(Id, Id)] =
    edges.filter(edge => not(isEnded(s, edge._1)) and not(isEnded(s, edge._2)))

  // 包含は start 前には child -> parent、start 後の完了時には parent -> child の待ちになる。
  // 両方の phase projection を非循環にし、状態遷移で顕在化する循環も入力時に拒否する。
  pure def activationWaitEdges(s: State): Set[(Id, Id)] =
    liveEdges(s, expandedDependencyEdges(s).union(expandedAfterEdges(s)).union(parentEdges(s)))

  pure def completionWaitEdges(s: State): Set[(Id, Id)] =
    liveEdges(s, expandedDependencyEdges(s).union(expandedAfterEdges(s)).union(childEdges(s)))

  pure def validParents(s: State): bool =
    ENTITIES.forall(i =>
      s.parent.get(i) == NO_PARENT or GROUPS.contains(s.parent.get(i)))

  pure def relationsSafe(s: State): bool = and {
    validParents(s),
    acyclic(activationWaitEdges(s)),
    acyclic(completionWaitEdges(s)),
  }

  pure def hasEndedAncestor(s: State, i: Id): bool =
    ancestors(s, i).exists(group => isEnded(s, group))

  pure def lifecycleStructureSafe(s: State): bool = and {
    GROUPS.forall(group =>
      isEnded(s, group) implies groupCompletionSatisfied(s, group)),
    GROUPS.forall(group =>
      entity(s, group).progress == NotStarted implies
        descendants(s, group).forall(i => entity(s, i).progress != InProgress)),
  }

  pure def updateEntity(s: State, i: Id, updated: Entity): State =
    {...s, entities: s.entities.set(i, updated)}

  pure def applyDecide(s: State, i: Id, choice: Disposition): State = {
    val current = entity(s, i)
    updateEntity(s, i, {...current, disposition: choice})
  }

  pure def canDecide(s: State, i: Id, choice: Disposition): bool = {
    val updated = applyDecide(s, i, choice)
    and {
      entity(s, i).disposition != choice,
      not(hasEndedAncestor(s, i)) or terminal(updated, i),
    }
  }

  pure def applyStart(s: State, i: Id): State = {
    val current = entity(s, i)
    updateEntity(s, i, {...current, progress: InProgress})
  }

  pure def applyDone(s: State, i: Id): State = {
    val current = entity(s, i)
    updateEntity(s, i, {...current, progress: Ended})
  }

  pure def canCompleteIssue(s: State, i: Id): bool = and {
    isIssue(s, i),
    entity(s, i).progress == InProgress,
  }

  pure def applyRelease(s: State, i: Id): State = {
    val current = entity(s, i)
    updateEntity(s, i, {...current, progress: NotStarted})
  }

  pure def canReleaseIssue(s: State, i: Id): bool = and {
    isIssue(s, i),
    entity(s, i).progress == InProgress,
  }

  pure def canReleaseGroup(s: State, group: Id): bool = and {
    isGroup(s, group),
    entity(s, group).progress == InProgress,
    descendants(s, group).forall(i => entity(s, i).progress != InProgress),
  }

  pure def applyResurface(s: State, i: Id, condition: ResurfaceCondition): State = {
    val current = entity(s, i)
    updateEntity(s, i, {...current, resurface: condition})
  }

  pure def canSetDate(s: State, i: Id, date: int): bool =
    entity(s, i).resurface != AtDate(date)

  pure def canSetAfter(s: State, i: Id, target: Id): bool = {
    val updated = applyResurface(s, i, AfterEntity(target))
    and {
      entity(s, i).resurface != AfterEntity(target),
      relationsSafe(updated),
    }
  }

  pure def canClearWhen(s: State, i: Id): bool =
    entity(s, i).resurface != Always

  pure def structureMutable(s: State, i: Id, nextParent: Id): bool = and {
    not(isGroup(s, i) and isEnded(s, i)),
    not(hasEndedAncestor(s, i)),
    nextParent == NO_PARENT or and {
      not(isEnded(s, nextParent)),
      not(hasEndedAncestor(s, nextParent)),
    },
  }

  pure def applyParent(s: State, i: Id, group: Id): State =
    {...s, parent: s.parent.set(i, group)}

  pure def canSetParent(s: State, i: Id, group: Id): bool = {
    val updated = applyParent(s, i, group)
    and {
      isGroup(s, group),
      s.parent.get(i) != group,
      structureMutable(s, i, group),
      relationsSafe(updated),
      lifecycleStructureSafe(updated),
    }
  }

  pure def canUnsetParent(s: State, i: Id): bool = {
    val updated = applyParent(s, i, NO_PARENT)
    and {
      s.parent.get(i) != NO_PARENT,
      structureMutable(s, i, NO_PARENT),
      relationsSafe(updated),
      lifecycleStructureSafe(updated),
    }
  }

  pure def dependencyMutable(s: State, source: Id): bool =
    not(isGroup(s, source) and isEnded(s, source))

  pure def applyAddDependency(s: State, source: Id, target: Id): State =
    {...s, dependencies: s.dependencies.union(Set((source, target)))}

  pure def canAddDependency(s: State, source: Id, target: Id): bool = {
    val updated = applyAddDependency(s, source, target)
    and {
      not(s.dependencies.contains((source, target))),
      dependencyMutable(s, source),
      relationsSafe(updated),
    }
  }

  pure def applyRemoveDependency(s: State, source: Id, target: Id): State =
    {...s, dependencies: s.dependencies.exclude(Set((source, target)))}

  pure def canRemoveDependency(s: State, source: Id, target: Id): bool = and {
    s.dependencies.contains((source, target)),
    dependencyMutable(s, source),
  }

  pure def canTick(s: State): bool = s.clock < CLOCK_MAX

  pure def applyTick(s: State): State = {...s, clock: s.clock + 1}

  // ---- State machine ----

  var state: State
  var observed: Set[Operation]

  action init: bool = all {
    state' = INITIAL_STATE,
    observed' = Set(),
  }

  action doDecide: bool = {
    nondet i = ENTITIES.oneOf()
    nondet choice = DISPOSITIONS.oneOf()
    all {
      canDecide(state, i, choice),
      state' = applyDecide(state, i, choice),
      observed' = observed.union(Set(DecideOp)),
    }
  }

  action doStart: bool = {
    nondet i = ENTITIES.oneOf()
    all {
      ready(state, i),
      state' = applyStart(state, i),
      observed' = observed.union(Set(StartOp)),
    }
  }

  action doDoneIssue: bool = {
    nondet i = ISSUES.oneOf()
    all {
      canCompleteIssue(state, i),
      state' = applyDone(state, i),
      observed' = observed.union(Set(DoneIssueOp)),
    }
  }

  action doDoneGroup: bool = {
    nondet group = GROUPS.oneOf()
    all {
      canCompleteGroup(state, group),
      state' = applyDone(state, group),
      observed' = observed.union(Set(DoneGroupOp)),
    }
  }

  action doReleaseIssue: bool = {
    nondet i = ISSUES.oneOf()
    all {
      canReleaseIssue(state, i),
      state' = applyRelease(state, i),
      observed' = observed.union(Set(ReleaseIssueOp)),
    }
  }

  action doReleaseGroup: bool = {
    nondet group = GROUPS.oneOf()
    all {
      canReleaseGroup(state, group),
      state' = applyRelease(state, group),
      observed' = observed.union(Set(ReleaseGroupOp)),
    }
  }

  action doSetDate: bool = {
    nondet i = ENTITIES.oneOf()
    nondet date = DATES.oneOf()
    all {
      canSetDate(state, i, date),
      state' = applyResurface(state, i, AtDate(date)),
      observed' = observed.union(Set(SetDateOp)),
    }
  }

  action doSetAfter: bool = {
    nondet i = ENTITIES.oneOf()
    nondet target = ENTITIES.oneOf()
    all {
      canSetAfter(state, i, target),
      state' = applyResurface(state, i, AfterEntity(target)),
      observed' = observed.union(Set(SetAfterOp)),
    }
  }

  action doClearWhen: bool = {
    nondet i = ENTITIES.oneOf()
    all {
      canClearWhen(state, i),
      state' = applyResurface(state, i, Always),
      observed' = observed.union(Set(ClearWhenOp)),
    }
  }

  action doSetParent: bool = {
    nondet i = ENTITIES.oneOf()
    nondet group = GROUPS.oneOf()
    all {
      canSetParent(state, i, group),
      state' = applyParent(state, i, group),
      observed' = observed.union(Set(SetParentOp)),
    }
  }

  action doUnsetParent: bool = {
    nondet i = ENTITIES.oneOf()
    all {
      canUnsetParent(state, i),
      state' = applyParent(state, i, NO_PARENT),
      observed' = observed.union(Set(UnsetParentOp)),
    }
  }

  action doAddDependency: bool = {
    nondet source = ENTITIES.oneOf()
    nondet target = ENTITIES.oneOf()
    all {
      canAddDependency(state, source, target),
      state' = applyAddDependency(state, source, target),
      observed' = observed.union(Set(AddDependencyOp)),
    }
  }

  action doRemoveDependency: bool = {
    nondet source = ENTITIES.oneOf()
    nondet target = ENTITIES.oneOf()
    all {
      canRemoveDependency(state, source, target),
      state' = applyRemoveDependency(state, source, target),
      observed' = observed.union(Set(RemoveDependencyOp)),
    }
  }

  action doTick: bool = all {
    canTick(state),
    state' = applyTick(state),
    observed' = observed.union(Set(TickOp)),
  }

  action doSetManual: bool = {
    nondet i = ENTITIES.oneOf()
    all {
      entity(state, i).resurface != Manual,
      state' = applyResurface(state, i, Manual),
      observed' = observed.union(Set(SetManualOp)),
    }
  }

  action doSetCommand: bool = {
    nondet i = ENTITIES.oneOf()
    all {
      entity(state, i).resurface != Command,
      state' = applyResurface(state, i, Command),
      observed' = observed.union(Set(SetCommandOp)),
    }
  }

  action doObserveCommand: bool = {
    nondet i = ENTITIES.oneOf()
    val updated = {...state, commandSatisfied: state.commandSatisfied.set(i, not(state.commandSatisfied.get(i)))}
    all {
      entity(state, i).resurface == Command,
      state' = updated,
      observed' = observed.union(Set(ObserveCommandOp)).union(
        if (isGroup(state, i) and opensDescendants(state, i) and
            descendants(state, i).exists(child => entity(state, child).progress == InProgress))
          Set(CommandGateClosedOp) else Set()),
    }
  }

  val invCommandObservation: bool = ENTITIES.forall(i =>
    entity(state, i).resurface == Command implies
      (surfaced(state, i) iff state.commandSatisfied.get(i)))
  val invCommandObservationPreservesControl: bool = ENTITIES.forall(i => {
    val updated = {...state, commandSatisfied: state.commandSatisfied.set(i, false)}
    and {
      updated.entities == state.entities,
      updated.parent == state.parent,
      updated.dependencies == state.dependencies,
      (isGroup(state, i) and entity(state, i).resurface == Command) implies
        not(opensDescendants(updated, i)),
    }
  })
  val invManualNotSurfaced = ENTITIES.forall(i =>
    entity(state, i).resurface == Manual implies not(surfaced(state, i)))
  val invManualPreservesControl = ENTITIES.forall(i => {
    val updated = applyResurface(state, i, Manual)
    and {
      entity(updated, i).progress == entity(state, i).progress,
      entity(updated, i).disposition == entity(state, i).disposition,
      updated.parent == state.parent,
      updated.dependencies == state.dependencies,
      ENTITIES.exclude(Set(i)).forall(j => entity(updated, j) == entity(state, j)),
      isGroup(state, i) implies not(opensDescendants(updated, i)),
    }
  })
  val wSetManual = observed.contains(SetManualOp)
  val wManualGroup = GROUPS.exists(i =>
    entity(state, i).resurface == Manual and
    descendants(state, i).exists(child => entity(state, child).progress == InProgress))
  val wSetCommand = observed.contains(SetCommandOp)
  val wObserveCommand = observed.contains(ObserveCommandOp)
  val wCommandGateClosed = observed.contains(CommandGateClosedOp)

  action step: bool = any {
    doSetManual,
    doSetCommand,
    doObserveCommand,
    doDecide,
    doStart,
    doDoneIssue,
    doDoneGroup,
    doReleaseIssue,
    doReleaseGroup,
    doSetDate,
    doSetAfter,
    doClearWhen,
    doSetParent,
    doUnsetParent,
    doAddDependency,
    doRemoveDependency,
    doTick,
  }

  // ---- Safety properties ----

  val invEntityKindsStable: bool =
    ENTITIES.forall(i => entity(state, i).kind == initialKind(i))

  val invRelationsSafe: bool = relationsSafe(state)

  val invReadyExclusive: bool =
    ENTITIES.forall(i => ready(state, i) implies and {
      not(blocked(state, i)),
      not(orphaned(state, i)),
      withinActiveScope(state, i),
    })

  val invInactiveGroupDescendantsNotReady: bool =
    GROUPS.forall(group =>
      not(opensDescendants(state, group)) implies
        descendants(state, group).forall(i => not(ready(state, i))))

  val invRejectedDependencyOrphans: bool =
    state.dependencies.forall(edge =>
      isRejected(state, edge._2) implies
        waitingScope(state, edge._1).forall(i => orphaned(state, i)))

  val invEndedGroupDescendantsTerminal: bool =
    GROUPS.forall(group =>
      isEnded(state, group) implies groupCompletionSatisfied(state, group))

  val invRejectedChildGroupAllowsCompletion: bool =
    GROUPS.forall(group =>
      GROUPS.forall(child =>
        (state.parent.get(child) == group
          and isRejected(state, child)
          and descendants(state, group)
            .filter(i => i != child)
            .forall(i => terminal(state, i)))
          implies groupCompletionSatisfied(state, group)))

  val invReleasedGroupHasNoInProgressDescendant: bool =
    GROUPS.forall(group =>
      entity(state, group).progress == NotStarted implies
        descendants(state, group)
          .forall(i => entity(state, i).progress != InProgress))

  val invEndedGroupsCannotMove: bool =
    GROUPS.forall(group =>
      isEnded(state, group) implies and {
        GROUPS.forall(target => not(canSetParent(state, group, target))),
        not(canUnsetParent(state, group)),
      })

  val invBlockedHasCause: bool =
    ENTITIES.forall(i => blocked(state, i) implies blockingCauses(state, i) != Set())

  val invCauseIsUnresolved: bool =
    ENTITIES.forall(i =>
      blockingCauses(state, i).forall(cause => not(terminal(state, cause))))

  val invCauseStopsAtRoot: bool =
    ENTITIES.forall(i =>
      blockingCauses(state, i).forall(cause => isBlockingRootCause(state, cause)))

  val invAfterRejectedSurfaces: bool =
    ENTITIES.forall(i =>
      match entity(state, i).resurface {
      | AfterEntity(target) => isRejected(state, target) implies surfaced(state, i)
      | _ => true
      })

  val invTriageIsCurrentFrontier: bool =
    ENTITIES.forall(i => triage(state, i) implies and {
      withinActiveScope(state, i),
      ancestors(state, i).forall(group => not(triage(state, group))),
    })

  // ---- Reachability witnesses ----

  val wDecide: bool = observed.contains(DecideOp)
  val wStart: bool = observed.contains(StartOp)
  val wDoneIssue: bool = observed.contains(DoneIssueOp)
  val wDoneGroup: bool = observed.contains(DoneGroupOp)
  val wReleaseIssue: bool = observed.contains(ReleaseIssueOp)
  val wReleaseGroup: bool = observed.contains(ReleaseGroupOp)
  val wSetDate: bool = observed.contains(SetDateOp)
  val wSetAfter: bool = observed.contains(SetAfterOp)
  val wClearWhen: bool = observed.contains(ClearWhenOp)
  val wSetParent: bool = observed.contains(SetParentOp)
  val wUnsetParent: bool = observed.contains(UnsetParentOp)
  val wAddDependency: bool = observed.contains(AddDependencyOp)
  val wRemoveDependency: bool = observed.contains(RemoveDependencyOp)
  val wTick: bool = observed.contains(TickOp)

  // 子がすべて終端になっても、group は InProgress のまま明示 done を待てる。
  val wGroupAwaitingExplicitDone: bool =
    GROUPS.exists(group => and {
      descendants(state, group) != Set(),
      canCompleteGroup(state, group),
    })

  val wRejectedChildGroupCanComplete: bool =
    GROUPS.exists(group =>
      GROUPS.exists(child => and {
        state.parent.get(child) == group,
        isRejected(state, child),
        canCompleteGroup(state, group),
      }))

  val wGroupDependencyOrphaned: bool =
    ENTITIES.exists(i => and {
      orphaned(state, i),
      dependencyTargets(state, i).exists(target =>
        isGroup(state, target) and isRejected(state, target)),
    })

  val wGroupDependencyBlocksDescendant: bool =
    GROUPS.exists(group => and {
      directDependencyTargets(state, group)
        .exists(target => not(terminal(state, target))),
      descendants(state, group).exists(i => blocked(state, i)),
    })

  val wInheritedGroupBlockingCause: bool =
    GROUPS.exists(group =>
      directDependencyTargets(state, group).exists(target => and {
        not(terminal(state, target)),
        descendants(state, group).exists(i =>
          blocked(state, i) and blockingCauses(state, i).contains(target)),
      }))

  val wGroupAfterGroup: bool =
    GROUPS.exists(group =>
      match entity(state, group).resurface {
      | AfterEntity(target) => isGroup(state, target)
      | _ => false
      })

  val wNestedEntityReady: bool =
    ENTITIES.exists(i => ancestors(state, i) != Set() and ready(state, i))

  val wTriageFrontier: bool =
    GROUPS.exists(group => and {
      triage(state, group),
      descendants(state, group).forall(i => not(triage(state, i))),
    })
}
