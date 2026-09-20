//! Core engine for Branchy: a dependency graph of tasks with derived status and priority ordering.
//!
//! This crate has no UI or platform dependencies, so it can be tested on its own and reused by
//! the desktop shell and, later, the mobile one.
//!
//! # What is stored, and what is not
//!
//! A [`Graph`] holds nodes, areas, and which node needs which. Everything else
//! is computed from those on demand:
//!
//! - [`Graph::status`] — locked, available, done, or cyclic
//! - [`Graph::tier`] — how far along the chain a node sits
//! - [`Graph::queue`] — every available node, highest priority first
//! - [`Graph::path_to_unlock`] — what stands between you and a locked node
//!
//! Nothing derived is ever written down, so nothing derived can disagree with
//! the edges.
//!
//! # Cycles
//!
//! The graph must stay acyclic, because a cycle makes both status and tier
//! undefined: nothing inside one can ever become available, and the tier
//! recurrence does not terminate. [`Graph::add_prerequisite`] refuses any edge
//! that would close a loop and hands back the loop it found.
//!
//! Refusing at insert time is not sufficient once devices sync. Two devices,
//! each offline, can add one edge apiece that is legal alone and cyclic
//! together, and a CRDT merges both without complaint. So acyclicity is
//! repairable here, not guaranteed: every derived computation terminates on a
//! cyclic graph, and [`Graph::find_cycles`] reports what has to be broken.
//!
//! # Example
//!
//! ```
//! use branchy_core::{Graph, NewArea, NewNode, Status};
//!
//! let mut graph = Graph::new();
//! let maths = graph.add_area(NewArea::new("Hard skills", "#4fd1c5"));
//!
//! let algebra = graph.add_node(NewNode::new("School algebra", maths))?;
//! let calculus = graph.add_node(NewNode::new("Calculus", maths).with_priority(7))?;
//! graph.add_prerequisite(calculus, algebra)?;
//!
//! assert_eq!(graph.status(calculus), Some(Status::Locked));
//! graph.set_done(algebra, true)?;
//! assert_eq!(graph.status(calculus), Some(Status::Available));
//! assert_eq!(graph.queue(), vec![calculus]);
//!
//! // the reverse edge would close a loop, so it is refused
//! assert!(graph.add_prerequisite(algebra, calculus).is_err());
//! # Ok::<(), branchy_core::Error>(())
//! ```

mod error;
mod graph;
mod id;
mod node;

pub use error::Error;
pub use graph::Graph;
pub use id::{AreaId, NodeId};
pub use node::{Area, NewArea, NewNode, Node, Status};
