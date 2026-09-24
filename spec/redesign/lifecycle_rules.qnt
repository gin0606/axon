// 再設計後の Issue と Group が共有する lifecycle の状態と基本遷移だけを持つ純粋な定義。
// 状態変数も action も持たず、plan_lifecycle がこの module を import して遷移規則を共有する。
// Group は保存される lifecycle（stored）として InProgress を持たず、Start・Release を受け付けない。
// Group の完了は stored が NotStarted のときに行い、進行中かどうかは子から導出する（plan_lifecycle）。
// 対象外: 包含、dependency、実効 lifecycle の導出、再浮上条件、文面と記録。
module lifecycle_rules {
  // 状態は5種類、明示操作は8種類。Reopen は完了を取り消して未着手へ戻す。
  // Kind は Entity の現在の種類で、変換操作によって変わりうる（plan_lifecycle）。
  type Lifecycle = Undecided | NotStarted | InProgress | Completed | Cancelled
  type Operation = Accept | Withdraw | Start | Release | Complete | Cancel | Reconsider | Reopen
  type Kind = Issue | Group

  // Issue の基本遷移の前提。Completed は Reopen だけを、Cancelled は Reconsider だけを受ける。
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
      | Reopen => current == Completed
    }

  // 種類ごとの基本遷移の前提。Group は Start・Release を持たず、保存される lifecycle が
  // NotStarted のまま完了し、取りやめは Undecided・NotStarted からだけ行う。
  // Group の Withdraw が実効 lifecycle を見ることなど、包含に依存する前提は plan_lifecycle が加える。
  pure def canPerformAs(kind: Kind, current: Lifecycle, op: Operation): bool =
    match kind {
      | Issue => canPerform(current, op)
      | Group => match op {
        | Start => false
        | Release => false
        | Complete => current == NotStarted
        | Cancel => Set(Undecided, NotStarted).contains(current)
        | _ => canPerform(current, op)
      }
    }

  // 操作の遷移先。前提の判定は canPerform・canPerformAs が担う。
  pure def applyOperation(current: Lifecycle, op: Operation): Lifecycle =
    match op {
      | Accept => NotStarted
      | Withdraw => Undecided
      | Start => InProgress
      | Release => NotStarted
      | Complete => Completed
      | Cancel => Cancelled
      | Reconsider => Undecided
      | Reopen => NotStarted
    }
}
