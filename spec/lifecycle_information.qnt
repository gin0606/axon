// title・description の編集範囲、Note の追記、状態変更履歴、種類の変換を固定2 Entity（Issue として登録した
// 1件と Group として登録した1件）で扱うモデル。Issue と Group に共通するローカルな情報操作だけを対象にする。
// 文面の内容は不透明な整数で更新と保持だけを区別し、履歴は lifecycle 遷移の変更前後の状態と種類の変換の
// 追記列へ抽象化する。
// 対象外: 包含・dependency・Group の実効 lifecycle と最終確認による追加の制約、Group から Issue への変換が
// 要求する「子がない」（いずれも group_lifecycle が扱う）、日時・任意の理由・記録者情報、表示順、公開 ID、
// 永続化と保存失敗。
// 基本遷移は lifecycle_rules の種類ごとの規則を import するため、このモデルが許す遷移だけで
// 実際の Group や依存を持つ Issue の操作が許可されるわけではない。
// 対応する契約: ../docs/reference/lifecycle.md
module lifecycle_information {
  import lifecycle_rules.* from "lifecycle_rules"

  type Id = int

  // Entity ごとに現在の種類、保存される現在の lifecycle、文面、追記列の Note、履歴を保持する。
  // Group の lifecycle は保存値で、子から導出する実効値はこのモデルの外。
  // 履歴は lifecycle 遷移の記録と種類の変換の記録の追記列。変換は lifecycle 遷移ではないが記録として残り、
  // 変換前後の種類が読める。lifecycle 遷移の記録がどの種類の規則に従うかは、履歴を先頭から再生したときの
  // その時点の種類で決まり、Entity の現在の種類では決まらない。
  // Event は検証用の観測が区別するイベントの種類。
  type HistoryEntry = Transition({ before: Lifecycle, after: Lifecycle }) | Conversion({ from: Kind, to: Kind })
  type EntityInformation = {
    kind: Kind,
    lifecycle: Lifecycle,
    title: int,
    description: int,
    notes: List[int],
    history: List[HistoryEntry],
  }
  type Event = Initial | TitleEdited(Id) | DescriptionEdited(Id) | NoteAdded({ id: Id, body: int })
    | Transitioned({ id: Id, op: Operation }) | Converted(Id)
  type Observation = { before: Id -> EntityInformation, event: Event }

  // 固定2 Entity（0 は Issue、1 は Group として登録）、文面の値は3種類。登録操作と初期状態の記録は扱わず、
  // 登録時の種類（記録の集合では登録の記録が持つ現在値）は ID から引く。
  pure val IDS = Set(0, 1)
  pure val TEXTS = Set(0, 1, 2)
  pure def initialKind(id: Id): Kind = if (id == 0) Issue else Group
  var information: Id -> EntityInformation
  var observation: Observation

  // 未判断の登録済み Entity から始める。
  pure val initialInformation: Id -> EntityInformation = IDS.mapBy(id => {
    kind: initialKind(id),
    lifecycle: Undecided, title: 0, description: 0, notes: List(), history: List(),
  })
  action init = all {
    information' = initialInformation,
    observation' = { before: initialInformation, event: Initial },
  }

  // 文面を編集できるのは Undecided・NotStarted・InProgress だけ。Completed・Cancelled では固定し、
  // 取りやめた Entity は Reconsider で未判断へ、完了した Entity は Reopen で未着手へ戻れば再び編集できる。
  pure def editable(e: EntityInformation): bool =
    Set(Undecided, NotStarted, InProgress).contains(e.lifecycle)
  pure def canEditTitle(e: EntityInformation, value: int): bool = editable(e) and e.title != value
  pure def canEditDescription(e: EntityInformation, value: int): bool = editable(e) and e.description != value
  pure def applyTitle(e: EntityInformation, value: int): EntityInformation = { ...e, title: value }
  pure def applyDescription(e: EntityInformation, value: int): EntityInformation = { ...e, description: value }

  // Note は追記だけで、既存の記録を書き換えない。同じ内容も別の記録として残る。
  pure def applyNote(e: EntityInformation, body: int): EntityInformation = { ...e, notes: e.notes.append(body) }

  // 状態変更と履歴追加は一つの更新として確定し、片方だけを残さない。前提は種類ごとの基本遷移で、
  // Group は Start・Release を持たず NotStarted から完了する。
  pure def canTransition(e: EntityInformation, op: Operation): bool = canPerformAs(e.kind, e.lifecycle, op)
  pure def applyTransition(e: EntityInformation, op: Operation): EntityInformation = {
    val targetState = applyOperation(e.lifecycle, op)
    { ...e, lifecycle: targetState,
      history: e.history.append(Transition({ before: e.lifecycle, after: targetState })) }
  }

