//! What every Branchy front end needs and `branchy-core` deliberately does not have.
//!
//! The core crate is the graph and nothing else: no filesystem, no
//! serialization, no opinion about how a user interface wants its data. That is
//! what keeps it identical on Linux, Windows and Android. This crate adds the
//! two things built on top of it that the terminal, the desktop shell and later
//! the Android app all need, so none of them grows its own copy:
//!
//! - [`store`] — reading and writing the document, atomically, with one step of
//!   undo. Phase 3 replaces its body with Automerge behind the same functions.
//! - [`snapshot`] — the whole derived picture in one value, so a user interface
//!   never recomputes status, tier or the queue for itself.

pub mod snapshot;
pub mod store;

pub use snapshot::{Outcome, Snapshot};
pub use store::StoreError;
