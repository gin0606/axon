//! Reading the store of one registered management root, and telling the results of earlier
//! requests apart from the current one.

use super::registry::ProjectRoot;
use axon::{
    Error,
    file::{HEADER_FILE, Store},
    lifecycle::record::{self, Header},
    location::Location,
};
use std::path::Path;

/// Read access to the store of one registered management root. Every request goes to the root
/// fixed at construction, so a request made before switching still reads the root it was made
/// for. Nothing here takes the store's lock or writes to it. The functions block; run them on
/// the background executor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectConnection {
    root: ProjectRoot,
}

impl ProjectConnection {
    pub fn new(root: ProjectRoot) -> Self {
        Self { root }
    }
    /// Opens the root as given, without discovery, as `axon storage check ROOT` does: inside a
    /// Git worktree an unmerged index is an error. A missing root or header is an error, never
    /// an empty store; a missing records directory is an empty store, as in the CLI, since Git
    /// does not keep an empty directory.
    fn open(&self) -> axon::Result<Store> {
        let root = self.root.path();
        // A registered store is never created by the application, so a root or header that is
        // gone is reported as missing: not as empty, and without the CLI's advice to
        // initialize.
        let missing = |what: &Path| {
            Error::Invalid(format!(
                "the registered store is missing: {} does not exist",
                what.display()
            ))
        };
        // A path through something that is no longer a directory is as missing as one that is
        // gone; any other failure to look is reported with the path.
        let present = |path: &Path| match std::fs::symlink_metadata(path) {
            Ok(_) => Ok(true),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(Error::Invalid(format!("{}: {error}", path.display()))),
        };
        if !present(root)? {
            return Err(missing(root));
        }
        if !root.is_dir() {
            return Err(Error::Invalid(format!(
                "the registered management root is not a directory: {}",
                root.display()
            )));
        }
        let location = Location::explicit(root)?;
        let header = root.join(".axon").join(HEADER_FILE);
        if !present(&header)? {
            // An unresolved merge explains a missing header better than its absence does.
            location.check_index()?;
            return Err(missing(&header));
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

/// Identifies one request, made for `key` (usually the [`ProjectRoot`] it reads). A newer
/// request for the same purpose makes it stale, so its result is dropped instead of being shown
/// for another registration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticket<K = ProjectRoot> {
    key: K,
    serial: u64,
}
impl<K> Ticket<K> {
    pub fn key(&self) -> &K {
        &self.key
    }
}

/// The latest request of one purpose, such as loading the selected registration.
#[derive(Debug)]
pub struct Requests<K = ProjectRoot> {
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
    /// How many requests have been started.
    pub fn issued(&self) -> u64 {
        self.issued
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_latest_request_is_applied_once() {
        let a = ProjectRoot::new(std::env::temp_dir().join("a")).unwrap();
        let b = ProjectRoot::new(std::env::temp_dir().join("b")).unwrap();
        let mut requests = Requests::default();
        let first = requests.begin(a.clone());
        let second = requests.begin(b.clone());
        let again = requests.begin(a.clone());
        assert_eq!(again.key(), &a);
        assert_ne!(
            first, again,
            "a new request for the same root is a new ticket"
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