  // 種類の変換。保存値が Undecided・NotStarted のときだけ行い（InProgress の Issue は先に Release する）、
  // 種類と履歴への変換の追記以外（lifecycle・文面・Note）を変えない。
  pure def otherKind(kind: Kind): Kind = match kind { | Issue => Group | Group => Issue }
  pure def canConvert(e: EntityInformation): bool = Set(Undecided, NotStarted).contains(e.lifecycle)
  pure def applyConvert(e: EntityInformation): EntityInformation =
    { ...e, kind: otherKind(e.kind),
      history: e.history.append(Conversion({ from: e.kind, to: otherKind(e.kind) })) }

  // 5種類の操作。文面編集は現在値だけを変え、Note 追加はどの状態でも許して状態を変えない。
  action editTitle(id: Id, value: int): bool = all {
    canEditTitle(information.get(id), value),
    information' = information.set(id, applyTitle(information.get(id), value)),
    observation' = { before: information, event: TitleEdited(id) },
  }
  action editDescription(id: Id, value: int): bool = all {
    canEditDescription(information.get(id), value),
    information' = information.set(id, applyDescription(information.get(id), value)),
    observation' = { before: information, event: DescriptionEdited(id) },
  }
  action addNote(id: Id, body: int): bool = all {
    information' = information.set(id, applyNote(information.get(id), body)),
    observation' = { before: information, event: NoteAdded({ id: id, body: body }) },
  }
  action transition(id: Id, op: Operation): bool = all {
    canTransition(information.get(id), op),
    information' = information.set(id, applyTransition(information.get(id), op)),
    observation' = { before: information, event: Transitioned({ id: id, op: op }) },
  }
  action convert(id: Id): bool = all {
    canConvert(information.get(id)),
    information' = information.set(id, applyConvert(information.get(id))),
    observation' = { before: information, event: Converted(id) },
  }

  // 文面編集、Note 追加、状態変更、種類の変換を混ぜて探索する。
  action step = {
    nondet id = IDS.oneOf()
    nondet value = TEXTS.oneOf()
    nondet op = OPERATIONS.oneOf()
    any { editTitle(id, value), editDescription(id, value), addNote(id, value), transition(id, op), convert(id) }
  }
  pure def affected(event: Event): Set[Id] = match event {
    | Initial => Set()
    | TitleEdited(id) => Set(id)
    | DescriptionEdited(id) => Set(id)
    | NoteAdded(change) => Set(change.id)
    | Transitioned(change) => Set(change.id)
    | Converted(id) => Set(id)
  }

  // 履歴の再生。lifecycle 遷移の記録はその時点の種類の規則で判定し、変換の記録は変換前の種類と
  // 変換できる lifecycle を確認して種類を進める。kind に再生中の種類でなく固定の種類を渡すと、
  // 現在の種類で全記録を判定する誤った再生になる（wHistoryRejectedByCurrentKind が観測する）。
  type Replay = { valid: bool, current: Lifecycle, kind: Kind }
  pure def replayEntry(acc: Replay, entry: HistoryEntry, kindAtEntry: Kind): Replay = match entry {
    | Transition(t) => {
        valid: acc.valid and acc.current == t.before and OPERATIONS.exists(op =>
          canPerformAs(kindAtEntry, t.before, op) and applyOperation(t.before, op) == t.after),
        current: t.after, kind: acc.kind,
      }
    | Conversion(c) => {
        valid: acc.valid and acc.kind == c.from and c.to == otherKind(c.from)
          and Set(Undecided, NotStarted).contains(acc.current),
        current: acc.current, kind: c.to,
      }
  }
  pure def replayByKindAtEachEntry(id: Id, e: EntityInformation): Replay =
    e.history.foldl({ valid: true, current: Undecided, kind: initialKind(id) },
      (acc, entry) => replayEntry(acc, entry, acc.kind))
  pure def replayByCurrentKind(id: Id, e: EntityInformation): Replay =
    e.history.foldl({ valid: true, current: Undecided, kind: initialKind(id) },
      (acc, entry) => replayEntry(acc, entry, e.kind))

