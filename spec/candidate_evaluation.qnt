// 候補一覧の1回の取得を、Entity ごとの評価という観測ステップへ具体化するモデル。
// group_lifecycle の状態・許可条件・一覧の行を再利用し、そちらの bool 入力と行の集合を
// 失敗のない場合の基準として、評価の回数・省略・判定失敗を検査する。
// proposals は stored が Undecided の Entity、tasks は stored が NotStarted の Issue と Group の浮上と、
// 浮上に関係なく進行中の Issue と実効 lifecycle が InProgress の Group を対象にする。
// 対象外: 行の状況（Ready・Blocked など）の導出（group_lifecycle が扱い、このモデルは行の集合だけを見る）、
// 実際のコマンド内容と shell、作業ディレクトリ、終了コードの解釈、timeout と中断、
// 出力の上限と診断、非実行の閲覧、永続化、並行実行。これらは実装側で検証する。
// 対応する契約: ../docs/reference/candidates.md
module candidate_evaluation {
  import lifecycle_rules.* from "lifecycle_rules"
  import group_lifecycle.* from "group_lifecycle"

  // 保存する条件は Unset か外部コマンド一つ。変更は置き換え、解除は Unset への変更になる。
  // コマンド内容は不透明な識別子へ抽象化し、探索では2つの識別子で置き換えを区別する。
  // 識別子の数は探索上の制限で、製品のコマンド種類や長さの上限ではない。
  // 外部の結果は評価時に成立・未成立・判定失敗から選ぶ。
  type Setting = Unset | ExternalCommand(int)
  type ObservationResult = Unevaluated | Satisfied | Unsatisfied | EvaluationFailed
  type ListingKind = JudgmentList | TasksList
  type ListingStatus = Idle | Running | Succeeded(Set[Id]) | Failed(Id)

  // Listing は1回の取得の間だけ持つ検証用の一時状態で、Entity への保存項目ではない。
  // cache は同じ呼び出し中の評価結果を共有し、counts は評価回数、
  // previousCache は前回と異なる結果への到達性を調べる ghost state で候補判定には参照しない。
  // 各 map は未登録の枠も含む全 ID を持ち、未登録の枠は Unevaluated・0 回のまま使われない。
  type Listing = {
    kind: ListingKind,
    status: ListingStatus,
    base: Set[Id],
    cache: Id -> ObservationResult,
    counts: Id -> int,
    previousCache: Id -> ObservationResult,
  }
  type ListingEvent = ListingInitialized | Began | EvaluatedEntity(Id)
    | Published | SettingEdited(Id) | PlanChanged
  type ListingObservation = {
    beforePlan: PlanState,
    beforeSettings: Id -> Setting,
    beforeListing: Listing,
    event: ListingEvent,
  }
  var settings: Id -> Setting
  var listing: Listing
  var listingObservation: ListingObservation

  pure def emptyListing(kind: ListingKind): Listing = {
    kind: kind, status: Idle, base: Set(),
    cache: IDS.mapBy(_ => Unevaluated), counts: IDS.mapBy(_ => 0),
    previousCache: IDS.mapBy(_ => Unevaluated),
  }

