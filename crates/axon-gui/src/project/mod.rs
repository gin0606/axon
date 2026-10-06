//! Projects: the management roots of existing stores that the user registered.
//!
//! [`registry`] holds the list and its rules as values; [`data`] keeps the list in the
//! application data directory and [`connection`] reads the registered stores. None of them
//! depends on GPUI.

pub mod connection;
pub mod data;
pub mod registry;

pub use connection::{ProjectConnection, Requests, Ticket};
pub use data::{AppData, InstanceError, InstanceLock, RegistryError, UpdateError};
pub use registry::{ProjectRoot, Registry};
