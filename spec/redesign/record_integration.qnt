// 記録の集合として保存した Entity の状態を、複数の replica（Git worktree）が任意の順序・
// 任意の部分集合で持ち寄って統合するモデル。保存先は不変な記録の集合で、現在値は記録から導く。
// Entity ごとの記録は因果 DAG を成し、head が複数あれば「衝突」、
// 解決記録（Resolve）が全 head を親にして一つの値を選ぶ。解決記録以外の記録は親を一つしか持たない。
// Note は因果を持たない集合。
// Git の merge・rebase・cherry-pick は、いずれも相手の記録の部分集合を取り込む一つの action で表す。
// 構造の不整合は「違反」の集合として導出し、通常操作は違反を増やさず、違反に含まれる Entity には
// 修復のための固定規則の免除を与える。
// 対象外: file の bytes と行の統合、lock、探索、ID の衝突確率、変換操作、再浮上条件と一覧の状況。
module record_integration {
  type Id = int
  type Replica = int
  type Actor = int
  type Lifecycle = Undecided | NotStarted | InProgress | Completed | Cancelled
  type Kind = Issue | Group
  type Parent = Unassigned | InGroup(Id)
  type Owner = Nobody | Owned(Actor)

  // 記録が保持する「その記録の後の現在値」。Group の stored lifecycle は InProgress にならない。
  // owner は Issue の Start を行った actor で、着手の排他性を現在値の比較に載せる。
  type Current = { kind: Kind, life: Lifecycle, owner: Owner, parent: Parent, deps: Set[Id], text: int }
  type Event = Created | Accept | Withdraw | Start | Release | Complete | Cancel | Reconsider | Reopen
    | Edit | Move | AddDep | RemoveDep | Resolve

  // 記録 ID は実装では内容の hash。ここでは発行元 replica と連番で一意にし、
  // 「同じ ID なら同じ内容」を構成で保証する。同じ操作を両側で行っても別の記録になる（日時が違う）。
  // anc は親をたどった祖先の記録 ID 全体（自身を含まない）。記録は不変なので作成時に確定する。
  // 検証で祖先閉包を毎回計算しないための冗長な保持で、head の判定には使わない（実装は親だけを知る）。
  type RecId = { origin: Replica, seq: int }
  type Record = { id: RecId, entity: Id, event: Event, actor: Actor, parents: Set[RecId], anc: Set[RecId], after: Current }
  type Note = { id: RecId, entity: Id, body: int }
  type Store = { records: Set[Record], notes: Set[Note] }
  type SyncMode = Full | Prefix | Single

  pure val REPLICAS = Set(0, 1)
  pure val ACTORS = Set(0, 1)
  pure val ENTITIES = Set(0, 1, 2, 3, 4)
  pure val TEXTS = Set(0, 1, 2)
  pure val PARENTS = Set(Unassigned, InGroup(0), InGroup(1), InGroup(2), InGroup(3), InGroup(4))
  pure val LIFECYCLE_OPS = Set(Accept, Withdraw, Start, Release, Complete, Cancel, Reconsider, Reopen)

  // 検証用の観測。直前の store と、直前に起きたことを保持する。製品の保存項目ではない。
  type Step = Init
    | Local({ replica: Replica, entity: Id, event: Event })
    | Resolved({ replica: Replica, entity: Id })
    | Noted({ replica: Replica, entity: Id })
    | Synced({ replica: Replica, source: Replica, mode: SyncMode })
  type Seen = { conflict: bool, invalid: Set[Replica], gap: bool }

  var stores: Replica -> Store
  var seqs: Replica -> int
  var previous: Replica -> Store
  var last: Step
  var seen: Seen
  // views と prevViews は stores と previous から導出できる値の写しで、状態ごとに一度だけ更新する。
  // invariant と witness が view を何度も計算し直さないための検証専用の変数。
  var views: Replica -> View
  var prevViews: Replica -> View

  // ---- 記録の集合からの導出 ----
  // 読取は store から一度だけ view を導出し、以後の判定は view に対して行う。

