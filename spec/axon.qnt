// 過去資料: 置換前の三軸モデル。現行仕様は lifecycle_proposal.md を参照。
// docs/reference/state-model.md で整理した、axon の思想的なコアとなる状態モデル。
// CLI・永続化・再試行などの実装契約は Rust のテストか一時的なモデルで扱う。
module axon {
  type Progress = NotStarted | InProgress | Ended
  type Disposition = Undecided | Accepted | Rejected

  // Command の文字列は抽象化し、外部の観測値を入力として扱う。
  type ResurfaceCondition =
    | Always
    | AtDate(int)
    | AfterIssue(int)
    | Command
    | Manual

  type Id = int

  pure val ISSUES: Set[Id] = Set(0, 1, 2)
  pure val DISPOSITIONS = Set(Undecided, Accepted, Rejected)
  pure val NEVER = -1          // resurfaceAt = null (常に浮上)
  pure val CLOCK_MAX = 3

  // ---- 保存される状態 ----
  var progress: Id -> Progress
  var disposition: Id -> Disposition
  var resurfaceCond: Id -> ResurfaceCondition
  var deps: Set[(Id, Id)]      // (y, x) = y は x に依存する
  var clock: int

  // 外部入力と到達性の ghost state。DB には保存しない。
  var commandSatisfied: Id -> bool
  var commandFell: bool

  // ---- 導出値 ----
  def depsOf(y: Id): Set[Id] = ISSUES.filter(x => deps.contains((y, x)))

  pure def directTargets(edges: Set[(Id, Id)], y: Id, nodes: Set[Id]): Set[Id] =
    nodes.filter(x => edges.contains((y, x)))

  // このモデルの node 数ぶん展開した到達集合。自分へ戻れば循環している。
  pure def reach1(edges: Set[(Id, Id)], y: Id, nodes: Set[Id]): Set[Id] =
    directTargets(edges, y, nodes)
  pure def reach2(edges: Set[(Id, Id)], y: Id, nodes: Set[Id]): Set[Id] =
    reach1(edges, y, nodes)
      .union(reach1(edges, y, nodes).map(x => reach1(edges, x, nodes)).flatten())
  pure def reach3(edges: Set[(Id, Id)], y: Id, nodes: Set[Id]): Set[Id] =
    reach2(edges, y, nodes)
      .union(reach2(edges, y, nodes).map(x => reach1(edges, x, nodes)).flatten())

  pure def acyclic(edges: Set[(Id, Id)], nodes: Set[Id]): bool =
    nodes.forall(y => not(reach3(edges, y, nodes).contains(y)))

  def hasAfterIssue(i: Id): bool =
    match resurfaceCond.get(i) {
    | AfterIssue(_) => true
    | _ => false
    }

  def afterIssueTarget(i: Id): Id =
    match resurfaceCond.get(i) {
    | AfterIssue(x) => x
    | _ => i
    }

  // D と C は意味論と保存先が違うが、待機の循環検査だけは同じ graph で行う。
  val issueWaitEdges: Set[(Id, Id)] = deps.union(
    ISSUES.filter(i => hasAfterIssue(i)).map(i => (i, afterIssueTarget(i))))

  def withoutResurfaceEdge(i: Id): Set[(Id, Id)] =
    issueWaitEdges.exclude(Set((i, afterIssueTarget(i))))

  def isEnded(i: Id): bool = progress.get(i) == Ended
  def isRejected(i: Id): bool = disposition.get(i) == Rejected

  def surfaced(i: Id): bool =
    match resurfaceCond.get(i) {
    | Always => true
    | Manual => false
    | Command => commandSatisfied.get(i)
    | AtDate(d) => d <= clock
    | AfterIssue(x) => isEnded(x) or isRejected(x)
    }

  // D-1: 依存先が不採用 => 前提喪失
  def orphaned(y: Id): bool = depsOf(y).exists(x => isRejected(x))

  // 依存先に未終端 (完了でも不採用でもない) のものがある
  def blocked(y: Id): bool =
    depsOf(y).exists(x => not(isEnded(x)) and not(isRejected(x)))

  def ready(i: Id): bool = and {
    progress.get(i) == NotStarted,
    disposition.get(i) == Accepted,
    surfaced(i),
    not(blocked(i)),
    not(orphaned(i)),
  }

  // ---- 遷移 ----
  action init = all {
    progress' = ISSUES.mapBy(_ => NotStarted),
    disposition' = ISSUES.mapBy(_ => Undecided),
    resurfaceCond' = ISSUES.mapBy(_ => Always),
    deps' = Set(),
    clock' = 0, commandSatisfied' = ISSUES.mapBy(_ => false), commandFell' = false,
  }

