//! The undo log: changes taken back by their inverse commands.

use std::fs;
use std::path::{Path, PathBuf};

use branchy_app::store::{self, StoreError};
use branchy_core::{Graph, NodeId, Stage};

/// A throwaway vault folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("branchy-history-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }

    fn document(&self) -> PathBuf {
        self.0.join("graph.json")
    }

    fn undo_dir(&self) -> PathBuf {
        self.0.join(".branchy").join("undo")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Applies lines the way every front end does and saves with their inverses.
fn edit(path: &Path, lines: &[&str]) {
    let graph = store::load(path).expect("loads");
    let edit = branchy_app::apply_lines(graph, lines).expect("lines apply");
    store::save_change(path, &edit.graph, &edit.undo).expect("saves");
}

fn names(path: &Path) -> Vec<String> {
    store::load(path)
        .expect("loads")
        .nodes()
        .map(|(_, node)| node.name.clone())
        .collect()
}

/// A vault holding a direction and one task.
fn started(tag: &str) -> Scratch {
    let scratch = Scratch::new(tag);
    store::save(&scratch.document(), &Graph::new()).expect("saves");
    edit(&scratch.document(), &["area Work", "add One"]);
    scratch
}

#[test]
fn a_change_is_taken_back_by_its_inverse() {
    let scratch = started("inverse");
    edit(&scratch.document(), &["add Two", "rename One Uno"]);
    assert_eq!(names(&scratch.document()), ["Uno", "Two"]);
    store::undo(&scratch.document()).expect("undoes");
    assert_eq!(names(&scratch.document()), ["One"]);
}

#[test]
fn the_log_costs_far_less_than_a_copy_of_the_document() {
    let scratch = started("small");
    let note = "a long note ".repeat(10_000);
    edit(&scratch.document(), &[&format!("note One \"{note}\"")]);
    for i in 0..10 {
        edit(&scratch.document(), &[&format!("add \"Task {i}\"")]);
    }
    let document = fs::metadata(scratch.document()).expect("exists").len();
    let log = fs::metadata(scratch.undo_dir().join("graph.json.log"))
        .expect("exists")
        .len();
    // the note's own change carries the old, empty note, and every later
    // change a few hundred bytes; ten whole copies would be ten documents
    assert!(log * 10 < document, "log {log} bytes, document {document}");
}

#[test]
fn undo_is_a_stack_that_runs_out() {
    let scratch = started("stack");
    edit(&scratch.document(), &["add Two"]);
    edit(&scratch.document(), &["add Three"]);
    store::undo(&scratch.document()).expect("undoes");
    assert_eq!(names(&scratch.document()), ["One", "Two"]);
    store::undo(&scratch.document()).expect("undoes");
    assert_eq!(names(&scratch.document()), ["One"]);
    // the first save of a new document had nothing before it to keep
    store::undo(&scratch.document()).expect("undoes the first edit");
    assert!(names(&scratch.document()).is_empty());
    assert!(matches!(
        store::undo(&scratch.document()),
        Err(StoreError::NothingToUndo)
    ));
}

#[test]
fn a_removal_comes_back_with_the_edges_that_pointed_at_it() {
    let scratch = started("restore");
    edit(
        &scratch.document(),
        &["add Two after One", "due One 2027-01-31"],
    );
    edit(&scratch.document(), &["rm One"]);
    store::undo(&scratch.document()).expect("undoes");
    let graph = store::load(&scratch.document()).expect("loads");
    let one = branchy_core::resolve_node(&graph, "One").expect("back");
    let two = branchy_core::resolve_node(&graph, "Two").expect("there");
    assert!(graph.node(two).expect("there").prereqs.contains(&one));
    assert_eq!(
        graph.node(one).expect("back").due.map(|d| d.to_string()),
        Some("2027-01-31".to_string())
    );
}

#[test]
fn only_the_newest_twenty_changes_are_kept() {
    let scratch = started("depth");
    for i in 0..25 {
        edit(&scratch.document(), &[&format!("add \"Task {i}\"")]);
    }
    assert_eq!(store::undo_depth(&scratch.document()), 20);
}

#[test]
fn a_document_changed_outside_branchy_is_not_undone_into() {
    let scratch = started("stale");
    edit(&scratch.document(), &["add Two"]);
    // an older build, or a hand edit, writes something else
    let foreign = fs::read_to_string(scratch.document())
        .expect("reads")
        .replace("Two", "Edited by hand");
    fs::write(scratch.document(), &foreign).expect("writes");

    assert_eq!(store::undo_depth(&scratch.document()), 0);
    match store::undo(&scratch.document()) {
        Err(StoreError::UndoStale(previous)) => {
            let kept = Path::new(".branchy").join("undo").join("graph.json.prev");
            assert!(previous.ends_with(&kept), "{}", previous.display());
            assert!(
                !previous.to_string_lossy().contains(r"\\?\"),
                "{}",
                previous.display()
            );
            assert!(fs::read_to_string(previous).expect("kept").contains("One"));
        }
        other => panic!("expected a stale history, got {other:?}"),
    }
    assert_eq!(
        fs::read_to_string(scratch.document()).expect("reads"),
        foreign,
        "the document is left exactly as it was"
    );
    assert!(matches!(
        store::undo(&scratch.document()),
        Err(StoreError::NothingToUndo)
    ));
}

#[test]
fn a_stack_from_before_the_log_is_taken_back_after_it() {
    let scratch = Scratch::new("legacy");
    let document = scratch.document();
    edit_new(&document, &["area Work", "add Old"]);
    // what 0.2.9 left: a whole copy in slot 1, here of an empty graph
    fs::create_dir_all(scratch.undo_dir()).expect("dir");
    let empty = scratch.0.join("empty.json");
    store::save(&empty, &Graph::new()).expect("saves");
    fs::rename(&empty, scratch.undo_dir().join("graph.json.1")).expect("moves");

    edit(&document, &["add New"]);
    assert_eq!(store::undo_depth(&document), 2);
    store::undo(&document).expect("undoes the logged change");
    assert_eq!(names(&document), ["Old"]);
    store::undo(&document).expect("undoes the old copy");
    assert!(names(&document).is_empty());
}

/// Creates a document by edits without any history before it.
fn edit_new(path: &Path, lines: &[&str]) {
    let edit = branchy_app::apply_lines(Graph::new(), lines).expect("lines apply");
    store::save_change(path, &edit.graph, &edit.undo).expect("saves");
}

#[test]
fn a_direct_save_is_undone_from_its_copy() {
    let scratch = started("direct");
    let mut graph = store::load(&scratch.document()).expect("loads");
    graph
        .set_name(branchy_core::NodeId::new(0), "Renamed")
        .expect("exists");
    store::save(&scratch.document(), &graph).expect("saves");
    store::undo(&scratch.document()).expect("undoes");
    assert_eq!(names(&scratch.document()), ["One"]);
}

// ── format 2: stages ────────────────────────────────────────────────────

#[test]
fn a_format_one_document_loads_with_done_as_a_stage_and_saves_as_format_two() {
    let scratch = Scratch::new("format-one");
    fs::write(
        scratch.document(),
        r##"{"version":1,"next_node":2,"next_area":1,
            "areas":[{"id":0,"name":"Work","color":"#4fd1c5"}],
            "nodes":[{"id":0,"name":"Finished","area":0,"priority":5,"done":true},
                     {"id":1,"name":"Open","area":0,"priority":5,"done":false}]}"##,
    )
    .expect("write");
    let graph = store::load(&scratch.document()).expect("format 1 still reads");
    let stages: Vec<Stage> = graph.nodes().map(|(_, node)| node.stage).collect();
    assert_eq!(stages, [Stage::Done, Stage::Backlog]);

    edit(&scratch.document(), &["start Open"]);
    let text = fs::read_to_string(scratch.document()).expect("reads");
    assert!(text.contains(r#""version": 2"#), "{text}");
    assert!(text.contains(r#""stage": "doing""#), "{text}");
    assert!(
        !text.contains(r#""done":"#),
        "format 2 does not write done: {text}"
    );
}

#[test]
fn a_stage_this_build_does_not_know_is_reported() {
    let scratch = Scratch::new("format-unknown-stage");
    fs::write(
        scratch.document(),
        r##"{"version":2,"areas":[{"id":0,"name":"Work","color":"#fff"}],
            "nodes":[{"id":0,"name":"Odd","area":0,"priority":5,"stage":"blocked"}]}"##,
    )
    .expect("write");
    let error = store::load(&scratch.document()).unwrap_err();
    assert!(error.to_string().contains("blocked"), "{error}");
}

#[test]
fn a_stage_change_is_undone() {
    let scratch = started("stage-undo");
    edit(&scratch.document(), &["start One"]);
    edit(&scratch.document(), &["review One"]);
    store::undo(&scratch.document()).expect("undoes");
    let graph = store::load(&scratch.document()).expect("loads");
    assert_eq!(
        graph.node(NodeId::new(0)).expect("exists").stage,
        Stage::Doing
    );
}

#[test]
fn a_log_written_before_stages_still_undoes() {
    let scratch = started("log-before-stages");
    let document = scratch.document();
    // what 0.2.10 logged for `done One`: the inverse set_done, false
    edit(&document, &["done One"]);
    let text = fs::read_to_string(&document).expect("reads");
    let log = scratch.undo_dir().join("graph.json.log");
    let entries = fs::read_to_string(&log).expect("reads");
    let last = entries.lines().last().expect("an entry");
    let old = last.replace(
        r#"{"op":"restore_stage","node":0,"stage":"backlog"}"#,
        r#"{"op":"set_done","node":0,"done":false}"#,
    );
    assert_ne!(old, last, "the entry is shaped as expected: {last}");
    let rewritten: Vec<&str> = entries
        .lines()
        .take(entries.lines().count() - 1)
        .chain([old.as_str()])
        .collect();
    fs::write(&log, rewritten.join("\n") + "\n").expect("writes");
    assert_eq!(fs::read_to_string(&document).expect("reads"), text);

    store::undo(&document).expect("undoes");
    let graph = store::load(&document).expect("loads");
    assert_eq!(
        graph.node(NodeId::new(0)).expect("exists").stage,
        Stage::Backlog
    );
}
