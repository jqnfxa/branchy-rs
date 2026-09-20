//! Every mutation, as one value.
//!
//! A [`Command`] is what the in-app command line, the `branchy` binary and,
//! later, Tauri's IPC all produce. Applying one returns its inverse, which is
//! what makes undo possible, and each maps onto an Automerge operation when
//! persistence arrives.

use std::collections::BTreeSet;

use crate::error::Error;
use crate::graph::Graph;
use crate::id::{AreaId, NodeId};
use crate::node::{Area, NewArea, NewNode, Node};

/// One change to a graph.
///
/// Most variants correspond to something a person can type. The two `Restore`
/// variants do not: they are produced by [`Graph::apply`] as the inverse of a
/// removal, and carry enough to put back exactly what was taken away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Create an area.
    AddArea {
        /// Display name.
        name: String,
        /// Accent colour.
        color: String,
    },
    /// Remove an area that holds no nodes.
    RemoveArea(AreaId),
    /// Put a removed area back at its original id.
    RestoreArea {
        /// The id it had.
        id: AreaId,
        /// The area itself.
        area: Area,
    },

    /// Create a node, optionally wired up in the same step.
    ///
    /// The prerequisites and dependents are part of the command because the new
    /// node has no id until it exists, so they cannot be expressed as separate
    /// commands that run after it.
    AddNode {
        /// Display name.
        name: String,
        /// Longer description.
        note: String,
        /// Which area it belongs to.
        area: AreaId,
        /// Higher sorts earlier in the queue.
        priority: u8,
        /// Existing nodes the new one will need.
        prereqs: BTreeSet<NodeId>,
        /// Existing nodes that will need the new one.
        dependents: BTreeSet<NodeId>,
    },
    /// Remove a node and strip it from everything that needed it.
    RemoveNode(NodeId),
    /// Put a removed node back, with the edges that pointed at it.
    RestoreNode {
        /// The id it had.
        id: NodeId,
        /// The node itself, boxed to keep this enum small.
        node: Box<Node>,
        /// Nodes that needed it before it was removed.
        dependents: BTreeSet<NodeId>,
    },

    /// Mark a node done or not done.
    SetDone {
        /// The node.
        node: NodeId,
        /// The new value.
        done: bool,
    },
    /// Change a node's priority.
    SetPriority {
        /// The node.
        node: NodeId,
        /// The new value.
        priority: u8,
    },
    /// Rename a node.
    SetName {
        /// The node.
        node: NodeId,
        /// The new name.
        name: String,
    },
    /// Replace a node's note.
    SetNote {
        /// The node.
        node: NodeId,
        /// The new note.
        note: String,
    },
    /// Move a node to another area.
    SetArea {
        /// The node.
        node: NodeId,
        /// The area to move it to.
        area: AreaId,
    },

    /// Record that one node needs another.
    AddPrerequisite {
        /// The node that gains a prerequisite.
        dependent: NodeId,
        /// What it now needs.
        prerequisite: NodeId,
    },
    /// Remove such a record.
    RemovePrerequisite {
        /// The node that loses a prerequisite.
        dependent: NodeId,
        /// What it no longer needs.
        prerequisite: NodeId,
    },
}

/// What happened when a command was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The node the command created or changed, when there was one.
    pub node: Option<NodeId>,
    /// The area the command created or changed, when there was one.
    pub area: Option<AreaId>,
    /// Commands that put the graph back as it was, in the order to run them.
    pub undo: Vec<Command>,
}

