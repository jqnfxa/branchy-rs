//! Everything a user interface needs to draw the graph, in one value.
//!
//! The frontend must not recompute status, tier or the queue: that logic lives
//! in `branchy-core` and having a second copy in JavaScript is how the two
//! start disagreeing. So the whole derived picture is handed over at once, and
//! the frontend only draws it.
//!
//! The Tauri shell will return this from its `snapshot` command. `branchy
//! snapshot` prints the same thing, which is what makes the frontend
//! developable, and the app scriptable, without a running window.

use branchy_core::{Graph, NodeId, Status};
use serde::Serialize;

/// The graph as a user interface wants it.
#[derive(Debug, Serialize)]
pub struct Snapshot {
    /// Directions, in id order.
    pub areas: Vec<AreaView>,
    /// Tasks, in id order, each carrying its derived status and tier.
    pub nodes: Vec<NodeView>,
    /// Available tasks, highest priority first.
    pub queue: Vec<String>,
    /// Groups of mutually blocking tasks. Empty on a healthy graph.
    pub cycles: Vec<Vec<String>>,
    /// Counts for the top bar.
    pub tally: Tally,
}

/// One direction.
#[derive(Debug, Serialize)]
pub struct AreaView {
    /// Stable identifier, as `a0`.
    pub id: String,
    /// Display name. User data, never translated.
    pub name: String,
    /// Accent colour.
    pub color: String,
    /// How many of this direction's tasks are done.
    pub done: usize,
    /// How many tasks it holds.
    pub total: usize,
}

/// One task, with everything derived already worked out.
#[derive(Debug, Serialize)]
pub struct NodeView {
    /// Stable identifier, as `n0`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Longer description, possibly empty.
    pub note: String,
    /// Which direction it belongs to.
    pub area: String,
    /// Higher sorts earlier in the queue.
    pub priority: u8,
    /// Whether it is finished.
    pub done: bool,
    /// What it needs.
    pub prereqs: Vec<String>,
    /// What needs it.
    pub dependents: Vec<String>,
    /// `locked`, `available`, `done` or `cyclic`.
    pub status: String,
    /// Absent when the task sits on or after a cycle.
    pub tier: Option<u32>,
}

/// Counts for the top bar.
#[derive(Debug, Serialize)]
pub struct Tally {
    /// Every task.
    pub total: usize,
    /// Finished tasks.
    pub done: usize,
    /// Tasks whose prerequisites are all met.
    pub available: usize,
}

fn key(id: NodeId) -> String {
    id.to_string()
}

impl Snapshot {
    /// Captures a graph.
    #[must_use]
    pub fn of(graph: &Graph) -> Self {
        let statuses = graph.statuses();
        let tiers = graph.tiers();

        let areas = graph
            .areas()
            .map(|(area_id, area)| {
                let held: Vec<bool> = graph
                    .nodes()
                    .filter(|(_, node)| node.area == area_id)
                    .map(|(_, node)| node.done)
                    .collect();
                AreaView {
                    id: area_id.to_string(),
                    name: area.name.clone(),
                    color: area.color.clone(),
                    done: held.iter().filter(|done| **done).count(),
                    total: held.len(),
                }
            })
            .collect();

        let nodes: Vec<NodeView> = graph
            .nodes()
            .map(|(id, node)| NodeView {
                id: key(id),
                name: node.name.clone(),
                note: node.note.clone(),
                area: node.area.to_string(),
                priority: node.priority,
                done: node.done,
                prereqs: node.prereqs.iter().copied().map(key).collect(),
                dependents: graph.dependents(id).into_iter().map(key).collect(),
                status: match statuses.get(&id) {
                    Some(Status::Done) => "done",
                    Some(Status::Available) => "available",
                    Some(Status::Cyclic) => "cyclic",
                    _ => "locked",
                }
                .to_string(),
                tier: tiers.get(&id).copied(),
            })
            .collect();

        let done = nodes.iter().filter(|node| node.done).count();
        let queue: Vec<String> = graph.queue().into_iter().map(key).collect();

        Self {
            tally: Tally {
                total: nodes.len(),
                done,
                available: queue.len(),
            },
            areas,
            queue,
            cycles: graph
                .find_cycles()
                .into_iter()
                .map(|group| group.into_iter().map(key).collect())
                .collect(),
            nodes,
        }
    }
}

/// What a command did, and the graph afterwards.
///
/// A user interface needs both: the snapshot to redraw, and the id so it can
/// select and fly to whatever was just created. Working the id out by diffing
/// two snapshots would be guesswork.
#[derive(Debug, Serialize)]
pub struct Outcome {
    /// The task the command created or changed, if there was one.
    pub node: Option<String>,
    /// The direction the command created or changed, if there was one.
    pub area: Option<String>,
    /// The whole graph, after.
    pub snapshot: Snapshot,
}
