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
        View { by_id, order, deps, groups, group_deps }
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
        match &issue.condition {
            None => true,
            Some(Condition::At(date)) => *date <= Utc::now().date_naive(),
            Some(Condition::AfterIssue(id)) => {
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
            .any(|d| d.commitment == Commitment::Rejected)
    }

    pub fn is_ready(&self, issue: &Issue) -> bool {
        matches!(issue.progress, Progress::NotStarted)
            && issue.commitment == Commitment::Accepted
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

    /// 進捗。分母は採用したものだけ (docs/axes.md §3)。
    pub fn group_progress(&self, g: &GroupId) -> GroupProgress {
        let issues = self.issues_in(g);
        let accepted: Vec<_> = issues
            .iter()
            .filter(|i| i.commitment == Commitment::Accepted)
            .collect();
        GroupProgress {
            done: accepted
                .iter()
                .filter(|i| matches!(i.progress, Progress::Ended))
                .count(),
            total: accepted.len(),
            undecided: issues
                .iter()
                .filter(|i| i.commitment == Commitment::Undecided)
                .count(),
        }
    }

    pub fn ready(&self) -> Vec<&Issue> {
        self.iter().filter(|i| self.is_ready(i)).collect()
    }

    /// `id` が終端に達したことで新たに着手可能になるもの。`done` の後押しに使う。
    pub fn newly_ready_after(&self, id: &IssueId) -> Vec<&Issue> {
        self.dependents(id)
            .into_iter()
            .filter(|i| {
                matches!(i.progress, Progress::NotStarted)
                    && i.commitment == Commitment::Accepted
                    && self.is_surfaced(i)
                    && !self.is_orphaned(&i.id)
                    // 対象の issue 以外に未終端の依存が残っていないこと
                    && self
                        .depends_on(&i.id)
                        .iter()
                        .all(|d| &d.id == id || d.is_terminal())
            })
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
