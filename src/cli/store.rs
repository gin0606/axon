use axon::{
    Result,
    file::Store,
    lifecycle::{Context, EntityId, record},
    location::Location,
    read,
};

pub(super) fn context() -> Context {
    axon::context_now()
}
/// Resolves a complete ID or a unique suffix among the Entities of the view, including one
/// that has only Notes: its Notes are readable, and the operations that need a record
/// reject it themselves.
pub(super) fn resolve(view: &record::View, value: &str) -> Result<EntityId> {
    let exact: EntityId = value.to_owned().try_into()?;
    // A complete ID always resolves to itself, even when it is also the tail of a longer ID.
    if view.is_known(&exact) || view.noted_only().contains(&exact) {
        return Ok(exact);
    }
    let mut matches: Vec<_> = view
        .known()
        .chain(view.noted_only())
        .filter(|id| id.as_ref().ends_with(value))
        .cloned()
        .collect();
    matches.sort();
    match matches.as_slice() {
        [id] => Ok(id.clone()),
        [] => Err(axon::Error::Invalid(format!("no such Entity: {value}"))),
        ids => Err(axon::Error::Invalid(format!(
            "ambiguous Entity ID {value}: {}",
            ids.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}
pub(super) fn fresh_entity_id(prefix: &str, view: &record::View) -> Result<EntityId> {
    fresh_entity_id_with(view, || record::new_entity_id(prefix).map_err(Into::into))
}
pub(super) fn fresh_entity_id_with(
    view: &record::View,
    mut generate: impl FnMut() -> Result<EntityId>,
) -> Result<EntityId> {
    for _ in 0..100 {
        let id = generate()?;
        if !view.is_known(&id) && !view.noted_only().contains(&id) {
            return Ok(id);
        }
    }
    Err(axon::Error::Invalid(
        "could not allocate a unique Entity ID after 100 attempts".into(),
    ))
}
pub(super) fn open() -> Result<(Location, Store)> {
    let location = Location::discover(&std::env::current_dir()?, false)?;
    let store = location.open()?;
    Ok((location, store))
}
/// The record set and its derived view, for reads. Conflicts, violations and gaps do not
/// stop a read; a one-line notice about them goes to stderr.
pub(super) struct Read {
    pub(super) location: Location,
    pub(super) records: record::Store,
    pub(super) view: record::View,
}
impl Read {
    pub(super) fn open() -> Result<Self> {
        let (location, store) = open()?;
        let (_, records, view) = store.read()?;
        Ok(Self {
            location,
            records,
            view,
        })
    }
    pub(super) fn read(&self) -> read::View<'_> {
        read::View::new(&self.records, &self.view)
    }
    /// The stderr notice a list or detail carries when the store is not clean.
    pub(super) fn notice(&self) -> String {
        self.notice_excluding(0)
    }
    /// The notice for a read that shows `shown` of the conflicted Entities itself: the
    /// other conflicts, the violations and the gaps.
    pub(super) fn notice_excluding(&self, shown: usize) -> String {
        let conflicted = self.view.conflicted().len().saturating_sub(shown);
        let violations = self.view.violations().len();
        let gaps = self.view.gaps().len() + self.view.noted_only().len();
        if conflicted + violations + gaps == 0 {
            return String::new();
        }
        let mut parts = Vec::new();
        if conflicted > 0 {
            parts.push(format!("{conflicted} conflicted"));
        }
        if violations > 0 {
            parts.push(format!("{violations} violations"));
        }
        if gaps > 0 {
            parts.push(format!("{gaps} with missing records"));
        }
        format!(
            "Store needs attention: {}; run axon storage check\n",
            parts.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon::lifecycle::record::{Current, Entry, Kind, Lifecycle, Store};
    use std::collections::BTreeSet;
    #[test]
    fn collisions_retry_until_an_unused_id_is_generated() {
        let mut store = Store::new();
        let first: EntityId = "p-abcdef".to_string().try_into().unwrap();
        let record = store
            .create(
                first.clone(),
                Current {
                    kind: Kind::Issue,
                    lifecycle: Lifecycle::Undecided,
                    owner: None,
                    title: "original".into(),
                    description: String::new(),
                    label: axon::lifecycle::Label::Chore,
                    condition: None,
                    parent: None,
                    needs: BTreeSet::new(),
                },
                context(),
            )
            .unwrap();
        store.insert(Entry::Record(record)).unwrap();
        let view = store.view().unwrap();
        let mut suffixes = ["abcdef", "123456"].into_iter();
        assert_eq!(
            fresh_entity_id_with(&view, || Ok(format!("p-{}", suffixes.next().unwrap())
                .try_into()
                .unwrap()))
            .unwrap()
            .to_string(),
            "p-123456"
        );
    }
}
