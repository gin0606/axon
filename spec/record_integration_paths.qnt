// record_integration の到達性を補う探索入口。init と各操作は record_integration のものをそのまま使い、
// 1 step で選ぶ操作を絞るだけで、許可条件・状態更新・検査する性質は変えない。
// 通常探索では、同じ Entity への両側の操作と取り込みが噛み合う経路や、
// 構造の不整合を作ってから直す経路に到達しにくいため、それらを狙う。
// 探索の選び方であって、製品の workflow を定めるものではない。
module record_integration_paths {
  import record_integration.* from "record_integration"

  // 両 replica が Issue 1 だけを操作し、全部取り込みと解決を繰り返す。
  // 並行した Start・Accept の衝突、完了と解放の衝突、解決記録どうしの並行へ到達させる。
  action divergeAndResolve = {
    nondet r = REPLICAS.oneOf()
    nondet from = REPLICAS.oneOf()
    nondet op = Set(Accept, Withdraw, Start, Release, Complete, Cancel, Reconsider).oneOf()
    nondet actor = ACTORS.oneOf()
    nondet anchor = stores.get(from).records.oneOf()
    nondet pick = stores.get(r).records.oneOf()
    val v = views.get(r)
    any {
      lifecycleOp(r, v, 1, op, actor, true),
      resolve(r, v, 1, actor, pick),
      sync(r, from, Full, anchor),
    }
  }

  // 着手・完了・再開・取りやめ、所属変更、dependency の追加、Group 0 か 2 への Issue 4 の登録と、
  // 全部取り込みを選ぶ。終了した Group への子の流入、両側の移動による循環、
  // 完了済みへの未完了 dependency の流入と、Reopen による修復へ到達させる。
  action breakAndRepair = {
    nondet r = REPLICAS.oneOf()
    nondet from = REPLICAS.oneOf()
    nondet e = ENTITIES.oneOf()
    nondet op = Set(Start, Complete, Reopen, Cancel).oneOf()
    nondet actor = ACTORS.oneOf()
    nondet p = Set(Unassigned, InGroup(0), InGroup(2)).oneOf()
    nondet t = ENTITIES.oneOf()
    nondet anchor = stores.get(from).records.oneOf()
    nondet pick = stores.get(r).records.oneOf()
    val v = views.get(r)
    any {
      lifecycleOp(r, v, e, op, actor, true),
      move(r, v, e, actor, p),
      addDep(r, v, e, actor, t),
      create(r, v, 4, actor, Issue, NotStarted, p),
      resolve(r, v, e, actor, pick),
      sync(r, from, Full, anchor),
    }
  }

  // 両側で Group 0 と 2 を互いの下へ移し、全部取り込みで包含の循環へ到達させる。
  action crossMoves = {
    nondet r = REPLICAS.oneOf()
    nondet from = REPLICAS.oneOf()
    nondet actor = ACTORS.oneOf()
    nondet anchor = stores.get(from).records.oneOf()
    val v = views.get(r)
    any {
      move(r, v, 0, actor, InGroup(2)),
      move(r, v, 2, actor, InGroup(0)),
      sync(r, from, Full, anchor),
    }
  }

  // Group 2 を完了して Issue 1 の dependency にし、片側で 1 を完了、他側で 2 を Reopen して、
  // 完了済みの Entity に未完了の dependency が流入する状態へ到達させる。
  // dependency の追加と完了は同じ Entity の記録を変えるので、その組合せは衝突になり黙って通らない。
  action reopenUnderDependent = {
    nondet r = REPLICAS.oneOf()
    nondet from = REPLICAS.oneOf()
    nondet e = Set(1, 2).oneOf()
    nondet op = Set(Start, Complete, Reopen).oneOf()
    nondet actor = ACTORS.oneOf()
    nondet anchor = stores.get(from).records.oneOf()
    nondet pick = stores.get(r).records.oneOf()
    val v = views.get(r)
    any {
      lifecycleOp(r, v, e, op, actor, true),
      addDep(r, v, 1, actor, 2),
      resolve(r, v, e, actor, pick),
      sync(r, from, Full, anchor),
    }
  }

  // Issue 1 を終了して Group 0 を完了し、他側で Group 0 に Issue 4 を登録して取り込み、
  // 終了した Group への子の流入を Group 0 の Reopen で直す経路と、完了済みの子を持つ Group の Reopen へ到達させる。
  action reopenAfterInflow = {
    nondet r = REPLICAS.oneOf()
    nondet from = REPLICAS.oneOf()
    nondet actor = ACTORS.oneOf()
    nondet op = Set(Start, Complete, Cancel).oneOf()
    nondet anchor = stores.get(from).records.oneOf()
    val v = views.get(r)
    any {
      lifecycleOp(r, v, 1, op, actor, true),
      lifecycleOp(r, v, 0, Complete, actor, true),
      lifecycleOp(r, v, 0, Reopen, actor, true),
      create(r, v, 4, actor, Issue, NotStarted, InGroup(0)),
      sync(r, from, Full, anchor),
    }
  }
}
