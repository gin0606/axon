//! 導出値。ready / blocked / orphaned は保存せず、状態と依存から毎回計算する。

use crate::domain::*;
use chrono::Utc;
use std::collections::{HashMap, HashSet};

pub struct View {
    by_id: HashMap<IssueId, Issue>,
    order: Vec<IssueId>,
    deps: Vec<(IssueId, IssueId)>,
    groups: Vec<Group>,
    group_deps: Vec<(GroupId, GroupId)>,
}

impl View {
    pub fn new(
        issues: Vec<Issue>,
        deps: Vec<(IssueId, IssueId)>,
        groups: Vec<Group>,
        group_deps: Vec<(GroupId, GroupId)>,
    ) -> Self {
        let order = issues.iter().map(|i| i.id.clone()).collect();
        let by_id = issues.into_iter().map(|i| (i.id.clone(), i)).collect();
        View {
            by_id,
            order,
            deps,
            groups,
            group_deps,
        }
    }

    pub fn get(&self, id: &IssueId) -> Option<&Issue> {
        self.by_id.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Issue> {
        self.order.iter().filter_map(|id| self.by_id.get(id))
    }

    /// この issue が前提としている issue。
    pub fn depends_on(&self, id: &IssueId) -> Vec<&Issue> {
        self.deps
            .iter()
            .filter(|(from, _)| from == id)
            .filter_map(|(_, to)| self.by_id.get(to))
            .collect()
    }

    /// この issue の完了を待っている issue。
    pub fn dependents(&self, id: &IssueId) -> Vec<&Issue> {
        self.deps
            .iter()
            .filter(|(_, to)| to == id)
            .filter_map(|(from, _)| self.by_id.get(from))
            .collect()
    }

    /// C の条件を満たして浮上しているか。条件が無ければ常に浮上している。
    pub fn is_surfaced(&self, issue: &Issue) -> bool {
        match &issue.resurface_condition {
            ResurfaceCondition::Always => true,
            ResurfaceCondition::AtDate(date) => *date <= Utc::now().date_naive(),
            ResurfaceCondition::AfterIssue(id) => {
                // 参照先が消えていたら待つ理由も無い
                self.by_id.get(id).map(|i| i.is_terminal()).unwrap_or(true)
            }
        }
    }

    /// 依存先に未終端のものがある。
    pub fn is_blocked(&self, id: &IssueId) -> bool {
        self.depends_on(id).iter().any(|d| !d.is_terminal())
    }

    /// 依存先が不採用になり、前提が永久に満たされない。推移させない (docs/axes.md D-1)。
    pub fn is_orphaned(&self, id: &IssueId) -> bool {
        self.depends_on(id)
            .iter()
            .any(|d| d.disposition == Disposition::Rejected)
    }

    pub fn is_ready(&self, issue: &Issue) -> bool {
        matches!(issue.progress, Progress::NotStarted)
            && issue.disposition == Disposition::Accepted
            && self.is_surfaced(issue)
            && !self.is_blocked(&issue.id)
            && !self.is_orphaned(&issue.id)
            && !self.is_group_blocked(issue)
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    pub fn group(&self, id: &GroupId) -> Option<&Group> {
        self.groups.iter().find(|g| &g.id == id)
    }

    /// グループとその子孫グループ。訪問済みを覚えることで、親子が循環しても止まる。
    fn subtree(&self, root: &GroupId) -> HashSet<GroupId> {
        let mut seen = HashSet::new();
        let mut stack = vec![root.clone()];
        while let Some(g) = stack.pop() {
            if !seen.insert(g.clone()) {
                continue;
            }
            for child in self.groups.iter().filter(|c| c.parent.as_ref() == Some(&g)) {
                stack.push(child.id.clone());
            }
        }
        seen
    }

    /// 自分自身と祖先グループ。親の依存は子にも効く。
    fn ancestors(&self, from: &GroupId) -> Vec<GroupId> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut cur = Some(from.clone());
        while let Some(g) = cur {
            if !seen.insert(g.clone()) {
                break;
            }
            out.push(g.clone());
            cur = self.group(&g).and_then(|x| x.parent.clone());
        }
        out
    }

    pub fn issues_in(&self, g: &GroupId) -> Vec<&Issue> {
        let subtree = self.subtree(g);
        self.iter()
            .filter(|i| i.group.as_ref().is_some_and(|gid| subtree.contains(gid)))
            .collect()
    }

    /// 全子孫 issue が終端に達したか。不採用が混じっていても解除する (docs/axes.md G-3)。
    pub fn group_satisfied(&self, g: &GroupId) -> bool {
        self.issues_in(g).iter().all(|i| i.is_terminal())
    }

    /// 所属グループか祖先グループの依存先に、未達のものがあるか。
    pub fn is_group_blocked(&self, issue: &Issue) -> bool {
        let Some(gid) = &issue.group else {
            return false;
        };
        self.ancestors(gid).iter().any(|g| {
            self.group_deps
                .iter()
                .filter(|(from, _)| from == g)
                .any(|(_, to)| !self.group_satisfied(to))
        })
    }

    /// グループが待っている相手のうち、まだ満たされていないもの。
    pub fn group_waiting_on(&self, g: &GroupId) -> Vec<&Group> {
        self.ancestors(g)
            .iter()
            .flat_map(|a| {
                self.group_deps
                    .iter()
                    .filter(move |(from, _)| from == a)
                    .filter_map(|(_, to)| self.group(to))
            })
            .filter(|target| !self.group_satisfied(&target.id))
            .collect()
    }

    /// これ以上遡っても意味がない原因かどうか。
    /// 前提を失っている・後送り中・未解決の依存を持たない、のいずれか。
    fn is_root_cause(&self, issue: &Issue) -> bool {
        self.is_orphaned(&issue.id)
            || !self.is_surfaced(issue)
            || self.depends_on(&issue.id).iter().all(|d| d.is_terminal())
    }

    /// issue 依存を遡って、実際に止めている issue を集める。
    /// orphaned を推移させない代わりにこれが原因を説明する (docs/axes.md D-1 派生)。
    pub fn blocking_causes(&self, id: &IssueId) -> Vec<&Issue> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        self.walk_causes(id, &mut seen, &mut out);
        out
    }

