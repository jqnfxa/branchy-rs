//! What every Branchy front end needs and `branchy-core` deliberately does not have.
//!
//! The core crate is the graph and nothing else: no filesystem, no
//! serialization, no opinion about how a user interface wants its data. That is
//! what keeps it identical on Linux, Windows and Android. This crate adds the
//! five things built on top of it that the terminal, the desktop shell and later
//! the Android app all need, so none of them grows its own copy:
//!
//! - [`edit`] — applying command lines as one change, and explaining a
//!   refusal in words.
//! - [`store`] — reading and writing the document, atomically, with an undo
//!   stack twenty changes deep. Phase 3 replaces its body with Automerge behind
//!   the same functions.
//! - [`snapshot`] — the whole derived picture in one value, so a user interface
//!   never recomputes status, tier or the queue for itself.
//! - [`today`](mod@today) — the clock, which the graph engine deliberately cannot read.
//! - [`vault`] — which folder holds the graph, and the short list of recent
//!   ones that the terminal and the window share.

pub mod edit;
pub mod snapshot;
pub mod store;
pub mod today;
pub mod vault;

pub use edit::{Edit, Made, Refused, apply_lines};
pub use snapshot::{Outcome, Snapshot};
pub use store::{Dirs, StoreError};
pub use today::{Urgency, today};
pub use vault::{Vault, Vaults};
