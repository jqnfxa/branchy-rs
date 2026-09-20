//! The document on disk.
//!
//! The format is written out by hand rather than derived from the graph's
//! internals, so the two can change independently. `branchy-core` keeps no
//! dependencies and no opinion about storage; this module owns the format, its
//! version number, and the fact that it is JSON at all. Phase 3 replaces the
//! body with an Automerge document behind the same two functions.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use branchy_core::{Area, AreaId, Command, Graph, Node, NodeId};
use serde::{Deserialize, Serialize};

/// Bumped when the shape changes in a way an older reader could not cope with.
const FORMAT_VERSION: u32 = 1;

/// How many changes can be taken back.
///
/// Kept as whole documents beside the real one rather than as a command log,
/// because a document that will not parse can still be recovered by hand, and
/// twenty copies of a planning graph cost nothing.
const UNDO_DEPTH: usize = 20;

/// One stored graph.
#[derive(Debug, Serialize, Deserialize)]
struct Document {
    version: u32,
    areas: Vec<AreaRecord>,
    nodes: Vec<NodeRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AreaRecord {
    id: u64,
    name: String,
    color: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct NodeRecord {
    id: u64,
    name: String,
    #[serde(default)]
    note: String,
    area: u64,
    priority: u8,
    done: bool,
    #[serde(default)]
    prereqs: Vec<u64>,
}

/// Anything that can go wrong reading or writing the document.
#[derive(Debug)]
pub enum StoreError {
    /// The file could not be read or written.
    Io(std::io::Error),
    /// The file is not the JSON this expects.
    Json(serde_json::Error),
    /// The file was written by a newer version of Branchy.
    Version(u32),
    /// The stored graph is not internally consistent.
    Graph(branchy_core::Error),
    /// There is nothing to undo.
    NothingToUndo,
    /// No sensible place to keep the document could be worked out.
    NoHome,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Json(e) => write!(f, "the file is not valid Branchy JSON: {e}"),
            Self::Version(v) => write!(
                f,
                "the file was written by a newer Branchy (format {v}, this build reads {FORMAT_VERSION})"
            ),
            Self::Graph(e) => write!(f, "the stored graph is inconsistent: {e}"),
            Self::NothingToUndo => write!(f, "nothing to undo"),
            Self::NoHome => write!(
                f,
                "could not work out where to keep the document; pass --file"
            ),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
impl From<branchy_core::Error> for StoreError {
    fn from(e: branchy_core::Error) -> Self {
        Self::Graph(e)
    }
}

impl Document {
    /// Captures a graph.
    fn from_graph(graph: &Graph) -> Self {
        Self {
            version: FORMAT_VERSION,
            areas: graph
                .areas()
                .map(|(id, area)| AreaRecord {
                    id: id.raw(),
                    name: area.name.clone(),
                    color: area.color.clone(),
                })
                .collect(),
            nodes: graph
                .nodes()
                .map(|(id, node)| NodeRecord {
                    id: id.raw(),
                    name: node.name.clone(),
                    note: node.note.clone(),
                    area: node.area.raw(),
                    priority: node.priority,
                    done: node.done,
                    prereqs: node.prereqs.iter().copied().map(NodeId::raw).collect(),
                })
                .collect(),
        }
    }

    /// Rebuilds a graph, ids and all.
    ///
    /// Restoring goes through the ordinary command layer rather than a private
    /// back door, which is also what keeps the id counters ahead of everything
    /// loaded.
    ///
    /// # Errors
    ///
    /// [`StoreError::Version`] for a file from a newer build, or
    /// [`StoreError::Graph`] if the records do not form a usable graph.
    fn into_graph(self) -> Result<Graph, StoreError> {
        if self.version > FORMAT_VERSION {
            return Err(StoreError::Version(self.version));
        }
        let mut graph = Graph::new();
        for record in self.areas {
            graph.apply(Command::RestoreArea {
                id: AreaId::new(record.id),
                area: Area {
                    name: record.name,
                    color: record.color,
                },
            })?;
        }
        // nodes first, then edges, so an edge can never point at a node that
        // has not been restored yet
        let edges: Vec<(u64, Vec<u64>)> = self
            .nodes
            .iter()
            .map(|n| (n.id, n.prereqs.clone()))
            .collect();
        for record in self.nodes {
            graph.apply(Command::RestoreNode {
                id: NodeId::new(record.id),
                node: Box::new(Node {
                    name: record.name,
                    note: record.note,
                    area: AreaId::new(record.area),
                    priority: record.priority,
                    done: record.done,
                    prereqs: BTreeSet::new(),
                }),
                dependents: BTreeSet::new(),
            })?;
        }
        for (dependent, prereqs) in edges {
            for prereq in prereqs {
                // unchecked: a stored document may legitimately hold a cycle
                // that arrived through a merge
                graph.add_prerequisite_unchecked(NodeId::new(dependent), NodeId::new(prereq))?;
            }
        }
        Ok(graph)
    }
}

/// Where the document lives when none was named.
///
/// `BRANCHY_FILE` wins, because the per-user data directory is derived from
/// `HOME` and sandboxes rewrite it: run from inside a snap and you would
/// silently get a second, empty graph. Every front end resolves the path this
/// way so they always agree.
///
/// # Errors
///
/// [`StoreError::NoHome`] when the platform offers no data directory and the
/// environment variable is not set either.
pub fn default_path() -> Result<PathBuf, StoreError> {
    if let Some(given) = std::env::var_os("BRANCHY_FILE") {
        return Ok(PathBuf::from(given));
    }
    directories::ProjectDirs::from("dev", "jqnfxa", "branchy")
        .map(|dirs| dirs.data_dir().join("graph.json"))
        .ok_or(StoreError::NoHome)
}

/// Reads the graph, or returns an empty one if the file is not there yet.
///
/// # Errors
///
/// [`StoreError::Io`], [`StoreError::Json`], [`StoreError::Version`] or
/// [`StoreError::Graph`].
pub fn load(path: &Path) -> Result<Graph, StoreError> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<Document>(&text)?.into_graph(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Graph::new()),
        Err(e) => Err(StoreError::Io(e)),
    }
}

/// Writes the graph, pushing the previous contents onto the undo stack.
///
/// The new file is written beside the target and renamed over it, because a
/// half-written document is exactly what a sync tool would happily propagate to
/// every other device.
///
/// # Errors
///
/// [`StoreError::Io`] or [`StoreError::Json`].
pub fn save(path: &Path, graph: &Graph) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.exists() {
        push_undo(path)?;
    }
    write_atomically(path, graph)
}