  // invariant: 操作が対象以外の Entity を変えないこと、編集が編集可能状態に限ること、
  // 終了後の文面が固定されること、Note が追記専用であること、
  // 状態変更と履歴が一体で確定し、履歴を各時点の種類で再生すると現在の状態と種類になること、
  // 完了から抜けるのは Reopen だけで未着手へ戻ること、
  // 種類は変換だけで変わり、変換は種類と履歴への変換の追記以外を変えないこと。
  val invOtherEntitiesUnchanged = IDS.exclude(affected(observation.event)).forall(id =>
    information.get(id) == observation.before.get(id))
  val invTextEditScope = match observation.event {
    | TitleEdited(id) => editable(observation.before.get(id))
      and information.get(id) == { ...observation.before.get(id), title: information.get(id).title }
    | DescriptionEdited(id) => editable(observation.before.get(id))
      and information.get(id) == { ...observation.before.get(id), description: information.get(id).description }
    | _ => true
  }
  val invTerminalTextFixed = IDS.forall(id => not(editable(observation.before.get(id))) implies
    (information.get(id).title == observation.before.get(id).title
      and information.get(id).description == observation.before.get(id).description))
  val invNoteAppendOnly = match observation.event {
    | NoteAdded(change) => information.get(change.id) == {
        ...observation.before.get(change.id),
        notes: observation.before.get(change.id).notes.append(change.body),
      }
    | _ => IDS.forall(id => information.get(id).notes == observation.before.get(id).notes)
  }
  val invTransitionAndHistoryAtomic = match observation.event {
    | Transitioned(change) => {
        val before = observation.before.get(change.id)
        val after = information.get(change.id)
        and {
          canTransition(before, change.op),
          after.lifecycle == applyOperation(before.lifecycle, change.op),
          after.history == before.history.append(Transition({
            before: before.lifecycle, after: after.lifecycle,
          })),
          after.kind == before.kind,
          after.title == before.title, after.description == before.description, after.notes == before.notes,
        }
      }
    | Converted(_) => IDS.forall(id => information.get(id).lifecycle == observation.before.get(id).lifecycle)
    | _ => IDS.forall(id => information.get(id).lifecycle == observation.before.get(id).lifecycle
      and information.get(id).history == observation.before.get(id).history)
  }
  val invHistoryExplainsCurrentState = IDS.forall(id => {
    val replay = replayByKindAtEachEntry(id, information.get(id))
    replay.valid and replay.current == information.get(id).lifecycle and replay.kind == information.get(id).kind
  })
  val invCompletedLeavesOnlyByReopen = IDS.forall(id =>
    (observation.before.get(id).lifecycle == Completed and information.get(id).lifecycle != Completed)
      implies (observation.event == Transitioned({ id: id, op: Reopen })
        and information.get(id).lifecycle == NotStarted))
  val invKindChangesOnlyByConversion = match observation.event {
    | Converted(id) => {
        val before = observation.before.get(id)
        val after = information.get(id)
        and {
          canConvert(before),
          after.kind == otherKind(before.kind),
          after == { ...before, kind: after.kind,
            history: before.history.append(Conversion({ from: before.kind, to: after.kind })) },
        }
      }
    | _ => IDS.forall(id => information.get(id).kind == observation.before.get(id).kind)
  }
  val invGroupNeverStoredInProgress = IDS.forall(id =>
    information.get(id).kind == Group implies information.get(id).lifecycle != InProgress)

