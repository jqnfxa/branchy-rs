//! The undo log: how to take each change back, as commands.
//!
//! Up to 0.2.9 every change pushed a whole copy of the document onto a stack
//! twenty deep. On an 800-task vault that was 12 MB of copies, and every edit
//! renamed twenty files. Every command already knows its own inverse, which is
//! what `Graph::apply` returns, so a change is now taken back by applying
//! those, and the log holds a few hundred bytes per change.
//!
//! The one thing a log of inverses cannot survive is the document changing
//! under it: an edit by an older build, by hand, or later by sync. Each entry
//! therefore records a hash of the document as Branchy wrote it, and undo
//! refuses to apply inverses to a document that no longer matches. One whole
//! copy of the document before the latest change is still kept beside the log,
//! because a document that will not parse can be recovered from it by hand.
//!
//! Like the document, the format is written out here by hand rather than
//! derived from the core's types, so the two can change independently.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use branchy_core::{Area, AreaId, Command, Date, Node, NodeId};
use serde::{Deserialize, Serialize};

use crate::store::{NodeRecord, StoreError};

/// How many changes can be taken back.
pub(crate) const DEPTH: usize = 20;

/// One change, and how to take it back.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Entry {
    /// FNV-1a of the document text this change left behind, in hex.
    pub after: String,
    /// The commands that take the change back, in the order to apply them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub undo: Vec<Op>,
    /// The whole document as it was before, for a change made without
    /// commands: a document saved directly rather than edited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
}

/// A command as stored. Every variant of [`Command`] has one, though only the
/// inverses ever reach the log.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(crate) enum Op {
    AddArea {
        name: String,
        color: String,
    },
    SetAreaName {
        area: u64,
        name: String,
    },
    SetAreaColor {
        area: u64,
        color: String,
    },
    RemoveArea {
        area: u64,
    },
    RestoreArea {
        id: u64,
        name: String,
        color: String,
    },
    AddNode {
        name: String,
        note: String,
        area: u64,
        priority: u8,
        due: Option<String>,
        prereqs: Vec<u64>,
        dependents: Vec<u64>,
    },
    RemoveNode {
        node: u64,
    },
    RestoreNode {
        node: NodeRecord,
        dependents: Vec<u64>,
    },
    SetDone {
        node: u64,
        done: bool,
    },
    SetDue {
        node: u64,
        due: Option<String>,
    },
    SetPriority {
        node: u64,
        priority: u8,
    },
    SetName {
        node: u64,
        name: String,
    },
    SetNote {
        node: u64,
        note: String,
    },
    SetArea {
        node: u64,
        area: u64,
    },
    AddPrerequisite {
        dependent: u64,
        prerequisite: u64,
    },
    RemovePrerequisite {
        dependent: u64,
        prerequisite: u64,
    },
}

fn ids(set: &BTreeSet<NodeId>) -> Vec<u64> {
    set.iter().copied().map(NodeId::raw).collect()
}

fn id_set(raw: &[u64]) -> BTreeSet<NodeId> {
    raw.iter().copied().map(NodeId::new).collect()
}

fn date(text: Option<String>) -> Result<Option<Date>, StoreError> {
    text.map(|t| t.parse::<Date>())
        .transpose()
        .map_err(StoreError::Graph)
}

impl Op {
    pub(crate) fn of(command: &Command) -> Self {
        match command {
            Command::AddArea { name, color } => Self::AddArea {
                name: name.clone(),
                color: color.clone(),
            },
            Command::SetAreaName { area, name } => Self::SetAreaName {
                area: area.raw(),
                name: name.clone(),
            },
            Command::SetAreaColor { area, color } => Self::SetAreaColor {
                area: area.raw(),
                color: color.clone(),
            },
            Command::RemoveArea(area) => Self::RemoveArea { area: area.raw() },
            Command::RestoreArea { id, area } => Self::RestoreArea {
                id: id.raw(),
                name: area.name.clone(),
                color: area.color.clone(),
            },
            Command::AddNode {
                name,
                note,
                area,
                priority,
                due,
                prereqs,
                dependents,
            } => Self::AddNode {
                name: name.clone(),
                note: note.clone(),
                area: area.raw(),
                priority: *priority,
                due: due.map(|d| d.to_string()),
                prereqs: ids(prereqs),
                dependents: ids(dependents),
            },
            Command::RemoveNode(node) => Self::RemoveNode { node: node.raw() },
            Command::RestoreNode {
                id,
                node,
                dependents,
            } => Self::RestoreNode {
                node: NodeRecord::of(*id, node),
                dependents: ids(dependents),
            },
            Command::SetDone { node, done } => Self::SetDone {
                node: node.raw(),
                done: *done,
            },
            Command::SetDue { node, due } => Self::SetDue {
                node: node.raw(),
                due: due.map(|d| d.to_string()),
            },
            Command::SetPriority { node, priority } => Self::SetPriority {
                node: node.raw(),
                priority: *priority,
            },
            Command::SetName { node, name } => Self::SetName {
                node: node.raw(),
                name: name.clone(),
            },
            Command::SetNote { node, note } => Self::SetNote {
                node: node.raw(),
                note: note.clone(),
            },
            Command::SetArea { node, area } => Self::SetArea {
                node: node.raw(),
                area: area.raw(),
            },
            Command::AddPrerequisite {
                dependent,
                prerequisite,
            } => Self::AddPrerequisite {
                dependent: dependent.raw(),
                prerequisite: prerequisite.raw(),
            },
            Command::RemovePrerequisite {
                dependent,
                prerequisite,
            } => Self::RemovePrerequisite {
                dependent: dependent.raw(),
                prerequisite: prerequisite.raw(),
            },
        }
    }