impl Graph {
    /// Applies a command and returns how to undo it.
    ///
    /// Either the whole command takes effect or none of it does: a
    /// [`Command::AddNode`] whose edges would close a loop leaves no node
    /// behind.
    ///
    /// # Errors
    ///
    /// Whatever the underlying operation would return — [`Error::NoSuchNode`],
    /// [`Error::NoSuchArea`], [`Error::WouldCycle`], [`Error::NotAPrerequisite`]
    /// or [`Error::AreaNotEmpty`].
    #[allow(clippy::too_many_lines)]
    pub fn apply(&mut self, command: Command) -> Result<Applied, Error> {
        match command {
            Command::AddArea { name, color } => {
                let id = self.add_area(NewArea { name, color });
                Ok(Applied {
                    node: None,
                    area: Some(id),
                    undo: vec![Command::RemoveArea(id)],
                })
            }

            Command::RemoveArea(id) => {
                let area = self.remove_area(id)?;
                Ok(Applied {
                    node: None,
                    area: Some(id),
                    undo: vec![Command::RestoreArea { id, area }],
                })
            }

            Command::RestoreArea { id, area } => {
                self.insert_area_at(id, area);
                Ok(Applied {
                    node: None,
                    area: Some(id),
                    undo: vec![Command::RemoveArea(id)],
                })
            }

            Command::AddNode {
                name,
                note,
                area,
                priority,
                prereqs,
                dependents,
            } => {
                self.check_new_node_edges(&prereqs, &dependents)?;
                for id in prereqs.iter().chain(dependents.iter()) {
                    if self.node(*id).is_none() {
                        return Err(Error::NoSuchNode(*id));
                    }
                }

                let id = self.add_node(
                    NewNode::new(name, area)
                        .with_note(note)
                        .with_priority(priority),
                )?;
                for prereq in &prereqs {
                    self.add_prerequisite_unchecked(id, *prereq)?;
                }
                for dependent in &dependents {
                    self.add_prerequisite_unchecked(*dependent, id)?;
                }

                Ok(Applied {
                    node: Some(id),
                    area: Some(area),
                    undo: vec![Command::RemoveNode(id)],
                })
            }

            Command::RemoveNode(id) => {
                let dependents: BTreeSet<NodeId> = self.dependents(id).into_iter().collect();
                let node = self.remove_node(id)?;
                Ok(Applied {
                    node: Some(id),
                    area: Some(node.area),
                    undo: vec![Command::RestoreNode {
                        id,
                        node: Box::new(node),
                        dependents,
                    }],
                })
            }

            Command::RestoreNode {
                id,
                node,
                dependents,
            } => {
                self.insert_node_at(id, *node);
                for dependent in &dependents {
                    self.add_prerequisite_unchecked(*dependent, id)?;
                }
                Ok(Applied {
                    node: Some(id),
                    area: None,
                    undo: vec![Command::RemoveNode(id)],
                })
            }

            Command::SetDone { node, done } => {
                let was = self.require(node)?.done;
                self.set_done(node, done)?;
                Ok(Applied {
                    node: Some(node),
                    area: None,
                    undo: vec![Command::SetDone { node, done: was }],
                })
            }

            Command::SetPriority { node, priority } => {
                let was = self.require(node)?.priority;
                self.set_priority(node, priority)?;
                Ok(Applied {
                    node: Some(node),
                    area: None,
                    undo: vec![Command::SetPriority {
                        node,
                        priority: was,
                    }],
                })
            }

            Command::SetName { node, name } => {
                let was = self.require(node)?.name.clone();
                self.set_name(node, name)?;
                Ok(Applied {
                    node: Some(node),
                    area: None,
                    undo: vec![Command::SetName { node, name: was }],
                })
            }

            Command::SetNote { node, note } => {
                let was = self.require(node)?.note.clone();
                self.set_note(node, note)?;
                Ok(Applied {
                    node: Some(node),
                    area: None,
                    undo: vec![Command::SetNote { node, note: was }],
                })
            }

            Command::SetArea { node, area } => {
                let was = self.require(node)?.area;
                self.set_area(node, area)?;
                Ok(Applied {
                    node: Some(node),
                    area: Some(area),
                    undo: vec![Command::SetArea { node, area: was }],
                })
            }

            Command::AddPrerequisite {
                dependent,
                prerequisite,
            } => {
                let had = self.require(dependent)?.prereqs.contains(&prerequisite);
                self.add_prerequisite(dependent, prerequisite)?;
                let undo = if had {
                    // it was already there, so undoing must not remove it
                    Vec::new()
                } else {
                    vec![Command::RemovePrerequisite {
                        dependent,
                        prerequisite,
                    }]
                };
                Ok(Applied {
                    node: Some(dependent),
                    area: None,
                    undo,
                })
            }

            Command::RemovePrerequisite {
                dependent,
                prerequisite,
            } => {
                self.remove_prerequisite(dependent, prerequisite)?;
                Ok(Applied {
                    node: Some(dependent),
                    area: None,
                    undo: vec![Command::AddPrerequisite {
                        dependent,
                        prerequisite,
                    }],
                })
            }
        }
    }

    /// Applies a whole sequence, stopping at the first failure.
    ///
    /// The undo commands come back in the order that reverses them, so running
    /// them in sequence returns the graph to where it started. Nothing is rolled
    /// back automatically on failure: the caller has the undo list for what did
    /// take effect and decides what to do with it.
    ///
    /// # Errors
    ///
    /// The first error any command in the sequence produced, along with the
    /// undo list for everything applied before it.
    pub fn apply_all(
        &mut self,
        commands: impl IntoIterator<Item = Command>,
    ) -> Result<Vec<Command>, (Error, Vec<Command>)> {
        let mut undo: Vec<Command> = Vec::new();
        for command in commands {
            match self.apply(command) {
                Ok(applied) => {
                    // later undos must run first
                    let mut next = applied.undo;
                    next.extend(undo);
                    undo = next;
                }
                Err(error) => return Err((error, undo)),
            }
        }
        Ok(undo)
    }

    fn require(&self, id: NodeId) -> Result<&Node, Error> {
        self.node(id).ok_or(Error::NoSuchNode(id))
    }

    /// A new node needing `prereqs` and needed by `dependents` closes a loop
    /// exactly when one of those prerequisites already reaches one of those
    /// dependents.
    fn check_new_node_edges(
        &self,
        prereqs: &BTreeSet<NodeId>,
        dependents: &BTreeSet<NodeId>,
    ) -> Result<(), Error> {
        for prereq in prereqs {
            for dependent in dependents {
                if prereq == dependent {
                    return Err(Error::WouldCycle {
                        dependent: *dependent,
                        prerequisite: *prereq,
                        path: vec![*prereq, *prereq],
                    });
                }
                if let Some(path) = self.closing_path(*dependent, *prereq) {
                    return Err(Error::WouldCycle {
                        dependent: *dependent,
                        prerequisite: *prereq,
                        path,
                    });
                }
            }
        }
        Ok(())
    }
}