  // witness: 8遷移それぞれの到達（Start・Release は Issue だけ）、Group の NotStarted からの完了、
  // 採用後・着手中・再検討後・Reopen 後の文面編集、各状態での Note 追加、同じ内容の追記が別の記録になること、
  // 両方向の変換、変換後の lifecycle 遷移、Start・Release した Issue の Group への変換と
  // Complete・Reopen した Group の Issue への変換（どちらも現在の種類で再生すると不正と判定される履歴）。
  pure def isTransition(event: Event, op: Operation): bool = match event {
    | Transitioned(change) => change.op == op
    | _ => false
  }
  pure def lastEntryIsTransition(h: List[HistoryEntry], before: Lifecycle, after: Lifecycle): bool =
    if (h.length() == 0) false else match h.nth(h.length() - 1) {
      | Transition(t) => t.before == before and t.after == after
      | Conversion(_) => false
    }
  pure def hasTransition(h: List[HistoryEntry], before: Lifecycle, after: Lifecycle): bool =
    h.select(entry => match entry {
      | Transition(t) => t.before == before and t.after == after
      | Conversion(_) => false
    }).length() > 0
  pure def hasAdjacentTransitions(h: List[HistoryEntry], first: Lifecycle, middle: Lifecycle, last: Lifecycle): bool =
    h.indices().exists(i => i + 1 < h.length() and match h.nth(i) {
      | Transition(t) => t.before == first and t.after == middle and match h.nth(i + 1) {
        | Transition(u) => u.before == middle and u.after == last
        | Conversion(_) => false
      }
      | Conversion(_) => false
    })
  pure def hasConversion(h: List[HistoryEntry]): bool =
    h.select(entry => match entry { | Transition(_) => false | Conversion(_) => true }).length() > 0
  val wAccept = isTransition(observation.event, Accept)
  val wWithdraw = isTransition(observation.event, Withdraw)
  val wStart = isTransition(observation.event, Start)
  val wRelease = isTransition(observation.event, Release)
  val wComplete = isTransition(observation.event, Complete)
  val wCancel = isTransition(observation.event, Cancel)
  val wReconsider = isTransition(observation.event, Reconsider)
  val wReopen = isTransition(observation.event, Reopen)
  val wGroupComplete = match observation.event {
    | Transitioned(change) => change.op == Complete and information.get(change.id).kind == Group
      and observation.before.get(change.id).lifecycle == NotStarted
    | _ => false
  }
  val wTitleEdit = match observation.event { | TitleEdited(_) => true | _ => false }
  val wDescriptionEdit = match observation.event { | DescriptionEdited(_) => true | _ => false }
  val wEditAfterAcceptance = affected(observation.event).exists(id =>
    (wTitleEdit or wDescriptionEdit) and information.get(id).lifecycle == NotStarted)
  val wEditWhileWorking = affected(observation.event).exists(id =>
    (wTitleEdit or wDescriptionEdit) and information.get(id).lifecycle == InProgress)
  val wEditAfterReconsider = affected(observation.event).exists(id =>
    (wTitleEdit or wDescriptionEdit) and lastEntryIsTransition(information.get(id).history, Cancelled, Undecided))
  val wEditAfterReopen = affected(observation.event).exists(id =>
    (wTitleEdit or wDescriptionEdit) and lastEntryIsTransition(information.get(id).history, Completed, NotStarted))
  val wNoteUndecided = match observation.event {
    | NoteAdded(change) => information.get(change.id).lifecycle == Undecided
    | _ => false
  }
  val wNoteNotStarted = match observation.event {
    | NoteAdded(change) => information.get(change.id).lifecycle == NotStarted
    | _ => false
  }
  val wNoteInProgress = match observation.event {
    | NoteAdded(change) => information.get(change.id).lifecycle == InProgress
    | _ => false
  }
  val wNoteCompleted = match observation.event {
    | NoteAdded(change) => information.get(change.id).lifecycle == Completed
    | _ => false
  }
  val wNoteCancelled = match observation.event {
    | NoteAdded(change) => information.get(change.id).lifecycle == Cancelled
    | _ => false
  }
  val wRepeatedNote = match observation.event {
    | NoteAdded(change) => information.get(change.id).notes.select(body => body == change.body).length() > 1
    | _ => false
  }
  val wConvertToGroup = match observation.event {
    | Converted(id) => information.get(id).kind == Group
    | _ => false
  }
  val wConvertToIssue = match observation.event {
    | Converted(id) => information.get(id).kind == Issue
    | _ => false
  }
  val wTransitionAfterConversion = match observation.event {
    | Transitioned(change) => hasConversion(observation.before.get(change.id).history)
    | _ => false
  }
  // Issue として Start し続けて Release した後に Group へ変換した履歴。Group の規則には NotStarted → InProgress がない。
  // Start の後に Complete・Reopen や Cancel・Reconsider で戻った履歴は数えない。
  val wConvertToGroupAfterStartAndRelease = match observation.event {
    | Converted(id) => information.get(id).kind == Group
      and hasAdjacentTransitions(observation.before.get(id).history, NotStarted, InProgress, NotStarted)
    | _ => false
  }
  // Group として NotStarted から Complete し Reopen した後に Issue へ変換した履歴。
  // Issue の規則には NotStarted → Completed がない。
  val wConvertToIssueAfterReopen = match observation.event {
    | Converted(id) => information.get(id).kind == Issue
      and hasTransition(observation.before.get(id).history, NotStarted, Completed)
    | _ => false
  }
  // 各時点の種類では妥当な履歴が、現在の種類で全記録を判定すると不正になる状態。
  val wHistoryRejectedByCurrentKind = IDS.exists(id => not(replayByCurrentKind(id, information.get(id)).valid))

}
