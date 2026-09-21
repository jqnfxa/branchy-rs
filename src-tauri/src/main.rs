//! The Branchy desktop shell.
//!
//! Three commands and a window. All the thinking is in `branchy-core`; all the
//! reading and writing is in `branchy-app`. This binary only decides where the
//! document lives, keeps it behind a lock so two windows cannot write over each
//! other, and hands the frontend the same values `branchy snapshot` prints.

// Tauri opens its own window; a console behind it on Windows helps nobody.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;

use branchy_app::{Outcome, Snapshot, store};
use branchy_core::Graph;
use tauri::Manager;

/// The open document.
///
/// Only where it lives, not a copy of the graph. The terminal writes to the
/// same file, and a copy read when the window opened would be saved straight
/// over whatever the terminal had done since. So every command reads the file
/// afresh; for a planning graph that costs well under a millisecond.
struct Session {
    path: PathBuf,
}

impl Session {
    fn load(&self) -> Result<Graph, String> {
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

/// Shared across commands. Tauri runs them on a thread pool, so the lock is not
/// decoration: without it two quick clicks could interleave a read and a write.
struct Open(Mutex<Session>);

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
    use super::{Open, Outcome, Snapshot};
    use tauri::State;

    const BUSY: &str = "the document is busy";

    #[tauri::command]
    pub fn snapshot(open: State<'_, Open>) -> Result<Snapshot, String> {
        open.0.lock().map_err(|_| BUSY)?.snapshot()
    }

    /// Runs one command line in the shared grammar and saves the result.
    #[tauri::command]
    pub fn execute(line: String, open: State<'_, Open>) -> Result<Outcome, String> {
        open.0.lock().map_err(|_| BUSY)?.execute_all(&[line])
    }

    #[tauri::command]
    pub fn execute_all(lines: Vec<String>, open: State<'_, Open>) -> Result<Outcome, String> {
        open.0.lock().map_err(|_| BUSY)?.execute_all(&lines)
    }

    #[tauri::command]
    pub fn undo(open: State<'_, Open>) -> Result<Outcome, String> {
        open.0.lock().map_err(|_| BUSY)?.undo()
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // store::default_path already honours BRANCHY_FILE, so the shell
            // and the terminal always open the same document. It is not read
            // here: a document that will not load is reported in the window by
            // the first snapshot, rather than the window never opening at all.
            let path = store::default_path()?;
            app.manage(Open(Mutex::new(Session { path })));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::execute,
            commands::execute_all,
            commands::undo
        ])
        .run(tauri::generate_context!())
        .expect("the Branchy window could not start");
}

#[cfg(test)]
mod tests {
    use super::{Session, store};
    use std::path::{Path, PathBuf};

    /// A throwaway folder holding one document, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("branchy-shell-test-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch dir");
            Self(dir)
        }

        fn file(&self) -> PathBuf {
            self.0.join("graph.json")
        }

        fn session(&self) -> Session {
            Session { path: self.file() }
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
}
