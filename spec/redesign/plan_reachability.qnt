// plan_lifecycle の到達性を補う探索入口。init と各操作は plan_lifecycle のものをそのまま使い、
// 1 step で選ぶ操作を絞るだけで、許可条件・状態更新・検査する性質は変えない。
// 通常探索では取りやめ・見送り・変換が先行して到達しにくい経路を狙う。
// 探索の選び方であって、製品の workflow を定めるものではない。
module plan_reachability {
  import lifecycle_rules.* from "lifecycle_rules"
  import plan_lifecycle.* from "plan_lifecycle"

  // 採用・着手・完了・Reopen と移動・登録だけを選び、Reopen 後の再着手、完了済みの子を持つ Group の Reopen、
  // 三階層での孫 Issue の着手、完了済みの依存元による Reopen の拒否へ到達させる。
  action workAndReopen = {
    nondet id = IDS.oneOf()
    nondet target = IDS.oneOf()
    nondet op = Set(Accept, Start, Complete, Reopen).oneOf()
    nondet parent = PARENTS.oneOf()
    nondet kind = KINDS.oneOf()
    any {
      performGroup(id, op, true),
      performIssue(id, op),
      move(id, parent),
      register(id, kind, NotStarted, parent),
      addDependency(id, target),
    }
  }

  // 依存の追加と採用・着手・完了だけを選び、祖先や自身の依存先の完了で子孫の Issue が着手可能になる瞬間へ到達させる。
  action dependAndStart = {
    nondet id = IDS.oneOf()
    nondet target = IDS.oneOf()
    nondet op = Set(Accept, Start, Complete).oneOf()
    nondet parent = PARENTS.oneOf()
    any {
      addDependency(id, target),
      performGroup(id, op, true),
      performIssue(id, op),
      move(id, parent),
    }
  }

  // 条件の成立と採用・着手・移動・登録だけを選び、一覧で Ready と InProgress が重なる Group と、
  // 実効が InProgress の Group の移動へ到達させる。
  action listAndWork = {
    nondet id = IDS.oneOf()
    nondet op = Set(Accept, Start).oneOf()
    nondet parent = PARENTS.oneOf()
    nondet kind = KINDS.oneOf()
    any {
      changeCondition(id, true),
      performGroup(id, op, false),
      performIssue(id, op),
      move(id, parent),
      register(id, kind, NotStarted, parent),
    }
  }

  // lifecycle 操作と、Group 3 を Group 0 の下へ・Issue 1 を Group 3 の下へ置く移動だけを選び、
  // 完了済みの子を持つ Group の祖先を取りやめ・見送り・再検討で外したあとの採用・Reopen を集中して試す。
  // 着手・完了した仕事を抱える Group が未採用の祖先の下で実効 InProgress に戻らないことを確かめる。
  action readoptAfterClose = {
    nondet id = IDS.oneOf()
    nondet op = OPERATIONS.oneOf()
    any {
      performGroup(id, op, true),
      performIssue(id, op),
      move(3, InGroup(0)),
      move(1, InGroup(3)),
    }
  }
}
