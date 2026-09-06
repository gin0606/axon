// Two local snapshots, atomic ordinary operations and explicit integration. No message protocol.
// Payloads are opaque integers. Unique descending IDs abstract collision-free allocation;
// their numerical order is only a ghost proof of acyclicity, never a storage/display policy.
module branch_history {
  type Revision = { id: int, parent: int, value: int }
  type Record = { id: int, parents: Set[int], kind: int, current: int, last: int }
  type Branch = {
    draft: int, current: int, last: int, head: int,
    revisions: Set[Revision], records: Set[Record], notes: Set[int],
  }
  type State = { branches: int -> Branch, next: int }
  pure val BRANCHES = Set(0, 1)
  pure val initial: Branch = {
    draft: 0, current: 0, last: 0, head: -1, revisions: Set(), notes: Set(),
    records: Set({id: -1, parents: Set(), kind: 0, current: 0, last: 0}),
  }
  var state: State
  var before: State
  var operation: int
  action init = all {
    state' = { branches: BRANCHES.mapBy(_ => initial), next: -2 },
    before' = { branches: BRANCHES.mapBy(_ => initial), next: -2 },
    operation' = 0,
  }
  action save(branch: int, value: Branch, fresh: int, op: int): bool = all {
    before' = state,
    state' = {branches: state.branches.set(branch, value), next: fresh},
    operation' = op,
  }
  pure def selected(b: Branch): Set[Revision] = b.revisions.filter(r => r.id == b.last)
  pure def appendRecord(b: Branch, id: int, current: int, last: int, kind: int): Branch = {
    ...b, current: current, last: last, head: id,
    records: b.records.union(Set({id: id, parents: Set(b.head), kind: kind, current: current, last: last})),
  }
  action edit = {
    nondet branch = BRANCHES.oneOf()
    nondet value = Set(0,1).oneOf()
    val b = state.branches.get(branch)
    all { b.current == 0, b.draft != value, save(branch, {...b, draft: value}, state.next, 1) }
  }
  action decide = {
    nondet branch = BRANCHES.oneOf()
    val b = state.branches.get(branch)
    val reuse = selected(b).exists(r => r.value == b.draft)
    val revision = if (reuse) b.last else state.next
    val withRevision = if (reuse) b else {
      ...b, revisions: b.revisions.union(Set({id: revision, parent: b.last, value: b.draft})),
    }
    all { b.current == 0, save(branch, appendRecord(withRevision, state.next - 1, revision, revision, 1), state.next - 2, 2) }
  }
  action undecide = {
    nondet branch = BRANCHES.oneOf()
    val b = state.branches.get(branch)
    all { b.current != 0, save(branch, appendRecord(b, state.next, 0, b.last, 2), state.next - 1, 3) }
  }
  action note = {
    nondet branch = BRANCHES.oneOf()
    val b = state.branches.get(branch)
    save(branch, {...b, notes: b.notes.union(Set(state.next))}, state.next - 1, 4)
  }
  action integrate = {
    nondet target = BRANCHES.oneOf()
    nondet choice = BRANCHES.oneOf()
    val left = state.branches.get(0)
    val right = state.branches.get(1)
    val chosen = state.branches.get(choice)
    val combined = {...chosen,
      records: left.records.union(right.records), revisions: left.revisions.union(right.revisions),
      notes: left.notes.union(right.notes),
    }
    val merged = {...combined, head: state.next,
      records: combined.records.union(Set({id: state.next, parents: Set(left.head, right.head),
        kind: 3, current: chosen.current, last: chosen.last})),
    }
    all { left != right, save(target, merged, state.next - 1, 5) }
  }
  action step = any { edit, decide, undecide, note, integrate }
  val invReferences = BRANCHES.forall(i => {
    val b = state.branches.get(i)
    and {
      b.records.forall(r => r.parents.forall(p => b.records.exists(parent => parent.id == p))),
      b.revisions.forall(r => r.parent == 0 or b.revisions.exists(parent => parent.id == r.parent)),
      b.last == 0 or b.revisions.exists(r => r.id == b.last),
      b.current == 0 or b.revisions.exists(r => r.id == b.current and r.value == b.draft),
    }
  })
  val invCausal = BRANCHES.forall(i => {
    val b = state.branches.get(i)
    b.records.forall(r => r.parents.forall(p => p > r.id)) and
    b.revisions.forall(r => r.parent == 0 or r.parent > r.id)
  })
  val invRetained = BRANCHES.forall(i => {
    val b = before.branches.get(i)
    val a = state.branches.get(i)
    b.records.subseteq(a.records) and b.revisions.subseteq(a.revisions) and b.notes.subseteq(a.notes)
  })
  val invCurrentProof = BRANCHES.forall(i => {
    val b = state.branches.get(i)
    b.records.exists(r => r.id == b.head and r.current == b.current and r.last == b.last)
  })
  val invLastSurvivesUndecide = operation == 3 implies BRANCHES.forall(i =>
    before.branches.get(i).last == state.branches.get(i).last)
  val wEdit = operation == 1
  val wDecide = operation == 2
  val wUndecide = operation == 3
  val wNote = operation == 4
  val wMerge = operation == 5
  val wNonLatestSelected = BRANCHES.exists(i => {
    val b = state.branches.get(i)
    b.last != 0 and b.revisions.exists(r => r.id < b.last)
  })
  val wBothNotes = BRANCHES.exists(i => state.branches.get(i).notes.size() > 1) and operation == 5
}
