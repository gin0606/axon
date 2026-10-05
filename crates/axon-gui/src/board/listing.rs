//! The rows of the shared list: the matching Entities as a containment tree or a flat list.

use super::{Board, Filter};
use axon::lifecycle::EntityId;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// Matching Entities under their parents. An ancestor that does not match is shown for
    /// reference and is not counted.
    #[default]
    Tree,
    /// Matching Entities only, in creation order.
    Flat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRow {
    pub id: EntityId,
    /// Nesting below the top of the tree; always 0 in a flat list.
    pub depth: usize,
    /// False for an ancestor shown only for reference.
    pub matched: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    pub rows: Vec<ListRow>,
    /// The Entities that match the filter.
    pub matched: usize,
    /// The ancestors shown for reference.
    pub context: usize,
}

pub fn listing(board: &Board, filter: &Filter, layout: Layout) -> Listing {
    let matched: HashSet<&EntityId> = board
        .items()
        .iter()
        .filter(|item| board.matches(filter, &item.id))
        .map(|item| &item.id)
        .collect();
    let rows: Vec<ListRow> = match layout {
        Layout::Flat => board
            .items()
            .iter()
            .filter(|item| matched.contains(&item.id))
            .map(|item| ListRow {
                id: item.id.clone(),
                depth: 0,
                matched: true,
            })
            .collect(),
        Layout::Tree => {
            let order = board.tree();
            // Walking backwards, `below[d]` says whether a kept Entity at depth `d` has been
            // seen since the last Entity at a shallower depth: under the next one up.
            let mut keep = vec![false; order.len()];
            let mut below: Vec<bool> = Vec::new();
            for (ix, (id, depth)) in order.iter().enumerate().rev() {
                keep[ix] = matched.contains(id) || below.get(depth + 1).copied().unwrap_or(false);
                below.truncate(depth + 1);
                below.resize(depth + 1, false);
                below[*depth] |= keep[ix];
            }
            order
                .iter()
                .zip(keep)
                .filter(|(_, keep)| *keep)
                .map(|((id, depth), _)| ListRow {
                    matched: matched.contains(id),
                    id: id.clone(),
                    depth: *depth,
                })
                .collect()
        }
    };
    let context = rows.iter().filter(|row| !row.matched).count();
    Listing {
        matched: matched.len(),
        context,
        rows,
    }
}

/// Every known Entity in tree order with its depth. Siblings keep creation order. An Entity
/// whose parent the store does not hold starts a tree of its own, and so does the earliest
/// member of a containment cycle, so every Entity appears once.
pub(super) fn tree(board: &Board) -> Vec<(EntityId, usize)> {
    let mut children: HashMap<&EntityId, Vec<&EntityId>> = HashMap::new();
    let mut roots = Vec::new();
    for item in board.items() {
        match &item.parent {
            Some(parent) if board.item(parent).is_some() => {
                children.entry(parent).or_default().push(&item.id)
            }
            _ => roots.push(&item.id),
        }
    }
    let mut order = Vec::new();
    let mut seen: HashSet<EntityId> = HashSet::new();
    let walk = |start: &EntityId, seen: &mut HashSet<EntityId>, order: &mut Vec<_>| {
        let mut stack = vec![(start, 0usize)];
        while let Some((id, depth)) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            order.push((id.clone(), depth));
            if let Some(below) = children.get(id) {
                stack.extend(below.iter().rev().map(|child| (*child, depth + 1)));
            }
        }
    };
    for root in roots {
        walk(root, &mut seen, &mut order);
    }
    // Whatever is left hangs below a containment cycle. Each such tree starts at the earliest
    // member of its cycle, so the Entities below the cycle stay under it.
    for item in board.items() {
        if !seen.contains(&item.id) {
            walk(cycle_start(board, &item.id), &mut seen, &mut order);
        }
    }
    order
}

/// The earliest-created member of the containment cycle above `id`, which has a known parent
/// at every step up.
fn cycle_start<'a>(board: &'a Board, id: &'a EntityId) -> &'a EntityId {
    let parent = |id: &EntityId| {
        board
            .item(id)
            .and_then(|item| item.parent.as_ref())
            .and_then(|parent| board.item(parent))
            .map(|item| &item.id)
    };
    let mut path: Vec<&EntityId> = vec![id];
    let mut current = id;
    while let Some(next) = parent(current) {
        if let Some(at) = path.iter().position(|seen| *seen == next) {
            let index = |id: &EntityId| board.index.get(id).copied().unwrap_or(usize::MAX);
            return path[at..]
                .iter()
                .copied()
                .min_by_key(|member| index(member))
                .expect("a cycle has members");
        }
        path.push(next);
        current = next;
    }
    // Not below a cycle after all: it starts its own tree.
    id
}
