//! The Branchy desktop shell.
//!
//! A window and a handful of commands. All the thinking is in `branchy-core`;
//! all the reading and writing, including which vaults exist, is in
//! `branchy-app`. This binary only keeps track of which vault the window has
//! open, keeps that behind a lock so two quick clicks cannot write over each
//! other, and hands the frontend the same values `branchy snapshot` prints.

// Tauri opens its own window; a console behind it on Windows helps nobody.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use branchy_app::vault::{DOCUMENT, Entry};
use branchy_app::{Outcome, Snapshot, StoreError, Vault, Vaults, store};
use branchy_core::Graph;
use serde::Serialize;
use tauri::Manager;

/// The open document.
///
/// Only where it lives, not a copy of the graph. The terminal writes to the
/// same file, and a copy read when the window opened would be saved straight
/// over whatever the terminal had done since. So every command reads the file
/// afresh; for a planning graph that costs well under a millisecond.
struct Session {
    path: PathBuf,
    /// Opened as a vault, so its folder has to still be there. A file named
    /// by `BRANCHY_FILE` may create its folder; a vault that has been deleted
    /// must not quietly come back on the next click.
    vault: bool,
}

impl Session {
    fn in_vault(vault: &Vault) -> Self {
        Self {
            path: vault.document(),
            vault: true,
        }
    }

    fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }

    fn load(&self) -> Result<Graph, String> {
        if self.vault && !self.dir().is_dir() {
            return Err(StoreError::VaultMissing(self.dir().to_path_buf()).to_string());
        }
        store::load(&self.path).map_err(|e| e.to_string())
    }

    /// Everything the interface needs to draw the graph.
    fn snapshot(&self) -> Result<Snapshot, String> {
        Ok(Snapshot::of(&self.load()?))
    }

    /// Runs several lines as one change.
    ///
    /// Either all of them take effect or none do. An editing form produces a
    /// handful of commands at once — a rename, a new prerequisite, a priority
    /// — and leaving half of them applied because the fourth was refused would
    /// be worse than refusing the lot. Nothing is saved until every line has
    /// applied, so a refusal simply drops the graph they were applied to.
    ///
    /// Nothing is validated here either way: the graph is the authority on what
    /// it accepts, so each line goes to the parser and any refusal comes
    /// straight back.
    fn execute_all(&self, lines: &[String]) -> Result<Outcome, String> {
        let mut graph = self.load()?;
        let mut node = None;
        let mut area = None;

        for line in lines {
            let command = branchy_core::parse(&graph, line).map_err(|e| e.to_string())?;
            let applied = graph.apply(command).map_err(|e| describe(&graph, &e))?;
            node = applied.node.or(node);
            area = applied.area.or(area);
        }

        store::save(&self.path, &graph).map_err(|e| e.to_string())?;
        Ok(Outcome {
            node: node.map(|id| id.to_string()),
            area: area.map(|id| id.to_string()),
            snapshot: Snapshot::of(&graph),
        })
    }

    /// Takes back the last change.
    fn undo(&self) -> Result<Outcome, String> {
        store::undo(&self.path).map_err(|e| e.to_string())?;
        Ok(Outcome {
            node: None,
            area: None,
            snapshot: self.snapshot()?,
        })
    }
}

/// What the vault screen and the switcher need.
#[derive(Debug, Serialize)]
struct VaultsView {
    /// The vault the window is showing, if any. `None` means the vault screen.
    open: Option<Entry>,
    /// The recent vaults, newest first.
    recent: Vec<Entry>,
    /// Whether the window skips the vault screen and reopens the last vault.
    open_last: bool,
    /// The window was pointed at one file by `BRANCHY_FILE` rather than
    /// opened on a vault from the list.
    pinned: bool,
}

/// Which vault the window has open, and the list it chose from.
struct Desk {
    list: Vaults,
    open: Option<Session>,
    pinned: bool,
}