    fn walk_causes<'a>(
        &'a self,
        id: &IssueId,
        seen: &mut HashSet<IssueId>,
        out: &mut Vec<&'a Issue>,
    ) {
        for d in self.depends_on(id) {
            if d.is_terminal() || !seen.insert(d.id.clone()) {
                continue;
            }
            if self.is_root_cause(d) {
                out.push(d);
            } else {
                self.walk_causes(&d.id, seen, out);
            }
        }
    }

    /// グループ依存で止まっている場合の内訳。どのグループの何が残っているかを返す。
    pub fn group_blocking_causes(&self, issue: &Issue) -> Vec<(&Group, Vec<&Issue>)> {
        let Some(gid) = &issue.group else {
            return Vec::new();
        };
        self.ancestors(gid)
            .iter()
            .flat_map(|a| {
                self.group_deps
                    .iter()
                    .filter(move |(from, _)| from == a)
                    .filter_map(|(_, to)| self.group(to))
            })
            .filter(|g| !self.group_satisfied(&g.id))
            .map(|g| {
                let blockers = self
                    .issues_in(&g.id)
                    .into_iter()
                    .filter(|i| !i.is_terminal())
                    .collect();
                (g, blockers)
            })
            .collect()
    }

    /// 放置されたまま残っている claim。
    /// 時間だけで判断すると長時間の作業を誤検出するため、プロセスの生存も併せて見る。
    /// 検出するだけで、解放は人手に委ねる (docs/data-model.md D-6)。
    pub fn stale_claims(&self, threshold_hours: i64) -> Vec<(&Issue, &Claim)> {
        let now = Utc::now();
        self.iter()
            .filter_map(|i| i.progress.claim().map(|c| (i, c)))
            .filter(|(_, c)| {
                let elapsed = now.signed_duration_since(c.at).num_hours();
                elapsed >= threshold_hours && !crate::actor::process_alive(c.pid)
            })
            .collect()
    }

    /// 進捗。分母は採用したものだけ (docs/axes.md §3)。
    pub fn group_progress(&self, g: &GroupId) -> GroupProgress {
        let issues = self.issues_in(g);
        let accepted: Vec<_> = issues
            .iter()
            .filter(|i| i.disposition == Disposition::Accepted)
            .collect();
        GroupProgress {
            done: accepted
                .iter()
                .filter(|i| matches!(i.progress, Progress::Ended))
                .count(),
            total: accepted.len(),
            undecided: issues
                .iter()
                .filter(|i| i.disposition == Disposition::Undecided)
                .count(),
        }
    }

    pub fn ready(&self) -> Vec<&Issue> {
        self.iter().filter(|i| self.is_ready(i)).collect()
    }

    /// 人間の判断を待っているもの。機械には決められない。
    pub fn triage(&self) -> Vec<(&Issue, TriageReason)> {
        self.iter()
            .filter_map(|i| {
                if i.disposition == Disposition::Undecided {
                    Some((i, TriageReason::Undecided))
                } else if !i.is_terminal() && self.is_orphaned(&i.id) {
                    Some((i, TriageReason::Orphaned))
                } else {
                    None
                }
            })
            .collect()
    }

    /// `id` が終端に達したことで新たに着手可能になるもの。`done` の後押しに使う。
    pub fn newly_ready_after(&self, id: &IssueId) -> Vec<&Issue> {
        self.dependents(id)
            .into_iter()
            .filter(|i| self.is_ready(i))
            .collect()
    }
}