  pure val NONE: Current = { kind: Issue, life: Undecided, owner: Nobody, parent: Unassigned, deps: Set(), text: 0 }
  // 全体検査の違反。settled な Entity の現在値がコアの構造制約に反する箇所を Entity ごとに列挙する。
  type Violation =
    | ContainmentCycle(Id)        // 自身が包含の循環に含まれる
    | UnknownParent(Id)           // 親が存在しない、または親が Group でない
    | OpenUnderTerminal(Id)       // 終了した親の下に未終了の Entity がある
    | UnadoptedAncestor(Id)       // InProgress の Entity か実効 InProgress の Group の祖先に未採用がある
    | CompletedWithOpenDep(Id)    // Completed の Entity の依存先に Completed でないものがある
    | UnknownDep(Id)              // 依存先が存在しない
    | CompletionCycle(Id)         // 通常完了経路の循環に含まれる
  type View = {
    heads: Id -> Set[Record],   // 誰の親にもなっていない記録
    known: Set[Id],             // 記録が一つでもある Entity
    conflicted: Set[Id],        // head が複数ある（値が等しくても、同じ ID の二重登録でも衝突）
    settled: Set[Id],           // known かつ衝突していない
    current: Id -> Current,     // settled な Entity の現在値（唯一の head の値）
    gaps: Set[Id],              // 親記録が集合にない記録を持つ Entity（cherry-pick や squash の痕跡）
    violations: Set[Violation], // 全体検査の違反（settled な Entity だけを見る。衝突中は判定を保留）
  }
  // view は二段で作る。core は記録から head と現在値を導き、そのうえで違反を一度だけ導出する。
  pure def view(s: Store): View = {
    val core = viewCore(s)
    { ...core, violations: deriveViolations(core) }
  }
  pure def viewCore(s: Store): View = {
    val referenced = s.records.map(c => c.parents).flatten()
    val ids = s.records.map(r => r.id)
    val hs = ENTITIES.mapBy(e => s.records.filter(r => r.entity == e and not(referenced.contains(r.id))))
    val vals = ENTITIES.mapBy(e => hs.get(e).map(r => r.after))
    val knownIds = ENTITIES.filter(e => hs.get(e).size() > 0)
    val conflictedIds = knownIds.filter(e => hs.get(e).size() > 1)
    val settledIds = knownIds.exclude(conflictedIds)
    {
      heads: hs,
      known: knownIds,
      conflicted: conflictedIds,
      settled: settledIds,
      current: ENTITIES.mapBy(e => if (settledIds.contains(e)) vals.get(e).getOnlyElement() else NONE),
      gaps: ENTITIES.filter(e => s.records.exists(r => r.entity == e and not(r.parents.subseteq(ids)))),
      violations: Set(),
    }
  }
  pure def noConflicts(v: View): bool = v.conflicted.size() == 0

  // ---- 構造の導出（settled な Entity だけを見る）----

  pure def terminal(l: Lifecycle): bool = Set(Completed, Cancelled).contains(l)
  pure def parentIds(p: Parent): Set[Id] = match p {
    | Unassigned => Set()
    | InGroup(g) => Set(g)
  }
  pure def settledParent(v: View, e: Id): Set[Id] =
    parentIds(v.current.get(e).parent).filter(g => v.settled.contains(g))
  pure def ancestors(v: View, e: Id): Set[Id] =
    ENTITIES.fold(settledParent(v, e), (acc, _) =>
      acc.union(acc.map(g => settledParent(v, g)).flatten()))
  pure def children(v: View, g: Id): Set[Id] =
    v.settled.filter(c => v.current.get(c).parent == InGroup(g))
  pure def descendants(v: View, g: Id): Set[Id] =
    ENTITIES.fold(children(v, g), (acc, _) => acc.union(acc.map(c => children(v, c)).flatten()))
  pure def settledDeps(v: View, e: Id): Set[Id] = v.current.get(e).deps.filter(d => v.settled.contains(d))
  pure def depsCompleted(v: View, e: Id): bool =
    v.current.get(e).deps.forall(d => v.settled.contains(d) and v.current.get(d).life == Completed)
  // Group の実効 lifecycle。stored が NotStarted で、直属の子に InProgress・Completed か実効 InProgress の
  // 子 Group があれば InProgress。子 Group を通じて上へ伝播する。
  pure def workingGroups(v: View): Set[Id] =
    ENTITIES.fold(Set(), (active, _) => v.settled.filter(g =>
      v.current.get(g).kind == Group and v.current.get(g).life == NotStarted
        and children(v, g).exists(c => Set(InProgress, Completed).contains(v.current.get(c).life) or active.contains(c))))
  pure def effective(v: View, e: Id): Lifecycle =
    if (workingGroups(v).contains(e)) InProgress else v.current.get(e).life
  pure def hasStartedDescendant(v: View, g: Id): bool =
    descendants(v, g).exists(d => Set(InProgress, Completed).contains(v.current.get(d).life))
  // 全祖先が採用済み（stored が NotStarted）か。親が衝突中なら判定できないので偽。所属なしは通る。
  pure def ancestorsAdopted(v: View, e: Id): bool =
    parentIds(v.current.get(e).parent).forall(g => v.settled.contains(g))
      and ancestors(v, e).forall(a => v.current.get(a).life == NotStarted)
  pure def completionPredecessors(v: View, e: Id): Set[Id] =
    children(v, e).union(settledDeps(v, e))
      .union(ancestors(v, e).map(a => settledDeps(v, a)).flatten())
  // 通常完了経路（直属の子 ∪ 自身の依存先 ∪ 全祖先の依存先）の循環に含まれる Entity。
  pure def completionCycleMembers(v: View): Set[Id] = {
    val preds = v.settled.mapBy(e => completionPredecessors(v, e))
    v.settled.fold(v.settled, (remaining, _) => remaining.filter(e =>
      preds.get(e).intersect(remaining).size() > 0))
  }