/// Takes back the last change.
///
/// This is a stack, not a swap: calling it twice goes back two changes rather
/// than returning where it started. There is no redo, so an undo too far is
/// re-entered by hand — which is the predictable half of the trade, and the
/// reason the old toggle had to go.
///
/// # Errors
///
/// [`StoreError::NothingToUndo`] when the stack is empty, otherwise
/// [`StoreError::Io`].
pub fn undo(path: &Path) -> Result<(), StoreError> {
    let newest = undo_path(path, 1);
    if !newest.exists() {
        return Err(StoreError::NothingToUndo);
    }
    fs::rename(&newest, path)?;

    // everything older moves up one place
    for slot in 2..=UNDO_DEPTH {
        let from = undo_path(path, slot);
        if !from.exists() {
            break;
        }
        fs::rename(&from, undo_path(path, slot - 1))?;
    }
    Ok(())
}

/// How many changes could still be taken back.
#[must_use]
pub fn undo_depth(path: &Path) -> usize {
    (1..=UNDO_DEPTH)
        .take_while(|slot| undo_path(path, *slot).exists())
        .count()
}

/// Moves the current document into slot 1, shifting the rest down and dropping
/// whatever falls off the end.
fn push_undo(path: &Path) -> Result<(), StoreError> {
    let oldest = undo_path(path, UNDO_DEPTH);
    if oldest.exists() {
        fs::remove_file(&oldest)?;
    }
    for slot in (1..UNDO_DEPTH).rev() {
        let from = undo_path(path, slot);
        if from.exists() {
            fs::rename(&from, undo_path(path, slot + 1))?;
        }
    }
    fs::copy(path, undo_path(path, 1))?;
    Ok(())
}

fn undo_path(path: &Path, slot: usize) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".undo{slot}"));
    path.with_file_name(name)
}

fn write_atomically(path: &Path, graph: &Graph) -> Result<(), StoreError> {
    let text = serde_json::to_string_pretty(&Document::from_graph(graph))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, text)?;
    fs::rename(&temporary, path)?;
    Ok(())
}
