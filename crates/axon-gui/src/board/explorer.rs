//! What the window explores of the selected project: the board last read, the filter, the
//! layout, the rows they give and the Entity open in the detail pane.

use super::{Board, EntityDetail, Filter, Layout, Listing, State, filter::Exclusion, listing};
use crate::project::ProjectId;
use axon::lifecycle::EntityId;

#[derive(Default)]
pub struct Explorer {
    /// The project the board and the selection belong to.
    project: Option<ProjectId>,
    board: Option<Board>,
    filter: Filter,
    /// The filter offers the Conflicted state: the last board read for the project holds a
    /// conflicted Entity.
    offers_conflicted: bool,
    layout: Layout,
    listing: Listing,
    selected: Option<EntityId>,
    detail: Option<Result<EntityDetail, String>>,
}

impl Explorer {
    /// The project the board and the selection belong to.
    pub fn project(&self) -> Option<&ProjectId> {
        self.project.as_ref()
    }
    pub fn board(&self) -> Option<&Board> {
        self.board.as_ref()
    }
    pub fn filter(&self) -> &Filter {
        &self.filter
    }
    pub fn layout(&self) -> Layout {
        self.layout
    }
    /// The states the filter offers as choices. Conflicted is offered only while the last
    /// board read for the project holds a conflicted Entity; while it is not, the filter keeps
    /// it chosen, so a conflict that appears later is never hidden by an earlier choice.
    pub fn offered_states(&self) -> impl Iterator<Item = State> + '_ {
        State::ALL
            .into_iter()
            .filter(|state| *state != State::Conflicted || self.offers_conflicted)
    }
    pub fn listing(&self) -> &Listing {
        &self.listing
    }
    pub fn selected(&self) -> Option<&EntityId> {
        self.selected.as_ref()
    }
    /// The detail of the selected Entity, or why it could not be derived.
    pub fn detail(&self) -> Option<&Result<EntityDetail, String>> {
        self.detail.as_ref()
    }
    /// Why the filter leaves the selected Entity out; empty when it matches or nothing is
    /// selected.
    pub fn selected_exclusions(&self) -> Vec<Exclusion> {
        match (&self.board, &self.selected) {
            (Some(board), Some(id)) => board.exclusions(&self.filter, id),
            _ => Vec::new(),
        }
    }

    /// Forgets the board while `project` is read (again). The selection survives a reload of
    /// the same project and is dropped when the project changes.
    pub fn unload(&mut self, project: Option<&ProjectId>) {
        if self.project.as_ref() != project {
            self.selected = None;
            self.project = project.cloned();
            // Whether the other project had conflicts says nothing of this one.
            self.offer_conflicted(false);
        }
        self.board = None;
        self.listing = Listing::default();
        self.detail = None;
    }

    /// Shows a board read for `project`. A board of any other project is ignored.
    pub fn load(&mut self, project: &ProjectId, board: Board) {
        if self.project.as_ref() != Some(project) {
            return;
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|id| board.item(id).is_none())
        {
            self.selected = None;
        }
        self.offer_conflicted(board.has_conflicts());
        self.board = Some(board);
        self.refresh();
    }

    pub fn update_filter(&mut self, change: impl FnOnce(&mut Filter)) {
        change(&mut self.filter);
        self.choose_unoffered_states();
        self.refresh_listing();
    }
    pub fn set_layout(&mut self, layout: Layout) {
        self.layout = layout;
        self.refresh_listing();
    }

    /// Opens a known Entity in the detail pane, whether or not the filter matches it; an ID
    /// the board does not hold is ignored.
    pub fn select(&mut self, id: EntityId) {
        if self.board.as_ref().is_some_and(|b| b.item(&id).is_some()) {
            self.selected = Some(id);
            self.refresh_detail();
        }
    }
    pub fn deselect(&mut self) {
        self.selected = None;
        self.detail = None;
    }

    /// Moves the selection by `step` rows of the list, starting from the first or last row
    /// when the selection is not in the list.
    pub fn step(&mut self, step: isize) {
        let rows = &self.listing.rows;
        if rows.is_empty() {
            return;
        }
        let position = self
            .selected
            .as_ref()
            .and_then(|id| rows.iter().position(|row| &row.id == id));
        let next = match position {
            Some(ix) => ix.saturating_add_signed(step).min(rows.len() - 1),
            None if step < 0 => rows.len() - 1,
            None => 0,
        };
        let id = rows[next].id.clone();
        self.select(id);
    }

    fn offer_conflicted(&mut self, offered: bool) {
        self.offers_conflicted = offered;
        self.choose_unoffered_states();
    }
    /// Chooses every state the filter does not offer, so no hidden choice leaves anything out.
    fn choose_unoffered_states(&mut self) {
        if !self.offers_conflicted {
            self.filter.states.insert(State::Conflicted);
        }
    }

    fn refresh(&mut self) {
        self.refresh_listing();
        self.refresh_detail();
    }
    fn refresh_listing(&mut self) {
        self.listing = match &self.board {
            Some(board) => listing::listing(board, &self.filter, self.layout),
            None => Listing::default(),
        };
    }
    fn refresh_detail(&mut self) {
        self.detail = match (&self.board, &self.selected) {
            (Some(board), Some(id)) => Some(board.detail(id).map_err(|e| e.to_string())),
            _ => None,
        };
    }
}
