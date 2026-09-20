//! The things a graph holds: areas, nodes, and the derived status of a node.

use std::collections::BTreeSet;

use crate::date::Date;
use crate::id::{AreaId, NodeId};

/// A user-defined grouping with its own accent colour: hard skills, health,
/// a work project, anything. Prerequisites may cross areas freely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Area {
    /// Display name. User data, never translated.
    pub name: String,
    /// Accent colour, as the UI wants to store it (for example `#4fd1c5`).
    pub color: String,
}

/// The fields a caller supplies when creating an area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewArea {
    /// Display name.
    pub name: String,
    /// Accent colour.
    pub color: String,
}

impl NewArea {
    /// A new area with the given name and colour.
    #[must_use]
    pub fn new(name: impl Into<String>, color: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            color: color.into(),
        }
    }
}

/// One task.
///
/// A node does not carry its own id: the key it is stored under *is* its
/// identity, so the two cannot drift apart. Nor does it store its status or
/// tier, both of which are derived from the graph on every read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// Display name. User data, never translated.
    pub name: String,
    /// Longer description. May be empty.
    pub note: String,
    /// The area this node belongs to.
    pub area: AreaId,
    /// Higher sorts earlier in the queue.
    pub priority: u8,
    /// Whether the task is finished. The only stored part of a node's status.
    pub done: bool,
    /// When this has to be finished by, if anything says so.
    ///
    /// A deadline does not change a node's status: a task is not blocked by
    /// having a date on it. What it changes is urgency, and it reaches
    /// backwards — see [`Graph::effective_due`](crate::Graph::effective_due).
    pub due: Option<Date>,
    /// Everything that must be done before this becomes available.
    ///
    /// A set rather than a list: duplicate edges are impossible by
    /// construction, and set union is what merges correctly when two devices
    /// each add a different prerequisite offline.
    pub prereqs: BTreeSet<NodeId>,
}

/// The fields a caller supplies when creating a node.
///
/// The graph assigns the id, so a caller cannot build a complete [`Node`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNode {
    /// Display name.
    pub name: String,
    /// Longer description. May be empty.
    pub note: String,
    /// The area this node belongs to.
    pub area: AreaId,
    /// Higher sorts earlier in the queue.
    pub priority: u8,
    /// When it has to be finished by.
    pub due: Option<Date>,
}

impl NewNode {
    /// A new node in `area`, with an empty note and middling priority.
    #[must_use]
    pub fn new(name: impl Into<String>, area: AreaId) -> Self {
        Self {
            name: name.into(),
            note: String::new(),
            area,
            priority: 5,
            due: None,
        }
    }

    /// Sets the deadline.
    #[must_use]
    pub const fn with_due(mut self, due: Option<Date>) -> Self {
        self.due = due;
        self
    }

    /// Sets the note.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    /// Sets the priority.
    #[must_use]
    pub const fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }
}

/// A node's status. Always derived, never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    /// At least one prerequisite is not done.
    Locked,
    /// Every prerequisite is done, and this is not.
    Available,
    /// Finished.
    Done,
    /// This node sits on, or downstream of, a dependency cycle, so it can never
    /// become available. Only reachable on a document that arrived with a cycle
    /// already in it — see [`Graph::find_cycles`](crate::Graph::find_cycles).
    Cyclic,
}
