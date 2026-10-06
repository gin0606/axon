//! Projects: each one is a management root of its own under the application data directory.
//!
//! [`registry`] holds the list and its rules as values; [`data`] creates projects and
//! [`connection`] reads their stores. None of them depends on GPUI.

pub mod connection;
pub mod data;
pub mod registry;

pub use connection::{ProjectConnection, Requests, Ticket};
pub use data::{
    AppData, CreateError, FaultPoint, InstanceError, InstanceLock, RegistryError, Step,
};
pub use registry::{NameError, Project, ProjectId, Registry, Status};