  action doDecide = {
    nondet i = ISSUES.oneOf()
    nondet c = DISPOSITIONS.oneOf()
    all {
      disposition.get(i) != c,
      disposition' = disposition.set(i, c),
      progress' = progress, resurfaceCond' = resurfaceCond, deps' = deps,
      clock' = clock, commandSatisfied' = commandSatisfied, commandFell' = commandFell,
    }
  }

  action doStart = {
    nondet i = ISSUES.oneOf()
    all {
      ready(i),
      progress' = progress.set(i, InProgress),
      disposition' = disposition, resurfaceCond' = resurfaceCond, deps' = deps,
      clock' = clock, commandSatisfied' = commandSatisfied, commandFell' = commandFell,
    }
  }

  action doEnd = {
    nondet i = ISSUES.oneOf()
    all {
      progress.get(i) == InProgress,
      progress' = progress.set(i, Ended),
      disposition' = disposition, resurfaceCond' = resurfaceCond, deps' = deps,
      clock' = clock, commandSatisfied' = commandSatisfied, commandFell' = commandFell,
    }
  }

  action doRelease = {
    nondet i = ISSUES.oneOf()
    all {
      progress.get(i) == InProgress,
      progress' = progress.set(i, NotStarted),
      disposition' = disposition, resurfaceCond' = resurfaceCond, deps' = deps,
      clock' = clock, commandSatisfied' = commandSatisfied, commandFell' = commandFell,
    }
  }

  action doDeferDate = {
    nondet i = ISSUES.oneOf()
    nondet d = Set(1, 2).oneOf()
    val target = AtDate(clock + d)
    all {
      resurfaceCond.get(i) != target,
      resurfaceCond' = resurfaceCond.set(i, target),
      progress' = progress, disposition' = disposition, deps' = deps,
      clock' = clock, commandSatisfied' = commandSatisfied, commandFell' = commandFell,
    }
  }

  // C の参照条件。D (依存) とは意味が違うので、循環を避ける以外の制約は置かない
  action doDeferAfter = {
    nondet i = ISSUES.oneOf()
    nondet x = ISSUES.oneOf()
    val target = AfterIssue(x)
    val candidate = withoutResurfaceEdge(i).union(Set((i, x)))
    all {
      acyclic(candidate, ISSUES),
      resurfaceCond.get(i) != target,
      resurfaceCond' = resurfaceCond.set(i, target),
      progress' = progress, disposition' = disposition, deps' = deps,
      clock' = clock, commandSatisfied' = commandSatisfied, commandFell' = commandFell,
    }
  }

  action doClearCondition = {
    nondet i = ISSUES.oneOf()
    all {
      resurfaceCond.get(i) != Always,
      resurfaceCond' = resurfaceCond.set(i, Always),
      progress' = progress, disposition' = disposition, deps' = deps,
      clock' = clock, commandSatisfied' = commandSatisfied, commandFell' = commandFell,
    }
  }

  action doAddDep = {
    nondet y = ISSUES.oneOf()
    nondet x = ISSUES.oneOf()
    val candidate = issueWaitEdges.union(Set((y, x)))
    all {
      acyclic(candidate, ISSUES),
      not(deps.contains((y, x))),
      deps' = deps.union(Set((y, x))),
      progress' = progress, disposition' = disposition, resurfaceCond' = resurfaceCond,
      clock' = clock, commandSatisfied' = commandSatisfied, commandFell' = commandFell,
    }
  }

  action doRemoveDep = {
    nondet y = ISSUES.oneOf()
    nondet x = ISSUES.oneOf()
    all {
      deps.contains((y, x)),
      deps' = deps.exclude(Set((y, x))),
      progress' = progress, disposition' = disposition, resurfaceCond' = resurfaceCond,
      clock' = clock, commandSatisfied' = commandSatisfied, commandFell' = commandFell,
    }
  }

  action doTick = all {
    clock < CLOCK_MAX,
    clock' = clock + 1, commandSatisfied' = commandSatisfied, commandFell' = commandFell,
    progress' = progress, disposition' = disposition, resurfaceCond' = resurfaceCond,
    deps' = deps,
  }

  action doSetManual = {
    nondet i = ISSUES.oneOf()
    all {
      resurfaceCond.get(i) != Manual,
      resurfaceCond' = resurfaceCond.set(i, Manual),
      progress' = progress, disposition' = disposition, deps' = deps,
      clock' = clock, commandSatisfied' = commandSatisfied, commandFell' = commandFell,
    }
  }

  action doSetCommand = {
    nondet i = ISSUES.oneOf()
    all {
      resurfaceCond.get(i) != Command,
      resurfaceCond' = resurfaceCond.set(i, Command),
      progress' = progress, disposition' = disposition, deps' = deps,
      clock' = clock, commandSatisfied' = commandSatisfied, commandFell' = commandFell,
    }
  }

