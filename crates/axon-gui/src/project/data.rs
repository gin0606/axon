//! The application data directory: the single-instance lock and the file listing the
//! registered management roots. Every function here blocks on the filesystem; the window calls
//! them on the background executor, except for reading and last writing the session file
//! ([`crate::session`]) as the window opens and closes.
//!
//! ```text
//! <data directory>/
//!     instance.lock   held while the application runs
//!     roots.json      the registry: the registered management roots
//!     session.json    where the last run left the window, the root, the filter and the layout
//! ```
//!
//! The session file is written by [`crate::session`]; unlike the registry, losing it costs
//! nothing, so a file that cannot be read starts from the defaults.
//!
//! `projects.json` and `projects/`, which earlier builds kept their own stores in, are neither
//! read nor removed.

use super::registry::{ChangeError, DecodeError, ProjectRoot, Registry};
use axon::file::HEADER_FILE;
use std::{
    fmt,
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

/// The environment variable that replaces the data directory, for trying the application or
/// testing it without touching the real data. It must be an absolute path.
pub const DATA_DIR_ENV: &str = "AXON_GUI_DATA_DIR";
/// The directory created under the OS data directory (`dirs::data_dir`).
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub const APP_DIRECTORY: &str = "Axon";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub const APP_DIRECTORY: &str = "axon";
pub const INSTANCE_LOCK: &str = "instance.lock";
pub const REGISTRY_FILE: &str = "roots.json";
pub const SESSION_FILE: &str = "session.json";

/// The location of the application's data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppData {
    dir: PathBuf,
}

/// Why the data directory could not be determined.
#[derive(Debug, PartialEq, Eq)]
pub enum LocateError {
    /// [`DATA_DIR_ENV`] is set to a relative path, which would depend on where the
    /// application was started.
    Relative(PathBuf),
    /// The OS reports no data directory for this user.
    NoDataDirectory,
}
impl fmt::Display for LocateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Relative(path) => write!(
                f,
                "{DATA_DIR_ENV} must be an absolute path, not {}",
                path.display()
            ),
            Self::NoDataDirectory => {
                f.write_str("the OS reports no absolute data directory for this user")
            }
        }
    }
}

/// Why the single-instance lock was not taken.
#[derive(Debug)]
pub enum InstanceError {
    /// Another process holds it: an instance is running on the same data directory.
    AlreadyRunning,
    Io(io::Error),
}
impl fmt::Display for InstanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning => f.write_str("another instance is using this data directory"),
            Self::Io(error) => write!(f, "cannot take the instance lock: {error}"),
        }
    }
}

/// Why the registry could not be read. A registry that cannot be read is never treated as
/// empty and never overwritten.
#[derive(Debug)]
pub enum RegistryError {
    Io(io::Error),
    Decode(DecodeError),
}
impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Decode(error) => write!(f, "{error}"),
        }
    }
}

/// Why registering or unregistering a root changed nothing, or may not have.
#[derive(Debug)]
pub enum UpdateError {
    /// The chosen path cannot be resolved or looked at.
    Unreachable {
        path: PathBuf,
        error: io::Error,
    },
    NotADirectory(PathBuf),
    /// The directory has no `.axon` header: the missing header's path.
    NotAStore(PathBuf),
    /// The path cannot be written to the registry file, which holds UTF-8.
    Unrepresentable(PathBuf),
    Duplicate(ProjectRoot),
    /// The root a link asked for resolves to another directory now.
    Elsewhere {
        asked: ProjectRoot,
        resolved: PathBuf,
    },
    /// The root to unregister is not registered.
    Unknown(ProjectRoot),
    /// The registry could not be read, so nothing was written.
    Read(RegistryError),
    /// Writing the registry failed. The file may already hold the change; read it to tell.
    Save(io::Error),
}
impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable { path, error } => write!(f, "{}: {error}", path.display()),
            Self::NotADirectory(path) => write!(f, "{} is not a directory", path.display()),
            Self::NotAStore(header) => write!(f, "{} does not exist", header.display()),
            Self::Unrepresentable(path) => write!(f, "{} is not valid UTF-8", path.display()),
            Self::Duplicate(root) => write!(f, "{root} is already registered"),
            Self::Elsewhere { asked, resolved } => {
                write!(f, "{asked} resolves to {}", resolved.display())
            }
            Self::Unknown(root) => write!(f, "{root} is not registered"),
            Self::Read(error) => write!(f, "cannot read {REGISTRY_FILE}: {error}"),
            Self::Save(error) => write!(f, "cannot write {REGISTRY_FILE}: {error}"),
        }
    }
}

/// Held for the lifetime of the application, and the only way to change the registry: one
/// process at a time does. The OS releases it when the process ends, even abnormally, so a
/// crash never blocks the next start.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
    data: AppData,
    /// Held across each read-modify-write of the registry, so changes from one process never
    /// overwrite one another either.
    changing: Mutex<()>,
}

impl AppData {
    /// Data in `dir`, which must be absolute so that nothing depends on the working directory.
    pub fn at(dir: impl Into<PathBuf>) -> Result<Self, LocateError> {
        let dir = dir.into();
        if dir.is_relative() {
            return Err(LocateError::Relative(dir));
        }
        Ok(Self { dir })
    }

