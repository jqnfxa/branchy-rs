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

use branchy_core::{Area, AreaId, Command, Date, Graph, Node, NodeId};
use serde::{Deserialize, Serialize};

use crate::vault::{DOCUMENT, Vaults};

/// Bumped when the shape changes in a way an older reader could not cope with.
const FORMAT_VERSION: u32 = 1;

/// How many changes can be taken back.
///
/// Kept as whole documents rather than as a command log, because a document
/// that will not parse can still be recovered by hand, and twenty copies of a
/// planning graph cost nothing.
const UNDO_DEPTH: usize = 20;

/// State beside the document that belongs to this device alone, and that sync
/// will leave out: the undo stack, for now.
const LOCAL_DIR: &str = ".branchy";

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
    /// Absent in documents written before deadlines existed, hence the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    due: Option<String>,
    #[serde(default)]
    prereqs: Vec<u64>,
}

/// Anything that can go wrong reading or writing the document or the list of
/// vaults.
#[derive(Debug)]
#[non_exhaustive]
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
    /// No vault is open, and none was named.
    NoVault,
    /// The vault's folder is gone: moved, renamed or deleted since it was
    /// opened.
    VaultMissing(PathBuf),
    /// A new vault would land in a folder that already has things in it.
    VaultExists(PathBuf),
    /// The name would not work as a folder name everywhere Branchy runs.
    BadVaultName(String),
    /// A vault has to be a folder, and this is not one.
    NotAFolder(PathBuf),
    /// No recent vault goes by that name, and it is not a folder either.
    UnknownVault(String),
    /// More than one recent vault goes by that name.
    AmbiguousVault(String),
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
            Self::NoVault => write!(
                f,
                "no vault is open. Create one with `branchy vault new <name>`, \
                 or open a folder with `branchy vault open <folder>`"
            ),
            Self::VaultMissing(dir) => write!(
                f,
                "the vault folder {} is gone. `branchy vault forget` takes it off the list",
                dir.display()
            ),
            Self::VaultExists(dir) => write!(
                f,
                "{} already exists and is not empty. Pick another name, or open it as a vault",
                dir.display()
            ),
            Self::BadVaultName(name) => write!(
                f,
                "\"{name}\" cannot be a vault name: it has to work as a folder name, \
                 so it cannot be empty or hold any of / \\ : * ? \" < > |"
            ),
            Self::NotAFolder(path) => write!(f, "{} is not a folder", path.display()),
            Self::UnknownVault(name) => write!(
                f,
                "no recent vault is called {name}, and there is no folder by that name either"
            ),
            Self::AmbiguousVault(name) => write!(
                f,
                "more than one recent vault is called {name}; give its folder instead"
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
                    due: node.due.map(|date| date.to_string()),
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
                    due: match record.due {
                        Some(text) => Some(text.parse::<Date>()?),
                        None => None,
                    },
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
/// `BRANCHY_FILE` wins over everything, which is what scripts, tests and
/// sandboxes use to pin a front end to one file. Otherwise it is the document
/// in the vault opened last, so the terminal works wherever the window does.
///
/// # Errors
///
/// [`StoreError::NoVault`] when no vault has been opened yet,
/// [`StoreError::VaultMissing`] when the last one's folder is gone, or an
/// error reading the list of vaults.
pub fn default_path() -> Result<PathBuf, StoreError> {
    if let Some(given) = std::env::var_os("BRANCHY_FILE") {
        return Ok(PathBuf::from(given));
    }
    let vaults = Vaults::user()?;
    let dir = vaults.current().ok_or(StoreError::NoVault)?;
    // checked here, because a save would quietly recreate a deleted folder
    if !dir.is_dir() {
        return Err(StoreError::VaultMissing(dir.to_path_buf()));
    }
    Ok(dir.join(DOCUMENT))
}

/// The per-user directories, named once so every caller agrees.
pub(crate) fn project_dirs() -> Result<directories::ProjectDirs, StoreError> {
    directories::ProjectDirs::from("dev", "jqnfxa", "branchy").ok_or(StoreError::NoHome)
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
    let path = &real_path(path);
    tidy_loose_undo(path)?;
    if path.exists() {
        push_undo(path)?;
    }
    let text = serde_json::to_string_pretty(&Document::from_graph(graph))?;
    write_atomically(path, &text)?;
    Ok(())
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
    let path = &real_path(path);
    tidy_loose_undo(path)?;
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
    let path = &real_path(path);
    // best effort: failing to tidy only means counting the new place as empty
    let _ = tidy_loose_undo(path);
    (1..=UNDO_DEPTH)
        .take_while(|slot| undo_path(path, *slot).exists())
        .count()
}

/// Where writes to `path` really have to go.
///
/// Saving writes a temporary file and renames it over the target. If the
/// target is a symlink, that rename replaces the link itself with a regular
/// file, and from then on the link and the file it pointed at are two
/// documents that silently drift apart. Resolving first makes the rename land
/// on the real file and leaves the link alone — which matters, because
/// linking the document into a synced or version-controlled folder is exactly
/// what people do with it.
fn real_path(path: &Path) -> PathBuf {
    if let Ok(resolved) = fs::canonicalize(path) {
        return resolved;
    }
    // a link whose target does not exist yet: create the target, keep the link
    if let Ok(target) = fs::read_link(path) {
        if target.is_absolute() {
            return target;
        }
        if let Some(parent) = path.parent() {
            return parent.join(target);
        }
    }
    path.to_path_buf()
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
    let copy = undo_path(path, 1);
    if let Some(dir) = copy.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::copy(path, copy)?;
    Ok(())
}

/// `.branchy/undo/graph.json.1` for `graph.json`: out of sight, and named after
/// the document so two documents in one folder keep separate stacks.
fn undo_path(path: &Path, slot: usize) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{slot}"));
    path.with_file_name(LOCAL_DIR).join("undo").join(name)
}

/// Where versions before vaults kept the stack: right beside the document.
fn loose_undo_path(path: &Path, slot: usize) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".undo{slot}"));
    path.with_file_name(name)
}

/// Moves an undo stack left by an older version into `.branchy/undo/`.
///
/// It used to be twenty loose files beside the document, in what is now a
/// folder people open and browse as a vault. Moving them rather than leaving
/// them keeps the history, so the first undo after upgrading still works.
fn tidy_loose_undo(path: &Path) -> std::io::Result<()> {
    if !loose_undo_path(path, 1).exists() || undo_path(path, 1).exists() {
        return Ok(());
    }
    if let Some(dir) = undo_path(path, 1).parent() {
        fs::create_dir_all(dir)?;
    }
    for slot in 1..=UNDO_DEPTH {
        let from = loose_undo_path(path, slot);
        if !from.exists() {
            break;
        }
        fs::rename(&from, undo_path(path, slot))?;
    }
    Ok(())
}

/// Replaces a file's contents in one step, following a symlink to its target.
pub(crate) fn replace_file(path: &Path, text: &str) -> std::io::Result<()> {
    write_atomically(&real_path(path), text)
}

fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut temporary = path.as_os_str().to_os_string();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    fs::write(&temporary, text)?;
    fs::rename(&temporary, path)
}