impl Desk {
    /// How the window starts.
    ///
    /// `BRANCHY_FILE` opens that file directly and leaves the list alone, which
    /// is what makes the shell drivable against a fixture. Otherwise the last
    /// vault reopens if the list says to and it is still there, and in every
    /// other case the window starts on the vault screen.
    fn start() -> Result<Self, StoreError> {
        let list = Vaults::user()?;
        if let Some(file) = std::env::var_os("BRANCHY_FILE") {
            return Ok(Self {
                list,
                open: Some(Session {
                    path: PathBuf::from(file),
                    vault: false,
                }),
                pinned: true,
            });
        }
        Ok(Self::resume(list))
    }

    fn resume(list: Vaults) -> Self {
        let open = match list.current() {
            Some(dir) if list.open_last() && dir.is_dir() => Some(Session {
                path: dir.join(DOCUMENT),
                vault: true,
            }),
            _ => None,
        };
        Self {
            list,
            open,
            pinned: false,
        }
    }

    fn session(&self) -> Result<&Session, String> {
        self.open
            .as_ref()
            .ok_or_else(|| "no vault is open".to_string())
    }

    /// The list as it is on disk now: the terminal may have changed it.
    fn view(&mut self) -> Result<VaultsView, StoreError> {
        self.list = self.list.reload()?;
        Ok(VaultsView {
            open: self.open.as_ref().map(|session| Entry::of(session.dir())),
            recent: self.list.entries(),
            open_last: self.list.open_last(),
            pinned: self.pinned,
        })
    }

    fn create(&mut self, parent: &Path, name: &str) -> Result<VaultsView, StoreError> {
        let vault = Vault::create(parent, name)?;
        self.enter(&vault)
    }

    fn open(&mut self, dir: &Path) -> Result<VaultsView, StoreError> {
        if !dir.exists() {
            return Err(StoreError::VaultMissing(dir.to_path_buf()));
        }
        let vault = Vault::open(dir)?;
        self.enter(&vault)
    }

    /// Opens a vault and puts it at the front of the list, which also makes
    /// it the one the terminal works in.
    fn enter(&mut self, vault: &Vault) -> Result<VaultsView, StoreError> {
        let mut list = self.list.reload()?;
        list.opened(vault);
        list.save()?;
        self.list = list;
        self.open = Some(Session::in_vault(vault));
        self.pinned = false;
        self.view()
    }

    fn close(&mut self) -> Result<VaultsView, StoreError> {
        self.open = None;
        self.pinned = false;
        self.view()
    }

    /// Takes a vault off the list. The folder is never touched.
    fn forget(&mut self, given: &str) -> Result<VaultsView, StoreError> {
        let mut list = self.list.reload()?;
        let dir = list.forget(given)?;
        list.save()?;
        self.list = list;
        if self
            .open
            .as_ref()
            .is_some_and(|session| session.dir() == dir)
        {
            self.open = None;
        }
        self.view()
    }

    fn set_open_last(&mut self, on: bool) -> Result<VaultsView, StoreError> {
        let mut list = self.list.reload()?;
        list.set_open_last(on);
        list.save()?;
        self.list = list;
        self.view()
    }
}

/// Shared across commands. Tauri runs them on a thread pool, so the lock is not
/// decoration: without it two quick clicks could interleave a read and a write.
struct Shared(Mutex<Desk>);

/// Graph errors carry ids; a person wants names. The frontend has the snapshot
/// and could resolve them, but the refusal has to read well even in a log.
fn describe(graph: &Graph, error: &branchy_core::Error) -> String {
    let name = |id: branchy_core::NodeId| {
        graph
            .node(id)
            .map_or_else(|| id.to_string(), |node| node.name.clone())
    };
    match error {
        branchy_core::Error::WouldCycle { path, .. } => {
            let loop_text: Vec<String> = path.iter().map(|id| name(*id)).collect();
            format!(
                "Refused: that would create a cycle. {}",
                loop_text.join(" \u{2192} ")
            )
        }
        branchy_core::Error::NotAPrerequisite {
            dependent,
            prerequisite,
        } => format!("{} does not need {}", name(*dependent), name(*prerequisite)),
        other => other.to_string(),
    }
}