  pure def deriveViolations(v: View): Set[Violation] = {
    val cycle = v.settled.filter(e => ancestors(v, e).contains(e)).map(e => ContainmentCycle(e))
    val unknownParent = v.settled.filter(e => match v.current.get(e).parent {
      | Unassigned => false
      | InGroup(g) => not(v.known.contains(g)) or (v.settled.contains(g) and v.current.get(g).kind != Group)
    }).map(e => UnknownParent(e))
    val underTerminal = v.settled.filter(e => match v.current.get(e).parent {
      | Unassigned => false
      | InGroup(g) => v.settled.contains(g) and terminal(v.current.get(g).life) and not(terminal(v.current.get(e).life))
    }).map(e => OpenUnderTerminal(e))
    val working = workingGroups(v)
    val unadopted = v.settled.filter(e =>
      (v.current.get(e).life == InProgress or working.contains(e))
        and ancestors(v, e).exists(a => v.current.get(a).life != NotStarted)).map(e => UnadoptedAncestor(e))
    val openDep = v.settled.filter(e => v.current.get(e).life == Completed
      and settledDeps(v, e).exists(d => v.current.get(d).life != Completed)).map(e => CompletedWithOpenDep(e))
    val unknownDep = v.settled.filter(e => v.current.get(e).deps.exists(d => not(v.known.contains(d)))).map(e => UnknownDep(e))
    val completionCycle = completionCycleMembers(v).map(e => CompletionCycle(e))
    cycle.union(unknownParent).union(underTerminal).union(unadopted).union(openDep).union(unknownDep).union(completionCycle)
  }
  pure def violated(x: Violation): Id = match x {
    | ContainmentCycle(e) => e
    | UnknownParent(e) => e
    | OpenUnderTerminal(e) => e
    | UnadoptedAncestor(e) => e
    | CompletedWithOpenDep(e) => e
    | UnknownDep(e) => e
    | CompletionCycle(e) => e
  }
  pure def structurallyValid(v: View): bool = v.violations.size() == 0
  pure def valid(v: View): bool = noConflicts(v) and structurallyValid(v)
  // 違反に含まれる Entity。修復のため、この Entity については終了した親の配下の固定と
  // Completed の dependency の固定を免除する。
  pure def inViolation(v: View, e: Id): bool = v.violations.exists(x => violated(x) == e)

  // ---- 通常操作の前提と効果（一つの replica の settled な現在値に対して）----

  pure def parentOpen(v: View, e: Id): bool = match v.current.get(e).parent {
    | Unassigned => true
    | InGroup(g) => v.settled.contains(g) and not(terminal(v.current.get(g).life))
  }
  pure def parentOpenOrRepair(v: View, e: Id): bool = parentOpen(v, e) or inViolation(v, e)
  pure def childrenEnded(v: View, g: Id): bool =
    children(v, g).forall(c => terminal(v.current.get(c).life))
  pure def hasCompletedDependent(v: View, e: Id): bool =
    v.settled.exists(d => v.current.get(d).life == Completed and v.current.get(d).deps.contains(e))

  pure def canLifecycle(v: View, e: Id, op: Event, passed: bool): bool = {
    val c = v.current.get(e)
    and {
      v.settled.contains(e),
      parentOpenOrRepair(v, e),
      match op {
        | Accept => c.life == Undecided
            and ((c.kind == Group and hasStartedDescendant(v, e)) implies ancestorsAdopted(v, e))
        | Withdraw => c.life == NotStarted and effective(v, e) == NotStarted
        | Start => and {
            c.kind == Issue,
            c.life == NotStarted,
            depsCompleted(v, e),
            ancestorsAdopted(v, e),
            ancestors(v, e).forall(a => depsCompleted(v, a)),
          }
        | Release => c.kind == Issue and c.life == InProgress
        | Complete => depsCompleted(v, e) and ancestorsAdopted(v, e) and (
            if (c.kind == Issue) c.life == InProgress
            else c.life == NotStarted and passed and childrenEnded(v, e))
        | Cancel =>
            if (c.kind == Issue) Set(Undecided, NotStarted, InProgress).contains(c.life)
            else Set(Undecided, NotStarted).contains(c.life) and childrenEnded(v, e)
        | Reconsider => c.life == Cancelled
        | Reopen => c.life == Completed and ancestorsAdopted(v, e)
            and (not(hasCompletedDependent(v, e)) or inViolation(v, e))
        | _ => false
      }
    }
  }
  pure def applyLifecycle(c: Current, op: Event, actor: Actor): Current = match op {
    | Accept => { ...c, life: NotStarted }
    | Withdraw => { ...c, life: Undecided }
    | Start => { ...c, life: InProgress, owner: Owned(actor) }
    | Release => { ...c, life: NotStarted, owner: Nobody }
    | Complete => { ...c, life: Completed, owner: Nobody }
    | Cancel => { ...c, life: Cancelled, owner: Nobody }
    | Reconsider => { ...c, life: Undecided }
    | Reopen => { ...c, life: NotStarted }
    | _ => c
  }

