//! The shared filter of the list: states, kinds and labels chosen by checkboxes, and a literal
//! search in the title and the description.

use super::{Item, State};
use axon::lifecycle::{Kind, Label, record::Current};
use axon::read;
use std::collections::BTreeSet;

/// Choices within one facet are ORed and the facets are ANDed, so a facet with nothing chosen
/// matches nothing. An empty query matches everything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    pub states: BTreeSet<State>,
    pub kinds: BTreeSet<Kind>,
    pub labels: BTreeSet<Label>,
    /// Literal, case-sensitive text in the title or the description, as `axon list --search`.
    pub query: String,
}

impl Default for Filter {
    /// The unfinished work and the conflicts, of both kinds and every label.
    fn default() -> Self {
        Self {
            states: BTreeSet::from([
                State::Undecided,
                State::NotStarted,
                State::InProgress,
                State::Conflicted,
            ]),
            kinds: BTreeSet::from([Kind::Issue, Kind::Group]),
            labels: Label::ALL.into_iter().collect(),
            query: String::new(),
        }
    }
}

/// Why the filter leaves an Entity out: its value in a facet with that value not chosen, or a
/// search its title and description do not contain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exclusion {
    State(State),
    Kind(Kind),
    Label(Label),
    Query(String),
}

impl Filter {
    /// Whether an Entity with this row and presented value matches. The presented value is the
    /// current one, or the first head's while conflicted.
    pub fn matches(&self, item: &Item, presented: &Current) -> bool {
        self.states.contains(&item.state)
            && self.kinds.contains(&item.kind)
            && self.labels.contains(&item.label)
            && (self.query.is_empty() || !read::matches_in(presented, &self.query).is_empty())
    }

    /// Each facet that leaves the Entity out, in the order the filter shows them; empty
    /// exactly when [`Self::matches`] holds.
    pub fn exclusions(&self, item: &Item, presented: &Current) -> Vec<Exclusion> {
        let mut found = Vec::new();
        if !self.states.contains(&item.state) {
            found.push(Exclusion::State(item.state));
        }
        if !self.kinds.contains(&item.kind) {
            found.push(Exclusion::Kind(item.kind));
        }
        if !self.labels.contains(&item.label) {
            found.push(Exclusion::Label(item.label));
        }
        if !self.query.is_empty() && read::matches_in(presented, &self.query).is_empty() {
            found.push(Exclusion::Query(self.query.clone()));
        }
        found
    }

    pub fn toggle_state(&mut self, state: State, on: bool) {
        toggle(&mut self.states, state, on);
    }
    pub fn toggle_kind(&mut self, kind: Kind, on: bool) {
        toggle(&mut self.kinds, kind, on);
    }
    pub fn toggle_label(&mut self, label: Label, on: bool) {
        toggle(&mut self.labels, label, on);
    }
}

fn toggle<T: Ord>(set: &mut BTreeSet<T>, value: T, on: bool) {
    if on {
        set.insert(value);
    } else {
        set.remove(&value);
    }
}
