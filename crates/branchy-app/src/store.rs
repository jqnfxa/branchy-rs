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

use crate::history;
use crate::vault::{DOCUMENT, Vaults};

/// Bumped when the shape changes in a way an older reader could not cope with.
const FORMAT_VERSION: u32 = 1;

/// How deep the stack left by versions up to 0.2.9 went: twenty whole copies
/// of the document. Still read, so history from before the upgrade can be
/// taken back; no longer written. The log in [`crate::history`] replaced it.
const UNDO_DEPTH: usize = 20;

/// State beside the document that belongs to this device alone, and that sync
/// will leave out: the undo log, for now.
pub(crate) const LOCAL_DIR: &str = ".branchy";

/// One stored graph.
#[derive(Debug, Serialize, Deserialize)]
struct Document {
    version: u32,
    /// The id the next node will get. Stored because it is not always one
    /// past the highest id present: a removed node's id stays spent, and
    /// working the counter out from what is left would hand it out again.
    /// Absent in documents written before 0.2.8, hence the default, which
    /// leaves the counter where the loaded ids put it.
    #[serde(default)]
    next_node: u64,
    /// The id the next area will get, for the same reason.
    #[serde(default)]
    next_area: u64,
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
pub(crate) struct NodeRecord {
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

impl NodeRecord {
    pub(crate) fn of(id: NodeId, node: &Node) -> Self {
        Self {
            id: id.raw(),
            name: node.name.clone(),
            note: node.note.clone(),
            area: node.area.raw(),
            priority: node.priority,
            done: node.done,
            due: node.due.map(|date| date.to_string()),
            prereqs: node.prereqs.iter().copied().map(NodeId::raw).collect(),
        }
    }

