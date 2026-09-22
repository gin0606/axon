// group_lifecycle の到達性を補う探索入口。init と各操作は group_lifecycle のものをそのまま使い、
// 1 step で選ぶ操作を絞るだけで、許可条件・状態更新・検査する性質は変えない。
// 通常探索では依存の追加や取りやめが先行して到達しにくい経路を狙う。
// 探索の選び方であって、製品の workflow を定めるものではない。
// 対応する契約: ../docs/reference/lifecycle.md
module lifecycle_reachability {
  import lifecycle_rules.* from "lifecycle_rules"
  import group_lifecycle.* from "group_lifecycle"

  // 依存を持たないまま採用・着手・移動・登録を選び、進行中の部分木の移動へ到達させる。
  action workAndMove = {
    nondet group = GROUPS.oneOf()
    nondet id = ISSUES.oneOf()
    nondet op = Set(Accept, Start).oneOf()
    nondet parent = PARENTS.oneOf()
    nondet registration = Set(Proposal, AdoptedWork).oneOf()
    any {
      performGroup(group, op, false),
      performIssue(id, op),
      moveGroup(group, parent),
      moveIssue(id, parent),
      registerIssue(id, parent, registration),
    }
  }

  // 採用・着手・完了・取りやめと計画1の計画0への所属を選び、
  // 計画0が最終確認可能になった時点での新規登録へ到達させる。
  action finishAndExtend = {
    nondet group = GROUPS.oneOf()
    nondet id = ISSUES.oneOf()
    nondet groupOp = Set(Accept, Start, Complete).oneOf()
    nondet issueOp = Set(Accept, Start, Complete, Cancel).oneOf()
    nondet satisfied = Set(false, true).oneOf()
    any {
      performGroup(group, groupOp, true),
      performIssue(id, issueOp),
      moveGroup(1, InGroup(0)),
      all {
        children(plan, 0).size() > 0,
        canGroup(plan, 0, Complete, true),
        registerIssue(2, InGroup(0), Proposal),
      },
      changeGroupCondition(group, satisfied),
    }
  }

  // 依存を張ったうえで採用・着手・完了だけを選び、依存先の完了による後続の解禁と、
  // 全子完了を経た計画の完了へ到達させる。移動・取りやめ・再検討・最終確認の不合格は選ばない。
  action dependAndFinish = {
    nondet group = GROUPS.oneOf()
    nondet source = ISSUES.oneOf()
    nondet target = ISSUES.oneOf()
    nondet op = Set(Accept, Start, Complete).oneOf()
    any {
      addDependency(IssueRef(source), IssueRef(target)),
      performIssue(source, op),
      performGroup(group, op, true),
    }
  }
}
