//! Projects: each one is a management root of its own under the application data directory.
//!
//! [`registry`] holds the list and its rules as values; [`data`] and [`connection`] are the
//! shell that reads and writes files. None of them depends on GPUI.

pub mod connection;
pub mod data;
pub mod registry;

pub use connection::{ProjectConnection, Requests, Ticket, WriteOutcome};
pub use data::{
    AppData, CreateError, FaultPoint, InstanceError, InstanceLock, RegistryError, Step,
};
pub use registry::{NameError, Project, ProjectId, Registry, Status};
