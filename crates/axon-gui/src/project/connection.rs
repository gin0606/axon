//! Reading and writing one project's store, and telling the results of earlier requests apart
//! from the current one.

use super::registry::ProjectId;
use axon::{
    Error,
    file::{HEADER_FILE, RECORDS_DIRECTORY, Store},
    lifecycle::record::{self, Entry, Header},
    location::Location,
};
use std::path::{Path, PathBuf};

/// Access to the store of one project. Every request goes to the management root fixed at
/// construction, so a request made before switching projects still reads or writes the project
/// it was made for. The functions block; run them on the background executor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectConnection {
    project: ProjectId,
    root: PathBuf,
}

/// The result of a write, as the store reports it.
#[derive(Debug)]
pub enum WriteOutcome<T> {
    /// Every record was published.
    Applied(T),
    /// Nothing was written: the change was refused, the store could not be read or locked, or
    /// publication failed before the first record. The same input can be submitted again.
    NotApplied(Error),
    /// Publication stopped after some records were written. Read the store again and compare
    /// before deciding to resubmit; never resubmit automatically.
    PublicationUnknown(Error),
}
impl<T> From<axon::Result<T>> for WriteOutcome<T> {
    fn from(result: axon::Result<T>) -> Self {
        match result {
            Ok(value) => Self::Applied(value),
            Err(error @ Error::PublicationUnknown(_)) => Self::PublicationUnknown(error),
            Err(error) => Self::NotApplied(error),
        }
    }
}

impl ProjectConnection {
    pub fn new(project: ProjectId, root: PathBuf) -> Self {
        Self { project, root }
    }
    pub fn project(&self) -> &ProjectId {
        &self.project
    }
    /// The management root: the directory holding `.axon`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A missing or uninitialized store is an error, never an empty one. The root is the
    /// application's own, so Git is not consulted.
    fn open(&self) -> axon::Result<Store> {
        // The store of a project is created once and never again in its place, so any part of
        // it that is gone is reported as missing: not as empty, and without the CLI's advice
        // to initialize.
        let missing = |what: &Path| {
            Error::Invalid(format!(
                "the store of this project is missing: {} does not exist; it is not created again in its place",
                what.display()
            ))
        };
        let present = |path: &Path| match std::fs::symlink_metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(Error::Io(error)),
        };
        if !present(&self.root)? {
            return Err(missing(&self.root));
        }
        let location = Location::standalone(&self.root)?;
        let directory = self.root.join(".axon");
        for path in [
            directory.join(HEADER_FILE),
            directory.join(RECORDS_DIRECTORY),
        ] {
            if !present(&path)? {
                return Err(missing(&path));
            }
        }
        location.open()
    }

    /// Reads the intact store and derives a value from it. Conflicts, violations and gaps do
    /// not fail a read; corruption does.
    pub fn read<T>(
        &self,
        derive: impl FnOnce(&Header, &record::Store, &record::View) -> T,
    ) -> axon::Result<T> {
        let (header, records, view) = self.load()?;
        Ok(derive(&header, &records, &view))
    }

    /// Reads the intact store and keeps it, for a caller that derives from it later.
    pub fn load(&self) -> axon::Result<(Header, record::Store, record::View)> {
        self.open()?.read()
    }

    /// Runs `change` under the store's write lock against its current records and publishes
    /// the records it returns.
    pub fn update<T>(
        &self,
        change: impl FnOnce(&Header, &record::Store, &record::View) -> axon::Result<(Vec<Entry>, T)>,
    ) -> WriteOutcome<T> {
        self.open()
            .and_then(|mut store| store.update(change))
            .into()
    }
}

/// Identifies one request, made for `key` (usually the [`ProjectId`] it reads or writes). A
/// newer request for the same purpose makes it stale, so its result is dropped instead of being
/// shown for another project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticket<K = ProjectId> {
    key: K,
    serial: u64,
}
impl<K> Ticket<K> {
    pub fn key(&self) -> &K {
        &self.key
    }
}

/// The latest request of one purpose, such as loading the selected project.
#[derive(Debug)]
pub struct Requests<K = ProjectId> {
    issued: u64,
    current: Option<Ticket<K>>,
}
impl<K> Default for Requests<K> {
    fn default() -> Self {
        Self {
            issued: 0,
            current: None,
        }
    }
}
impl<K: Clone + PartialEq> Requests<K> {
    /// Starts a request for `key`; any earlier one becomes stale.
    pub fn begin(&mut self, key: K) -> Ticket<K> {
        self.issued += 1;
        let ticket = Ticket {
            key,
            serial: self.issued,
        };
        self.current = Some(ticket.clone());
        ticket
    }
    /// Makes every outstanding request stale.
    pub fn cancel(&mut self) {
        self.current = None;
    }
    /// Whether the result of `ticket` is the one to apply; a current ticket is consumed.
    pub fn finish(&mut self, ticket: &Ticket<K>) -> bool {
        if self.current.as_ref() == Some(ticket) {
            self.current = None;
            true
        } else {
            false
        }
    }
    pub fn pending(&self) -> Option<&Ticket<K>> {
        self.current.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_latest_request_is_applied_once() {
        let a = ProjectId::from_bits(1);
        let b = ProjectId::from_bits(2);
        let mut requests = Requests::default();
        let first = requests.begin(a.clone());
        let second = requests.begin(b.clone());
        let again = requests.begin(a.clone());
        assert_eq!(again.key(), &a);
        assert_ne!(
            first, again,
            "a new request for the same project is a new ticket"
        );
        assert!(!requests.finish(&first));
        assert!(!requests.finish(&second));
        assert_eq!(requests.pending(), Some(&again));
        assert!(requests.finish(&again));
        assert!(!requests.finish(&again));
        let cancelled = requests.begin(b);
        requests.cancel();
        assert!(!requests.finish(&cancelled));
    }

    #[test]
    fn write_results_are_classified_by_whether_anything_was_published() {
        assert!(matches!(
            WriteOutcome::from(Ok::<_, Error>(1)),
            WriteOutcome::Applied(1)
        ));
        assert!(matches!(
            WriteOutcome::<()>::from(Err(Error::PublicationUnknown("renamed".into()))),
            WriteOutcome::PublicationUnknown(_)
        ));
        assert!(matches!(
            WriteOutcome::<()>::from(Err(Error::Invalid("not applied".into()))),
            WriteOutcome::NotApplied(_)
        ));
        assert!(matches!(
            WriteOutcome::<()>::from(Err(Error::Io(std::io::Error::other("lock")))),
            WriteOutcome::NotApplied(_)
        ));
    }
}