  pure def destinationOpen(v: View, e: Id, p: Parent): bool = match p {
    | Unassigned => true
    | InGroup(g) => and {
        v.settled.contains(g),
        g != e,
        v.current.get(g).kind == Group,
        not(terminal(v.current.get(g).life)),
        not(ancestors(v, g).contains(e)),
        Set(InProgress, Completed).contains(effective(v, e)) implies
          (v.current.get(g).life == NotStarted and ancestors(v, g).forall(a => v.current.get(a).life == NotStarted)),
      }
  }
  pure def canMove(v: View, e: Id, p: Parent): bool = and {
    v.settled.contains(e),
    v.current.get(e).parent != p,
    parentOpenOrRepair(v, e),
    destinationOpen(v, e, p),
  }
  pure def canAddDep(v: View, e: Id, t: Id): bool = and {
    v.settled.contains(e),
    e != t,
    v.settled.contains(t),
    v.current.get(e).life != Completed,
    not(v.current.get(e).deps.contains(t)),
    not(ancestors(v, e).contains(t)),
    not(ancestors(v, t).contains(e)),
  }
  pure def canRemoveDep(v: View, e: Id, t: Id): bool = and {
    v.settled.contains(e),
    v.current.get(e).deps.contains(t),
    v.current.get(e).life != Completed or inViolation(v, e),
  }
  pure def canEdit(v: View, e: Id, text: int): bool =
    v.settled.contains(e) and not(terminal(v.current.get(e).life)) and v.current.get(e).text != text
  pure def canCreate(v: View, e: Id, p: Parent): bool = and {
    not(v.known.contains(e)),
    match p {
      | Unassigned => true
      | InGroup(g) => v.settled.contains(g) and v.current.get(g).kind == Group and not(terminal(v.current.get(g).life))
    },
  }

  pure def newRecord(v: View, r: Replica, n: int, e: Id, ev: Event, actor: Actor, after: Current): Record =
    { id: { origin: r, seq: n }, entity: e, event: ev, actor: actor,
      parents: v.heads.get(e).map(h => h.id),
      anc: v.heads.get(e).map(h => h.anc.union(Set(h.id))).flatten(), after: after }
  pure def appendRecord(s: Store, rec: Record): Store =
    { ...s, records: s.records.union(Set(rec)) }
  pure def baseCurrent(kind: Kind, life: Lifecycle, p: Parent): Current =
    { kind: kind, life: life, owner: Nobody, parent: p, deps: Set(), text: 0 }
  pure def baseRecord(n: int, e: Id, c: Current): Record =
    { id: { origin: 0, seq: n }, entity: e, event: Created, actor: 0, parents: Set(), anc: Set(), after: c }

  // 通常操作が作る次の store の候補。修復可能性の検査で使う。
  pure def lifecycleResult(s: Store, v: View, r: Replica, n: int, e: Id, op: Event): Store =
    appendRecord(s, newRecord(v, r, n, e, op, 0, applyLifecycle(v.current.get(e), op, 0)))
  pure def moveResult(s: Store, v: View, r: Replica, n: int, e: Id, p: Parent): Store =
    appendRecord(s, newRecord(v, r, n, e, Move, 0, { ...v.current.get(e), parent: p }))
  pure def removeDepResult(s: Store, v: View, r: Replica, n: int, e: Id, t: Id): Store =
    appendRecord(s, newRecord(v, r, n, e, RemoveDep, 0, { ...v.current.get(e), deps: v.current.get(e).deps.exclude(Set(t)) }))
  // 修復の進み具合。違反の集合が縮むか、集合が同じでも違反に含まれる Entity の dependency と所属の辺が減れば前進。
  // 二重に絡んだ循環は一手では違反の集合を縮められないが、辺を一本ずつ外して直せる。
  pure def repairEdges(v: View): int =
    v.violations.map(x => violated(x)).fold(0, (n, e) =>
      n + v.current.get(e).deps.size() + parentIds(v.current.get(e).parent).size())
  pure def strictlyFewer(before: View, after: Store): bool = {
    val va = view(after)
    va.violations.subseteq(before.violations) and (
      va.violations.size() < before.violations.size()
        or (va.violations.size() == before.violations.size() and repairEdges(va) < repairEdges(before)))
  }
  // 違反があり衝突がない store で、修復を確実に進める通常操作が一つはあるか。
  pure def repairable(s: Store, v: View): bool = or {
    v.settled.exists(e => LIFECYCLE_OPS.exists(op =>
      canLifecycle(v, e, op, true) and strictlyFewer(v, lifecycleResult(s, v, 0, 1000, e, op)))),
    v.settled.exists(e => canMove(v, e, Unassigned) and strictlyFewer(v, moveResult(s, v, 0, 1000, e, Unassigned))),
    v.settled.exists(e => v.current.get(e).deps.exists(t =>
      canRemoveDep(v, e, t) and strictlyFewer(v, removeDepResult(s, v, 0, 1000, e, t)))),
  }

  // ---- 初期状態 ----
  // 両 replica は同じ base を持つ。0 は Group、1 は 0 の子 Issue、2 は所属なしの Group、
  // 3 は所属なしの Issue、4 は未登録の枠。
  pure val BASE: Store = {
    records: Set(
      baseRecord(0, 0, baseCurrent(Group, NotStarted, Unassigned)),
      baseRecord(1, 1, baseCurrent(Issue, NotStarted, InGroup(0))),
      baseRecord(2, 2, baseCurrent(Group, NotStarted, Unassigned)),
      baseRecord(3, 3, baseCurrent(Issue, NotStarted, Unassigned))
    ),
    notes: Set(),
  }

  action init = all {
    stores' = REPLICAS.mapBy(_ => BASE),
    seqs' = REPLICAS.mapBy(r => if (r == 0) 4 else 0),
    previous' = REPLICAS.mapBy(_ => BASE),
    last' = Init,
    seen' = { conflict: false, invalid: Set(), gap: false },
    views' = REPLICAS.mapBy(_ => view(BASE)),
    prevViews' = REPLICAS.mapBy(_ => view(BASE)),
  }

