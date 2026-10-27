// 候補一覧の1回の取得と axon show の1回の表示を、Entity ごとの評価という観測ステップへ具体化するモデル。
// group_lifecycle の状態・許可条件・一覧の行を再利用し、そちらの bool 入力と行の集合を
// 失敗のない場合の基準として、評価の回数・省略・判定失敗を検査する。
// proposals は stored が Undecided の Entity、tasks は stored が NotStarted の Issue と Group の浮上と、
// 浮上に関係なく進行中の Issue と実効 lifecycle が InProgress の Group を対象にする。
// show は対象が tasks の行になる場合と同じ範囲（対象自身と、Group なら着手可能な子孫の Issue、
// それらの祖先）だけを評価し、対象の状況と詰まっている理由がその範囲で決まることを検査する。
// 条件の許可は、このマシンで許可したコマンド内容の集合として持つ。利用者が CLI で指定して設定した条件は
// 自動で許可し、統合や解決で入った条件は許可しない。許可していない条件は実行せず、成立として扱う。
// 対象外: 行の状況（Ready・Blocked など）の導出（group_lifecycle が扱い、このモデルは行の集合だけを見る）、
// 実際のコマンド内容と shell、作業ディレクトリ、終了コードの解釈、timeout と中断、
// 出力の上限と診断、非実行の閲覧、永続化、並行実行、許可の保存場所と digest、許可を読めない場合。
// これらは実装側で検証する。
// 対応する契約: ../docs/reference/candidates.md
module candidate_evaluation {
  import lifecycle_rules.* from "lifecycle_rules"
  import group_lifecycle.* from "group_lifecycle"

  // 保存する条件は Unset か外部コマンド一つ。変更は置き換え、解除は Unset への変更になる。
  // コマンド内容は不透明な識別子へ抽象化し、探索では2つの識別子で置き換えを区別する。
  // 識別子の数は探索上の制限で、製品のコマンド種類や長さの上限ではない。
  // 外部の結果は評価時に成立・未成立・判定失敗から選ぶ。許可されていない外部コマンドは実行せず、
  // 結果を Unapproved とする。Unapproved は浮上の判定では成立と同じに扱う。
  type Setting = Unset | ExternalCommand(int)
  type ObservationResult = Unevaluated | Satisfied | Unsatisfied | EvaluationFailed | Unapproved
  // ShowTarget は axon show ID の1回の表示で、Id は対象。未登録の枠を指す場合は何も評価しない。
  type ListingKind = JudgmentList | TasksList | ShowTarget(Id)
  type ListingStatus = Idle | Running | Succeeded(Set[Id]) | Failed(Id)

  // Listing は1回の取得の間だけ持つ検証用の一時状態で、Entity への保存項目ではない。
  // cache は同じ呼び出し中の評価結果を共有し、counts は評価回数、
  // runs は実際に外部コマンドを起動した回数、approvals は取得の開始時に読んだ許可の集合。
  // previousCache は前回と異なる結果への到達性を調べる ghost state で候補判定には参照しない。
  // 各 map は未登録の枠も含む全 ID を持ち、未登録の枠は Unevaluated・0 回のまま使われない。
  type Listing = {
    kind: ListingKind,
    status: ListingStatus,
    base: Set[Id],
    approvals: Set[int],
    cache: Id -> ObservationResult,
    counts: Id -> int,
    runs: Id -> int,
    previousCache: Id -> ObservationResult,
  }
  type ListingEvent = ListingInitialized | Began | EvaluatedEntity(Id)
    | Published | SettingEdited(Id) | EntitySetChanged | Allowed((Id, int))
  // approving は SettingEdited が自動で許可する設定（利用者が CLI でコマンドを指定した設定）だったことを示す。
  type ListingObservation = {
    beforeEntitySet: EntitySetState,
    beforeSettings: Id -> Setting,
    beforeApproved: Set[int],
    beforeListing: Listing,
    event: ListingEvent,
    approving: bool,
  }
  var settings: Id -> Setting
  // このマシンで許可したコマンド内容の集合。許可の範囲（Git 内では clone、Git 外では管理 root）ごとに一つで、
  // 記録には含まない。利用者が許可の file を手で編集して取り消す経路はモデルに含めない。
  var approved: Set[int]
  var listing: Listing
  var listingObservation: ListingObservation

