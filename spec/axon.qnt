// docs/axes.md で整理した状態モデルの検証用。
module axon {
  type Progress = NotStarted | InProgress | Done
  type Commitment = Undecided | Accepted | Rejected

  // C-1 (差し替え後): 再浮上条件は直和型。将来 3 種類目 (外部コマンド) を足せる形にしておく
  type Condition =
    | Always
    | AtDate(int)
    | AfterIssue(int)

  type Id = int
  type Operation = Init | Transition | Setting | ClockTick

  pure val ISSUES: Set[Id] = Set(0, 1, 2)
  pure val COMMITMENTS = Set(Undecided, Accepted, Rejected)
  pure val NEVER = -1          // resurfaceAt = null (常に浮上)
  pure val CLOCK_MAX = 3

  // ---- 保存される状態 ----
  var progress: Id -> Progress
  var commitment: Id -> Commitment
  var resurfaceCond: Id -> Condition
  var deps: Set[(Id, Id)]      // (y, x) = y は x に依存する
  var clock: int

  // コマンド契約を観測するための状態。updatedAt は実時刻ではなく更新回数を表す。
  var updatedAt: Id -> int
  var decisionEvents: Id -> int
  var progressEvents: Id -> int
  var settingValue: Id -> int
  var lastOperation: Operation
  var lastSucceeded: bool
  var lastChanged: bool

  // ---- 導出値 ----
  def depsOf(y: Id): Set[Id] = ISSUES.filter(x => deps.contains((y, x)))

  def isDone(i: Id): bool = progress.get(i) == Done
  def isRejected(i: Id): bool = commitment.get(i) == Rejected

  def surfaced(i: Id): bool =
    match resurfaceCond.get(i) {
    | Always => true
    | AtDate(d) => d <= clock
    | AfterIssue(x) => isDone(x) or isRejected(x)
    }

  // D-1: 依存先が不採用 => 前提喪失
  def orphaned(y: Id): bool = depsOf(y).exists(x => isRejected(x))

  // 依存先に未終端 (完了でも不採用でもない) のものがある
  def blocked(y: Id): bool =
    depsOf(y).exists(x => not(isDone(x)) and not(isRejected(x)))

  def ready(i: Id): bool = and {
    progress.get(i) == NotStarted,
    commitment.get(i) == Accepted,
    surfaced(i),
    not(blocked(i)),
    not(orphaned(i)),
    not(groupBlocked(i)),
  }

  // ---- 遷移 ----
  action init = all {
    progress' = ISSUES.mapBy(_ => NotStarted),
    commitment' = ISSUES.mapBy(_ => Undecided),
    resurfaceCond' = ISSUES.mapBy(_ => Always),
    deps' = Set(),
    clock' = 0,
    updatedAt' = ISSUES.mapBy(_ => 0),
    decisionEvents' = ISSUES.mapBy(_ => 0),
    progressEvents' = ISSUES.mapBy(_ => 0),
    settingValue' = ISSUES.mapBy(_ => 0),
    lastOperation' = Init,
    lastSucceeded' = true,
    lastChanged' = false,
  }

  action unchanged(operation: Operation, succeeded: bool): bool = all {
    progress' = progress, commitment' = commitment, resurfaceCond' = resurfaceCond,
    deps' = deps, clock' = clock, updatedAt' = updatedAt,
    decisionEvents' = decisionEvents, progressEvents' = progressEvents,
    settingValue' = settingValue, lastOperation' = operation,
    lastSucceeded' = succeeded, lastChanged' = false,
  }

  action doDecide = {
    nondet i = ISSUES.oneOf()
    nondet c = COMMITMENTS.oneOf()
    if (commitment.get(i) == c)
      unchanged(Transition, false)
    else
      all {
        commitment' = commitment.set(i, c),
        updatedAt' = updatedAt.set(i, updatedAt.get(i) + 1),
        decisionEvents' = decisionEvents.set(i, decisionEvents.get(i) + 1),
        progress' = progress, resurfaceCond' = resurfaceCond, deps' = deps, clock' = clock,
        progressEvents' = progressEvents, settingValue' = settingValue,
        lastOperation' = Transition, lastSucceeded' = true, lastChanged' = true,
      }
  }

  action doStart = {
    nondet i = ISSUES.oneOf()
    if (ready(i))
      all {
        progress' = progress.set(i, InProgress),
        updatedAt' = updatedAt.set(i, updatedAt.get(i) + 1),
        progressEvents' = progressEvents.set(i, progressEvents.get(i) + 1),
        commitment' = commitment, resurfaceCond' = resurfaceCond, deps' = deps, clock' = clock,
        decisionEvents' = decisionEvents, settingValue' = settingValue,
        lastOperation' = Transition, lastSucceeded' = true, lastChanged' = true,
      }
    else
      unchanged(Transition, false)
  }

