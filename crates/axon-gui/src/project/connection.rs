//! Reading one project's store, and telling the results of earlier requests apart from the
//! current one.

use super::registry::ProjectId;
use axon::{
    Error,
    file::{HEADER_FILE, RECORDS_DIRECTORY, Store},
    lifecycle::record::{self, Header},
    location::Location,
};
use std::path::{Path, PathBuf};

/// Read access to the store of one project. Every request goes to the management root fixed
/// at construction, so a request made before switching projects still reads the project it was
/// made for. Nothing here takes the store's lock or writes to it. The functions block; run them
/// on the background executor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectConnection {
    project: ProjectId,
    root: PathBuf,
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
}

/// Identifies one request, made for `key` (usually the [`ProjectId`] it reads). A
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
}