  // ---- 通常操作 ----
  // 衝突が一つでも残っている replica では、Resolve と Note 以外の操作を行えない。
  // 構造の違反は操作を止めない。所属変更・dependency の追加・登録は違反を増やさないことを要求し、
  // それ以外は各操作の前提で扱う。

  action commitLocal(r: Replica, v: View, rec: Record, noWorse: bool): bool = {
    val s2 = appendRecord(stores.get(r), rec)
    val v2 = view(s2)
    all {
      noWorse implies v2.violations.subseteq(v.violations),
      stores' = stores.set(r, s2),
      seqs' = seqs.setBy(r, n => n + 1),
      previous' = stores,
      last' = Local({ replica: r, entity: rec.entity, event: rec.event }),
      seen' = seen,
      views' = views.set(r, v2),
      prevViews' = views,
    }
  }

  action lifecycleOp(r: Replica, v: View, e: Id, op: Event, actor: Actor, passed: bool): bool = all {
    noConflicts(v),
    canLifecycle(v, e, op, passed),
    commitLocal(r, v, newRecord(v, r, seqs.get(r), e, op, actor, applyLifecycle(v.current.get(e), op, actor)), false),
  }
  action edit(r: Replica, v: View, e: Id, actor: Actor, text: int): bool = all {
    noConflicts(v),
    canEdit(v, e, text),
    commitLocal(r, v, newRecord(v, r, seqs.get(r), e, Edit, actor, { ...v.current.get(e), text: text }), false),
  }
  action move(r: Replica, v: View, e: Id, actor: Actor, p: Parent): bool = all {
    noConflicts(v),
    canMove(v, e, p),
    commitLocal(r, v, newRecord(v, r, seqs.get(r), e, Move, actor, { ...v.current.get(e), parent: p }), true),
  }
  action addDep(r: Replica, v: View, e: Id, actor: Actor, t: Id): bool = all {
    noConflicts(v),
    canAddDep(v, e, t),
    commitLocal(r, v, newRecord(v, r, seqs.get(r), e, AddDep, actor,
      { ...v.current.get(e), deps: v.current.get(e).deps.union(Set(t)) }), true),
  }
  action removeDep(r: Replica, v: View, e: Id, actor: Actor, t: Id): bool = all {
    noConflicts(v),
    canRemoveDep(v, e, t),
    commitLocal(r, v, newRecord(v, r, seqs.get(r), e, RemoveDep, actor,
      { ...v.current.get(e), deps: v.current.get(e).deps.exclude(Set(t)) }), false),
  }
  action create(r: Replica, v: View, e: Id, actor: Actor, kind: Kind, life: Lifecycle, p: Parent): bool = all {
    noConflicts(v),
    canCreate(v, e, p),
    commitLocal(r, v, newRecord(v, r, seqs.get(r), e, Created, actor, baseCurrent(kind, life, p)), true),
  }
  action note(r: Replica, v: View, e: Id, body: int): bool = {
    val s = stores.get(r)
    all {
      v.known.contains(e),
      stores' = stores.set(r, { ...s, notes: s.notes.union(Set({ id: { origin: r, seq: seqs.get(r) }, entity: e, body: body })) }),
      seqs' = seqs.setBy(r, n => n + 1),
      previous' = stores,
      last' = Noted({ replica: r, entity: e }),
      seen' = seen,
      views' = views,
      prevViews' = views,
    }
  }

  // 解決。衝突した Entity の head から一つを選び、全 head を親にする解決記録を足す。
  // 値の等しい head どうしの衝突も、同じ ID の二重登録も同じ操作で解決する（二重登録は捨てた側の内容を登録し直す）。
  // 解決後の構造の違反は通常操作で直す。
  action resolve(r: Replica, v: View, e: Id, actor: Actor, pick: Record): bool = {
    val s2 = appendRecord(stores.get(r), newRecord(v, r, seqs.get(r), e, Resolve, actor, pick.after))
    all {
      v.conflicted.contains(e),
      v.heads.get(e).contains(pick),
      stores' = stores.set(r, s2),
      seqs' = seqs.setBy(r, n => n + 1),
      previous' = stores,
      last' = Resolved({ replica: r, entity: e }),
      seen' = seen,
      views' = views.set(r, view(s2)),
      prevViews' = views,
    }
  }