  action doDone = {
    nondet i = ISSUES.oneOf()
    if (progress.get(i) == InProgress)
      all {
        progress' = progress.set(i, Done),
        updatedAt' = updatedAt.set(i, updatedAt.get(i) + 1),
        progressEvents' = progressEvents.set(i, progressEvents.get(i) + 1),
        commitment' = commitment, resurfaceCond' = resurfaceCond, deps' = deps, clock' = clock,
        decisionEvents' = decisionEvents, settingValue' = settingValue,
        lastOperation' = Transition, lastSucceeded' = true, lastChanged' = true,
      }
    else
      unchanged(Transition, false)
  }

  action doRelease = {
    nondet i = ISSUES.oneOf()
    if (progress.get(i) == InProgress)
      all {
        progress' = progress.set(i, NotStarted),
        updatedAt' = updatedAt.set(i, updatedAt.get(i) + 1),
        progressEvents' = progressEvents.set(i, progressEvents.get(i) + 1),
        commitment' = commitment, resurfaceCond' = resurfaceCond, deps' = deps, clock' = clock,
        decisionEvents' = decisionEvents, settingValue' = settingValue,
        lastOperation' = Transition, lastSucceeded' = true, lastChanged' = true,
      }
    else
      unchanged(Transition, false)
  }

  action doDeferDate = {
    nondet i = ISSUES.oneOf()
    nondet d = Set(1, 2).oneOf()
    val target = AtDate(clock + d)
    if (resurfaceCond.get(i) == target)
      unchanged(Transition, false)
    else
      all {
        resurfaceCond' = resurfaceCond.set(i, target),
        updatedAt' = updatedAt.set(i, updatedAt.get(i) + 1),
        decisionEvents' = decisionEvents.set(i, decisionEvents.get(i) + 1),
        progress' = progress, commitment' = commitment, deps' = deps, clock' = clock,
        progressEvents' = progressEvents, settingValue' = settingValue,
        lastOperation' = Transition, lastSucceeded' = true, lastChanged' = true,
      }
  }

  // C の参照条件。D (依存) とは意味が違うので、循環を避ける以外の制約は置かない
  action doDeferAfter = {
    nondet i = ISSUES.oneOf()
    nondet x = ISSUES.oneOf()
    val target = AfterIssue(x)
    if (i == x or resurfaceCond.get(i) == target)
      unchanged(Transition, false)
    else
      all {
        resurfaceCond' = resurfaceCond.set(i, target),
        updatedAt' = updatedAt.set(i, updatedAt.get(i) + 1),
        decisionEvents' = decisionEvents.set(i, decisionEvents.get(i) + 1),
        progress' = progress, commitment' = commitment, deps' = deps, clock' = clock,
        progressEvents' = progressEvents, settingValue' = settingValue,
        lastOperation' = Transition, lastSucceeded' = true, lastChanged' = true,
      }
  }

  action doClearCondition = {
    nondet i = ISSUES.oneOf()
    if (resurfaceCond.get(i) == Always)
      unchanged(Transition, false)
    else
      all {
        resurfaceCond' = resurfaceCond.set(i, Always),
        updatedAt' = updatedAt.set(i, updatedAt.get(i) + 1),
        decisionEvents' = decisionEvents.set(i, decisionEvents.get(i) + 1),
        progress' = progress, commitment' = commitment, deps' = deps, clock' = clock,
        progressEvents' = progressEvents, settingValue' = settingValue,
        lastOperation' = Transition, lastSucceeded' = true, lastChanged' = true,
      }
  }

  // 循環を避けるため x < y のみ許す
  action doAddDep = {
    nondet y = ISSUES.oneOf()
    nondet x = ISSUES.oneOf()
    all {
      x < y,
      deps' = deps.union(Set((y, x))),
      progress' = progress, commitment' = commitment, resurfaceCond' = resurfaceCond,
      clock' = clock, updatedAt' = updatedAt, decisionEvents' = decisionEvents,
      progressEvents' = progressEvents, settingValue' = settingValue,
      lastOperation' = Setting, lastSucceeded' = true,
      lastChanged' = not(deps.contains((y, x))),
    }
  }

  action doRemoveDep = {
    nondet y = ISSUES.oneOf()
    nondet x = ISSUES.oneOf()
    all {
      x < y,
      deps' = deps.exclude(Set((y, x))),
      progress' = progress, commitment' = commitment, resurfaceCond' = resurfaceCond,
      clock' = clock, updatedAt' = updatedAt, decisionEvents' = decisionEvents,
      progressEvents' = progressEvents, settingValue' = settingValue,
      lastOperation' = Setting, lastSucceeded' = true,
      lastChanged' = deps.contains((y, x)),
    }
  }