    pub(crate) fn into_command(self) -> Result<Command, StoreError> {
        Ok(match self {
            Self::AddArea { name, color } => Command::AddArea { name, color },
            Self::SetAreaName { area, name } => Command::SetAreaName {
                area: AreaId::new(area),
                name,
            },
            Self::SetAreaColor { area, color } => Command::SetAreaColor {
                area: AreaId::new(area),
                color,
            },
            Self::RemoveArea { area } => Command::RemoveArea(AreaId::new(area)),
            Self::RestoreArea { id, name, color } => Command::RestoreArea {
                id: AreaId::new(id),
                area: Area { name, color },
            },
            Self::AddNode {
                name,
                note,
                area,
                priority,
                due,
                prereqs,
                dependents,
            } => Command::AddNode {
                name,
                note,
                area: AreaId::new(area),
                priority,
                due: date(due)?,
                prereqs: id_set(&prereqs),
                dependents: id_set(&dependents),
            },
            Self::RemoveNode { node } => Command::RemoveNode(NodeId::new(node)),
            Self::RestoreNode { node, dependents } => {
                let (id, node): (NodeId, Node) = node.into_node()?;
                Command::RestoreNode {
                    id,
                    node: Box::new(node),
                    dependents: id_set(&dependents),
                }
            }
            Self::SetDone { node, done } => Command::SetDone {
                node: NodeId::new(node),
                done,
            },
            Self::SetDue { node, due } => Command::SetDue {
                node: NodeId::new(node),
                due: date(due)?,
            },
            Self::SetPriority { node, priority } => Command::SetPriority {
                node: NodeId::new(node),
                priority,
            },
            Self::SetName { node, name } => Command::SetName {
                node: NodeId::new(node),
                name,
            },
            Self::SetNote { node, note } => Command::SetNote {
                node: NodeId::new(node),
                note,
            },
            Self::SetArea { node, area } => Command::SetArea {
                node: NodeId::new(node),
                area: AreaId::new(area),
            },
            Self::AddPrerequisite {
                dependent,
                prerequisite,
            } => Command::AddPrerequisite {
                dependent: NodeId::new(dependent),
                prerequisite: NodeId::new(prerequisite),
            },
            Self::RemovePrerequisite {
                dependent,
                prerequisite,
            } => Command::RemovePrerequisite {
                dependent: NodeId::new(dependent),
                prerequisite: NodeId::new(prerequisite),
            },
        })
    }
}

/// FNV-1a, 64 bits, in hex.
///
/// Written out rather than taken from `std`, whose hasher is free to change
/// between Rust releases, and a log written by one build has to be readable by
/// the next. It only has to notice that a document changed, not resist anyone.
pub(crate) fn fingerprint(text: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;
    let hash = text.bytes().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    });
    format!("{hash:016x}")
}

/// `.branchy/undo/graph.json.log` for `graph.json`.
pub(crate) fn log_path(document: &Path) -> PathBuf {
    beside(document, ".log")
}

/// `.branchy/undo/graph.json.prev`: the whole document as it was before the
/// latest change, for recovery by hand.
pub(crate) fn previous_path(document: &Path) -> PathBuf {
    beside(document, ".prev")
}

fn beside(document: &Path, suffix: &str) -> PathBuf {
    let mut name = document.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    document
        .with_file_name(crate::store::LOCAL_DIR)
        .join("undo")
        .join(name)
}

/// The log, oldest entry first. A log that will not parse reads as empty:
/// losing the history is better than refusing every later change.
pub(crate) fn read(document: &Path) -> Vec<Entry> {
    let Ok(text) = fs::read_to_string(log_path(document)) else {
        return Vec::new();
    };
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str::<Entry>)
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default()
}

/// Writes the log, keeping only the newest [`DEPTH`] entries. An empty log is
/// removed rather than left as an empty file.
pub(crate) fn write(document: &Path, entries: &[Entry]) -> Result<(), StoreError> {
    let path = log_path(document);
    let kept = &entries[entries.len().saturating_sub(DEPTH)..];
    if kept.is_empty() {
        match fs::remove_file(&path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(StoreError::Io(e)),
        }
    }
    let mut text = String::new();
    for entry in kept {
        text.push_str(&serde_json::to_string(entry)?);
        text.push('\n');
    }
    crate::store::replace_file(&path, &text)?;
    Ok(())
}
