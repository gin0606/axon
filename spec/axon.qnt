// docs/axes.md で整理した、axon の思想的なコアとなる状態モデル。
// CLI・永続化・再試行などの実装契約は Rust のテストか一時的なモデルで扱う。
module axon {
  type Progress = NotStarted | InProgress | Ended
  type Disposition = Undecided | Accepted | Rejected

  // C-1 (差し替え後): 再浮上条件は直和型。将来 3 種類目 (外部コマンド) を足せる形にしておく
  type ResurfaceCondition =
    | Always
    | AtDate(int)
    | AfterIssue(int)

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

  // ---- 導出値 ----
  def depsOf(y: Id): Set[Id] = ISSUES.filter(x => deps.contains((y, x)))

  def isEnded(i: Id): bool = progress.get(i) == Ended
  def isRejected(i: Id): bool = disposition.get(i) == Rejected

  def surfaced(i: Id): bool =
    match resurfaceCond.get(i) {
    | Always => true
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
    not(groupBlocked(i)),
  }

  // ---- 遷移 ----
  action init = all {
    progress' = ISSUES.mapBy(_ => NotStarted),
    disposition' = ISSUES.mapBy(_ => Undecided),
    resurfaceCond' = ISSUES.mapBy(_ => Always),
    deps' = Set(),
    clock' = 0,
  }

  action doDecide = {
    nondet i = ISSUES.oneOf()
    nondet c = DISPOSITIONS.oneOf()
    all {
      disposition.get(i) != c,
      disposition' = disposition.set(i, c),
      progress' = progress, resurfaceCond' = resurfaceCond, deps' = deps, clock' = clock,
    }
  }

  action doStart = {
    nondet i = ISSUES.oneOf()
    all {
      ready(i),
      progress' = progress.set(i, InProgress),
      disposition' = disposition, resurfaceCond' = resurfaceCond, deps' = deps, clock' = clock,
    }
  }

  action doEnd = {
    nondet i = ISSUES.oneOf()
    all {
      progress.get(i) == InProgress,
      progress' = progress.set(i, Ended),
      disposition' = disposition, resurfaceCond' = resurfaceCond, deps' = deps, clock' = clock,
    }
  }

  action doRelease = {
    nondet i = ISSUES.oneOf()
    all {
      progress.get(i) == InProgress,
      progress' = progress.set(i, NotStarted),
      disposition' = disposition, resurfaceCond' = resurfaceCond, deps' = deps, clock' = clock,
    }
  }

  action doDeferDate = {
    nondet i = ISSUES.oneOf()
    nondet d = Set(1, 2).oneOf()
    val target = AtDate(clock + d)
    all {
      resurfaceCond.get(i) != target,
      resurfaceCond' = resurfaceCond.set(i, target),
      progress' = progress, disposition' = disposition, deps' = deps, clock' = clock,
    }
  }

  // C の参照条件。D (依存) とは意味が違うので、循環を避ける以外の制約は置かない
  action doDeferAfter = {
    nondet i = ISSUES.oneOf()
    nondet x = ISSUES.oneOf()
    val target = AfterIssue(x)
    all {
      i != x,
      resurfaceCond.get(i) != target,
      resurfaceCond' = resurfaceCond.set(i, target),
      progress' = progress, disposition' = disposition, deps' = deps, clock' = clock,
    }
  }

  action doClearCondition = {
    nondet i = ISSUES.oneOf()
    all {
      resurfaceCond.get(i) != Always,
      resurfaceCond' = resurfaceCond.set(i, Always),
      progress' = progress, disposition' = disposition, deps' = deps, clock' = clock,
    }
  }

  // 循環を避けるため x < y のみ許す
  action doAddDep = {
    nondet y = ISSUES.oneOf()
    nondet x = ISSUES.oneOf()
    all {
      x < y,
      not(deps.contains((y, x))),
      deps' = deps.union(Set((y, x))),
      progress' = progress, disposition' = disposition, resurfaceCond' = resurfaceCond, clock' = clock,
    }
  }

  action doRemoveDep = {
    nondet y = ISSUES.oneOf()
    nondet x = ISSUES.oneOf()
    all {
      x < y,
      deps.contains((y, x)),
      deps' = deps.exclude(Set((y, x))),
      progress' = progress, disposition' = disposition, resurfaceCond' = resurfaceCond, clock' = clock,
    }
  }

  action doTick = all {
    clock < CLOCK_MAX,
    clock' = clock + 1,
    progress' = progress, disposition' = disposition, resurfaceCond' = resurfaceCond, deps' = deps,
  }

  action step = any {
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

  def terminal(i: Id): bool = isEnded(i) or isRejected(i)

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
  def denom(g: Id): Set[Id] = desc3(g).filter(i => disposition.get(i) == Accepted)
  def numer(g: Id): Set[Id] = denom(g).filter(i => isEnded(i))
  def acceptedAllEnded(g: Id): bool = denom(g) == numer(g)

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
    GROUPS.forall(g => acceptedAllEnded(g) implies groupSatisfied(g))

  // グループ依存で塞がれた issue の blocking cause を説明できるか
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
    GROUPS.forall(g => (progressDefined(g) and acceptedAllEnded(g)) implies groupSatisfied(g))

  // 逆方向: グループが完了しているなら、採用分の進捗は必ず 100%
  val invGroupCompleteImpliesFullProgress =
    GROUPS.forall(g => groupSatisfied(g) implies acceptedAllEnded(g))

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