  // group_lifecycle の init に加え、全 Entity の条件を Unset から始める。
  // 前段のモデルの false 初期入力は未成立の外部入力を指す指定であって、条件未設定の初期値ではない。
  action queryInit = all {
    init,
    settings' = IDS.mapBy(_ => Unset),
    listing' = emptyListing(JudgmentList),
    listingObservation' = {
      beforePlan: initialPlan,
      beforeSettings: IDS.mapBy(_ => Unset), beforeListing: emptyListing(JudgmentList),
      event: ListingInitialized,
    },
  }

  // 評価が必要な範囲。proposals は stored が Undecided、tasks の浮上判定は stored が NotStarted の
  // Issue と Group を対象とし、どちらも dependency の完了や祖先の採用を要求しない。
  // 実効 lifecycle が InProgress の Group も stored は NotStarted なので、それ自身が対象に入る。
  // 終了した祖先の配下は対象から外す。祖先は上から評価し、未成立ならその配下を評価しない。
  // Group は stored が InProgress にならないため、候補自身でない進行中の祖先だけを評価する経路はない。
  pure def baseCandidates(s: PlanState, kind: ListingKind): Set[Id] = {
    val selected = match kind {
      | JudgmentList => s.registered.filter(e => storedOf(s, e) == Undecided)
      | TasksList => s.registered.filter(e => storedOf(s, e) == NotStarted)
    }
    selected.filter(e => ancestorsOf(s, e).forall(a => not(terminal(storedOf(s, a)))))
  }
  pure def requiredEntities(s: PlanState, q: Listing): Set[Id] =
    q.base.union(q.base.map(e => ancestorsOf(s, e)).flatten())
  pure def parentsSatisfied(s: PlanState, q: Listing, e: Id): bool =
    ancestorsOf(s, e).forall(a => q.cache.get(a) == Satisfied)
  pure def pending(s: PlanState, q: Listing): Set[Id] =
    requiredEntities(s, q).filter(e => q.cache.get(e) == Unevaluated and parentsSatisfied(s, q, e))

  // tasks は浮上した対象に加えて、浮上に関係なく進行中の Issue と実効が InProgress の Group を含む。
  pure def workingEntities(s: PlanState): Set[Id] =
    workingIssues(s).union(workingGroups(s))
  pure def visible(s: PlanState, q: Listing): Set[Id] = {
    val surfacedBase = q.base.filter(e => q.cache.get(e) == Satisfied and parentsSatisfied(s, q, e))
    if (q.kind == TasksList) surfacedBase.union(workingEntities(s)) else surfacedBase
  }
  def recordListing(event: ListingEvent): ListingObservation = {
    beforePlan: plan, beforeSettings: settings, beforeListing: listing, event: event,
  }
  action preserveBase = all {
    plan' = plan, conditions' = conditions, observation' = observation, history' = history,
  }

  // 取得は開始・Entity ごとの評価・公開の3段で進む。開始時に対象を確定し、
  // 前回の結果は引き継がない。
  action beginListing(kind: ListingKind): bool = all {
    listing.status != Running,
    preserveBase,
    settings' = settings,
    listing' = { ...emptyListing(kind), status: Running, base: baseCandidates(plan, kind),
      previousCache: listing.cache },
    listingObservation' = recordListing(Began),
  }

  // 評価できるのは祖先がすべて成立した未評価の Entity だけで、各 Entity は1回の取得で最大一回。
  // 未設定は常に成立とする。判定失敗はその場で取得全体を失敗にし、途中の候補を公開しない。
  action evaluateEntity(e: Id, result: ObservationResult): bool = all {
    listing.status == Running,
    pending(plan, listing).contains(e),
    Set(Satisfied, Unsatisfied, EvaluationFailed).contains(result),
    settings.get(e) == Unset implies result == Satisfied,
    preserveBase,
    settings' = settings,
    listing' = {
      ...listing,
      status: if (result == EvaluationFailed) Failed(e) else Running,
      cache: listing.cache.set(e, result),
      counts: listing.counts.set(e, listing.counts.get(e) + 1),
    },
    listingObservation' = recordListing(EvaluatedEntity(e)),
  }

  // 未評価の必要 Entity が残っていないときにだけ結果を公開する。
  action publishListing = all {
    listing.status == Running,
    pending(plan, listing).size() == 0,
    preserveBase,
    settings' = settings,
    listing' = { ...listing, status: Succeeded(visible(plan, listing)) },
    listingObservation' = recordListing(Published),
  }

  // 条件の設定・訂正・解除と、lifecycle・包含・dependency・登録・種類の変換は評価を呼ばない。
  // どちらも取得中には行わず、cache を初期化して次の取得へ結果を引き継がない。
  // 取得中に保存状態を固定するのは単一呼び出しを調べるための前提であり、
  // 並行更新や snapshot の実装契約を決めるものではない。
  action editSetting(e: Id, setting: Setting): bool = all {
    listing.status != Running,
    plan.registered.contains(e),
    settings.get(e) != setting,
    preserveBase,
    settings' = settings.set(e, setting),
    listing' = emptyListing(listing.kind),
    listingObservation' = recordListing(SettingEdited(e)),
  }
  action changePlan = all {
    listing.status != Running,
    step,
    settings' = settings,
    listing' = emptyListing(listing.kind),
    listingObservation' = recordListing(PlanChanged),
  }

  // 取得の各段と、取得外の条件編集・計画変更を混ぜて探索する。
  action queryStep = {
    nondet e = IDS.oneOf()
    nondet setting = Set(Unset, ExternalCommand(0), ExternalCommand(1)).oneOf()
    nondet kind = Set(JudgmentList, TasksList).oneOf()
    nondet result = Set(Satisfied, Unsatisfied, EvaluationFailed).oneOf()
    any {
      changePlan,
      editSetting(e, setting),
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
        listing' = emptyListing(listing.kind),
        listingObservation' = recordListing(PlanChanged),
      },
      editSetting(e, setting),
      beginListing(TasksList),
      evaluateEntity(e, result),
      publishListing,
    }
  }

  // 未評価の入力をすべて成立と仮定した場合と、すべて未成立と仮定した場合の
  // 抽象モデル側の候補集合を求める。どちらも公開した結果と一致すれば、
  // 省いた評価が候補の欠落を隠していないことになる。
  pure def observedConditions(q: Listing, unseen: bool): Conditions =
    IDS.mapBy(e => match q.cache.get(e) {
      | Satisfied => true
      | Unevaluated => unseen
      | _ => false
    })
  pure def abstractCandidates(s: PlanState, q: Listing, unseen: bool): Set[Id] = {
    val c = observedConditions(q, unseen)
    match q.kind {
      | JudgmentList => judgments(s, c)
      | TasksList => issueRows(s, c).union(groupRows(s, c))
    }
  }

  // invariant: 取得と外部の結果が保存する計画・条件を変えないこと、評価回数の上限、
  // 評価してよい範囲、未設定が成立になること、呼び出しごとの初期化と呼び出し内の cache 共有、
  // 失敗が単一の Entity に対応して即座に取得を終えること、
  // 公開した結果が抽象モデルの一覧の行と一致し、tasks が進行中をすべて含むこと、
  // 条件編集と明示操作が評価を呼ばないこと、進行中の Issue を評価しないこと。
  val invQueryPreservesPlan = listingObservation.event != PlanChanged
    implies plan == listingObservation.beforePlan
  val invSettingsOwnership = match listingObservation.event {
    | SettingEdited(e) => IDS.exclude(Set(e)).forall(other =>
        settings.get(other) == listingObservation.beforeSettings.get(other))
    | _ => settings == listingObservation.beforeSettings
  }
  val invAtMostOnce = IDS.forall(e => Set(0, 1).contains(listing.counts.get(e))
    and ((listing.counts.get(e) == 0) iff (listing.cache.get(e) == Unevaluated)))
  val invEvaluationScope = IDS.forall(e => listing.counts.get(e) == 1 implies and {
    requiredEntities(plan, listing).contains(e),
    not(terminal(storedOf(plan, e))),
    parentsSatisfied(plan, listing, e),
  })
  val invUnsetSatisfied = IDS.forall(e =>
    settings.get(e) == Unset and listing.counts.get(e) == 1 implies listing.cache.get(e) == Satisfied)
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
        result == abstractCandidates(plan, listing, false),
        result == abstractCandidates(plan, listing, true),
        pending(plan, listing).size() == 0,
      }
    | _ => true
  }
  val invNoEvaluationForEditsOrWork = match listingObservation.event {
    | SettingEdited(_) => listing.status == Idle and IDS.forall(e => listing.counts.get(e) == 0)
    | PlanChanged => listing.status == Idle and IDS.forall(e => listing.counts.get(e) == 0)
    | _ => true
  }
  val invBaseStableDuringQuery = listing.status != Idle
    implies listing.base == baseCandidates(plan, listing.kind)
  val invNoUnresolvedPublication = listing.status == Running and pending(plan, listing).size() == 0
    implies listing.base.forall(e => listing.cache.get(e) != Unevaluated
      or ancestorsOf(plan, e).exists(a => listing.cache.get(a) == Unsatisfied))
  val invTasksIncludesWorking = match listing.status {
    | Succeeded(result) => listing.kind == TasksList implies workingEntities(plan).subseteq(result)
    | _ => true
  }
  // 進行中の Issue は stored が InProgress なのでどちらの一覧の対象にもならず、
  // 子を持たないので祖先としても評価されない。tasks には評価なしで載る。
  val invWorkingIssuesNotEvaluated =
    workingIssues(plan).forall(e => listing.counts.get(e) == 0)

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
      and ancestorsOf(plan, e).exists(a => listing.cache.get(a) == Unsatisfied))
  val wSharedParentOnce = listingObservation.event == Published and groups(plan).exists(g =>
    listing.base.filter(e => ancestorsOf(plan, e).contains(g)).size() >= 2
      and settings.get(g) != Unset and listing.counts.get(g) == 1)
  val wTerminalSkipped = listingObservation.event == Published and plan.registered.exists(e =>
    terminal(storedOf(plan, e)) and settings.get(e) != Unset and listing.counts.get(e) == 0)
  val wDependencyBlockedVisible = listingObservation.event == Published and listing.kind == TasksList
    and visible(plan, listing).exists(e => storedOf(plan, e) == NotStarted
      and not(depsComplete(plan, e)))
  // 祖先に未判断の Group がある未着手も隠さず tasks に出す（状況は Blocked として導出される）。
  val wUnadoptedAncestorVisible = listingObservation.event == Published and listing.kind == TasksList
    and visible(plan, listing).exists(e => storedOf(plan, e) == NotStarted
      and ancestorsOf(plan, e).exists(a => storedOf(plan, a) == Undecided))
  // 進行中の Issue は評価しないため、未成立の結果を持ちながら載るのは進行中の Group だけである。
  val wWorkingHiddenVisible = listingObservation.event == Published and listing.kind == TasksList
    and workingEntities(plan).exists(e => listing.cache.get(e) == Unsatisfied
      and visible(plan, listing).contains(e))
  // 主に進行中の Issue が該当し、祖先が未成立の進行中の Group も評価なしで載る。
  val wWorkingConditionSkipped = listingObservation.event == Published and listing.kind == TasksList
    and workingEntities(plan).exists(e => settings.get(e) != Unset
      and listing.counts.get(e) == 0 and visible(plan, listing).contains(e))
  // 実効 lifecycle が InProgress の Group は stored が NotStarted なので、それ自身の浮上判定として評価される。
  val wWorkingGroupEvaluated = listingObservation.event == Published and listing.kind == TasksList
    and (match listing.status {
      | Succeeded(result) => groups(plan).exists(g => effective(plan, g) == InProgress
          and listing.counts.get(g) == 1 and result.contains(g))
      | _ => false
    })
  val wGroupRowVisible = listingObservation.event == Published and listing.kind == TasksList
    and (match listing.status {
      | Succeeded(result) => groups(plan).exists(g => storedOf(plan, g) == NotStarted
          and effective(plan, g) == NotStarted and result.contains(g))
      | _ => false
    })
  val wReopenedVisible = listingObservation.event == Published and listing.kind == TasksList
    and (match listing.status {
      | Succeeded(result) => result.exists(e => history.reopenedIssues.contains(e))
      | _ => false
    })
  val wJudgmentDespiteDependency = listingObservation.event == Published and listing.kind == JudgmentList
    and listing.base.exists(e => not(depsComplete(plan, e)) and visible(plan, listing).contains(e))
  val wStartAfterEvaluationFailure = listingObservation.event == PlanChanged
    and (match observation.event {
      | LifecycleOperation(change) => change.op == Start
        and listingObservation.beforeListing.status == Failed(change.id)
      | _ => false
    })
  val wNestedAncestorSkipped = wHiddenDescendantSkipped and listing.base.exists(e =>
    ancestorsOf(plan, e).size() >= 2 and settings.get(e) != Unset
      and listing.cache.get(e) == Unevaluated
      and ancestorsOf(plan, e).exists(a => listing.cache.get(a) == Unsatisfied))
  val wNoEligibleSkipsConfigured = wEmptyListing and listing.base.size() == 0
    and plan.registered.exists(e => settings.get(e) != Unset)
    and IDS.forall(e => listing.counts.get(e) == 0)

}
