//! The application data directory: the single-instance lock, the registry file and one
//! management root per project. Every function here blocks on the filesystem; the window
//! calls them on the background executor.
//!
//! ```text
//! <data directory>/
//!     instance.lock          held while the application runs
//!     projects.json          the registry: IDs, names and creation status
//!     projects/<id>/.axon/   each project's store, in the format the CLI uses
//! ```

use super::{
    connection::ProjectConnection,
    registry::{ChangeError, DecodeError, NameError, Project, ProjectId, Registry, Status},
};
use axon::location::{Location, Presence, presence};
use std::{
    fmt,
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
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
pub const REGISTRY_FILE: &str = "projects.json";
pub const PROJECTS_DIRECTORY: &str = "projects";
/// The ID prefix of every project's store: Entity IDs read `axon-…` as in the CLI's default
/// examples, whatever the project is called.
pub const STORE_PREFIX: &str = "axon";

/// The location of the application's data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppData {
    dir: PathBuf,
    fault: Fault,
}

/// Where a test makes creating or finishing a project fail.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultPoint {
    /// Before the step does anything.
    Before(Step),
    /// After the step replaced the registry file, before the replacement is made durable.
    Replaced(Step),
}

/// A failure injected into creation by a test of the window; the application never sets one.
#[derive(Clone, Default)]
struct Fault(Option<Arc<dyn Fn(FaultPoint) -> io::Result<()> + Send + Sync>>);
impl Fault {
    fn inject(&self, point: FaultPoint) -> io::Result<()> {
        match &self.0 {
            Some(fault) => fault(point),
            None => Ok(()),
        }
    }
}
impl fmt::Debug for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_some() {
            "Fault(set)"
        } else {
            "Fault(none)"
        })
    }
}
/// Two locations of the same directory are equal whatever their test hooks.
impl PartialEq for Fault {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for Fault {}

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

/// Held for the lifetime of the application. The OS releases it when the process ends, even
/// abnormally, so a crash never blocks the next start.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

/// Why the registry could not be read. A registry that cannot be read is never treated as
/// empty and never overwritten.
#[derive(Debug)]
pub enum RegistryError {
    Io(io::Error),
    Decode(DecodeError),
    /// There is no registry file, but `projects/` holds a store: the list was lost, and an
    /// empty one would hide those projects and be written over them.
    Missing {
        store: PathBuf,
    },
}
impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Decode(error) => write!(f, "{error}"),
            Self::Missing { store } => write!(
                f,
                "{REGISTRY_FILE} is missing although a project store exists at {}",
                store.display()
            ),
        }
    }
}

/// The step of creating a project that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Reading the registry, or finding the project in it. Nothing was written.
    Read,
    /// Making the project's directory. Nothing is registered.
    Directory,
    /// Adding the project to the registry in [`Status::Creating`]. When the replacement of the
    /// file failed, nothing is registered; when only making it durable failed, the project is
    /// registered and its directory kept. Read the registry to tell which.
    Register,
    /// Initializing the store. The project is registered and finishing it can be retried.
    Initialize,
    /// Marking the project ready. The store exists; finishing it can be retried, and the
    /// registry may already say ready.
    Finish,
}

/// Why creating or finishing a project failed.
#[derive(Debug)]
pub enum CreateError {
    /// The name was refused; nothing was written.
    Name(NameError),
    /// `project` is the project the failure concerns once it may be registered: from a
    /// replaced registration on, and for every failure of finishing.
    Failed {
        step: Step,
        reason: String,
        project: Option<ProjectId>,
    },
}
impl CreateError {
    pub fn project(&self) -> Option<&ProjectId> {
        match self {
            Self::Name(_) => None,
            Self::Failed { project, .. } => project.as_ref(),
        }
    }
    fn concerning(self, id: &ProjectId) -> Self {
        match self {
            Self::Failed { step, reason, .. } => Self::Failed {
                step,
                reason,
                project: Some(id.clone()),
            },
            other => other,
        }
    }
}
impl fmt::Display for CreateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(error) => write!(f, "invalid name: {error:?}"),
            Self::Failed { step, reason, .. } => write!(f, "{step:?} failed: {reason}"),
        }
    }
}

/// A failed replacement of the registry file, and whether the new file was already in place.
struct SaveError {
    replaced: bool,
    error: io::Error,
}

/// Attempts to find an unused directory name before giving up.
const ID_ATTEMPTS: usize = 16;

fn failed(step: Step) -> impl Fn(io::Error) -> CreateError {
    move |error| CreateError::Failed {
        step,
        reason: error.to_string(),
        project: None,
    }
}