    /// The directory [`DATA_DIR_ENV`] names when set and not empty, otherwise
    /// [`APP_DIRECTORY`] in the OS data directory.
    pub fn locate() -> Result<Self, LocateError> {
        match std::env::var_os(DATA_DIR_ENV) {
            Some(dir) if !dir.is_empty() => Self::at(dir),
            // A relative answer from the OS (a relative HOME) is as unusable as none.
            _ => match dirs::data_dir().filter(|dir| dir.is_absolute()) {
                Some(dir) => Self::at(dir.join(APP_DIRECTORY)),
                None => Err(LocateError::NoDataDirectory),
            },
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Takes the single-instance lock, creating the data directory if needed.
    pub fn lock_instance(&self) -> Result<InstanceLock, InstanceError> {
        let created = !self.dir.exists();
        fs::create_dir_all(&self.dir).map_err(InstanceError::Io)?;
        // A new data directory is made durable once, so a registry saved in it is not lost
        // with the directory's entry.
        if created && let Some(parent) = self.dir.parent() {
            sync_directory(parent).map_err(InstanceError::Io)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.dir.join(INSTANCE_LOCK))
            .map_err(InstanceError::Io)?;
        match file.try_lock() {
            Ok(()) => Ok(InstanceLock {
                _file: file,
                data: self.clone(),
                changing: Mutex::new(()),
            }),
            Err(TryLockError::WouldBlock) => Err(InstanceError::AlreadyRunning),
            Err(TryLockError::Error(error)) => Err(InstanceError::Io(error)),
        }
    }

    /// The registry. No file is the empty registry of a first start.
    pub fn load_registry(&self) -> Result<Registry, RegistryError> {
        match fs::read(self.dir.join(REGISTRY_FILE)) {
            Ok(bytes) => Registry::decode(&bytes).map_err(RegistryError::Decode),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Registry::default()),
            Err(error) => Err(RegistryError::Io(error)),
        }
    }

    /// Replaces the registry file atomically: a reader sees the old file or the new one.
    fn save_registry(&self, registry: &Registry) -> io::Result<()> {
        self.replace(REGISTRY_FILE, &registry.encode())
    }

    /// Replaces the file `name` in the data directory atomically with `bytes`: a reader sees
    /// the old file or the new one. Callers in one process never replace the same file at once.
    pub(crate) fn replace(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let path = self.dir.join(name);
        let temp = self.dir.join(format!("{name}.tmp"));
        let written = (|| {
            // Whatever is at the temporary name is removed rather than opened, which would
            // follow a symlink and truncate its target.
            match fs::remove_file(&temp) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&temp, &path)
        })();
        if let Err(error) = written {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        sync_directory(&self.dir)
    }
}

/// The management root `path` names, normalized: absolute, without symlinks, `.` or `..`. It
/// must be a directory holding an `.axon` header; nothing else about the store is checked, so
/// a store that cannot be read is registered and shows its error once selected.
pub fn management_root(path: &Path) -> Result<ProjectRoot, UpdateError> {
    let unreachable = |path: &Path| {
        let path = path.to_path_buf();
        move |error| UpdateError::Unreachable { path, error }
    };
    let resolved = fs::canonicalize(path).map_err(unreachable(path))?;
    if !fs::metadata(&resolved)
        .map_err(unreachable(&resolved))?
        .is_dir()
    {
        return Err(UpdateError::NotADirectory(resolved));
    }
    let header = resolved.join(".axon").join(HEADER_FILE);
    match fs::symlink_metadata(&header) {
        Ok(_) => {}
        // `.axon` that is not a directory holds no header either.
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return Err(UpdateError::NotAStore(header));
        }
        Err(error) => return Err(unreachable(&header)(error)),
    }
    ProjectRoot::new(resolved.clone()).map_err(|_| UpdateError::Unrepresentable(resolved))
}

impl InstanceLock {
    pub fn data(&self) -> &AppData {
        &self.data
    }

    /// Registers the management root `path` names and returns the registry now on disk with
    /// the root it registered. The registry is read again first, so the change is made to the
    /// file, not to what a window holds.
    pub fn register(&self, path: &Path) -> Result<(Registry, ProjectRoot), UpdateError> {
        self.add(management_root(path)?)
    }

    /// [`register`](Self::register) for a root the user confirmed by its path: refused unless
    /// that path still resolves to itself, so what is registered is what the user saw.
    pub fn register_exact(
        &self,
        root: &ProjectRoot,
    ) -> Result<(Registry, ProjectRoot), UpdateError> {
        if let Ok(resolved) = fs::canonicalize(root.path())
            && resolved.as_os_str() != root.path().as_os_str()
        {
            return Err(UpdateError::Elsewhere {
                asked: root.clone(),
                resolved,
            });
        }
        let resolved = management_root(root.path())?;
        if resolved.path().as_os_str() != root.path().as_os_str() {
            return Err(UpdateError::Elsewhere {
                asked: root.clone(),
                resolved: resolved.path().to_path_buf(),
            });
        }
        self.add(resolved)
    }

    fn add(&self, root: ProjectRoot) -> Result<(Registry, ProjectRoot), UpdateError> {
        let _changing = self.changing.lock().unwrap_or_else(|e| e.into_inner());
        let registry = self.data.load_registry().map_err(UpdateError::Read)?;
        let next = registry
            .with(root.clone())
            .map_err(|_: ChangeError| UpdateError::Duplicate(root.clone()))?;
        self.data.save_registry(&next).map_err(UpdateError::Save)?;
        Ok((next, root))
    }

    /// Removes `root` from the registry, leaving its files as they are, and returns the
    /// registry now on disk.
    pub fn unregister(&self, root: &ProjectRoot) -> Result<Registry, UpdateError> {
        let _changing = self.changing.lock().unwrap_or_else(|e| e.into_inner());
        let registry = self.data.load_registry().map_err(UpdateError::Read)?;
        let next = registry
            .without(root)
            .map_err(|_: ChangeError| UpdateError::Unknown(root.clone()))?;
        self.data.save_registry(&next).map_err(UpdateError::Save)?;
        Ok(next)
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}
#[cfg(not(unix))]
fn sync_directory(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