  // 取り込み。相手が持っていて自分にない記録を、全部（merge）、連番の接頭辞（rebase の途中）、
  // 一件だけ（cherry-pick・squash）のいずれかで取り込む。Note は常に和集合。
  action sync(r: Replica, from: Replica, mode: SyncMode, anchor: Record): bool = {
    val mine = stores.get(r)
    val theirs = stores.get(from)
    val missing = theirs.records.exclude(mine.records)
    val taken = match mode {
      | Full => missing
      | Prefix => missing.filter(x => x.id.seq <= anchor.id.seq)
      | Single => Set(anchor)
    }
    val merged = { records: mine.records.union(taken), notes: mine.notes.union(theirs.notes) }
    val mv = view(merged)
    all {
      r != from,
      missing.contains(anchor),
      stores' = stores.set(r, merged),
      seqs' = seqs,
      previous' = stores,
      last' = Synced({ replica: r, source: from, mode: mode }),
      seen' = {
        conflict: seen.conflict or not(noConflicts(mv)),
        invalid: seen.invalid.union(if (noConflicts(mv) and not(structurallyValid(mv))) Set(r) else Set()),
        gap: seen.gap or mv.gaps.size() > 0,
      },
      views' = views.set(r, mv),
      prevViews' = views,
    }
  }

  action step = {
    nondet r = REPLICAS.oneOf()
    nondet from = REPLICAS.oneOf()
    nondet e = ENTITIES.oneOf()
    nondet op = LIFECYCLE_OPS.oneOf()
    nondet actor = ACTORS.oneOf()
    nondet passed = Set(true, false).oneOf()
    nondet p = PARENTS.oneOf()
    nondet t = ENTITIES.oneOf()
    nondet text = TEXTS.oneOf()
    nondet kind = Set(Issue, Group).oneOf()
    nondet life = Set(Undecided, NotStarted).oneOf()
    nondet mode = Set(Full, Prefix, Single).oneOf()
    nondet anchor = stores.get(from).records.oneOf()
    nondet pick = ENTITIES.map(x => views.get(r).heads.get(x)).flatten().oneOf()
    val v = views.get(r)
    any {
      lifecycleOp(r, v, e, op, actor, passed),
      edit(r, v, e, actor, text),
      move(r, v, e, actor, p),
      addDep(r, v, e, actor, t),
      removeDep(r, v, e, actor, t),
      create(r, v, e, actor, kind, life, p),
      note(r, v, e, text),
      resolve(r, v, e, actor, pick),
      sync(r, from, mode, anchor),
    }
  }

  // ---- 補助 ----
  pure def presentParents(s: Store, x: Record): Set[Record] = s.records.filter(p => x.parents.contains(p.id))
  pure def reaches(h: Record, x: Record): bool = h.id == x.id or h.anc.contains(x.id)
  // 一方の全 head が他方のある head の祖先に含まれる（他方が先へ進んでいる）。
  pure def dominates(winner: Set[Record], loser: Set[Record]): bool =
    loser.forall(l => winner.exists(w => reaches(w, l)))
  def store(r: Replica): Store = stores.get(r)
  def eachReplica(f: (Store, View) => bool): bool = REPLICAS.forall(r => f(stores.get(r), views.get(r)))
  def someReplica(f: (Store, View) => bool): bool = REPLICAS.exists(r => f(stores.get(r), views.get(r)))
  def lastLocal(ev: Event): bool = match last {
    | Local(x) => x.event == ev
    | _ => false
  }

  // ---- invariant ----

  // 構成の確認: views の写しが導出値と一致する。
  val invViewsCached = REPLICAS.forall(r => views.get(r) == view(stores.get(r)))
  // 構成の確認: 記録は消えず、記録 ID は一意で、全部取り込んだ後は相手の記録をすべて持つ。
  val invMonotone = REPLICAS.forall(r => previous.get(r).records.subseteq(store(r).records)
    and previous.get(r).notes.subseteq(store(r).notes))
  val invRecordIdsUnique = REPLICAS.forall(r => store(r).records.map(x => x.id).size() == store(r).records.size())
  val invFullSyncSuperset = match last {
    | Synced(x) => x.mode == Full implies store(x.source).records.subseteq(store(x.replica).records)
    | _ => true
  }
  // 構成の確認: 解決記録以外の記録は親を一つしか持たない。解決記録は親のどれかの値を採る。
  val invSingleParentUnlessResolve = REPLICAS.forall(r => store(r).records.forall(x =>
    x.event != Resolve implies x.parents.size() <= 1))
  val invResolveTakesInputValue = REPLICAS.forall(r => store(r).records.forall(x =>
    (x.event == Resolve and presentParents(store(r), x).size() == x.parents.size()) implies
      presentParents(store(r), x).exists(p => p.after == x.after)))
  // 解決の直後にその Entity は settled。二重登録の解決も含む。
  val invResolveSettles = match last {
    | Resolved(x) => views.get(x.replica).settled.contains(x.entity)
    | _ => true
  }
  // 通常操作は衝突のない replica でしか起きず、違反を増やさず、gap を変えず、他の Entity の記録を増やさない。
  pure def localStep(before: Store, vb: View, after: Store, va: View, e: Id): bool = and {
    noConflicts(vb),
    va.violations.subseteq(vb.violations),
    vb.gaps == va.gaps,
    after.records.exclude(before.records).forall(rec => rec.entity == e),
  }
  val invLocalStep = match last {
    | Local(x) => localStep(previous.get(x.replica), prevViews.get(x.replica), store(x.replica), views.get(x.replica), x.entity)
    | _ => true
  }
  // 有効な store では修復の免除が働かない（免除は違反に含まれる Entity にだけ与える）。
  val invNoWaiverWhenValid = eachReplica((s, v) => structurallyValid(v) implies ENTITIES.forall(e => not(inViolation(v, e))))
  // 違反があり衝突がない store には、修復を確実に進める通常操作が一つはある（行き止まりがない）。
  val invInvalidStateRepairable = eachReplica((s, v) =>
    (noConflicts(v) and not(structurallyValid(v))) implies repairable(s, v))
  // 全部取り込みは値を捏造しない。取り込み後に settled な Entity の現在値は、取り込み前の自分か相手の現在値。
  pure def sideValues(vb: View, vt: View, e: Id): Set[Current] =
    (if (vb.settled.contains(e)) Set(vb.current.get(e)) else Set())
      .union(if (vt.settled.contains(e)) Set(vt.current.get(e)) else Set())
  val invFullSyncTakesSideValue = match last {
    | Synced(x) => x.mode == Full implies {
        val va = views.get(x.replica)
        val vb = prevViews.get(x.replica)
        val vt = views.get(x.source)
        va.settled.forall(e => sideValues(vb, vt, e).contains(va.current.get(e)))
      }
    | _ => true
  }
  // 全部取り込みで、両側が settled で値が異なり、どちらの head も他方に先んじていなければ、取り込み後は衝突。
  val invFullSyncConflictWhenDivergent = match last {
    | Synced(x) => x.mode == Full implies {
        val va = views.get(x.replica)
        val vb = prevViews.get(x.replica)
        val vt = views.get(x.source)
        ENTITIES.forall(e => and {
          vb.settled.contains(e), vt.settled.contains(e),
          vb.current.get(e) != vt.current.get(e),
          not(dominates(vb.heads.get(e), vt.heads.get(e))),
          not(dominates(vt.heads.get(e), vb.heads.get(e))),
        } implies va.conflicted.contains(e))
      }
    | _ => true
  }
  // settled な Issue が InProgress なら owner を持ち、それ以外は持たない。Group の stored は InProgress にならない。
  val invOwnerMatchesLife = eachReplica((s, v) => v.settled.forall(e =>
    (v.current.get(e).life == InProgress) iff (v.current.get(e).owner != Nobody)))
  val invGroupNeverStoresInProgress = REPLICAS.forall(r => store(r).records.forall(x =>
    x.after.kind == Group implies x.after.life != InProgress))

  // ---- witness ----

  pure def conflictWith(v: View, e: Id, f: (Record) => bool): bool =
    v.conflicted.contains(e) and v.heads.get(e).exists(f)

  // 同じ Issue を別の actor が並行して Start すると衝突する。
  pure def concurrentStartConflict(s: Store, v: View): bool = ENTITIES.exists(e =>
    v.conflicted.contains(e)
      and v.heads.get(e).filter(h => h.after.life == InProgress).map(h => h.after.owner).size() > 1)
  val wConcurrentStartConflict = someReplica(concurrentStartConflict)
  // 値の等しい並行した記録（同じ actor の Start、両側の Accept）も衝突として見え、解決で片方を選ぶ。
  pure def equalValueConflict(s: Store, v: View): bool = ENTITIES.exists(e =>
    v.conflicted.contains(e) and v.heads.get(e).map(h => h.after).size() == 1)
  val wEqualValueConflict = someReplica(equalValueConflict)
  val wEqualValueResolved = REPLICAS.exists(r => store(r).records.exists(x =>
    x.event == Resolve and presentParents(store(r), x).size() == x.parents.size()
      and presentParents(store(r), x).map(p => p.after).size() == 1))
  val wResolved = match last { | Resolved(_) => true | _ => false }
  // 両側が同じ衝突を別々に解決し、解決記録が並行になる。同じ値を選んでいても違う値でも再び衝突になり、
  // もう一段の解決（解決の解決）で収束する。
  pure def concurrentResolvesAgree(s: Store, v: View): bool = ENTITIES.exists(e =>
    v.conflicted.contains(e) and v.heads.get(e).forall(h => h.event == Resolve)
      and v.heads.get(e).map(h => h.after).size() == 1)
  val wConcurrentResolvesAgree = someReplica(concurrentResolvesAgree)
  pure def concurrentResolvesDisagree(s: Store, v: View): bool = ENTITIES.exists(e =>
    v.conflicted.contains(e) and v.heads.get(e).forall(h => h.event == Resolve)
      and v.heads.get(e).map(h => h.after).size() > 1)
  val wConcurrentResolvesDisagree = someReplica(concurrentResolvesDisagree)
  val wResolveOfResolves = REPLICAS.exists(r => store(r).records.exists(x =>
    x.event == Resolve and presentParents(store(r), x).filter(p => p.event == Resolve).size() > 1))
  // 記録の一部だけが入って親が欠ける。後で埋まる。
  pure def hasGap(s: Store, v: View): bool = v.gaps.size() > 0
  val wGap = someReplica(hasGap)
  val wGapFilled = seen.gap and not(someReplica(hasGap))
  // 親が欠けた記録が入ると、実際には祖先である記録と並んで head になり、衝突として見える。
  pure def spuriousGapConflict(s: Store, v: View): bool = ENTITIES.exists(e =>
    v.conflicted.contains(e) and v.heads.get(e).exists(h => v.heads.get(e).exists(o => h != o and h.anc.contains(o.id))))
  val wSpuriousGapConflict = someReplica(spuriousGapConflict)
  val wPrefixSync = match last { | Synced(x) => x.mode == Prefix | _ => false }
  val wSingleSync = match last { | Synced(x) => x.mode == Single | _ => false }
  // 両 replica が収束し、双方の記録を含む。
  val wConverged = store(0) == store(1)
    and store(0).records.exists(x => x.id.origin == 0 and x.id.seq >= 4)
    and store(0).records.exists(x => x.id.origin == 1)
  pure def hasViolation(v: View, f: (Violation) => bool): bool = v.violations.exists(f)
  // 取り込みだけで起きる構造の違反の種類ごとの観測。
  val wTerminalGroupGainsChild = someReplica((s, v) => noConflicts(v) and hasViolation(v, x => match x { | OpenUnderTerminal(_) => true | _ => false }))
  val wContainmentCycle = someReplica((s, v) => noConflicts(v) and hasViolation(v, x => match x { | ContainmentCycle(_) => true | _ => false }))
  val wCompletionCycle = someReplica((s, v) => noConflicts(v) and hasViolation(v, x => match x { | CompletionCycle(_) => true | _ => false }))
  val wUnadoptedAncestor = someReplica((s, v) => noConflicts(v) and hasViolation(v, x => match x { | UnadoptedAncestor(_) => true | _ => false }))
  val wCompletedWithOpenDep = someReplica((s, v) => noConflicts(v) and hasViolation(v, x => match x { | CompletedWithOpenDep(_) => true | _ => false }))
  // 取り込みで壊れた構造が直る。Reopen で直る。修復の免除を使った操作で直る。
  val wRepairedAfterSync = seen.invalid.exists(r => valid(views.get(r)))
  val wReopenRepairs = match last {
    | Local(x) => x.event == Reopen and not(structurallyValid(prevViews.get(x.replica)))
        and structurallyValid(views.get(x.replica))
    | _ => false
  }
  val wWaivedRepair = match last {
    | Local(x) => {
        val vb = prevViews.get(x.replica)
        val waived = not(parentOpen(vb, x.entity)) or (x.event == RemoveDep and vb.current.get(x.entity).life == Completed)
        inViolation(vb, x.entity) and waived and views.get(x.replica).violations.size() < vb.violations.size()
      }
    | _ => false
  }
  // 片方が完了、他方が解放して未着手のまま。未完了側を選び、もう一度着手する。
  pure def completeVsRelease(s: Store, v: View): bool = ENTITIES.exists(e =>
    conflictWith(v, e, h => h.event == Complete) and conflictWith(v, e, h => h.event == Release))
  val wCompleteVsRelease = someReplica(completeVsRelease)
  val wChoseNotStartedOverCompleted = REPLICAS.exists(r => store(r).records.exists(x =>
    x.event == Resolve and x.after.life == NotStarted
      and presentParents(store(r), x).exists(p => p.after.life == Completed)))
  val wStartAfterResolve = REPLICAS.exists(r => store(r).records.exists(x =>
    x.event == Start and presentParents(store(r), x).exists(p => p.event == Resolve)))
  pure def cancelVsStart(s: Store, v: View): bool = ENTITIES.exists(e =>
    conflictWith(v, e, h => h.event == Cancel) and conflictWith(v, e, h => h.event == Start))
  val wCancelVsStart = someReplica(cancelVsStart)
  // 文面の並行編集は衝突。
  pure def textConflict(s: Store, v: View): bool = ENTITIES.exists(e =>
    v.conflicted.contains(e) and v.heads.get(e).forall(h => h.event == Edit))
  val wTextConflict = someReplica(textConflict)
  // Note は両側のものが残る。
  val wNotesFromBoth = REPLICAS.exists(r => ENTITIES.exists(e =>
    store(r).notes.exists(n => n.entity == e and n.id.origin == 0)
      and store(r).notes.exists(n => n.entity == e and n.id.origin == 1)))
  // 同じ ID を両側で登録する（ID の衝突）。作成記録が二つとも head なので通常の衝突として見え、片方の系列を選んで解決する。
  pure def duplicateCreation(s: Store, v: View): bool = ENTITIES.exists(e =>
    v.conflicted.contains(e) and s.records.filter(x => x.entity == e and x.event == Created).size() > 1)
  val wDuplicateCreation = someReplica(duplicateCreation)
  val wDuplicateResolved = REPLICAS.exists(r => store(r).records.exists(x =>
    x.event == Resolve and presentParents(store(r), x).filter(p => p.event == Created).size() > 1))
  // 各操作の到達。
  val wAccept = lastLocal(Accept)
  val wStart = lastLocal(Start)
  val wComplete = lastLocal(Complete)
  val wReopen = lastLocal(Reopen)
  val wCancel = lastLocal(Cancel)
  val wReconsider = lastLocal(Reconsider)
  val wWithdraw = lastLocal(Withdraw)
  val wRelease = lastLocal(Release)
  val wEdit = lastLocal(Edit)
  val wMove = lastLocal(Move)
  val wAddDep = lastLocal(AddDep)
  val wRemoveDep = lastLocal(RemoveDep)
  val wCreate = lastLocal(Created)
  pure def groupCompleted(s: Store, v: View): bool = v.settled.exists(e =>
    v.current.get(e).kind == Group and v.current.get(e).life == Completed)
  val wGroupCompleted = someReplica(groupCompleted)
  // Reopen した Group（Reopen の記録が head）が完了済みの子を持ち、実効 InProgress になる。
  pure def groupReopenedWithCompletedChild(s: Store, v: View): bool =
    v.settled.exists(g => v.heads.get(g).exists(h => h.event == Reopen)
      and children(v, g).exists(c => v.current.get(c).life == Completed))
  val wGroupReopenedWithCompletedChild = someReplica(groupReopenedWithCompletedChild)
}
