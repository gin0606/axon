// Issue と Group が共有する lifecycle の状態と基本遷移だけを持つ純粋な定義。
// 状態変数も action も持たず、他の全モデルがこの module を import して遷移規則を共有する。
// 対象外: 包含、dependency、再浮上条件、文面と記録。
// 対応する契約: ../docs/reference/lifecycle.md
module lifecycle_rules {
  // 状態は5種類、明示操作は7種類。Issue と計画（Group）で同じものを使う。
  type Lifecycle = Undecided | NotStarted | InProgress | Completed | Cancelled
  type Operation = Accept | Withdraw | Start | Release | Complete | Cancel | Reconsider

  // 操作の前提。Completed はどの操作も受け付けず、Cancelled は Reconsider だけを受ける。
  // 前提を満たさない操作は無効となり、状態を変えない。
  pure def canPerform(current: Lifecycle, op: Operation): bool =
    match op {
      | Accept => current == Undecided
      | Withdraw => current == NotStarted
      | Start => current == NotStarted
      | Release => current == InProgress
      | Complete => current == InProgress
      | Cancel => Set(Undecided, NotStarted, InProgress).contains(current)
      | Reconsider => current == Cancelled
    }

  // 操作の遷移先。前提の判定は canPerform が担う。
  pure def applyOperation(current: Lifecycle, op: Operation): Lifecycle =
    match op {
      | Accept => NotStarted
      | Withdraw => Undecided
      | Start => InProgress
      | Release => NotStarted
      | Complete => Completed
      | Cancel => Cancelled
      | Reconsider => Undecided
    }
}