impl AppData {
    /// Data in `dir`, which must be absolute so that nothing depends on the working directory.
    pub fn at(dir: impl Into<PathBuf>) -> Result<Self, LocateError> {
        let dir = dir.into();
        if dir.is_relative() {
            return Err(LocateError::Relative(dir));
        }
        Ok(Self {
            dir,
            fault: Fault::default(),
        })
    }

    /// The same data with `fault` called at each [`FaultPoint`] of creating or finishing a
    /// project; an error it returns fails the step there.
    /// For tests only.
    #[doc(hidden)]
    pub fn with_fault(
        mut self,
        fault: impl Fn(FaultPoint) -> io::Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.fault = Fault(Some(Arc::new(fault)));
        self
    }

    fn inject(&self, point: FaultPoint) -> io::Result<()> {
        self.fault.inject(point)
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

    fn projects_dir(&self) -> PathBuf {
        self.dir.join(PROJECTS_DIRECTORY)
    }

    /// The management root of a project: the directory holding its `.axon`.
    pub fn project_root(&self, id: &ProjectId) -> PathBuf {
        self.projects_dir().join(id.as_str())
    }

    pub fn connect(&self, project: &Project) -> ProjectConnection {
        ProjectConnection::new(project.id.clone(), self.project_root(&project.id))
    }

    /// Takes the single-instance lock, creating the data directory if needed.
    pub fn lock_instance(&self) -> Result<InstanceLock, InstanceError> {
        fs::create_dir_all(&self.dir).map_err(InstanceError::Io)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.dir.join(INSTANCE_LOCK))
            .map_err(InstanceError::Io)?;
        match file.try_lock() {
            Ok(()) => Ok(InstanceLock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(InstanceError::AlreadyRunning),
            Err(TryLockError::Error(error)) => Err(InstanceError::Io(error)),
        }
    }

    /// The registry. No file and no store under `projects/` is the empty registry of a first
    /// start; directories without a store are what failed creations leave and do not count.
    pub fn load_registry(&self) -> Result<Registry, RegistryError> {
        match fs::read(self.dir.join(REGISTRY_FILE)) {
            Ok(bytes) => Registry::decode(&bytes).map_err(RegistryError::Decode),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let entries = match fs::read_dir(self.projects_dir()) {
                    Ok(entries) => entries,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        return Ok(Registry::default());
                    }
                    Err(error) => return Err(RegistryError::Io(error)),
                };
                for entry in entries {
                    let store = entry.map_err(RegistryError::Io)?.path().join(".axon");
                    // Only a store known to be absent is absent: one that cannot be looked at
                    // may hold a project.
                    match fs::symlink_metadata(&store) {
                        Ok(_) => return Err(RegistryError::Missing { store }),
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                            ) => {}
                        Err(error) => return Err(RegistryError::Io(error)),
                    }
                }
                Ok(Registry::default())
            }
            Err(error) => Err(RegistryError::Io(error)),
        }
    }

    /// Creates a project named `name` and returns the registry that now includes it as
    /// [`Status::Ready`]. The registry is read again first, so the change is made to what is
    /// on disk; callers hold the instance lock and run one change at a time.
    pub fn create_project(&self, name: &str) -> Result<Registry, CreateError> {
        self.create_project_with(name, &mut rand::random, &mut |point| self.inject(point))
    }

    /// Finishes a project left in [`Status::Creating`]: initializes its store unless one is
    /// already there, makes it durable, then marks the project ready. An existing store is
    /// kept as it is.
    pub fn finish_creation(&self, id: &ProjectId) -> Result<Registry, CreateError> {
        self.finish_creation_with(id, &mut |point| self.inject(point))
    }

    pub(crate) fn create_project_with(
        &self,
        name: &str,
        bits: &mut dyn FnMut() -> u64,
        fault: &mut dyn FnMut(FaultPoint) -> io::Result<()>,
    ) -> Result<Registry, CreateError> {
        let registry = self.read_for_change()?;
        registry.check_name(name).map_err(CreateError::Name)?;
        let projects = self.projects_dir();
        let id = (|| {
            fault(FaultPoint::Before(Step::Directory))?;
            fs::create_dir_all(&projects)?;
            for _ in 0..ID_ATTEMPTS {
                let id = ProjectId::from_bits(bits());
                if registry.get(&id).is_some() {
                    continue;
                }
                // A directory left by an earlier failed creation is never reused: whatever it
                // holds stays where it is.
                match fs::create_dir(self.project_root(&id)) {
                    Ok(()) => {
                        sync_directory(&projects)?;
                        return Ok(id);
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            }
            Err(io::Error::other(
                "no unused project directory name was found",
            ))
        })()
        .map_err(failed(Step::Directory))?;
        let pending = registry
            .with_creating(id.clone(), name)
            .map_err(|error| match error {
                ChangeError::Name(error) => CreateError::Name(error),
                other => CreateError::Failed {
                    step: Step::Register,
                    reason: format!("{other:?}"),
                    project: None,
                },
            })?;
        let saved = fault(FaultPoint::Before(Step::Register))
            .map_err(|error| SaveError {
                replaced: false,
                error,
            })
            .and_then(|()| self.save_registry(&pending, Step::Register, fault));
        if let Err(SaveError { replaced, error }) = saved {
            // Before the replacement the empty directory belongs to no project, and a failure
            // to remove it only leaves an unused directory. After it, the project is registered
            // and its directory stays.
            if !replaced {
                let _ = fs::remove_dir(self.project_root(&id));
                return Err(failed(Step::Register)(error));
            }
            return Err(failed(Step::Register)(error).concerning(&id));
        }
        self.finish_in(&pending, &id, fault)
            .map_err(|error| error.concerning(&id))
    }

    pub(crate) fn finish_creation_with(
        &self,
        id: &ProjectId,
        fault: &mut dyn FnMut(FaultPoint) -> io::Result<()>,
    ) -> Result<Registry, CreateError> {
        let registry = self.read_for_change()?;
        self.finish_in(&registry, id, fault)
            .map_err(|error| error.concerning(id))
    }

    fn read_for_change(&self) -> Result<Registry, CreateError> {
        self.load_registry().map_err(|error| CreateError::Failed {
            step: Step::Read,
            reason: error.to_string(),
            project: None,
        })
    }

    fn finish_in(
        &self,
        registry: &Registry,
        id: &ProjectId,
        fault: &mut dyn FnMut(FaultPoint) -> io::Result<()>,
    ) -> Result<Registry, CreateError> {
        let project = registry.get(id).ok_or_else(|| CreateError::Failed {
            step: Step::Read,
            reason: format!("project {id} is not registered"),
            project: None,
        })?;
        if project.status == Status::Ready {
            return Ok(registry.clone());
        }
        let root = self.project_root(id);
        let mut initialize = || -> Result<(), String> {
            let text = |error: io::Error| error.to_string();
            fault(FaultPoint::Before(Step::Initialize)).map_err(text)?;
            // The registration is durable before a store exists that only it names: a store
            // without its entry would make the registry look lost.
            sync_directory(&self.dir).map_err(text)?;
            // The directory is missing when the registration was replaced but not made durable
            // and the creation removed nothing, or when it was lost since: nothing to keep.
            fs::create_dir_all(&root).map_err(text)?;
            let location = Location::standalone(&root).map_err(|error| error.to_string())?;
            // An existing store is the result of an earlier attempt and is kept; `init` refuses
            // anything else that is not the residue of an interrupted initialization.
            if !matches!(presence(&root), Ok(Presence::Store)) {
                location
                    .init(STORE_PREFIX)
                    .map_err(|error| error.to_string())?;
            }
            // `init` reports a store whose last sync failed as failed; a retry finds it present
            // and makes it durable here before the registry calls it ready.
            // Every level down from the directory holding the data directory, whose entry a
            // first start created.
            let mut directories = vec![root.join(".axon"), root.clone(), self.projects_dir()];
            directories.extend(self.dir.parent().map(Path::to_path_buf));
            for directory in directories {
                sync_directory(&directory).map_err(text)?;
            }
            Ok(())
        };
        initialize().map_err(|reason| CreateError::Failed {
            step: Step::Initialize,
            reason,
            project: None,
        })?;
        let ready = registry
            .with_ready(id)
            .map_err(|error| CreateError::Failed {
                step: Step::Finish,
                reason: format!("{error:?}"),
                project: None,
            })?;
        fault(FaultPoint::Before(Step::Finish))
            .map_err(|error| SaveError {
                replaced: false,
                error,
            })
            .and_then(|()| self.save_registry(&ready, Step::Finish, fault))
            .map_err(|SaveError { error, .. }| failed(Step::Finish)(error))?;
        Ok(ready)
    }

    /// Replaces the registry file atomically: a reader sees the old file or the new one.
    fn save_registry(
        &self,
        registry: &Registry,
        step: Step,
        fault: &mut dyn FnMut(FaultPoint) -> io::Result<()>,
    ) -> Result<(), SaveError> {
        let path = self.dir.join(REGISTRY_FILE);
        let temp = self.dir.join(format!("{REGISTRY_FILE}.tmp"));
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
            file.write_all(&registry.encode())?;
            file.sync_all()?;
            fs::rename(&temp, &path)
        })();
        if let Err(error) = written {
            let _ = fs::remove_file(&temp);
            return Err(SaveError {
                replaced: false,
                error,
            });
        }
        fault(FaultPoint::Replaced(step))
            .and_then(|()| sync_directory(&self.dir))
            .map_err(|error| SaveError {
                replaced: true,
                error,
            })
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