// `State<'_, T>` by value, and an owned value for a deserialized argument, are
// what `#[tauri::command]` requires. Clippy is right in general and wrong here:
// the signature is not ours to choose.
#[allow(clippy::needless_pass_by_value)]
mod commands {
    use std::path::Path;
    use std::sync::MutexGuard;

    use super::{Desk, Outcome, Shared, Snapshot, VaultsView};
    use tauri::{AppHandle, Manager, State};
    use tauri_plugin_dialog::DialogExt;

    fn desk<'a>(shared: &'a State<'_, Shared>) -> Result<MutexGuard<'a, Desk>, String> {
        shared
            .0
            .lock()
            .map_err(|_| "the document is busy".to_string())
    }

    #[tauri::command]
    pub fn snapshot(shared: State<'_, Shared>) -> Result<Snapshot, String> {
        desk(&shared)?.session()?.snapshot()
    }

    /// Runs one command line in the shared grammar and saves the result.
    #[tauri::command]
    pub fn execute(line: String, shared: State<'_, Shared>) -> Result<Outcome, String> {
        desk(&shared)?.session()?.execute_all(&[line])
    }

    #[tauri::command]
    pub fn execute_all(lines: Vec<String>, shared: State<'_, Shared>) -> Result<Outcome, String> {
        desk(&shared)?.session()?.execute_all(&lines)
    }

    #[tauri::command]
    pub fn undo(shared: State<'_, Shared>) -> Result<Outcome, String> {
        desk(&shared)?.session()?.undo()
    }