  // write / group set の代表。設定は同値でも成功し、updatedAt を進めない。
  action doSetValue = {
    nondet i = ISSUES.oneOf()
    nondet target = Set(0, 1).oneOf()
    val changed = settingValue.get(i) != target
    all {
      settingValue' = settingValue.set(i, target),
      updatedAt' = if (changed) updatedAt.set(i, updatedAt.get(i) + 1) else updatedAt,
      progress' = progress, commitment' = commitment, resurfaceCond' = resurfaceCond,
      deps' = deps, clock' = clock, decisionEvents' = decisionEvents,
      progressEvents' = progressEvents, lastOperation' = Setting,
      lastSucceeded' = true, lastChanged' = changed,
    }
  }

  // group reject は未達の子だけを不採用へ収束させ、再実行では履歴を増やさない。
  action doGroupReject = {
    val changed = ISSUES.exists(i => commitment.get(i) != Rejected)
    all {
      commitment' = ISSUES.mapBy(_ => Rejected),
      updatedAt' = ISSUES.mapBy(i =>
        if (commitment.get(i) == Rejected) updatedAt.get(i) else updatedAt.get(i) + 1),
      decisionEvents' = ISSUES.mapBy(i =>
        if (commitment.get(i) == Rejected)
          decisionEvents.get(i)
        else
          decisionEvents.get(i) + 1),
      progress' = progress, resurfaceCond' = resurfaceCond, deps' = deps, clock' = clock,
      progressEvents' = progressEvents, settingValue' = settingValue,
      lastOperation' = Setting, lastSucceeded' = true, lastChanged' = changed,
    }
  }

  action doTick = all {
    clock < CLOCK_MAX,
    clock' = clock + 1,
    progress' = progress, commitment' = commitment, resurfaceCond' = resurfaceCond,
    deps' = deps, updatedAt' = updatedAt, decisionEvents' = decisionEvents,
    progressEvents' = progressEvents, settingValue' = settingValue,
    lastOperation' = ClockTick, lastSucceeded' = true, lastChanged' = true,
  }

  action step = any {
    doDecide, doStart, doDone, doRelease, doDeferDate, doDeferAfter, doClearCondition,
    doAddDep, doRemoveDep, doSetValue, doGroupReject, doTick,
  }

  // ---- 検査したい性質 ----

  // (a) 「完了 かつ 不採用」の依存先を持つ issue は解放されない
  val invRejectedDoneStillBlocks =
    ISSUES.forall(y =>
      depsOf(y).exists(x => isDone(x) and isRejected(x)) implies not(ready(y)))

  // (b) 完了しえない依存先を持つのに orphaned と扱われない issue がいないか。
  //     Y -> X -> Z で Z が不採用のとき、X は orphaned だが Y は「ただの blocked」に見える。
  def cannotComplete(i: Id): bool =
    isRejected(i) or depsOf(i).exists(x => isRejected(x))

  val invNoHiddenDeadlock =
    ISSUES.forall(y =>
      (blocked(y) and depsOf(y).exists(x => cannotComplete(x))) implies orphaned(y))

  // (c) 到達可能性の観察。破れたら「その組み合わせは起こりうる」ということ。
  val invNoDoneUndecided =
    ISSUES.forall(i => not(isDone(i) and commitment.get(i) == Undecided))

  val invNoInProgressRejected =
    ISSUES.forall(i => not(progress.get(i) == InProgress and isRejected(i)))

  // (d) ready と blocked / orphaned は排他 (定義の健全性確認)
  val invReadyExclusive =
    ISSUES.forall(i => ready(i) implies (not(blocked(i)) and not(orphaned(i))))

  val invCommandOutcomeContract =
    match lastOperation {
    | Transition => lastSucceeded == lastChanged
    | Setting => lastSucceeded
    | ClockTick => lastSucceeded and lastChanged
    | Init => lastSucceeded and not(lastChanged)
    }

  val invEventsHaveUpdate =
    ISSUES.forall(i => decisionEvents.get(i) + progressEvents.get(i) <= updatedAt.get(i))

  // ---- D-1: 阻害の根本原因を辿る導出値 ----
  // 未終端 = 完了でも不採用でもない依存先
  def unresolved(y: Id): Set[Id] =
    depsOf(y).filter(x => not(isDone(x)) and not(isRejected(x)))

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
    ISSUES.forall(y => cause3(y).forall(x => not(isDone(x)) and not(isRejected(x))))

  // ---- §7 グループ ----
  // 構造は固定。G100 が共通基盤にあたる形を模している
  pure val GROUPS: Set[Id] = Set(100, 101, 102)
  pure val NO_GROUP = -1
  pure val groupOf: Id -> Id = Map(0 -> 100, 1 -> 101, 2 -> 102)
  pure val parentOf: Id -> Id = Map(100 -> NO_GROUP, 101 -> NO_GROUP, 102 -> 101)
  pure val groupDeps: Set[(Id, Id)] = Set((101, 100))   // (y, x) = y は x に依存

  def issuesIn(g: Id): Set[Id] = ISSUES.filter(i => groupOf.get(i) == g)
  def subgroups(g: Id): Set[Id] = GROUPS.filter(h => parentOf.get(h) == g)

  // 子孫 issue (深さ 3 まで展開)
  def desc1(g: Id): Set[Id] = issuesIn(g)
  def desc2(g: Id): Set[Id] = desc1(g).union(subgroups(g).map(h => desc1(h)).flatten())
  def desc3(g: Id): Set[Id] = desc2(g).union(subgroups(g).map(h => desc2(h)).flatten())

  def terminal(i: Id): bool = isDone(i) or isRejected(i)

  // G-3: 全子孫 issue が終端に達したら解除。orphaned は考えない
  def groupSatisfied(g: Id): bool = desc3(g).forall(i => terminal(i))

  // 所属グループとその祖先 (深さ 3 まで)
  def chain1(g: Id): Set[Id] = Set(g)
  def chain2(g: Id): Set[Id] =
    chain1(g).union(if (parentOf.get(g) == NO_GROUP) Set() else Set(parentOf.get(g)))
  def chain3(g: Id): Set[Id] =
    chain2(g).union(chain2(g).filter(h => parentOf.get(h) != NO_GROUP).map(h => parentOf.get(h)))

  def groupChainOf(i: Id): Set[Id] = chain3(groupOf.get(i))

  // 祖先も含めたグループ依存で塞がれているか
  def groupBlocked(i: Id): bool =
    groupChainOf(i).exists(g =>
      GROUPS.exists(x => groupDeps.contains((g, x)) and not(groupSatisfied(x))))

  // 進捗: 分母は B=採用 の子孫のみ
  def denom(g: Id): Set[Id] = desc3(g).filter(i => commitment.get(i) == Accepted)
  def numer(g: Id): Set[Id] = denom(g).filter(i => isDone(i))
  def acceptedAllDone(g: Id): bool = denom(g) == numer(g)

  // ---- グループに関する検査 ----

  // 分子は分母の部分集合
  val invProgressSubset =
    GROUPS.forall(g => numer(g).subseteq(denom(g)))

  // グループ依存で塞がれているなら ready ではない (定義の健全性)
  val invGroupBlockedNotReady =
    ISSUES.forall(i => groupBlocked(i) implies not(ready(i)))

  // G-3: 依存先グループに不採用が混じっていても解除されうる
  //      (不採用があると必ず未解決、にはならないこと)
  val invRejectedDoesNotPinGroup =
    GROUPS.forall(g =>
      (desc3(g).exists(i => isRejected(i)) and desc3(g).forall(i => terminal(i)))
        implies groupSatisfied(g))

  // 進捗が 100% ならグループ依存は解除されているか
  // (分母は B=採用 のみ。未判断が残っていると terminal でないため解除されない)
  val invFullProgressReleases =
    GROUPS.forall(g => acceptedAllDone(g) implies groupSatisfied(g))

  // グループ依存で塞がれた issue の原因を blockedReason で説明できるか
  val invGroupBlockedHasCause =
    ISSUES.forall(i => groupBlocked(i) implies cause3(i) != Set())

  // グループ依存による阻害の原因: 依存先グループの未終端な子孫 issue
  def groupCauses(i: Id): Set[Id] =
    groupChainOf(i)
      .map(g => GROUPS.filter(x => groupDeps.contains((g, x)) and not(groupSatisfied(x))))
      .flatten()
      .map(x => desc3(x).filter(j => not(terminal(j))))
      .flatten()

  // issue 間依存とグループ間依存の両方を辿った根本原因
  def causeAll(i: Id): Set[Id] = cause3(i).union(groupCauses(i))

  // 阻害されているなら、issue 依存でもグループ依存でも原因が特定できること
  val invAnyBlockedHasCause =
    ISSUES.forall(i => (blocked(i) or groupBlocked(i)) implies causeAll(i) != Set())

  // 原因として挙がるのは実際に未終端のものだけ
  val invCauseAllUnresolved =
    ISSUES.forall(i => causeAll(i).forall(x => not(terminal(x))))

  // 進捗率が定義できるのは分母が空でないときだけ。
  // 分母が空 (採用が 1 件もない) のグループを 100% と呼んではいけない。
  def progressDefined(g: Id): bool = denom(g) != Set()

  val invFullProgressReleasesGuarded =
    GROUPS.forall(g => (progressDefined(g) and acceptedAllDone(g)) implies groupSatisfied(g))

  // 逆方向: グループが完了しているなら、採用分の進捗は必ず 100%
  val invGroupCompleteImpliesFullProgress =
    GROUPS.forall(g => groupSatisfied(g) implies acceptedAllDone(g))

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
