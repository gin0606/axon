// record_integration の特定の経路を固定した順序で再現するテスト。
// 独立 review で見つかった経路（終了した Group どうしの循環、完了済みの相互依存、二重登録、
// 親が欠けた取り込み）も、修復できることの確認として残す。
module record_integration_test {
  import record_integration.* from "record_integration"

  action hold = all { stores' = stores, seqs' = seqs, previous' = previous, last' = last, seen' = seen, views' = views, prevViews' = prevViews }
  def v0 = views.get(0)
  def v1 = views.get(1)
  def own(r, n) = stores.get(r).records.filter(x => x.id == { origin: r, seq: n }).getOnlyElement()
  def theirs(r, from) = stores.get(from).records.exclude(stores.get(r).records)

  // 孫 Issue が InProgress の間、祖父 Group の Withdraw は拒否される。
  run withdrawOverWorkingGrandchildScenario = {
    init
      .then(move(0, v0, 2, 0, InGroup(0)))
      .then(move(0, v0, 1, 0, InGroup(2)))
      .then(lifecycleOp(0, v0, 1, Start, 0, false))
      .then(all {
        assert(valid(v0)),
        assert(not(canLifecycle(v0, 0, Withdraw, false))),
        hold,
      })
  }

  // 両側が別 actor で Issue 1 を Start し、replica 0 が取り込んで自分側を選んで解決する。
  // replica 1 が解決記録だけを一件取り込みすると、選ばれた側の親が無い gap の状態になるが、
  // 解決記録の値が現在値になる。
  run resolveArrivesBeforeParentScenario = {
    init
      .then(lifecycleOp(0, v0, 1, Start, 0, false))
      .then(lifecycleOp(1, v1, 1, Start, 1, false))
      .then(sync(0, 1, Full, own(1, 0)))
      .then(resolve(0, v0, 1, 0, own(0, 4)))
      .then(sync(1, 0, Single, stores.get(0).records.filter(x => x.event == Resolve).getOnlyElement()))
      .then(all {
        assert(v1.gaps.contains(1)),
        assert(v1.settled.contains(1)),
        assert(v1.current.get(1).owner == Owned(0)),
        hold,
      })
  }

  // 終了した Group どうしの包含の循環。違反に含まれる Entity は親が終了していても取り外せる。
  run terminalCycleRepairScenario = {
    init
      .then(lifecycleOp(0, v0, 2, Complete, 0, true))
      .then(move(0, v0, 2, 0, InGroup(0)))
      .then(lifecycleOp(1, v1, 1, Cancel, 0, true))
      .then(move(1, v1, 0, 0, InGroup(2)))
      .then(lifecycleOp(1, v1, 0, Complete, 0, true))
      .then(sync(1, 0, Full, own(0, 4)))
      .then(all {
        assert(noConflicts(v1)),
        assert(not(structurallyValid(v1))),
        assert(repairable(stores.get(1), v1)),
        hold,
      })
      .then(move(1, v1, 0, 0, Unassigned))
      .then(all { assert(valid(v1)), hold })
  }

  // 両側が互いを依存先にして完了し、両側で Cancel した Issue 1 は値が等しくても衝突になる。
  // 取り込むと 0・1・2 が衝突し、解決の選択で完了済みの相互依存を作る。
  run mutualCompletedDepsResolved = {
    init
      .then(lifecycleOp(0, v0, 1, Cancel, 0, true))
      .then(lifecycleOp(0, v0, 2, Complete, 0, true))
      .then(addDep(0, v0, 0, 0, 2))
      .then(lifecycleOp(0, v0, 0, Complete, 0, true))
      .then(lifecycleOp(1, v1, 1, Cancel, 0, true))
      .then(lifecycleOp(1, v1, 0, Complete, 0, true))
      .then(addDep(1, v1, 2, 0, 0))
      .then(lifecycleOp(1, v1, 2, Complete, 0, true))
      .then(sync(1, 0, Full, own(0, 4)))
      .then(all { assert(v1.conflicted == Set(0, 1, 2)), hold })
      .then(resolve(1, v1, 1, 0, own(1, 0)))
      .then(resolve(1, v1, 0, 0, own(0, 7)))
      .then(resolve(1, v1, 2, 0, own(1, 3)))
  }
  // 完了済みの相互依存を解決の選択で作ってしまっても、違反に含まれる Entity の dependency は外せる。
  run mutualCompletedDepsRepairScenario = {
    mutualCompletedDepsResolved
      .then(all {
        assert(noConflicts(v1)),
        assert(hasViolation(v1, x => match x { | CompletionCycle(_) => true | _ => false })),
        assert(repairable(stores.get(1), v1)),
        hold,
      })
      .then(removeDep(1, v1, 0, 0, 2))
      .then(all { assert(valid(v1)), hold })
  }

  // 同じ ID を両側で登録すると衝突になり、片方の系列を選んで解決できる。
  run duplicateCreationResolvedScenario = {
    init
      .then(create(0, v0, 4, 0, Issue, NotStarted, Unassigned))
      .then(create(1, v1, 4, 1, Issue, Undecided, InGroup(2)))
      .then(sync(0, 1, Full, own(1, 0)))
      .then(all { assert(v0.conflicted.contains(4)), hold })
      .then(resolve(0, v0, 4, 0, own(0, 4)))
      .then(all { assert(v0.settled.contains(4)), assert(v0.current.get(4).life == NotStarted), hold })
  }

  // Start → Complete のうち Complete の記録だけを cherry-pick すると、base の作成記録と並んで head になり、
  // 実際には祖先どうしなのに衝突として見える。新しい方を選んで解決でき、後で Start が届いても値は変わらない。
  run gapConflictScenario = {
    init
      .then(lifecycleOp(0, v0, 3, Start, 0, false))
      .then(lifecycleOp(0, v0, 3, Complete, 0, false))
      .then(sync(1, 0, Single, own(0, 5)))
      .then(all {
        assert(v1.conflicted.contains(3)),
        assert(spuriousGapConflict(stores.get(1), v1)),
        hold,
      })
      .then(resolve(1, v1, 3, 1, own(0, 5)))
      .then(all { assert(v1.current.get(3).life == Completed), hold })
      .then(sync(1, 0, Full, own(0, 4)))
      .then(all { assert(v1.settled.contains(3)), assert(v1.current.get(3).life == Completed), assert(v1.gaps == Set()), hold })
  }

  // 免除は操作の前提を緩めるだけで、違反を増やす lifecycle 操作は通らない。
  // Completed の Group 0 の下の Cancelled の Issue 1 は循環上にあり、終了した親の固定を免除されるが、
  // Reconsider は終了した親の下に未終了の Entity を作る。Group 0 の Reopen も Completed の依存元 Group 2 による
  // 阻止を免除されるが、Group 2 に未完了の依存先を作る。いずれも前提は通り、検査で拒否される。
  // 修復は違反を減らす方向の dependency の削除で行う。
  run waivedLifecycleOpsNeverAddViolationsScenario = {
    mutualCompletedDepsResolved
      .then(all {
        assert(noConflicts(v1)),
        assert(v1.violations == Set(CompletionCycle(0), CompletionCycle(1), CompletionCycle(2))),
        assert(inViolation(v1, 1) and not(parentOpen(v1, 1))),
        assert(canLifecycle(v1, 1, Reconsider, true)),
        assert(not(noNewViolations(v1, lifecycleResult(stores.get(1), v1, 1, 1000, 1, Reconsider)))),
        assert(inViolation(v1, 0) and hasCompletedDependent(v1, 0)),
        assert(canLifecycle(v1, 0, Reopen, true)),
        assert(not(noNewViolations(v1, lifecycleResult(stores.get(1), v1, 1, 1000, 0, Reopen)))),
        assert(canLifecycle(v1, 2, Reopen, true)),
        assert(not(noNewViolations(v1, lifecycleResult(stores.get(1), v1, 1, 1000, 2, Reopen)))),
        assert(repairable(stores.get(1), v1)),
        hold,
      })
      .then(removeDep(1, v1, 0, 0, 2))
      .then(all { assert(valid(v1)), hold })
  }
  // 同じ状態で、免除された Reconsider・Reopen の action そのものが失敗する。
  run waivedLifecycleOpsAreRejectedScenario = {
    mutualCompletedDepsResolved
      .then(any {
        lifecycleOp(1, v1, 1, Reconsider, 0, true),
        lifecycleOp(1, v1, 0, Reopen, 0, true),
        lifecycleOp(1, v1, 2, Reopen, 0, true),
      }.fail())
  }

  // 違反を減らす lifecycle 操作は通る。片側が Issue 1 を完了し、他側がその依存先 Group 2 を Reopen して
  // 取り込むと、Completed の Issue 1 に未完了の依存先ができる。依存元の Issue 1 から Reopen すれば直る。
  run reopenDependentFirstRepairsScenario = {
    init
      .then(lifecycleOp(0, v0, 2, Complete, 0, true))
      .then(addDep(0, v0, 1, 0, 2))
      .then(sync(1, 0, Full, own(0, 4)))
      .then(lifecycleOp(0, v0, 1, Start, 0, false))
      .then(lifecycleOp(0, v0, 1, Complete, 0, false))
      .then(lifecycleOp(1, v1, 2, Reopen, 0, false))
      .then(sync(0, 1, Full, own(1, 0)))
      .then(all {
        assert(noConflicts(v0)),
        assert(v0.violations == Set(CompletedWithOpenDep(1))),
        hold,
      })
      .then(lifecycleOp(0, v0, 1, Reopen, 0, false))
      .then(all { assert(valid(v0)), hold })
  }

  // Completed の依存元による Reopen の阻止の免除が働くのは、その依存元がすでに未完了の依存先を持つ違反にあるとき。
  // Issue 1 は Group 2 に、Issue 3 は Issue 1 と Group 2 に依存し、片側が 1 と 3 を完了、他側が 2 を Reopen する。
  // 取り込むと 1 と 3 がともに Completed のまま未完了の依存先を持つ。3 は 1 の Completed の依存元だが、
  // 1 の Reopen は 3 の違反を新しく作らないので通り、続けて 3 を Reopen すれば直る。
  run reopenWaivedByExistingViolationScenario = {
    init
      .then(lifecycleOp(0, v0, 2, Complete, 0, true))
      .then(addDep(0, v0, 1, 0, 2))
      .then(addDep(0, v0, 3, 0, 1))
      .then(addDep(0, v0, 3, 0, 2))
      .then(sync(1, 0, Full, own(0, 4)))
      .then(lifecycleOp(0, v0, 1, Start, 0, false))
      .then(lifecycleOp(0, v0, 1, Complete, 0, false))
      .then(lifecycleOp(0, v0, 3, Start, 0, false))
      .then(lifecycleOp(0, v0, 3, Complete, 0, false))
      .then(lifecycleOp(1, v1, 2, Reopen, 0, false))
      .then(sync(0, 1, Full, own(1, 0)))
      .then(all {
        assert(v0.violations == Set(CompletedWithOpenDep(1), CompletedWithOpenDep(3))),
        assert(hasCompletedDependent(v0, 1)),
        hold,
      })
      .then(lifecycleOp(0, v0, 1, Reopen, 0, false))
      .then(all { assert(v0.violations == Set(CompletedWithOpenDep(3))), hold })
      .then(lifecycleOp(0, v0, 3, Reopen, 0, false))
      .then(all { assert(valid(v0)), hold })
  }

  // 祖先の連なりの途中に記録の欠けた Entity がある状態。replica 0 が Issue 3 を Start し、Group 4 を登録して
  // Group 0 をその下へ移し、replica 1 が Start と移動の記録だけを受け取ると、Group 0 は親 4 の記録を持たず、
  // Issue 1 の settled な祖先は NotStarted の Group 0 だけになる。
  run brokenAncestorChain = {
    init
      .then(lifecycleOp(0, v0, 3, Start, 0, false))
      .then(create(0, v0, 4, 0, Group, NotStarted, Unassigned))
      .then(move(0, v0, 0, 0, InGroup(4)))
      .then(sync(1, 0, Prefix, own(0, 4)))
      .then(sync(1, 0, Single, own(0, 6)))
      .then(all {
        assert(noConflicts(v1)),
        assert(v1.violations == Set(UnknownParent(0))),
        assert(ancestors(v1, 1) == Set(0)),
        assert(v1.current.get(0).life == NotStarted),
        assert(v1.current.get(3).life == InProgress),
        hold,
      })
  }
  // その下の Issue 1 は全祖先が採用済みとはみなされず Start は拒否され、着手済みの Issue 3 を Group 0 の下へ
  // 移す操作も拒否される。Group 4 の記録が届けばどちらも通る。
  run startUnderBrokenAncestorChainScenario = {
    brokenAncestorChain
      .then(all {
        assert(not(ancestorsAdopted(v1, 1))),
        assert(not(canLifecycle(v1, 1, Start, false))),
        assert(not(canMove(v1, 3, InGroup(0)))),
        hold,
      })
      .then(sync(1, 0, Full, own(0, 5)))
      .then(all { assert(valid(v1)), assert(canLifecycle(v1, 1, Start, false)), hold })
      .then(lifecycleOp(1, v1, 1, Start, 0, false))
      .then(move(1, v1, 3, 0, InGroup(0)))
      .then(all { assert(valid(v1)), hold })
  }
  run startUnderBrokenAncestorChainActionFailsScenario = {
    brokenAncestorChain.then(lifecycleOp(1, v1, 1, Start, 0, false).fail())
  }
  run moveUnderBrokenAncestorChainActionFailsScenario = {
    brokenAncestorChain.then(move(1, v1, 3, 0, InGroup(0)).fail())
  }

  // 両側の dependency の追加で Group 2 と Issue 3 の循環ができ、Group 0 と Issue 4 は Group 2 を待ち、
  // Group 0 は Issue 4 も待つ状態。
  run waitersOnCycle = {
    init
      .then(create(0, v0, 4, 0, Issue, NotStarted, Unassigned))
      .then(addDep(0, v0, 4, 0, 2))
      .then(addDep(0, v0, 0, 0, 2))
      .then(addDep(0, v0, 2, 0, 3))
      .then(addDep(1, v1, 3, 0, 2))
      .then(sync(0, 1, Full, own(1, 0)))
      .then(addDep(0, v0, 0, 0, 4))
  }
  // 通常完了経路の循環の違反は循環上の Entity だけに付き、循環を待つだけの Entity は違反にも免除にも含めない。
  // 待つ側どうしに新しい循環を足す操作（Issue 4 に Group 0 への dependency を足す）は、新しい循環が
  // 新しい構成員を加えるので拒否される。循環を待つ側への循環にならない dependency の追加は通る。
  run cycleAmongWaitersIsRejectedScenario = {
    waitersOnCycle
      .then(all {
        assert(noConflicts(v0)),
        assert(v0.violations == Set(CompletionCycle(2), CompletionCycle(3))),
        assert(Set(0, 1, 4).forall(e => not(inViolation(v0, e)))),
        assert(repairable(stores.get(0), v0)),
        assert(canAddDep(v0, 4, 0)),
        assert(not(noNewViolations(v0, addDepResult(stores.get(0), v0, 0, 1000, 4, 0)))),
        hold,
      })
      .then(addDep(0, v0, 1, 0, 4))
      .then(all { assert(v0.violations == Set(CompletionCycle(2), CompletionCycle(3))), hold })
      .then(removeDep(0, v0, 2, 0, 3))
      .then(all { assert(valid(v0)), hold })
  }
  run cycleAmongWaitersActionFailsScenario = {
    waitersOnCycle.then(addDep(0, v0, 4, 0, 0).fail())
  }

  // すでに同じ循環上にある Entity どうしの間の dependency（循環への chord）は構成員を変えないので通る。
  // 両側の追加で Group 2 → Issue 3 → Issue 4 → Group 2 の循環ができた状態で、Group 2 に Issue 4 への
  // dependency を足しても違反は変わらない。修復は循環の辺を一本ずつ外す。
  run chordOnCycleScenario = {
    init
      .then(create(0, v0, 4, 0, Issue, NotStarted, Unassigned))
      .then(sync(1, 0, Prefix, own(0, 4)))
      .then(addDep(0, v0, 3, 0, 4))
      .then(addDep(0, v0, 2, 0, 3))
      .then(addDep(1, v1, 4, 0, 2))
      .then(sync(0, 1, Full, own(1, 0)))
      .then(all {
        assert(noConflicts(v0)),
        assert(v0.violations == Set(CompletionCycle(2), CompletionCycle(3), CompletionCycle(4))),
        hold,
      })
      .then(addDep(0, v0, 2, 0, 4))
      .then(all {
        assert(v0.violations == Set(CompletionCycle(2), CompletionCycle(3), CompletionCycle(4))),
        hold,
      })
      .then(removeDep(0, v0, 3, 0, 4))
      .then(all {
        assert(v0.violations == Set(CompletionCycle(2), CompletionCycle(4))),
        assert(repairable(stores.get(0), v0)),
        hold,
      })
      .then(removeDep(0, v0, 2, 0, 4))
      .then(all { assert(valid(v0)), hold })
  }

  // 循環を待つ Group への登録も通る。Group 0 が循環上の Group 2 を待っていても、Group 0 は違反に含まれず、
  // その下への Issue 4 の登録は違反を増やさない。
  run registrationUnderWaitingGroupScenario = {
    init
      .then(addDep(0, v0, 0, 0, 2))
      .then(addDep(0, v0, 2, 0, 3))
      .then(addDep(1, v1, 3, 0, 2))
      .then(sync(0, 1, Full, own(1, 0)))
      .then(all {
        assert(v0.violations == Set(CompletionCycle(2), CompletionCycle(3))),
        assert(not(inViolation(v0, 0)) and not(inViolation(v0, 1))),
        hold,
      })
      .then(create(0, v0, 4, 0, Issue, NotStarted, InGroup(0)))
      .then(all {
        assert(v0.violations == Set(CompletionCycle(2), CompletionCycle(3))),
        assert(not(inViolation(v0, 4))),
        hold,
      })
      .then(removeDep(0, v0, 3, 0, 2))
      .then(all { assert(valid(v0)), hold })
  }
}