    #[tauri::command]
    pub fn vaults(shared: State<'_, Shared>) -> Result<VaultsView, String> {
        desk(&shared)?.view().map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn vault_create(
        name: String,
        parent: String,
        shared: State<'_, Shared>,
    ) -> Result<VaultsView, String> {
        desk(&shared)?
            .create(Path::new(&parent), &name)
            .map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn vault_open(path: String, shared: State<'_, Shared>) -> Result<VaultsView, String> {
        desk(&shared)?
            .open(Path::new(&path))
            .map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn vault_close(shared: State<'_, Shared>) -> Result<VaultsView, String> {
        desk(&shared)?.close().map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn vault_forget(path: String, shared: State<'_, Shared>) -> Result<VaultsView, String> {
        desk(&shared)?.forget(&path).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn vault_open_last(on: bool, shared: State<'_, Shared>) -> Result<VaultsView, String> {
        desk(&shared)?.set_open_last(on).map_err(|e| e.to_string())
    }

    /// Asks for a folder with the system's own picker.
    ///
    /// `async` runs it off the main thread, which the dialog needs free while
    /// it waits for an answer. It starts in Documents, where vaults usually
    /// go, and returns nothing if the person cancels.
    #[tauri::command(async)]
    pub fn pick_folder(title: String, app: AppHandle) -> Option<String> {
        let mut dialog = app.dialog().file().set_title(title);
        if let Ok(start) = app.path().document_dir().or_else(|_| app.path().home_dir()) {
            dialog = dialog.set_directory(start);
        }
        dialog
            .blocking_pick_folder()
            .and_then(|folder| folder.into_path().ok())
            .map(|path| path.display().to_string())
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Nothing is read here. A document that will not load is reported
            // in the window by the first snapshot, rather than the window never
            // opening at all.
            app.manage(Shared(Mutex::new(Desk::start()?)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::execute,
            commands::execute_all,
            commands::undo,
            commands::vaults,
            commands::vault_create,
            commands::vault_open,
            commands::vault_close,
            commands::vault_forget,
            commands::vault_open_last,
            commands::pick_folder
        ])
        .run(tauri::generate_context!())
        .expect("the Branchy window could not start");
}

#[cfg(test)]
mod tests {
    use super::{Desk, Session, Vault, Vaults, store};
    use std::path::{Path, PathBuf};

    /// A throwaway folder, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("branchy-shell-test-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch dir");
            Self(Vault::open(&dir).expect("opens").dir().to_path_buf())
        }

        fn dir(&self) -> &Path {
            &self.0
        }

        fn file(&self) -> PathBuf {
            self.0.join("graph.json")
        }

        fn session(&self) -> Session {
            Session {
                path: self.file(),
                vault: true,
            }
        }

        /// A desk over a vault list of its own, never the user's.
        fn desk(&self) -> Desk {
            Desk::resume(self.list())
        }

        fn list(&self) -> Vaults {
            Vaults::load(&self.0.join("vaults.json")).expect("list loads")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// What `branchy <line>` does: load, apply, save.
    fn terminal(file: &Path, line: &str) {
        let mut graph = store::load(file).expect("loads");
        let command = branchy_core::parse(&graph, line).expect("parses");
        graph.apply(command).expect("applies");
        store::save(file, &graph).expect("saves");
    }

    fn names(file: &Path) -> Vec<String> {
        store::load(file)
            .expect("loads")
            .nodes()
            .map(|(_, node)| node.name.clone())
            .collect()
    }

    fn lines(lines: &[&str]) -> Vec<String> {
        lines.iter().map(ToString::to_string).collect()
    }

    // ── the open document ───────────────────────────────────────────────

    #[test]
    fn a_change_made_in_the_terminal_survives_the_next_click() {
        let scratch = Scratch::new("terminal");
        terminal(&scratch.file(), "area Work");
        let session = scratch.session();
        session.snapshot().expect("the window opens");

        terminal(&scratch.file(), "add Typed in Work");
        session
            .execute_all(&lines(&["add Clicked in Work"]))
            .expect("the click applies");

        let names = names(&scratch.file());
        assert!(names.contains(&"Typed".to_string()), "{names:?}");
        assert!(names.contains(&"Clicked".to_string()), "{names:?}");
    }

    #[test]
    fn the_window_sees_a_change_made_in_the_terminal() {
        let scratch = Scratch::new("sees");
        terminal(&scratch.file(), "area Work");
        let session = scratch.session();
        assert!(session.snapshot().expect("opens").nodes.is_empty());

        terminal(&scratch.file(), "add Typed in Work");
        let snapshot = session.snapshot().expect("refreshes");
        assert_eq!(snapshot.nodes.len(), 1);
    }

    #[test]
    fn a_refused_line_leaves_the_document_as_it_was() {
        let scratch = Scratch::new("refused");
        terminal(&scratch.file(), "area Work");
        let before = std::fs::read_to_string(scratch.file()).expect("reads");
        let depth = store::undo_depth(&scratch.file());

        let refused = scratch
            .session()
            .execute_all(&lines(&["add Half in Work", "link Half after Nowhere"]));

        assert!(refused.is_err());
        let after = std::fs::read_to_string(scratch.file()).expect("reads");
        assert_eq!(before, after, "nothing of the refused change was saved");
        assert_eq!(store::undo_depth(&scratch.file()), depth);
    }

    #[test]
    fn undo_takes_back_the_window_change_and_keeps_the_terminal_one() {
        let scratch = Scratch::new("undo");
        terminal(&scratch.file(), "area Work");
        terminal(&scratch.file(), "add Typed in Work");
        let session = scratch.session();
        session
            .execute_all(&lines(&["add Clicked in Work"]))
            .expect("applies");

        let outcome = session.undo().expect("undoes");
        let shown: Vec<String> = outcome
            .snapshot
            .nodes
            .iter()
            .map(|n| n.name.clone())
            .collect();
        assert_eq!(shown, vec!["Typed".to_string()]);
    }

    // ── vaults ──────────────────────────────────────────────────────────

    #[test]
    fn with_nothing_to_reopen_the_window_starts_on_the_vault_screen() {
        let scratch = Scratch::new("start-empty");
        let mut desk = scratch.desk();
        assert!(desk.open.is_none());
        assert!(desk.session().is_err(), "no graph commands without a vault");
        assert!(desk.view().expect("view").open.is_none());
    }

    #[test]
    fn a_created_vault_opens_and_joins_the_list() {
        let scratch = Scratch::new("create");
        let mut desk = scratch.desk();
        let view = desk.create(scratch.dir(), "Work").expect("created");

        assert_eq!(view.open.expect("open").name, "Work");
        assert_eq!(view.recent.len(), 1);
        desk.session()
            .expect("open")
            .execute_all(&lines(&["area Jobs"]))
            .expect("applies");
        assert!(scratch.dir().join("Work/graph.json").is_file());
    }

    #[test]
    fn the_last_vault_reopens_only_when_asked_to() {
        let scratch = Scratch::new("reopen");
        let mut desk = scratch.desk();
        desk.create(scratch.dir(), "Work").expect("created");

        assert!(
            scratch.desk().open.is_none(),
            "by default the window offers the list"
        );

        desk.set_open_last(true).expect("saves");
        let reopened = scratch.desk();
        assert_eq!(
            reopened.open.as_ref().expect("reopened").dir(),
            scratch.dir().join("Work")
        );
    }

    #[test]
    fn a_vault_that_is_gone_is_not_reopened_on_start() {
        let scratch = Scratch::new("reopen-gone");
        let mut desk = scratch.desk();
        desk.create(scratch.dir(), "Work").expect("created");
        desk.set_open_last(true).expect("saves");
        std::fs::remove_dir_all(scratch.dir().join("Work")).expect("delete");

        let mut start = scratch.desk();
        assert!(start.open.is_none());
        assert!(start.view().expect("view").recent[0].missing);
    }

    #[test]
    fn opening_a_vault_makes_it_the_terminals_too() {
        let scratch = Scratch::new("follow");
        let mut desk = scratch.desk();
        desk.create(scratch.dir(), "Work").expect("created");
        desk.create(scratch.dir(), "Life").expect("created");

        desk.open(&scratch.dir().join("Work")).expect("opens");

        // a fresh read of the list is what `branchy` does
        assert_eq!(
            scratch.list().current(),
            Some(scratch.dir().join("Work").as_path())
        );
    }

    #[test]
    fn closing_returns_to_the_vault_screen_and_keeps_the_list() {
        let scratch = Scratch::new("close");
        let mut desk = scratch.desk();
        desk.create(scratch.dir(), "Work").expect("created");

        let view = desk.close().expect("closes");
        assert!(view.open.is_none());
        assert_eq!(view.recent.len(), 1);
        assert!(desk.session().is_err());
    }

    #[test]
    fn forgetting_the_open_vault_closes_it_and_leaves_the_folder() {
        let scratch = Scratch::new("forget");
        let mut desk = scratch.desk();
        let view = desk.create(scratch.dir(), "Work").expect("created");
        let path = view.open.expect("open").path;

        let view = desk.forget(&path).expect("forgets");
        assert!(view.open.is_none());
        assert!(view.recent.is_empty());
        assert!(scratch.dir().join("Work/graph.json").is_file());
    }

    #[test]
    fn a_list_changed_by_the_terminal_is_not_saved_over() {
        let scratch = Scratch::new("list-race");
        let mut desk = scratch.desk();
        desk.create(scratch.dir(), "Work").expect("created");

        // the terminal opens another vault while the window is up
        let mut terminal_list = scratch.list();
        terminal_list.opened(&Vault::create(scratch.dir(), "Typed").expect("created"));
        terminal_list.save().expect("saves");

        desk.set_open_last(true).expect("saves");
        let names: Vec<String> = scratch
            .list()
            .entries()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["Typed", "Work"]);
    }

    #[test]
    fn a_vault_deleted_while_open_is_reported_not_recreated() {
        let scratch = Scratch::new("deleted-open");
        let mut desk = scratch.desk();
        desk.create(scratch.dir(), "Work").expect("created");
        std::fs::remove_dir_all(scratch.dir().join("Work")).expect("delete");

        let refused = desk
            .session()
            .expect("still open")
            .execute_all(&lines(&["area Jobs"]));
        assert!(refused.expect_err("refused").contains("is gone"));
        assert!(!scratch.dir().join("Work").exists());
    }

    #[test]
    fn a_missing_folder_cannot_be_opened() {
        let scratch = Scratch::new("open-missing");
        let mut desk = scratch.desk();
        assert!(desk.open(&scratch.dir().join("Nowhere")).is_err());
        assert!(desk.open.is_none());
    }
}
