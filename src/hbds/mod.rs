//! The HBDS store: the a.d.t. ADT'81 programs create and query, kept in
//! memory and, when given a directory, made durable by a journal.
//! docs/hbds.md tells how, and what is decided.

pub mod journal;
pub mod store;

pub use store::{AdtRef, Store};
