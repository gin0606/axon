//! Backend-independent implementation of `spec/lifecycle_proposal.md`.
//!
//! The binary still uses the earlier three-axis implementation while its adapters
//! are replaced. This library never discovers or opens that binary's storage.
pub mod lifecycle;