  pure def emptyListing(kind: ListingKind): Listing = {
    kind: kind, status: Idle, base: Set(), approvals: Set(),
    cache: IDS.mapBy(_ => Unevaluated), counts: IDS.mapBy(_ => 0), runs: IDS.mapBy(_ => 0),
    previousCache: IDS.mapBy(_ => Unevaluated),
  }
  // 浮上の判定で成立として扱う結果。未許可は実行していないが、浮上を妨げない。
  pure def surfacing(r: ObservationResult): bool = r == Satisfied or r == Unapproved
  pure def isUnapproved(setting: Setting, approvals: Set[int]): bool = match setting {
    | ExternalCommand(c) => not(approvals.contains(c))
    | Unset => false
  }

  // group_lifecycle の init に加え、全 Entity の条件を Unset から始める。
  // 前段のモデルの false 初期入力は未成立の外部入力を指す指定であって、条件未設定の初期値ではない。
  action queryInit = all {
    init,
    settings' = IDS.mapBy(_ => Unset),
    approved' = Set(),
    listing' = emptyListing(JudgmentList),
    listingObservation' = {
      beforeEntitySet: initialEntitySet,
      beforeSettings: IDS.mapBy(_ => Unset), beforeApproved: Set(),
      beforeListing: emptyListing(JudgmentList),
      event: ListingInitialized, approving: false,
    },
  }

  // 評価が必要な範囲。proposals は stored が Undecided、tasks の浮上判定は stored が NotStarted の
  // Issue と Group を対象とし、どちらも dependency の完了や祖先の採用を要求しない。
  // 実効 lifecycle が InProgress の Group も stored は NotStarted なので、それ自身が対象に入る。
  // 終了した祖先の配下は対象から外す。祖先は上から評価し、未成立ならその配下を評価しない。
  // Group は stored が InProgress にならないため、候補自身でない進行中の祖先だけを評価する経路はない。
  // show は対象が tasks の行になる場合（stored が NotStarted）だけ、対象自身と、Group なら着手可能な
  // 子孫の Issue を対象にする。祖先は requiredEntities が加える。Undecided・進行中・終了した対象は評価しない。
  pure def baseCandidates(s: EntitySetState, kind: ListingKind): Set[Id] = {
    val selected = match kind {
      | JudgmentList => s.registered.filter(e => storedOf(s, e) == Undecided)
      | TasksList => s.registered.filter(e => storedOf(s, e) == NotStarted)
      | ShowTarget(t) => if (s.registered.contains(t) and storedOf(s, t) == NotStarted)
          Set(t).union(descendants(s, t).filter(d => startableIssues(s).contains(d)))
        else Set()
    }
    selected.filter(e => ancestorsOf(s, e).forall(a => not(terminal(storedOf(s, a)))))
  }
  pure def requiredEntities(s: EntitySetState, q: Listing): Set[Id] =
    q.base.union(q.base.map(e => ancestorsOf(s, e)).flatten())
  pure def parentsSatisfied(s: EntitySetState, q: Listing, e: Id): bool =
    ancestorsOf(s, e).forall(a => surfacing(q.cache.get(a)))
  pure def pending(s: EntitySetState, q: Listing): Set[Id] =
    requiredEntities(s, q).filter(e => q.cache.get(e) == Unevaluated and parentsSatisfied(s, q, e))

  // tasks は浮上した対象に加えて、浮上に関係なく進行中の Issue と、それを子孫に持つ Group を含む。
  pure def workingEntities(s: EntitySetState): Set[Id] =
    workingIssues(s).union(groupsWithWorkingDescendant(s))
  pure def visible(s: EntitySetState, q: Listing): Set[Id] = {
    val surfacedBase = q.base.filter(e => surfacing(q.cache.get(e)) and parentsSatisfied(s, q, e))
    if (q.kind == TasksList) surfacedBase.union(workingEntities(s)) else surfacedBase
  }
  def recordListing(event: ListingEvent): ListingObservation = {
    beforeEntitySet: entitySet, beforeSettings: settings, beforeApproved: approved,
    beforeListing: listing, event: event, approving: false,
  }
  action preserveBase = all {
    entitySet' = entitySet, conditions' = conditions, observation' = observation, history' = history,
  }

  // 取得は開始・Entity ごとの評価・公開の3段で進む。開始時に対象と許可の集合を確定し、
  // 前回の結果は引き継がない。
  action beginListing(kind: ListingKind): bool = all {
    listing.status != Running,
    preserveBase,
    settings' = settings,
    approved' = approved,
    listing' = { ...emptyListing(kind), status: Running, base: baseCandidates(entitySet, kind),
      approvals: approved, previousCache: listing.cache },
    listingObservation' = recordListing(Began),
  }

  // 評価できるのは祖先がすべて浮上した未評価の Entity だけで、各 Entity は1回の取得で最大一回。
  // 未設定は常に成立とする。開始時の許可の集合にない外部コマンドは実行せず Unapproved とし、
  // 未設定と同じく1回の観測ステップとして数える。判定失敗はその場で取得全体を失敗にし、
  // 途中の候補を公開しない。
  action evaluateEntity(e: Id, result: ObservationResult): bool = all {
    listing.status == Running,
    pending(entitySet, listing).contains(e),
    val unapproved = isUnapproved(settings.get(e), listing.approvals)
    val runsCommand = settings.get(e) != Unset and not(unapproved)
    // 未許可なら外部の結果の選択によらず Unapproved とする。
    val observed = if (unapproved) Unapproved else result
    all {
      Set(Satisfied, Unsatisfied, EvaluationFailed).contains(result),
      settings.get(e) == Unset implies result == Satisfied,
      preserveBase,
      settings' = settings,
      approved' = approved,
      listing' = {
        ...listing,
        status: if (observed == EvaluationFailed) Failed(e) else Running,
        cache: listing.cache.set(e, observed),
        counts: listing.counts.set(e, listing.counts.get(e) + 1),
        runs: listing.runs.set(e, listing.runs.get(e) + if (runsCommand) 1 else 0),
      },
    },
    listingObservation' = recordListing(EvaluatedEntity(e)),
  }

  // 未評価の必要 Entity が残っていないときにだけ結果を公開する。
  action publishListing = all {
    listing.status == Running,
    pending(entitySet, listing).size() == 0,
    preserveBase,
    settings' = settings,
    approved' = approved,
    listing' = { ...listing, status: Succeeded(visible(entitySet, listing)) },
    listingObservation' = recordListing(Published),
  }

  // 条件の設定・訂正・解除と、lifecycle・包含・dependency・登録・種類の変換は評価を呼ばない。
  // どちらも取得中には行わず、cache を初期化して次の取得へ結果を引き継がない。
  // 取得中に保存状態を固定するのは単一呼び出しを調べるための前提であり、
  // 並行更新や snapshot の実装契約を決めるものではない。
  // approve は利用者が CLI でコマンドを指定した設定（axon capture --command・axon condition set --command）で、
  // そのコマンドを自動で許可する。approve のない変更は、統合で取り込んだ記録や axon resolve で採った head の値を表す。
  action editSetting(e: Id, setting: Setting, approve: bool): bool = all {
    listing.status != Running,
    entitySet.registered.contains(e),
    settings.get(e) != setting,
    preserveBase,
    settings' = settings.set(e, setting),
    approved' = match setting {
      | ExternalCommand(c) => if (approve) approved.union(Set(c)) else approved
      | Unset => approved
    },
    listing' = emptyListing(listing.kind),
    listingObservation' = { ...recordListing(SettingEdited(e)), approving: approve },
  }
  action changeEntitySet = all {
    listing.status != Running,
    step,
    settings' = settings,
    approved' = approved,
    listing' = emptyListing(listing.kind),
    listingObservation' = recordListing(EntitySetChanged),
  }
  // Entity e の現在の条件の許可。axon condition allow と、保存値と同じ文字列の axon condition set（記録を作らない）を表す。
  // 評価を呼ばず、取得の途中でも起きうるが、取得は開始時に読んだ許可の集合を使うため、その取得の結果は変わらない。
  // 衝突中の Entity と digest の照合はこのモデルの対象外で、実装側で検証する。
  pure def commandOf(setting: Setting): Set[int] = match setting {
    | ExternalCommand(c) => Set(c)
    | Unset => Set()
  }
  action allowEntity(e: Id): bool = all {
    entitySet.registered.contains(e),
    commandOf(settings.get(e)).exists(c => not(approved.contains(c))),
    nondet c = commandOf(settings.get(e)).oneOf()
    all {
      approved' = approved.union(Set(c)),
      listingObservation' = recordListing(Allowed((e, c))),
      preserveBase,
      settings' = settings,
      listing' = listing,
    },
  }

  // 取得の各段と、取得外の条件編集・Entity集合の変更を混ぜて探索する。show の対象は全 ID から選ぶ。
  action queryStep = {
    nondet e = IDS.oneOf()
    nondet setting = Set(Unset, ExternalCommand(0), ExternalCommand(1)).oneOf()
    nondet kind = Set(JudgmentList, TasksList).union(IDS.map(id => ShowTarget(id))).oneOf()
    nondet result = Set(Satisfied, Unsatisfied, EvaluationFailed).oneOf()
    nondet approve = Bool.oneOf()
    any {
      changeEntitySet,
      editSetting(e, setting, approve),
      allowEntity(e),
      beginListing(kind),
      evaluateEntity(e, result),
      publishListing,
    }
  }

  // 到達性を補う入口。Group 0 とその子 Issue 1 への採用・着手・完了・Reopen と条件の設定、
  // tasks の取得だけを選び、Reopen した Issue の再表示、進行中の Group の評価と非浮上のままの表示、
  // 評価に失敗した当の Entity への着手へ到達させる。許可条件・状態更新・検査する性質は変えない。
  // 探索の選び方であって、製品の workflow を定めるものではない。
  action workAndList = {
    nondet id = Set(0, 1).oneOf()
    nondet op = Set(Accept, Start, Complete, Reopen).oneOf()
    nondet e = IDS.oneOf()
    nondet setting = Set(Unset, ExternalCommand(0)).oneOf()
    nondet result = Set(Satisfied, Unsatisfied, EvaluationFailed).oneOf()
    any {
      all {
        listing.status != Running,
        any { performIssue(id, op), performGroup(id, op, true) },
        settings' = settings,
        approved' = approved,
        listing' = emptyListing(listing.kind),
        listingObservation' = recordListing(EntitySetChanged),
      },
      editSetting(e, setting, true),
      beginListing(TasksList),
      evaluateEntity(e, result),
      publishListing,
    }
  }

  // 未許可を成立に固定したうえで、残りの未評価の入力をすべて成立と仮定した場合と、
  // すべて未成立と仮定した場合の抽象モデル側の候補集合を求める。どちらも公開した結果と一致すれば、
  // 省いた評価が候補の欠落を隠していないことになる。
  pure def observedConditions(q: Listing, unseen: bool): Conditions =
    IDS.mapBy(e => match q.cache.get(e) {
      | Satisfied => true
      | Unapproved => true
      | Unevaluated => unseen
      | _ => false
    })
  pure def abstractCandidates(s: EntitySetState, q: Listing, unseen: bool): Set[Id] = {
    val c = observedConditions(q, unseen)
    match q.kind {
      | JudgmentList => judgments(s, c)
      | TasksList => issueRows(s, c).union(groupRows(s, c))
      | ShowTarget(_) => q.base.filter(e => surfaced(s, c, e))
    }
  }
  // show の対象の状況と詰まっている理由を、未評価の入力の仮定ごとに求める。
  // Group は行の状況と理由、Issue は浮上しているか（Ready・Blocked か Unsurfaced か）が読み手に見える導出。
  pure def shownSituation(s: EntitySetState, q: Listing, t: Id, unseen: bool): (TaskStatus, Set[StallReason], bool) = {
    val c = observedConditions(q, unseen)
    if (kindOf(s, t) == Group)
      (groupStatus(s, c, t), stallReasons(s, c, t), false)
    else
      (Blocked, Set(), surfaced(s, c, t))
  }

  // invariant: 取得と外部の結果がEntity集合の状態・条件を変えないこと、評価回数の上限、
  // 評価してよい範囲、未設定が成立になること、呼び出しごとの初期化と呼び出し内の cache 共有、
  // 失敗が単一の Entity に対応して即座に取得を終えること、
  // 公開した結果が抽象モデルの一覧の行と一致し、tasks が進行中をすべて含むこと、
  // 条件編集と明示操作が評価を呼ばないこと、進行中の Issue を評価しないこと。
  val invQueryPreservesEntitySet = listingObservation.event != EntitySetChanged
    implies entitySet == listingObservation.beforeEntitySet
  // 許可が増えるのは、自動で許可する設定でそのコマンドを指定したときと、Entity の現在の条件を許可したときだけで、
  // 減ることはない。自動で許可しない設定（統合・解決で入った値）は許可を変えない。
  val invApprovalOwnership = match listingObservation.event {
    | Allowed(allowed) => approved == listingObservation.beforeApproved.union(Set(allowed._2))
        and settings.get(allowed._1) == ExternalCommand(allowed._2)
    | SettingEdited(e) => if (listingObservation.approving) (match settings.get(e) {
          | ExternalCommand(c) => approved == listingObservation.beforeApproved.union(Set(c))
          | Unset => approved == listingObservation.beforeApproved
        })
        else approved == listingObservation.beforeApproved
    | _ => approved == listingObservation.beforeApproved
  }
  val invSettingsOwnership = match listingObservation.event {
    | SettingEdited(e) => IDS.exclude(Set(e)).forall(other =>
        settings.get(other) == listingObservation.beforeSettings.get(other))
    | _ => settings == listingObservation.beforeSettings
  }
  val invAtMostOnce = IDS.forall(e => Set(0, 1).contains(listing.counts.get(e))
    and ((listing.counts.get(e) == 0) iff (listing.cache.get(e) == Unevaluated)))
  val invEvaluationScope = IDS.forall(e => listing.counts.get(e) == 1 implies and {
    requiredEntities(entitySet, listing).contains(e),
    not(terminal(storedOf(entitySet, e))),
    parentsSatisfied(entitySet, listing, e),
  })
  val invUnsetSatisfied = IDS.forall(e =>
    settings.get(e) == Unset and listing.counts.get(e) == 1 implies listing.cache.get(e) == Satisfied)
  // 評価した Entity が未許可になるのは、開始時の許可の集合にない外部コマンドを持つときだけで、そのときは実行しない。
  // 実行するのは許可済みの外部コマンドだけで、未設定も実行しない。
  val invUnapprovedNotRun = IDS.forall(e => and {
    listing.counts.get(e) == 1 implies
      ((listing.cache.get(e) == Unapproved) iff isUnapproved(settings.get(e), listing.approvals)),
    listing.runs.get(e) <= listing.counts.get(e),
    (listing.runs.get(e) == 1) iff (listing.counts.get(e) == 1
      and settings.get(e) != Unset and not(isUnapproved(settings.get(e), listing.approvals))),
  })
  // 取得が使う許可の集合は開始時に読んだもので、取得の途中の許可はその取得に入らない。許可は取り消さない。
  val invApprovalSnapshot = and {
    listing.approvals.subseteq(approved),
    match listingObservation.event {
      | Began => listing.approvals == approved
      | EvaluatedEntity(_) => listing.approvals == listingObservation.beforeListing.approvals
      | Published => listing.approvals == listingObservation.beforeListing.approvals
      | Allowed(_) => listing.approvals == listingObservation.beforeListing.approvals
      | _ => true
    },
  }
  // 未許可は浮上を妨げない。祖先が浮上した未許可の対象は、成功した結果に含まれる。
  val invUnapprovedSurfaces = match listing.status {
    | Succeeded(result) => listing.base.forall(e =>
        listing.cache.get(e) == Unapproved and parentsSatisfied(entitySet, listing, e)
          implies result.contains(e))
    | _ => true
  }
  val invFreshInvocation = listingObservation.event == Began implies
    IDS.forall(e => listing.counts.get(e) == 0 and listing.cache.get(e) == Unevaluated)
  val invCacheShared = match listingObservation.event {
    | EvaluatedEntity(e) => and {
        listingObservation.beforeListing.cache.get(e) == Unevaluated,
        IDS.exclude(Set(e)).forall(other => listing.cache.get(other)
          == listingObservation.beforeListing.cache.get(other)),
        listing.counts.get(e) == listingObservation.beforeListing.counts.get(e) + 1,
      }
    | Published => listing.cache == listingObservation.beforeListing.cache
      and listing.counts == listingObservation.beforeListing.counts
    | _ => true
  }
  val invFailureIsError = match listing.status {
    | Failed(e) => listing.cache.get(e) == EvaluationFailed
      and IDS.exclude(Set(e)).forall(other => listing.cache.get(other) != EvaluationFailed)
    | _ => IDS.forall(e => listing.cache.get(e) != EvaluationFailed)
  }
  val invFailureImmediate = match listingObservation.event {
    | EvaluatedEntity(e) => (listing.cache.get(e) == EvaluationFailed) iff (listing.status == Failed(e))
    | _ => true
  }
  val invSuccessfulListingComplete = match listing.status {
    | Succeeded(result) => and {
        result == abstractCandidates(entitySet, listing, false),
        result == abstractCandidates(entitySet, listing, true),
        pending(entitySet, listing).size() == 0,
      }
    | _ => true
  }
  // 条件の設定（自動の許可を含む）、明示操作、allow は評価を呼ばない。allow は取得中の cache・回数も変えない。
  val invNoEvaluationForEditsOrWork = match listingObservation.event {
    | SettingEdited(_) => listing.status == Idle and IDS.forall(e => listing.counts.get(e) == 0)
    | EntitySetChanged => listing.status == Idle and IDS.forall(e => listing.counts.get(e) == 0)
    | Allowed(_) => listing.counts == listingObservation.beforeListing.counts
      and listing.runs == listingObservation.beforeListing.runs
      and listing.cache == listingObservation.beforeListing.cache
    | _ => true
  }
  val invBaseStableDuringQuery = listing.status != Idle
    implies listing.base == baseCandidates(entitySet, listing.kind)
  val invNoUnresolvedPublication = listing.status == Running and pending(entitySet, listing).size() == 0
    implies listing.base.forall(e => listing.cache.get(e) != Unevaluated
      or ancestorsOf(entitySet, e).exists(a => listing.cache.get(a) == Unsatisfied))
  val invTasksIncludesWorking = match listing.status {
    | Succeeded(result) => listing.kind == TasksList implies workingEntities(entitySet).subseteq(result)
    | _ => true
  }
  // 進行中の Issue は stored が InProgress なのでどちらの一覧の対象にもならず、
  // 子を持たないので祖先としても評価されない。tasks には評価なしで載る。show の対象にもならない。
  val invWorkingIssuesNotEvaluated =
    workingIssues(entitySet).forall(e => listing.counts.get(e) == 0)
  // show が評価する範囲は、同じ計画で tasks が評価する範囲に含まれる。
  // 構成の確認: show の対象は stored が NotStarted の Entity に限り、それは tasks の対象でもある。
  val invShowRangeWithinTasks = match listing.kind {
    | ShowTarget(_) => listing.status != Idle implies requiredEntities(entitySet, listing).subseteq(
        baseCandidates(entitySet, TasksList).union(baseCandidates(entitySet, TasksList).map(e => ancestorsOf(entitySet, e)).flatten()))
    | _ => true
  }
  // show の対象の状況と詰まっている理由は評価した範囲で決まる。未評価の入力をすべて成立と仮定しても、
  // すべて未成立と仮定しても、Group の状況と理由、Issue が着手候補かどうかは変わらない。
  val invShowSituationDetermined = match listing.kind {
    | ShowTarget(t) => match listing.status {
      | Succeeded(_) => (entitySet.registered.contains(t) and storedOf(entitySet, t) == NotStarted) implies
          shownSituation(entitySet, listing, t, true) == shownSituation(entitySet, listing, t, false)
      | _ => true
    }
    | _ => true
  }

  // witness: 成功・空・失敗の各結果、未設定と外部コマンドの評価、条件の設定と訂正、
  // 失敗した条件の解除・訂正による回復、成立の持続と次回の未成立化、
  // 祖先の未成立による評価の省略と同じ祖先の結果の共有、終了した Entity の評価省略、
  // 依存待ち・未判断の祖先を持つ未着手や非浮上の進行中が tasks に出ること、
  // 進行中の Group の評価、未着手の Group の行、Reopen した Issue の再表示、
  // 評価に失敗した当の Entity への明示的な着手。
  val wBeginJudgment = listingObservation.event == Began and listing.kind == JudgmentList
  val wBeginTasks = listingObservation.event == Began and listing.kind == TasksList
  val wListingSuccess = listingObservation.event == Published and (match listing.status {
    | Succeeded(result) => result.size() > 0
    | _ => false
  })
  val wEmptyListing = listingObservation.event == Published and listing.status == Succeeded(Set())
  val wListingFailure = match listingObservation.event {
    | EvaluatedEntity(e) => listing.status == Failed(e)
    | _ => false
  }
  val wUnsetEvaluated = match listingObservation.event {
    | EvaluatedEntity(e) => settings.get(e) == Unset and listing.cache.get(e) == Satisfied
    | _ => false
  }
  val wConfiguredSatisfied = match listingObservation.event {
    | EvaluatedEntity(e) => settings.get(e) != Unset and listing.cache.get(e) == Satisfied
    | _ => false
  }
  val wUnsatisfiedSuccess = listingObservation.event == Published
    and IDS.exists(e => listing.cache.get(e) == Unsatisfied)
  val wSetCondition = match listingObservation.event {
    | SettingEdited(e) => listingObservation.beforeSettings.get(e) == Unset and settings.get(e) != Unset
    | _ => false
  }
  val wCorrectCondition = match listingObservation.event {
    | SettingEdited(e) => listingObservation.beforeSettings.get(e) != Unset and settings.get(e) != Unset
    | _ => false
  }
  val wClearFailedCondition = match listingObservation.event {
    | SettingEdited(e) => listingObservation.beforeListing.status == Failed(e) and settings.get(e) == Unset
    | _ => false
  }
  val wCorrectFailedCondition = match listingObservation.event {
    | SettingEdited(e) => listingObservation.beforeListing.status == Failed(e) and settings.get(e) != Unset
    | _ => false
  }
  val wReevaluateChanged = match listingObservation.event {
    | EvaluatedEntity(e) => listing.previousCache.get(e) == Satisfied and listing.cache.get(e) == Unsatisfied
    | _ => false
  }
  val wFailureRecovers = match listingObservation.event {
    | EvaluatedEntity(e) => listing.previousCache.get(e) == EvaluationFailed and listing.cache.get(e) == Satisfied
    | _ => false
  }
  val wSatisfiedNotConsumed = match listingObservation.event {
    | EvaluatedEntity(e) => settings.get(e) != Unset
      and listing.previousCache.get(e) == Satisfied and listing.cache.get(e) == Satisfied
    | _ => false
  }
  val wHiddenDescendantSkipped = listingObservation.event == Published and listing.base.exists(e =>
    settings.get(e) != Unset and listing.cache.get(e) == Unevaluated
      and ancestorsOf(entitySet, e).exists(a => listing.cache.get(a) == Unsatisfied))
  val wSharedParentOnce = listingObservation.event == Published and groups(entitySet).exists(g =>
    listing.base.filter(e => ancestorsOf(entitySet, e).contains(g)).size() >= 2
      and settings.get(g) != Unset and listing.counts.get(g) == 1)
  val wTerminalSkipped = listingObservation.event == Published and entitySet.registered.exists(e =>
    terminal(storedOf(entitySet, e)) and settings.get(e) != Unset and listing.counts.get(e) == 0)
  val wDependencyBlockedVisible = listingObservation.event == Published and listing.kind == TasksList
    and visible(entitySet, listing).exists(e => storedOf(entitySet, e) == NotStarted
      and not(depsComplete(entitySet, e)))
  // 祖先に未判断の Group がある未着手も隠さず tasks に出す（状況は Issue と子のある Group で Blocked、子のない Group で Empty）。
  val wUnadoptedAncestorVisible = listingObservation.event == Published and listing.kind == TasksList
    and visible(entitySet, listing).exists(e => storedOf(entitySet, e) == NotStarted
      and ancestorsOf(entitySet, e).exists(a => storedOf(entitySet, a) == Undecided))
  // 進行中の Issue は評価しないため、未成立の結果を持ちながら載るのは進行中の Group だけである。
  val wWorkingHiddenVisible = listingObservation.event == Published and listing.kind == TasksList
    and workingEntities(entitySet).exists(e => listing.cache.get(e) == Unsatisfied
      and visible(entitySet, listing).contains(e))
  // 主に進行中の Issue が該当し、祖先が未成立の進行中の Group も評価なしで載る。
  val wWorkingConditionSkipped = listingObservation.event == Published and listing.kind == TasksList
    and workingEntities(entitySet).exists(e => settings.get(e) != Unset
      and listing.counts.get(e) == 0 and visible(entitySet, listing).contains(e))
  // 実効 lifecycle が InProgress の Group は stored が NotStarted なので、それ自身の浮上判定として評価される。
  val wWorkingGroupEvaluated = listingObservation.event == Published and listing.kind == TasksList
    and (match listing.status {
      | Succeeded(result) => groups(entitySet).exists(g => effective(entitySet, g) == InProgress
          and listing.counts.get(g) == 1 and result.contains(g))
      | _ => false
    })
  val wGroupRowVisible = listingObservation.event == Published and listing.kind == TasksList
    and (match listing.status {
      | Succeeded(result) => groups(entitySet).exists(g => storedOf(entitySet, g) == NotStarted
          and effective(entitySet, g) == NotStarted and result.contains(g))
      | _ => false
    })
  val wReopenedVisible = listingObservation.event == Published and listing.kind == TasksList
    and (match listing.status {
      | Succeeded(result) => result.exists(e => history.reopenedIssues.contains(e))
      | _ => false
    })
  val wJudgmentDespiteDependency = listingObservation.event == Published and listing.kind == JudgmentList
    and listing.base.exists(e => not(depsComplete(entitySet, e)) and visible(entitySet, listing).contains(e))
  val wStartAfterEvaluationFailure = listingObservation.event == EntitySetChanged
    and (match observation.event {
      | LifecycleOperation(change) => change.op == Start
        and listingObservation.beforeListing.status == Failed(change.id)
      | _ => false
    })
  val wNestedAncestorSkipped = wHiddenDescendantSkipped and listing.base.exists(e =>
    ancestorsOf(entitySet, e).size() >= 2 and settings.get(e) != Unset
      and listing.cache.get(e) == Unevaluated
      and ancestorsOf(entitySet, e).exists(a => listing.cache.get(a) == Unsatisfied))
  // witness: 許可。利用者が指定した設定の自動の許可、許可しない設定（統合・解決）で入った条件が
  // 未許可として実行されずに表示されること、未許可の祖先の配下を評価すること、allow 後の次の取得での実行、
  // 取得の途中の allow がその取得の未許可を変えないこと。
  val wAutoApproved = match listingObservation.event {
    | SettingEdited(e) => match settings.get(e) {
        | ExternalCommand(c) => not(listingObservation.beforeApproved.contains(c)) and approved.contains(c)
        | Unset => false
      }
    | _ => false
  }
  val wUnapprovedSetting = match listingObservation.event {
    | SettingEdited(e) => isUnapproved(settings.get(e), approved)
    | _ => false
  }
  val wUnapprovedVisible = listingObservation.event == Published and (match listing.status {
    | Succeeded(result) => result.exists(e => listing.cache.get(e) == Unapproved)
    | _ => false
  })
  val wUnapprovedAncestorDescends = listingObservation.event == Published and listing.base.exists(e =>
    listing.counts.get(e) == 1 and ancestorsOf(entitySet, e).exists(a => listing.cache.get(a) == Unapproved))
  val wRunAfterAllow = match listingObservation.event {
    | EvaluatedEntity(e) => listing.previousCache.get(e) == Unapproved and listing.runs.get(e) == 1
    | _ => false
  }
  val wAllowDuringListing = match listingObservation.event {
    | Allowed(allowed) => listing.status == Running and IDS.exists(e =>
        listing.cache.get(e) == Unapproved and settings.get(e) == ExternalCommand(allowed._2))
    | _ => false
  }
  val wUnapprovedAfterAllowPublished = listingObservation.event == Published and IDS.exists(e =>
    listing.cache.get(e) == Unapproved and not(isUnapproved(settings.get(e), approved)))
  val wNoEligibleSkipsConfigured = wEmptyListing and listing.base.size() == 0
    and entitySet.registered.exists(e => settings.get(e) != Unset)
    and IDS.forall(e => listing.counts.get(e) == 0)

  // witness: show の表示。Group の対象で浮上していない着手可能な子孫を評価して見つける、
  // Issue の対象が祖先の未成立で評価されないまま浮上しないと決まる、対象が tasks の行にならないため
  // 何も評価しない、tasks より狭い範囲で終わる、判定失敗で表示全体が失敗する。
  def shownTarget: Set[Id] = match listing.kind {
    | ShowTarget(t) => Set(t)
    | _ => Set()
  }
  val wShowGroupHiddenCandidate = listingObservation.event == Published and shownTarget.exists(t =>
    kindOf(entitySet, t) == Group and descendants(entitySet, t).exists(d =>
      startableIssues(entitySet).contains(d) and listing.cache.get(d) == Unsatisfied))
  val wShowIssueUnderUnsurfacedAncestor = listingObservation.event == Published and shownTarget.exists(t =>
    kindOf(entitySet, t) == Issue and listing.base.contains(t) and listing.counts.get(t) == 0
      and ancestorsOf(entitySet, t).exists(a => listing.cache.get(a) == Unsatisfied))
  val wShowNothingEvaluated = listingObservation.event == Published and shownTarget.exists(t =>
    entitySet.registered.contains(t) and storedOf(entitySet, t) != NotStarted
      and entitySet.registered.exists(e => settings.get(e) != Unset)
      and IDS.forall(e => listing.counts.get(e) == 0))
  val wShowNarrowerThanTasks = listingObservation.event == Published and shownTarget.size() > 0
    and baseCandidates(entitySet, TasksList).exists(e => settings.get(e) != Unset
      and not(requiredEntities(entitySet, listing).contains(e)))
  val wShowFailure = shownTarget.size() > 0 and (match listingObservation.event {
    | EvaluatedEntity(e) => listing.status == Failed(e)
    | _ => false
  })

}