/// 進捗は「採用したもののうち、どれだけ終わったか」。
/// 未判断が残っているとグループは完了しないため、その数を併せて持つ。
pub struct GroupProgress {
    pub done: usize,
    pub total: usize,
    pub undecided: usize,
}

impl GroupProgress {
    /// 分母が空のとき 100% と呼んではいけないので、率は Option で返す。
    pub fn ratio(&self) -> Option<f64> {
        (self.total > 0).then(|| self.done as f64 / self.total as f64)
    }
}

/// なぜ判断が要るのか。
pub enum TriageReason {
    /// やるかどうかを決めていない
    Undecided,
    /// 依存先が不採用になり、前提を失った
    Orphaned,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn iid(id: &str) -> IssueId {
        IssueId::from_stored(id)
    }

    fn gid(id: &str) -> GroupId {
        GroupId::from_stored(id)
    }

    fn issue(id: &str, progress: Progress, disposition: Disposition) -> Issue {
        let now = Utc::now();
        Issue {
            id: iid(id),
            title: id.to_string(),
            description: None,
            progress,
            disposition,
            resurface_condition: ResurfaceCondition::Always,
            group: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn accepted(id: &str) -> Issue {
        issue(id, Progress::NotStarted, Disposition::Accepted)
    }

    fn undecided(id: &str) -> Issue {
        issue(id, Progress::NotStarted, Disposition::Undecided)
    }

    fn rejected(id: &str) -> Issue {
        issue(id, Progress::NotStarted, Disposition::Rejected)
    }

    fn done(id: &str) -> Issue {
        issue(id, Progress::Ended, Disposition::Accepted)
    }

    /// 「完了 かつ 不採用」。
    fn done_rejected(id: &str) -> Issue {
        issue(id, Progress::Ended, Disposition::Rejected)
    }

    /// pid 0 は `process_alive` が常に死んでいると判定するため、OS に依存せず stale を作れる。
    fn claim(pid: i32, hours_ago: i64) -> Claim {
        Claim {
            actor: "tester".to_string(),
            session: "s".to_string(),
            pid,
            at: Utc::now() - Duration::hours(hours_ago),
        }
    }

    fn in_progress(id: &str, c: Claim) -> Issue {
        issue(id, Progress::InProgress(c), Disposition::Accepted)
    }

    fn in_group(mut i: Issue, g: &str) -> Issue {
        i.group = Some(gid(g));
        i
    }

    fn with_cond(mut i: Issue, c: ResurfaceCondition) -> Issue {
        i.resurface_condition = c;
        i
    }

    fn group(id: &str, parent: Option<&str>) -> Group {
        Group {
            id: gid(id),
            slug: id.to_string(),
            name: id.to_string(),
            description: None,
            parent: parent.map(gid),
        }
    }

    fn view(issues: Vec<Issue>, deps: &[(&str, &str)]) -> View {
        View::new(
            issues,
            deps.iter().map(|(a, b)| (iid(a), iid(b))).collect(),
            Vec::new(),
            Vec::new(),
        )
    }

    fn view_with_groups(
        issues: Vec<Issue>,
        deps: &[(&str, &str)],
        groups: Vec<Group>,
        group_deps: &[(&str, &str)],
    ) -> View {
        View::new(
            issues,
            deps.iter().map(|(a, b)| (iid(a), iid(b))).collect(),
            groups,
            group_deps.iter().map(|(a, b)| (gid(a), gid(b))).collect(),
        )
    }

    fn ids(issues: &[&Issue]) -> Vec<String> {
        let mut v: Vec<String> = issues.iter().map(|i| i.id.to_string()).collect();
        v.sort();
        v
    }

    // ---- ready の定義 ----

    #[test]
    fn ready_requires_accepted_and_not_started() {
        let v = view(
            vec![accepted("a"), undecided("b"), rejected("c"), done("d")],
            &[],
        );
        assert_eq!(ids(&v.ready()), ["a"]);
    }

    #[test]
    fn in_progress_is_not_ready() {
        let v = view(vec![in_progress("a", claim(1, 0))], &[]);
        assert!(v.ready().is_empty());
    }

    /// spec: invReadyExclusive — ready なら blocked でも orphaned でもない。
    #[test]
    fn inv_ready_exclusive() {
        let v = view(
            vec![
                accepted("a"),
                accepted("b"),
                accepted("c"),
                accepted("w"),
                rejected("z"),
            ],
            &[("b", "w"), ("c", "z")],
        );
        for i in v.ready() {
            assert!(!v.is_blocked(&i.id), "{} が blocked なのに ready", i.id);
            assert!(!v.is_orphaned(&i.id), "{} が orphaned なのに ready", i.id);
        }
        assert_eq!(ids(&v.ready()), ["a", "w"]);
    }

    /// spec: invRejectedDoneStillBlocks — 「完了 かつ 不採用」の依存先は解放しない。
    #[test]
    fn inv_rejected_done_still_blocks() {
        let v = view(vec![accepted("y"), done_rejected("x")], &[("y", "x")]);
        assert!(!v.is_blocked(&iid("y")), "終端なので blocked ではない");
        assert!(v.is_orphaned(&iid("y")));
        assert!(!v.is_ready(v.get(&iid("y")).unwrap()));
    }

    // ---- D-1 派生: orphaned は推移させない ----

    /// spec の invNoHiddenDeadlock は意図的に違反する (docs/axes.md D-1 派生)。
    #[test]
    fn orphaned_does_not_propagate() {
        let v = view(
            vec![accepted("y"), accepted("x"), rejected("z")],
            &[("y", "x"), ("x", "z")],
        );
        assert!(v.is_orphaned(&iid("x")));
        assert!(
            !v.is_orphaned(&iid("y")),
            "orphaned は直接の依存先だけを見る"
        );
        assert_eq!(ids(&v.blocking_causes(&iid("y"))), ["x"]);
    }

    /// spec: invBlockedHasCause / invCauseIsUnresolved
    #[test]
    fn inv_blocked_has_cause() {
        let v = view(
            vec![accepted("y"), accepted("x"), accepted("w"), done("d")],
            &[("y", "x"), ("y", "d"), ("x", "w")],
        );
        let cause = v.blocking_causes(&iid("y"));
        assert!(v.is_blocked(&iid("y")));
        assert!(!cause.is_empty(), "blocked なら原因が 1 つ以上ある");
        assert!(
            cause.iter().all(|i| !i.is_terminal()),
            "原因は未終端のものだけ"
        );
        assert_eq!(ids(&cause), ["w"], "中継の x ではなく鎖の先頭を指す");
    }

    #[test]
    fn deferred_dependency_is_a_root_cause() {
        let future = Utc::now().date_naive() + Duration::days(7);
        let v = view(
            vec![
                accepted("y"),
                with_cond(accepted("x"), ResurfaceCondition::AtDate(future)),
                accepted("w"),
            ],
            &[("y", "x"), ("x", "w")],
        );
        assert_eq!(ids(&v.blocking_causes(&iid("y"))), ["x"]);
    }

    #[test]
    fn cause_walk_stops_on_dependency_cycle() {
        let v = view(
            vec![accepted("a"), accepted("b")],
            &[("a", "b"), ("b", "a")],
        );
        assert!(v.is_blocked(&iid("a")));
        assert!(v.blocking_causes(&iid("a")).is_empty());
    }

    // ---- C: 浮上条件 ----

    #[test]
    fn condition_at_surfaces_on_or_before_today() {
        let today = Utc::now().date_naive();
        let v = view(
            vec![
                with_cond(
                    accepted("past"),
                    ResurfaceCondition::AtDate(today - Duration::days(1)),
                ),
                with_cond(accepted("today"), ResurfaceCondition::AtDate(today)),
                with_cond(
                    accepted("future"),
                    ResurfaceCondition::AtDate(today + Duration::days(1)),
                ),
            ],
            &[],
        );
        assert_eq!(ids(&v.ready()), ["past", "today"]);
    }

    /// spec: invCondRefSatisfiedByRejection — C の参照先が不採用でも浮上する。
    #[test]
    fn inv_resurface_ref_satisfied_by_rejection() {
        let v = view(
            vec![
                with_cond(accepted("y"), ResurfaceCondition::AfterIssue(iid("x"))),
                rejected("x"),
            ],
            &[],
        );
        assert!(v.is_ready(v.get(&iid("y")).unwrap()));
    }

    /// spec: invCondAndDepDiffer — 同じ相手でも C 参照と D 依存で挙動が分かれる。
    /// この非対称が C-1 差し替えの根拠 (docs/axes.md C-1)。
    #[test]
    fn inv_cond_and_dep_differ() {
        let by_cond = view(
            vec![
                with_cond(accepted("y"), ResurfaceCondition::AfterIssue(iid("x"))),
                rejected("x"),
            ],
            &[],
        );
        let by_dep = view(vec![accepted("y"), rejected("x")], &[("y", "x")]);
        assert!(
            by_cond.is_ready(by_cond.get(&iid("y")).unwrap()),
            "C 参照は浮上する"
        );
        assert!(by_dep.is_orphaned(&iid("y")), "D 依存は前提を失う");
        assert!(!by_dep.is_ready(by_dep.get(&iid("y")).unwrap()));
    }

    #[test]
    fn condition_ref_to_missing_issue_surfaces() {
        let v = view(
            vec![with_cond(
                accepted("y"),
                ResurfaceCondition::AfterIssue(iid("gone")),
            )],
            &[],
        );
        assert!(v.is_surfaced(v.get(&iid("y")).unwrap()));
    }

    #[test]
    fn condition_ref_waits_for_unterminated_issue() {
        let v = view(
            vec![
                with_cond(accepted("y"), ResurfaceCondition::AfterIssue(iid("x"))),
                accepted("x"),
            ],
            &[],
        );
        assert!(!v.is_surfaced(v.get(&iid("y")).unwrap()));
        assert_eq!(ids(&v.ready()), ["x"]);
    }

    // ---- グループ ----

    /// spec: invGroupBlockedNotReady
    #[test]
    fn inv_group_blocked_not_ready() {
        let v = view_with_groups(
            vec![in_group(accepted("a"), "g1"), in_group(accepted("b"), "g0")],
            &[],
            vec![group("g0", None), group("g1", None)],
            &[("g1", "g0")],
        );
        let a = v.get(&iid("a")).unwrap();
        assert!(v.is_group_blocked(a));
        assert!(!v.is_ready(a));
        assert_eq!(ids(&v.ready()), ["b"]);
    }

    #[test]
    fn group_dependency_applies_to_descendants() {
        let v = view_with_groups(
            vec![
                in_group(accepted("a"), "child"),
                in_group(accepted("b"), "g0"),
            ],
            &[],
            vec![
                group("g0", None),
                group("parent", None),
                group("child", Some("parent")),
            ],
            &[("parent", "g0")],
        );
        assert!(v.is_group_blocked(v.get(&iid("a")).unwrap()));
    }

    /// spec: invRejectedDoesNotPinGroup (docs/axes.md G-3)
    #[test]
    fn inv_rejected_does_not_pin_group() {
        let v = view_with_groups(
            vec![
                in_group(accepted("a"), "g1"),
                in_group(done("b"), "g0"),
                in_group(rejected("c"), "g0"),
            ],
            &[],
            vec![group("g0", None), group("g1", None)],
            &[("g1", "g0")],
        );
        assert!(v.group_satisfied(&gid("g0")));
        assert!(v.is_ready(v.get(&iid("a")).unwrap()));
    }

    #[test]
    fn undecided_child_pins_group() {
        let v = view_with_groups(
            vec![
                in_group(accepted("a"), "g1"),
                in_group(undecided("b"), "g0"),
            ],
            &[],
            vec![group("g0", None), group("g1", None)],
            &[("g1", "g0")],
        );
        assert!(!v.group_satisfied(&gid("g0")));
        assert!(v.is_group_blocked(v.get(&iid("a")).unwrap()));
    }

    /// spec の invFullProgressReleases は意図的に違反する (docs/axes.md §6)。
    #[test]
    fn full_progress_does_not_release_group_with_undecided() {
        let v = view_with_groups(
            vec![
                in_group(accepted("a"), "g1"),
                in_group(done("b"), "g0"),
                in_group(undecided("c"), "g0"),
            ],
            &[],
            vec![group("g0", None), group("g1", None)],
            &[("g1", "g0")],
        );
        let p = v.group_progress(&gid("g0"));
        assert_eq!(p.ratio(), Some(1.0), "採用分は全て終わっている");
        assert_eq!(p.undecided, 1);
        assert!(
            !v.group_satisfied(&gid("g0")),
            "未判断が残るので完了ではない"
        );
        assert!(v.is_group_blocked(v.get(&iid("a")).unwrap()));
    }

    /// spec: invGroupCompleteImpliesFullProgress — 逆方向は成り立つ。
    #[test]
    fn inv_group_complete_implies_full_progress() {
        let v = view_with_groups(
            vec![in_group(done("a"), "g0"), in_group(rejected("b"), "g0")],
            &[],
            vec![group("g0", None)],
            &[],
        );
        assert!(v.group_satisfied(&gid("g0")));
        assert_eq!(v.group_progress(&gid("g0")).ratio(), Some(1.0));
    }

    /// spec: invProgressSubset — 分子は分母の部分集合。
    #[test]
    fn inv_progress_subset() {
        let v = view_with_groups(
            vec![
                in_group(done("a"), "g0"),
                in_group(accepted("b"), "g0"),
                in_group(undecided("c"), "g0"),
                in_group(rejected("d"), "g0"),
            ],
            &[],
            vec![group("g0", None)],
            &[],
        );
        let p = v.group_progress(&gid("g0"));
        assert!(p.done <= p.total);
        assert_eq!((p.done, p.total, p.undecided), (1, 2, 1));
    }

    #[test]
    fn progress_ratio_is_none_without_accepted() {
        let v = view_with_groups(
            vec![in_group(undecided("a"), "g0")],
            &[],
            vec![group("g0", None)],
            &[],
        );
        assert_eq!(v.group_progress(&gid("g0")).ratio(), None);
    }

    /// spec: invGroupBlockedHasCause — グループ依存で止まっている原因を説明できる。
    #[test]
    fn inv_group_blocked_has_cause() {
        let v = view_with_groups(
            vec![
                in_group(accepted("a"), "g1"),
                in_group(accepted("b"), "g0"),
                in_group(done("c"), "g0"),
            ],
            &[],
            vec![group("g0", None), group("g1", None)],
            &[("g1", "g0")],
        );
        let a = v.get(&iid("a")).unwrap();
        let reason = v.group_blocking_causes(a);
        assert_eq!(reason.len(), 1);
        let (g, blockers) = &reason[0];
        assert_eq!(g.slug, "g0");
        assert_eq!(ids(blockers), ["b"], "終わっている c は原因に挙げない");
    }

    #[test]
    fn group_waiting_on_lists_unsatisfied_targets() {
        let v = view_with_groups(
            vec![in_group(accepted("b"), "g0"), in_group(done("c"), "gdone")],
            &[],
            vec![group("g0", None), group("gdone", None), group("g1", None)],
            &[("g1", "g0"), ("g1", "gdone")],
        );
        let waiting: Vec<&str> = v
            .group_waiting_on(&gid("g1"))
            .iter()
            .map(|g| g.slug.as_str())
            .collect();
        assert_eq!(waiting, ["g0"]);
    }

    #[test]
    fn issues_in_includes_descendant_groups() {
        let v = view_with_groups(
            vec![
                in_group(accepted("a"), "parent"),
                in_group(accepted("b"), "child"),
            ],
            &[],
            vec![group("parent", None), group("child", Some("parent"))],
            &[],
        );
        assert_eq!(ids(&v.issues_in(&gid("parent"))), ["a", "b"]);
        assert_eq!(ids(&v.issues_in(&gid("child"))), ["b"]);
    }

    #[test]
    fn group_traversal_stops_on_cycle() {
        let v = view_with_groups(
            vec![in_group(accepted("a"), "g1")],
            &[],
            vec![group("g1", Some("g2")), group("g2", Some("g1"))],
            &[],
        );
        assert_eq!(ids(&v.issues_in(&gid("g1"))), ["a"]);
        assert!(!v.is_group_blocked(v.get(&iid("a")).unwrap()));
    }

    #[test]
    fn issue_without_group_is_never_group_blocked() {
        let v = view_with_groups(
            vec![accepted("a"), in_group(accepted("b"), "g0")],
            &[],
            vec![group("g0", None), group("g1", None)],
            &[("g1", "g0")],
        );
        let a = v.get(&iid("a")).unwrap();
        assert!(!v.is_group_blocked(a));
        assert!(v.group_blocking_causes(a).is_empty());
    }

    #[test]
    fn group_lookup_by_id() {
        let v = view_with_groups(Vec::new(), &[], vec![group("g0", None)], &[]);
        assert_eq!(v.groups().len(), 1);
        assert_eq!(v.group(&gid("g0")).unwrap().slug, "g0");
        assert!(v.group(&gid("nope")).is_none());
    }

    // ---- 一覧 ----

    #[test]
    fn triage_lists_undecided_and_orphaned() {
        let v = view(
            vec![
                undecided("u"),
                accepted("y"),
                rejected("x"),
                accepted("ok"),
                done("d"),
            ],
            &[("y", "x")],
        );
        let got: Vec<(String, &str)> = v
            .triage()
            .into_iter()
            .map(|(i, r)| {
                let reason = match r {
                    TriageReason::Undecided => "undecided",
                    TriageReason::Orphaned => "orphaned",
                };
                (i.id.to_string(), reason)
            })
            .collect();
        assert_eq!(
            got,
            [
                ("u".to_string(), "undecided"),
                ("y".to_string(), "orphaned")
            ]
        );
    }

    #[test]
    fn triage_skips_terminal_orphans() {
        let v = view(vec![done("y"), rejected("x")], &[("y", "x")]);
        assert!(v.triage().is_empty());
    }

    #[test]
    fn newly_ready_after_lists_only_fully_unblocked() {
        let v = view(
            vec![
                done("x"),
                accepted("a"),
                accepted("b"),
                accepted("other"),
                undecided("u"),
            ],
            &[("a", "x"), ("b", "x"), ("b", "other"), ("u", "x")],
        );
        assert_eq!(ids(&v.newly_ready_after(&iid("x"))), ["a"]);
    }

    #[test]
    fn newly_ready_after_excludes_group_blocked_dependents() {
        let v = view_with_groups(
            vec![
                done("x"),
                in_group(accepted("target"), "delivery"),
                in_group(accepted("foundation-work"), "foundation"),
            ],
            &[("target", "x")],
            vec![group("foundation", None), group("delivery", None)],
            &[("delivery", "foundation")],
        );
        assert!(v.newly_ready_after(&iid("x")).is_empty());
    }

    #[test]
    fn depends_on_and_dependents_are_symmetric() {
        let v = view(vec![accepted("a"), accepted("b")], &[("a", "b")]);
        assert_eq!(ids(&v.depends_on(&iid("a"))), ["b"]);
        assert_eq!(ids(&v.dependents(&iid("b"))), ["a"]);
        assert!(v.depends_on(&iid("b")).is_empty());
    }

    #[test]
    fn stale_claims_need_both_age_and_dead_process() {
        let alive = std::process::id() as i32;
        let v = view(
            vec![
                in_progress("old_dead", claim(0, 48)),
                in_progress("old_alive", claim(alive, 48)),
                in_progress("fresh_dead", claim(0, 0)),
                accepted("not_started"),
            ],
            &[],
        );
        let stale: Vec<String> = v
            .stale_claims(24)
            .iter()
            .map(|(i, _)| i.id.to_string())
            .collect();
        assert_eq!(stale, ["old_dead"]);
    }

    #[test]
    fn iter_preserves_input_order() {
        let v = view(vec![accepted("c"), accepted("a"), accepted("b")], &[]);
        let order: Vec<String> = v.iter().map(|i| i.id.to_string()).collect();
        assert_eq!(order, ["c", "a", "b"]);
        assert!(v.get(&iid("nope")).is_none());
    }
}
