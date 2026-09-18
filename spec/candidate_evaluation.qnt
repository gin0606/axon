// 候補一覧の1回の取得を、Entity ごとの評価という観測ステップへ具体化するモデル。
// group_lifecycle の状態・許可条件・候補集合を再利用し、そちらの bool 入力と候補集合を
// 失敗のない場合の基準として、評価の回数・省略・判定失敗を検査する。
// 対象外: 実際のコマンド内容と shell、作業ディレクトリ、終了コードの解釈、timeout と中断、
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
  type ListingStatus = Idle | Running | Succeeded(Set[Entity]) | Failed(Entity)

  // Listing は1回の取得の間だけ持つ検証用の一時状態で、Entity への保存項目ではない。
  // cache は同じ呼び出し中の評価結果を共有し、counts は評価回数、
  // previousCache は前回と異なる結果への到達性を調べる ghost state で候補判定には参照しない。
  type Listing = {
    kind: ListingKind,
    status: ListingStatus,
    base: Set[Entity],
    cache: Entity -> ObservationResult,
    counts: Entity -> int,
    previousCache: Entity -> ObservationResult,
  }
  type ListingEvent = ListingInitialized | Began | EvaluatedEntity(Entity)
    | Published | SettingEdited(Entity) | PlanChanged
  type ListingObservation = {
    beforePlan: PlanState,
    beforeSettings: Entity -> Setting,
    beforeListing: Listing,
    event: ListingEvent,
  }
  var settings: Entity -> Setting
  var listing: Listing
  var listingObservation: ListingObservation

  pure def emptyListing(kind: ListingKind): Listing = {
    kind: kind, status: Idle, base: Set(),
    cache: REFS.mapBy(_ => Unevaluated), counts: REFS.mapBy(_ => 0),
    previousCache: REFS.mapBy(_ => Unevaluated),
  }

  // group_lifecycle の init に加え、全 Entity の条件を Unset から始める。
  // 前段のモデルの false 初期入力は未成立の外部入力を指す指定であって、条件未設定の初期値ではない。
  action queryInit = all {
    init,
    settings' = REFS.mapBy(_ => Unset),
    listing' = emptyListing(JudgmentList),
    listingObservation' = {
      beforePlan: {
        groups: GROUPS.mapBy(_ => Undecided), issues: ISSUES.mapBy(_ => Undecided),
        registered: Set(0, 1), parents: ISSUES.mapBy(id => if (id == 2) Unassigned else InGroup(0)),
        groupParents: GROUPS.mapBy(_ => Unassigned), dependencies: REFS.mapBy(_ => Set()),
      },
      beforeSettings: REFS.mapBy(_ => Unset), beforeListing: emptyListing(JudgmentList),
      event: ListingInitialized,
    },
  }

  // 評価が必要な範囲。proposals は未判断、tasks の浮上判定は未着手を対象とし、
  // どちらも dependency の完了や親の着手を要求しない。終了した祖先の配下は対象から外す。
  // 祖先は上から評価し、未成立ならその配下を評価しない。候補自身でない進行中の親でも、
  // 子の候補判定に必要なら評価する。
  pure def ancestors(s: PlanState, e: Entity): Set[Entity] =
    ancestorGroups(s, entityParent(s, e)).map(GroupRef)
  pure def baseCandidates(s: PlanState, kind: ListingKind): Set[Entity] = {
    val selected = match kind {
      | JudgmentList => entities(s).filter(e => entityState(s, e) == Undecided)
      | TasksList => entities(s).filter(e => entityState(s, e) == NotStarted)
    }
    selected.filter(e => ancestors(s, e).forall(a => not(terminal(entityState(s, a)))))
  }
  pure def requiredEntities(s: PlanState, q: Listing): Set[Entity] =
    q.base.union(q.base.map(e => ancestors(s, e)).flatten())
  pure def parentsSatisfied(s: PlanState, q: Listing, e: Entity): bool =
    ancestors(s, e).forall(a => q.cache.get(a) == Satisfied)
  pure def pending(s: PlanState, q: Listing): Set[Entity] =
    requiredEntities(s, q).filter(e => q.cache.get(e) == Unevaluated and parentsSatisfied(s, q, e))

  // tasks は浮上した未着手に加えて、浮上に関係なく全着手中を含む。
  pure def workingEntities(s: PlanState): Set[Entity] =
    entities(s).filter(e => entityState(s, e) == InProgress)
  pure def visible(s: PlanState, q: Listing): Set[Entity] = {
    val surfaced = q.base.filter(e => q.cache.get(e) == Satisfied and parentsSatisfied(s, q, e))
    if (q.kind == TasksList) surfaced.union(workingEntities(s)) else surfaced
  }
  def recordListing(event: ListingEvent): ListingObservation = {
    beforePlan: plan, beforeSettings: settings, beforeListing: listing, event: event,
  }
  action preserveBase = all {
    plan' = plan, conditions' = conditions, observation' = observation,
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
  action evaluateEntity(e: Entity, result: ObservationResult): bool = all {
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

  // 条件の設定・訂正・解除と、lifecycle・包含・dependency の変更は評価を呼ばない。
  // どちらも取得中には行わず、cache を初期化して次の取得へ結果を引き継がない。
  // 取得中に保存状態を固定するのは単一呼び出しを調べるための前提であり、
  // 並行更新や snapshot の実装契約を決めるものではない。
  action editSetting(e: Entity, setting: Setting): bool = all {
    listing.status != Running,
    entities(plan).contains(e),
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
    nondet e = REFS.oneOf()
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

  // 未評価の入力をすべて成立と仮定した場合と、すべて未成立と仮定した場合の
  // 抽象モデル側の候補集合を求める。どちらも公開した結果と一致すれば、
  // 省いた評価が候補の欠落を隠していないことになる。
  pure def observedConditions(q: Listing, unseen: bool): Conditions = {
    val satisfied = REFS.mapBy(e => match q.cache.get(e) {
      | Satisfied => true
      | Unevaluated => unseen
      | _ => false
    })
    { groups: GROUPS.mapBy(g => satisfied.get(GroupRef(g))),
      issues: ISSUES.mapBy(i => satisfied.get(IssueRef(i))) }
  }
  pure def abstractCandidates(s: PlanState, q: Listing, unseen: bool): Set[Entity] = {
    val c = observedConditions(q, unseen)
    match q.kind {
      | JudgmentList => judgmentIssues(s, c).map(IssueRef).union(judgmentGroups(s, c).map(GroupRef))
      | TasksList => workingEntities(s).union(entities(s).filter(e =>
          entityState(s, e) == NotStarted and parentSurfaced(s, c, entityParent(s, e))
            and (match e {
              | IssueRef(i) => c.issues.get(i)
              | GroupRef(g) => c.groups.get(g)
            })))
    }
  }

  // invariant: 取得と外部の結果が保存する計画・条件を変えないこと、評価回数の上限、
  // 評価してよい範囲、未設定が成立になること、呼び出しごとの初期化と呼び出し内の cache 共有、
  // 失敗が単一の Entity に対応して即座に取得を終えること、
  // 公開した結果が抽象モデルの候補集合と一致し、tasks が全着手中を含むこと、
  // 条件編集と明示操作が評価を呼ばないこと、着手中の評価が未着手の子孫のためだけであること。
  val invQueryPreservesPlan = listingObservation.event != PlanChanged
    implies plan == listingObservation.beforePlan
  val invSettingsOwnership = match listingObservation.event {
    | SettingEdited(e) => REFS.exclude(Set(e)).forall(other =>
        settings.get(other) == listingObservation.beforeSettings.get(other))
    | _ => settings == listingObservation.beforeSettings
  }
  val invAtMostOnce = REFS.forall(e => Set(0, 1).contains(listing.counts.get(e))
    and ((listing.counts.get(e) == 0) iff (listing.cache.get(e) == Unevaluated)))
  val invEvaluationScope = REFS.forall(e => listing.counts.get(e) == 1 implies and {
    requiredEntities(plan, listing).contains(e),
    not(terminal(entityState(plan, e))),
    parentsSatisfied(plan, listing, e),
  })
  val invUnsetSatisfied = REFS.forall(e =>
    settings.get(e) == Unset and listing.counts.get(e) == 1 implies listing.cache.get(e) == Satisfied)
  val invFreshInvocation = listingObservation.event == Began implies
    REFS.forall(e => listing.counts.get(e) == 0 and listing.cache.get(e) == Unevaluated)
  val invCacheShared = match listingObservation.event {
    | EvaluatedEntity(e) => and {
        listingObservation.beforeListing.cache.get(e) == Unevaluated,
        REFS.exclude(Set(e)).forall(other => listing.cache.get(other)
          == listingObservation.beforeListing.cache.get(other)),
        listing.counts.get(e) == listingObservation.beforeListing.counts.get(e) + 1,
      }
    | Published => listing.cache == listingObservation.beforeListing.cache
      and listing.counts == listingObservation.beforeListing.counts
    | _ => true
  }
  val invFailureIsError = match listing.status {
    | Failed(e) => listing.cache.get(e) == EvaluationFailed
      and REFS.exclude(Set(e)).forall(other => listing.cache.get(other) != EvaluationFailed)
    | _ => REFS.forall(e => listing.cache.get(e) != EvaluationFailed)
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
    | SettingEdited(_) => listing.status == Idle and REFS.forall(e => listing.counts.get(e) == 0)
    | PlanChanged => listing.status == Idle and REFS.forall(e => listing.counts.get(e) == 0)
    | _ => true
  }
  val invBaseStableDuringQuery = listing.status != Idle
    implies listing.base == baseCandidates(plan, listing.kind)
  val invNoUnresolvedPublication = listing.status == Running and pending(plan, listing).size() == 0
    implies listing.base.forall(e => listing.cache.get(e) != Unevaluated
      or ancestors(plan, e).exists(a => listing.cache.get(a) == Unsatisfied))

  // witness: 成功・空・失敗の各結果、未設定と外部コマンドの評価、条件の設定と訂正、
  // 失敗した条件の解除・訂正による回復、成立の持続と次回の未成立化、
  // 祖先の未成立による評価の省略と同じ祖先の結果の共有、終了した Entity の評価省略、
  // 依存待ち・親の着手待ちの未着手や非浮上の着手中が tasks に出ること、
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
    and REFS.exists(e => listing.cache.get(e) == Unsatisfied)
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
      and ancestors(plan, e).exists(a => listing.cache.get(a) == Unsatisfied))
  val wSharedParentOnce = listingObservation.event == Published and GROUPS.exists(g =>
    listing.base.filter(e => ancestors(plan, e).contains(GroupRef(g))).size() >= 2
      and settings.get(GroupRef(g)) != Unset and listing.counts.get(GroupRef(g)) == 1)
  val wWorkingParentEvaluated = listingObservation.event == Published and GROUPS.exists(g =>
    plan.groups.get(g) == InProgress and not(listing.base.contains(GroupRef(g)))
      and listing.counts.get(GroupRef(g)) == 1)
  val wTerminalSkipped = listingObservation.event == Published and entities(plan).exists(e =>
    terminal(entityState(plan, e)) and settings.get(e) != Unset and listing.counts.get(e) == 0)
  val wDependencyBlockedVisible = listingObservation.event == Published and listing.kind == TasksList
    and visible(plan, listing).exists(e => entityState(plan, e) == NotStarted
      and not(prerequisitesComplete(plan, e)))
  val wParentBlockedVisible = listingObservation.event == Published and listing.kind == TasksList
    and visible(plan, listing).exists(e => entityState(plan, e) == NotStarted
      and not(parentWorking(plan, entityParent(plan, e))))
  val wWorkingHiddenVisible = listingObservation.event == Published and listing.kind == TasksList
    and workingEntities(plan).exists(e => listing.cache.get(e) == Unsatisfied
      and visible(plan, listing).contains(e))
  val wWorkingConditionSkipped = listingObservation.event == Published and listing.kind == TasksList
    and workingEntities(plan).exists(e => settings.get(e) != Unset
      and listing.counts.get(e) == 0 and visible(plan, listing).contains(e))
  val invTasksIncludesWorking = (match listing.status {
    | Succeeded(result) => listing.kind == TasksList implies workingEntities(plan).subseteq(result)
    | _ => true
  })
  val invWorkingEvaluationOnlyForDescendants = listing.kind == TasksList implies
    workingEntities(plan).forall(e => listing.counts.get(e) == 1 implies
      listing.base.exists(child => ancestors(plan, child).contains(e)))
  val wJudgmentDespiteDependency = listingObservation.event == Published and listing.kind == JudgmentList
    and listing.base.exists(e => not(prerequisitesComplete(plan, e)) and visible(plan, listing).contains(e))
  val wStartAfterEvaluationFailure = listingObservation.event == PlanChanged
    and (match observation.event {
      | IssueOperation(change) => change.op == Start
        and listingObservation.beforeListing.status == Failed(IssueRef(change.id))
      | GroupOperation(change) => change.op == Start
        and listingObservation.beforeListing.status == Failed(GroupRef(change.id))
      | _ => false
    })
  val wNestedAncestorSkipped = wHiddenDescendantSkipped and listing.base.exists(e =>
    ancestors(plan, e).size() >= 2 and settings.get(e) != Unset
      and listing.cache.get(e) == Unevaluated
      and ancestors(plan, e).exists(a => listing.cache.get(a) == Unsatisfied))
  val wNoEligibleSkipsConfigured = wEmptyListing and listing.base.size() == 0
    and entities(plan).exists(e => settings.get(e) != Unset)
    and REFS.forall(e => listing.counts.get(e) == 0)

}