  action doObserveCommand = {
    nondet i = ISSUES.oneOf()
    all {
      resurfaceCond.get(i) == Command,
      commandSatisfied' = commandSatisfied.set(i, not(commandSatisfied.get(i))),
      commandFell' = (commandFell or commandSatisfied.get(i)),
      progress' = progress, disposition' = disposition, resurfaceCond' = resurfaceCond,
      deps' = deps, clock' = clock,
    }
  }

  val invCommandObservation = ISSUES.forall(i =>
    resurfaceCond.get(i) == Command implies (surfaced(i) iff commandSatisfied.get(i)))
  val invManualNotSurfaced = ISSUES.forall(i =>
    resurfaceCond.get(i) == Manual implies not(surfaced(i)))
  val wManual = ISSUES.exists(i => resurfaceCond.get(i) == Manual)
  val wCommandFell = commandFell

  action step = any {
    doSetManual, doSetCommand, doObserveCommand,
    doDecide, doStart, doEnd, doRelease, doDeferDate, doDeferAfter, doClearCondition,
    doAddDep, doRemoveDep, doTick,
  }

  // ---- 検査したい性質 ----

  // (a) 「完了 かつ 不採用」の依存先を持つ issue は解放されない
  val invRejectedEndedStillBlocks =
    ISSUES.forall(y =>
      depsOf(y).exists(x => isEnded(x) and isRejected(x)) implies not(ready(y)))

  // (b) 完了しえない依存先を持つのに orphaned と扱われない issue がいないか。
  //     Y -> X -> Z で Z が不採用のとき、X は orphaned だが Y は「ただの blocked」に見える。
  def cannotComplete(i: Id): bool =
    isRejected(i) or depsOf(i).exists(x => isRejected(x))

  val invNoHiddenDeadlock =
    ISSUES.forall(y =>
      (blocked(y) and depsOf(y).exists(x => cannotComplete(x))) implies orphaned(y))

  // (c) 到達可能性の観察。破れたら「その組み合わせは起こりうる」ということ。
  val invNoEndedUndecided =
    ISSUES.forall(i => not(isEnded(i) and disposition.get(i) == Undecided))

  val invNoInProgressRejected =
    ISSUES.forall(i => not(progress.get(i) == InProgress and isRejected(i)))

  // (d) ready と blocked / orphaned は排他 (定義の健全性確認)
  val invReadyExclusive =
    ISSUES.forall(i => ready(i) implies (not(blocked(i)) and not(orphaned(i))))

  val invIssueWaitsAcyclic = acyclic(issueWaitEdges, ISSUES)

  // ---- D-1: 阻害の根本原因を辿る導出値 ----
  // 未終端 = 完了でも不採用でもない依存先
  def unresolved(y: Id): Set[Id] =
    depsOf(y).filter(x => not(isEnded(x)) and not(isRejected(x)))

  // それ以上遡っても意味がない原因: 前提喪失 / 後送り中 / 依存先を持たない
  def isRootCause(x: Id): bool =
    orphaned(x) or not(surfaced(x)) or unresolved(x) == Set()

  def cause1(y: Id): Set[Id] = unresolved(y).filter(x => isRootCause(x))
  def cause2(y: Id): Set[Id] =
    cause1(y).union(unresolved(y).filter(x => not(isRootCause(x))).map(x => cause1(x)).flatten())
  def cause3(y: Id): Set[Id] =
    cause2(y).union(unresolved(y).filter(x => not(isRootCause(x))).map(x => cause2(x)).flatten())

  // 阻害されているなら、原因が必ず 1 つ以上特定できること
  val invBlockedHasCause =
    ISSUES.forall(y => blocked(y) implies cause3(y) != Set())

  // 原因として挙がるのは、実際に未終端のものだけであること
  val invCauseIsUnresolved =
    ISSUES.forall(y => cause3(y).forall(x => not(isEnded(x)) and not(isRejected(x))))

  // ---- C-1 差し替えの検査 ----
  // C の参照条件は、参照先が不採用でも満たされる (D と違い orphaned にならない)
  val invCondRefSatisfiedByRejection =
    ISSUES.forall(i =>
      match resurfaceCond.get(i) {
      | AfterIssue(x) => isRejected(x) implies surfaced(i)
      | _ => true
      })

  // 同じ相手に対して、D 依存と C 参照で挙動が分かれること。
  // X が不採用のとき: D 依存の Y は orphaned、C 参照だけの Y は浮上している
  val invCondAndDepDiffer =
    ISSUES.forall(y =>
      ISSUES.forall(x =>
        (isRejected(x)
         and resurfaceCond.get(y) == AfterIssue(x)
         and depsOf(y).forall(z => not(isRejected(z))))
          implies (surfaced(y) and not(orphaned(y)))))
}
