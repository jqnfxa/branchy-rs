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

/// The open document: the graph, and where it came from.
struct Session {
    graph: Graph,
    path: PathBuf,
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

// `State<'_, T>` by value, and an owned String for a deserialized argument,
// are what `#[tauri::command]` requires. Clippy is right in general and wrong
// here: the signature is not ours to choose.
#[allow(clippy::needless_pass_by_value)]
mod commands {
    use super::{Open, Outcome, Snapshot, describe, store};
    use tauri::State;

    /// Everything the interface needs to draw the graph.
    #[tauri::command]
    pub fn snapshot(open: State<'_, Open>) -> Result<Snapshot, String> {
        let session = open.0.lock().map_err(|_| "the document is busy")?;
        Ok(Snapshot::of(&session.graph))
    }

    /// Runs one command line in the shared grammar and saves the result.
    #[tauri::command]
    pub fn execute(line: String, open: State<'_, Open>) -> Result<Outcome, String> {
        execute_all(vec![line], open)
    }

    /// Runs several lines as one change.
    ///
    /// Either all of them take effect or none do. An editing form produces a
    /// handful of commands at once — a rename, a new prerequisite, a priority
    /// — and leaving half of them applied because the fourth was refused would
    /// be worse than refusing the lot.
    ///
    /// Nothing is validated here either way: the graph is the authority on what
    /// it accepts, so each line goes to the parser and any refusal comes
    /// straight back.
    #[tauri::command]
    pub fn execute_all(lines: Vec<String>, open: State<'_, Open>) -> Result<Outcome, String> {
        let mut session = open.0.lock().map_err(|_| "the document is busy")?;

        let mut node = None;
        let mut area = None;
        let mut undo: Vec<branchy_core::Command> = Vec::new();

        for line in lines {
            let parsed = match branchy_core::parse(&session.graph, &line) {
                Ok(command) => command,
                Err(error) => {
                    return Err(rollback(&mut session.graph, undo, &error.to_string()));
                }
            };
            match session.graph.apply(parsed) {
                Ok(applied) => {
                    node = applied.node.or(node);
                    area = applied.area.or(area);
                    // later undos have to run first
                    let mut next = applied.undo;
                    next.extend(undo);
                    undo = next;
                }
                Err(error) => {
                    let message = describe(&session.graph, &error);
                    return Err(rollback(&mut session.graph, undo, &message));
                }
            }
        }

        store::save(&session.path, &session.graph).map_err(|e| e.to_string())?;
        Ok(Outcome {
            node: node.map(|id| id.to_string()),
            area: area.map(|id| id.to_string()),
            snapshot: Snapshot::of(&session.graph),
        })
    }

    /// Puts back whatever already applied, and returns the original complaint.
    ///
    /// Nothing was saved, so even a failed rollback leaves the document on disk
    /// as it was; only the copy in memory would be wrong, and the next command
    /// would be refused on a graph the user did not expect.
    fn rollback(
        graph: &mut branchy_core::Graph,
        undo: Vec<branchy_core::Command>,
        why: &str,
    ) -> String {
        if graph.apply_all(undo).is_err() {
            return format!("{why} (and the partial change could not be undone)");
        }
        why.to_string()
    }

    /// Takes back the last change.
    #[tauri::command]
    pub fn undo(open: State<'_, Open>) -> Result<Outcome, String> {
        let mut session = open.0.lock().map_err(|_| "the document is busy")?;
        store::undo(&session.path).map_err(|e| e.to_string())?;
        session.graph = store::load(&session.path).map_err(|e| e.to_string())?;
        Ok(Outcome {
            node: None,
            area: None,
            snapshot: Snapshot::of(&session.graph),
        })
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // store::default_path already honours BRANCHY_FILE, so the shell
            // and the terminal always open the same document.
            let path = store::default_path()?;
            let graph = store::load(&path)?;
            app.manage(Open(Mutex::new(Session { graph, path })));
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
