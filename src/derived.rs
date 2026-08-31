//! 導出値。ready / blocked / orphaned は保存せず、状態と依存から毎回計算する。

use crate::domain::*;
use chrono::Utc;
use std::collections::HashMap;

pub struct View {
    by_id: HashMap<IssueId, Issue>,
    order: Vec<IssueId>,
    deps: Vec<(IssueId, IssueId)>,
}

impl View {
    pub fn new(issues: Vec<Issue>, deps: Vec<(IssueId, IssueId)>) -> Self {
        let order = issues.iter().map(|i| i.id.clone()).collect();
        let by_id = issues.into_iter().map(|i| (i.id.clone(), i)).collect();
        View { by_id, order, deps }
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
