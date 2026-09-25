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

  // 完了済みの相互依存を解決の選択で作ってしまっても、違反に含まれる Entity の dependency は外せる。
  // 両側で Cancel した Issue 1 は値が等しくても衝突になり、先に解決する。
  run mutualCompletedDepsRepairScenario = {
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
}
