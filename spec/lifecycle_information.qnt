// title・description の編集範囲、Note の追記、状態変更履歴を固定2 Entity で扱うモデル。
// Issue と Group に共通するローカルな情報操作だけを対象にする。
// 文面の内容は不透明な整数で更新と保持だけを区別し、履歴は変更前後の状態の追記列へ抽象化する。
// 対象外: 包含・dependency・Group の最終確認による追加の制約（group_lifecycle が扱う）、
// 日時・任意の理由・記録者情報、表示順、公開 ID、永続化と保存失敗。
// 基本遷移は lifecycle_rules を import するため、このモデルが許す遷移だけで
// 実際の Group や依存を持つ Issue の操作が許可されるわけではない。
// 対応する契約: ../docs/reference/lifecycle.md
module lifecycle_information {
  import lifecycle_rules.* from "lifecycle_rules"

  type Id = int

  // Entity ごとに現在の文面、追記列の Note、状態変更履歴を保持する。
  // Event は検証用の観測が区別するイベントの種類。
  type TransitionRecord = { before: Lifecycle, after: Lifecycle }
  type EntityInformation = {
    lifecycle: Lifecycle,
    title: int,
    description: int,
    notes: List[int],
    history: List[TransitionRecord],
  }
  type Event = Initial | TitleEdited(Id) | DescriptionEdited(Id) | NoteAdded({ id: Id, body: int })
    | Transitioned({ id: Id, op: Operation })
  type Observation = { before: Id -> EntityInformation, event: Event }

  // 固定2 Entity、文面の値は3種類。登録操作と初期状態の記録は扱わない。
  pure val IDS = Set(0, 1)
  pure val TEXTS = Set(0, 1, 2)
  pure val OPERATIONS = Set(Accept, Withdraw, Start, Release, Complete, Cancel, Reconsider)
  var information: Id -> EntityInformation
  var observation: Observation

  // 未判断の登録済み Entity から始める。
  pure val initialInformation: Id -> EntityInformation = IDS.mapBy(_ => {
    lifecycle: Undecided, title: 0, description: 0, notes: List(), history: List(),
  })
  action init = all {
    information' = initialInformation,
    observation' = { before: initialInformation, event: Initial },
  }

  // 文面を編集できるのは Undecided・NotStarted・InProgress だけ。Completed・Cancelled では固定し、
  // 取りやめた Entity は Reconsider で未判断へ戻れば再び編集できる。
  pure def editable(e: EntityInformation): bool =
    Set(Undecided, NotStarted, InProgress).contains(e.lifecycle)
  pure def canEditTitle(e: EntityInformation, value: int): bool = editable(e) and e.title != value
  pure def canEditDescription(e: EntityInformation, value: int): bool = editable(e) and e.description != value
  pure def applyTitle(e: EntityInformation, value: int): EntityInformation = { ...e, title: value }
  pure def applyDescription(e: EntityInformation, value: int): EntityInformation = { ...e, description: value }

  // Note は追記だけで、既存の記録を書き換えない。同じ内容も別の記録として残る。
  pure def applyNote(e: EntityInformation, body: int): EntityInformation = { ...e, notes: e.notes.append(body) }

  // 状態変更と履歴追加は一つの更新として確定し、片方だけを残さない。
  pure def applyTransition(e: EntityInformation, op: Operation): EntityInformation = {
    val targetState = applyOperation(e.lifecycle, op)
    { ...e, lifecycle: targetState,
      history: e.history.append({ before: e.lifecycle, after: targetState }) }
  }

  // 4種類の操作。文面編集は現在値だけを変え、Note 追加はどの状態でも許して状態を変えない。
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
    canPerform(information.get(id).lifecycle, op),
    information' = information.set(id, applyTransition(information.get(id), op)),
    observation' = { before: information, event: Transitioned({ id: id, op: op }) },
  }

  // 文面編集、Note 追加、状態変更を混ぜて探索する。
  action step = {
    nondet id = IDS.oneOf()
    nondet value = TEXTS.oneOf()
    nondet op = OPERATIONS.oneOf()
    any { editTitle(id, value), editDescription(id, value), addNote(id, value), transition(id, op) }
  }
  pure def affected(event: Event): Set[Id] = match event {
    | Initial => Set()
    | TitleEdited(id) => Set(id)
    | DescriptionEdited(id) => Set(id)
    | NoteAdded(change) => Set(change.id)
    | Transitioned(change) => Set(change.id)
  }

  // invariant: 操作が対象以外の Entity を変えないこと、編集が編集可能状態に限ること、
  // 終了後の文面が固定されること、Note が追記専用であること、
  // 状態変更と履歴が一体で確定し、履歴を再生すると現在の状態になること、完了が戻らないこと。
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
          canPerform(before.lifecycle, change.op),
          after.lifecycle == applyOperation(before.lifecycle, change.op),
          after.history == before.history.append({
            before: before.lifecycle, after: after.lifecycle,
          }),
          after.title == before.title, after.description == before.description, after.notes == before.notes,
        }
      }
    | _ => IDS.forall(id => information.get(id).lifecycle == observation.before.get(id).lifecycle
      and information.get(id).history == observation.before.get(id).history)
  }
  val invHistoryExplainsCurrentState = IDS.forall(id => {
    val e = information.get(id)
    val replay = e.history.foldl({ valid: true, current: Undecided }, (acc, entry) => {
      valid: acc.valid and acc.current == entry.before and OPERATIONS.exists(op =>
        canPerform(entry.before, op) and applyOperation(entry.before, op) == entry.after),
      current: entry.after,
    })
    replay.valid and replay.current == e.lifecycle
  })
  val invCompletedStaysCompleted = IDS.forall(id =>
    observation.before.get(id).lifecycle == Completed implies information.get(id).lifecycle == Completed)

  // witness: 7遷移それぞれの到達、採用後・着手中・再検討後の文面編集、
  // 各状態での Note 追加、同じ内容の追記が別の記録になること。
  pure def isTransition(event: Event, op: Operation): bool = match event {
    | Transitioned(change) => change.op == op
    | _ => false
  }
  val wAccept = isTransition(observation.event, Accept)
  val wWithdraw = isTransition(observation.event, Withdraw)
  val wStart = isTransition(observation.event, Start)
  val wRelease = isTransition(observation.event, Release)
  val wComplete = isTransition(observation.event, Complete)
  val wCancel = isTransition(observation.event, Cancel)
  val wReconsider = isTransition(observation.event, Reconsider)
  val wTitleEdit = match observation.event { | TitleEdited(_) => true | _ => false }
  val wDescriptionEdit = match observation.event { | DescriptionEdited(_) => true | _ => false }
  val wEditAfterAcceptance = affected(observation.event).exists(id =>
    (wTitleEdit or wDescriptionEdit) and information.get(id).lifecycle == NotStarted)
  val wEditWhileWorking = affected(observation.event).exists(id =>
    (wTitleEdit or wDescriptionEdit) and information.get(id).lifecycle == InProgress)
  val wEditAfterReconsider = affected(observation.event).exists(id => {
    val h = information.get(id).history
    if (h.length() == 0) false else
      (wTitleEdit or wDescriptionEdit) and h.nth(h.length() - 1).before == Cancelled
        and h.nth(h.length() - 1).after == Undecided
  })
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

}
