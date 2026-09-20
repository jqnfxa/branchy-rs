//! Everything that can go wrong in the graph.

use crate::id::{AreaId, NodeId};

/// An operation the graph refused.
///
/// Errors carry ids rather than names, because the graph is not the right place
/// to decide how something is spelled for a user. The UI layer resolves ids
/// against the graph when it builds a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No node with this id.
    NoSuchNode(NodeId),
    /// No area with this id.
    NoSuchArea(AreaId),
    /// Adding this prerequisite would close a dependency loop.
    ///
    /// `path` reads left to right as "needs", beginning and ending at the same
    /// node, so it can be shown to the user as the loop that was refused.
    WouldCycle {
        /// The node that would gain a prerequisite.
        dependent: NodeId,
        /// The prerequisite that was refused.
        prerequisite: NodeId,
        /// The loop the edge would have closed.
        path: Vec<NodeId>,
    },
    /// The edge asked to be removed is not there.
    NotAPrerequisite {
        /// The node that was expected to have the prerequisite.
        dependent: NodeId,
        /// The prerequisite that was expected.
        prerequisite: NodeId,
    },
    /// An area still holds nodes, so it cannot be removed.
    AreaNotEmpty {
        /// The area.
        area: AreaId,
        /// How many nodes are still in it.
        nodes: usize,
    },
    /// This node can never be unlocked, because a cycle sits between it and the
    /// nodes it depends on.
    CycleBlocks(NodeId),
    /// A date the calendar does not have, or one not written as `YYYY-MM-DD`.
    BadDate(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchNode(id) => write!(f, "no such node: {id}"),
            Self::NoSuchArea(id) => write!(f, "no such area: {id}"),
            Self::WouldCycle {
                dependent,
                prerequisite,
                path,
            } => {
                write!(
                    f,
                    "{dependent} cannot need {prerequisite}: that closes a cycle "
                )?;
                for (i, id) in path.iter().enumerate() {
                    if i > 0 {
                        write!(f, " -> ")?;
                    }
                    write!(f, "{id}")?;
                }
                Ok(())
            }
            Self::NotAPrerequisite {
                dependent,
                prerequisite,
            } => write!(f, "{dependent} does not need {prerequisite}"),
            Self::AreaNotEmpty { area, nodes } => {
                write!(f, "area {area} still holds {nodes} node(s)")
            }
            Self::CycleBlocks(id) => write!(f, "{id} is blocked by a dependency cycle"),
            Self::BadDate(text) => write!(f, "not a date: {text}, expected YYYY-MM-DD"),
        }
    }
}

impl std::error::Error for Error {}
