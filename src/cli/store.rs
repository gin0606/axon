use axon::{
    Result,
    lifecycle::{Context, EntityId, Recorder, Snapshot},
    location::{Location, Store},
};
use chrono::Utc;

pub(super) fn context() -> Context {
    Context {
        at: Utc::now(),
        recorder: axon_recorder::detect().map(|recorder| Recorder {
            actor: recorder.actor,
            data: recorder
                .data
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
        }),
    }
}
pub(super) fn resolve(snapshot: &Snapshot, value: &str) -> Result<EntityId> {
    let _: EntityId = value.to_owned().try_into()?;
    let mut matches: Vec<_> = snapshot
        .entities()
        .filter(|e| e.id.to_string().ends_with(value))
        .map(|e| e.id.clone())
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
pub(super) fn fresh_entity_id(prefix: &str, snapshot: &Snapshot) -> Result<EntityId> {
    fresh_entity_id_with(snapshot, || EntityId::generate(prefix))
}
pub(super) fn fresh_entity_id_with(
    snapshot: &Snapshot,
    mut generate: impl FnMut() -> EntityId,
) -> Result<EntityId> {
    for _ in 0..100 {
        let id = generate();
        if snapshot.entities().all(|e| e.id != id) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axon::lifecycle::{Current, Kind, Lifecycle, StoreId};
    use std::collections::BTreeSet;
    #[test]
    fn collisions_retry_without_replacing_existing_entities() {
        let mut snapshot = Snapshot::new(StoreId::generate());
        let first: EntityId = "p-abcdef".to_string().try_into().unwrap();
        snapshot
            .create(
                first.clone(),
                Kind::Issue,
                Current {
                    title: "original".into(),
                    description: String::new(),
                    lifecycle: Lifecycle::Undecided,
                    condition: None,
                    parent: None,
                    dependencies: BTreeSet::new(),
                },
                context(),
            )
            .unwrap();
        let before = snapshot.clone();
        let mut suffixes = ["abcdef", "123456"].into_iter();
        assert_eq!(
            fresh_entity_id_with(&snapshot, || format!("p-{}", suffixes.next().unwrap())
                .try_into()
                .unwrap())
            .unwrap()
            .to_string(),
            "p-123456"
        );
        assert_eq!(snapshot, before);
    }
}