    /// The node, prerequisites and all.
    pub(crate) fn into_node(self) -> Result<(NodeId, Node), StoreError> {
        Ok((
            NodeId::new(self.id),
            Node {
                name: self.name,
                note: self.note,
                area: AreaId::new(self.area),
                priority: self.priority,
                done: self.done,
                due: match self.due {
                    Some(text) => Some(text.parse::<Date>()?),
                    None => None,
                },
                prereqs: self.prereqs.into_iter().map(NodeId::new).collect(),
            },
        ))
    }
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
    /// The document changed since Branchy last wrote it, by hand or by an
    /// older build, so the undo history no longer fits it and was cleared.
    /// Carries where the copy from before Branchy's last change is kept.
    UndoStale(PathBuf),
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
                "\"{name}\" cannot be a vault name. It becomes a folder name on every \
                 system the vault may reach, so it cannot be empty, hold any of \
                 / \\ : * ? \" < > |, end in a dot, or be a name Windows reserves such as CON"
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
            Self::UndoStale(previous) => write!(
                f,
                "the document was changed outside Branchy since its last change, so the \
                 undo history no longer fits it and has been cleared. The document as it \
                 was before Branchy's last change is kept in {}",
                previous.display()
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
            next_node: graph.next_node_id().raw(),
            next_area: graph.next_area_id().raw(),
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
                .map(|(id, node)| NodeRecord::of(id, node))
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
        graph.reserve_ids(NodeId::new(self.next_node), AreaId::new(self.next_area));
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
            let (id, mut node) = record.into_node()?;
            node.prereqs = BTreeSet::new();
            graph.apply(Command::RestoreNode {
                id,
                node: Box::new(node),
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

    /// The ids this document has spent: the stored counters, or one past the
    /// highest id present when that is further on, as it is in a document
    /// written before the counters were stored.
    fn spent(&self) -> (u64, u64) {
        let node = self.nodes.iter().map(|n| n.id + 1).max().unwrap_or(0);
        let area = self.areas.iter().map(|a| a.id + 1).max().unwrap_or(0);
        (self.next_node.max(node), self.next_area.max(area))
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
    document_in(&Vaults::user()?)
}

/// Where the document lives, for directories given rather than discovered.
///
/// Neither `BRANCHY_FILE` nor `BRANCHY_VAULTS` is read here. A front end that
/// was handed its directories was handed its overrides too, and keeping the
/// environment out of this makes it a pure function of `dirs`.
///
/// # Errors
///
/// As [`default_path`], minus the ones about finding a home directory.
pub fn default_path_in(dirs: &Dirs) -> Result<PathBuf, StoreError> {
    document_in(&Vaults::for_dirs(dirs)?)
}

fn document_in(vaults: &Vaults) -> Result<PathBuf, StoreError> {
    let dir = vaults.current().ok_or(StoreError::NoVault)?;
    // checked here, because a save would quietly recreate a deleted folder
    if !dir.is_dir() {
        return Err(StoreError::VaultMissing(dir.to_path_buf()));
    }
    Ok(dir.join(DOCUMENT))
}

/// Where this device keeps Branchy's own files: the recent-vault list, and
/// the data directory a version from before vaults may have left a graph in.
///
/// The crate no longer goes looking for these by itself, because the right
/// answer depends on which front end is asking. The terminal derives them
/// from the platform's conventions, which is what [`Dirs::user`] does. A
/// shell that is told its directories by the platform builds one with
/// [`Dirs::new`] instead and passes it in, which is the only thing that can
/// work where there is no home directory to derive them from.
///
/// Discovery and the environment overrides stay at the edge, in
/// [`Dirs::user`], [`Vaults::user`] and [`default_path`]. Everything that
/// takes a `Dirs` is a pure function of it, which is what lets it be tested
/// without touching the environment that every other test in the process
/// shares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    config: PathBuf,
    data: PathBuf,
}

impl Dirs {
    /// Directories named outright, by a caller whose platform tells it.
    pub fn new(config: impl Into<PathBuf>, data: impl Into<PathBuf>) -> Self {
        Self {
            config: config.into(),
            data: data.into(),
        }
    }

    /// The conventional per-user directories for this platform.
    ///
    /// # Errors
    ///
    /// [`StoreError::NoHome`] when the platform offers nowhere to put them.
    pub fn user() -> Result<Self, StoreError> {
        let found =
            directories::ProjectDirs::from("dev", "jqnfxa", "branchy").ok_or(StoreError::NoHome)?;
        Ok(Self::new(found.config_dir(), found.data_dir()))
    }

    /// Where settings live, which so far is the recent-vault list alone.
    #[must_use]
    pub fn config(&self) -> &Path {
        &self.config
    }

    /// Where a version from before vaults would have left its graph.
    #[must_use]
    pub fn data(&self) -> &Path {
        &self.data
    }

    /// The file holding the recent-vault list.
    #[must_use]
    pub fn vault_list(&self) -> PathBuf {
        self.config.join(crate::vault::LIST_FILE)
    }
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

/// Writes the graph, keeping the whole previous document to undo to.
///
/// For a document saved directly rather than edited by commands. An edit has
/// its inverse commands and goes through [`save_change`], which keeps a few
/// hundred bytes instead of a copy of the document.
///
/// The new file is written beside the target and renamed over it, because a
/// half-written document is exactly what a sync tool would happily propagate to
/// every other device.
///
/// # Errors
///
/// [`StoreError::Io`] or [`StoreError::Json`].
pub fn save(path: &Path, graph: &Graph) -> Result<(), StoreError> {
    save_with(path, graph, None)
}

/// Writes the graph after an edit, recording the commands that take it back.
///
/// `undo` is what [`crate::apply_lines`] or `Graph::apply` returned for the
/// edit, in the order to apply them.
///
/// # Errors
///
/// [`StoreError::Io`] or [`StoreError::Json`].
pub fn save_change(path: &Path, graph: &Graph, undo: &[Command]) -> Result<(), StoreError> {
    save_with(path, graph, Some(undo))
}

fn save_with(path: &Path, graph: &Graph, undo: Option<&[Command]>) -> Result<(), StoreError> {
    let path = &real_path(path);
    tidy_loose_undo(path)?;
    let text = serde_json::to_string_pretty(&Document::from_graph(graph))?;
    let before = match fs::read_to_string(path) {
        Ok(before) => Some(before),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(StoreError::Io(e)),
    };
    if let Some(before) = before {
        let mut entries = history::read(path);
        entries.push(history::Entry {
            after: history::fingerprint(&text),
            undo: undo
                .map(|commands| commands.iter().map(history::Op::of).collect())
                .unwrap_or_default(),
            document: if undo.is_none() {
                Some(before.clone())
            } else {
                None
            },
        });
        // the log goes first: if the document write then fails, the newest
        // entry no longer matches the document and undo says so, rather than
        // applying inverses to a document they do not fit
        replace_file(&history::previous_path(path), &before)?;
        history::write(path, &entries)?;
    }
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
/// The log is used first. Once it is empty, a stack of whole copies left by a
/// version before 0.2.10 is still taken back, oldest last.
///
/// # Errors
///
/// [`StoreError::NothingToUndo`] when there is no history,
/// [`StoreError::UndoStale`] when the document changed outside Branchy since
/// its last change, otherwise [`StoreError::Io`].
pub fn undo(path: &Path) -> Result<(), StoreError> {
    let path = &real_path(path);
    tidy_loose_undo(path)?;
    let mut entries = history::read(path);
    let Some(entry) = entries.pop() else {
        return undo_copy(path);
    };

    let current = fs::read_to_string(path).unwrap_or_default();
    if history::fingerprint(&current) != entry.after {
        history::write(path, &[])?;
        return Err(StoreError::UndoStale(history::previous_path(path)));
    }

    let restored = if let Some(document) = entry.document {
        let spent = read_document(path).map(|current| current.spent());
        write_atomically(path, &document)?;
        if let Some(spent) = spent {
            keep_spent(path, spent)?;
        }
        fs::read_to_string(path)?
    } else {
        let commands = entry
            .undo
            .into_iter()
            .map(history::Op::into_command)
            .collect::<Result<Vec<_>, _>>()?;
        let mut graph = load(path)?;
        // the fingerprint matched, so these are the inverses of exactly
        // this document's last change and the graph has no reason to
        // refuse them
        graph
            .apply_all(commands)
            .map_err(|(error, _)| StoreError::Graph(error))?;
        let text = serde_json::to_string_pretty(&Document::from_graph(&graph))?;
        write_atomically(path, &text)?;
        text
    };

    // The entry below recorded the text it left behind, and the text just
    // written is that state again but not always byte for byte: the id
    // counters stay where they were. Its inverses still fit, so it is
    // re-stamped with what is really on disk now.
    if let Some(below) = entries.last_mut() {
        below.after = history::fingerprint(&restored);
    }
    history::write(path, &entries)?;
    Ok(())
}

/// Takes back one whole copy from the stack left by a version before 0.2.10.
fn undo_copy(path: &Path) -> Result<(), StoreError> {
    let newest = undo_path(path, 1);
    if !newest.exists() {
        return Err(StoreError::NothingToUndo);
    }
    // read before the rename, which is what throws this document away
    let spent = read_document(path).map(|document| document.spent());
    fs::rename(&newest, path)?;
    if let Some(spent) = spent {
        keep_spent(path, spent)?;
    }

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

/// Carries spent ids forward into a document brought back by undo.
///
/// Undo puts back a whole older document, and with it an older id counter.
/// Without this, undoing an addition would free its id for the next one, and
/// that id may already have been seen elsewhere: by another device once sync
/// exists, and by a script or an agent that remembered it today.
///
/// A document that does not parse is left exactly as it is, because undo is
/// also how a broken document gets recovered by hand.
fn keep_spent(path: &Path, (node, area): (u64, u64)) -> Result<(), StoreError> {
    let Some(mut document) = read_document(path) else {
        return Ok(());
    };
    let (had_node, had_area) = document.spent();
    if had_node >= node && had_area >= area {
        return Ok(());
    }
    document.next_node = had_node.max(node);
    document.next_area = had_area.max(area);
    write_atomically(path, &serde_json::to_string_pretty(&document)?)?;
    Ok(())
}

/// The document at `path`, or `None` if it is missing or does not parse.
fn read_document(path: &Path) -> Option<Document> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// How many changes could still be taken back.
///
/// A log that no longer fits the document counts as empty, since undo would
/// only refuse it.
#[must_use]
pub fn undo_depth(path: &Path) -> usize {
    let path = &real_path(path);
    // best effort: failing to tidy only means counting the new place as empty
    let _ = tidy_loose_undo(path);
    let entries = history::read(path);
    let logged = match entries.last() {
        Some(top)
            if fs::read_to_string(path)
                .is_ok_and(|current| history::fingerprint(&current) == top.after) =>
        {
            entries.len()
        }
        _ => 0,
    };
    let copies = (1..=UNDO_DEPTH)
        .take_while(|slot| undo_path(path, *slot).exists())
        .count();
    logged + copies
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

/// `.branchy/undo/graph.json.1` for `graph.json`: where versions up to 0.2.9
/// kept their stack of whole copies, named after the document so two
/// documents in one folder keep separate stacks.
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
