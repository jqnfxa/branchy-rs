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

use branchy_app::{Snapshot, store};
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
    use super::{Open, Snapshot, describe, store};
    use tauri::State;

    /// Everything the interface needs to draw the graph.
    #[tauri::command]
    pub fn snapshot(open: State<'_, Open>) -> Result<Snapshot, String> {
        let session = open.0.lock().map_err(|_| "the document is busy")?;
        Ok(Snapshot::of(&session.graph))
    }

    /// Runs one command line in the shared grammar and saves the result.
    ///
    /// The graph is the authority on what it will accept, so nothing is validated
    /// here: the line goes to the parser and any refusal comes straight back.
    #[tauri::command]
    pub fn execute(line: String, open: State<'_, Open>) -> Result<Snapshot, String> {
        let mut session = open.0.lock().map_err(|_| "the document is busy")?;

        let command = branchy_core::parse(&session.graph, &line).map_err(|e| e.to_string())?;
        session
            .graph
            .apply(command)
            .map_err(|e| describe(&session.graph, &e))?;
        store::save(&session.path, &session.graph).map_err(|e| e.to_string())?;

        Ok(Snapshot::of(&session.graph))
    }

    /// Puts the document back as it was before the last change.
    #[tauri::command]
    pub fn undo(open: State<'_, Open>) -> Result<Snapshot, String> {
        let mut session = open.0.lock().map_err(|_| "the document is busy")?;
        store::undo(&session.path).map_err(|e| e.to_string())?;
        session.graph = store::load(&session.path).map_err(|e| e.to_string())?;
        Ok(Snapshot::of(&session.graph))
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
            commands::undo
        ])
        .run(tauri::generate_context!())
        .expect("the Branchy window could not start");
}
