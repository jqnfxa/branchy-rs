//! Drives the built binary against a scratch document.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A throwaway document path, unique per test.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("branchy-cli-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir.join("graph.json"))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Some(dir) = self.0.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn binary() -> PathBuf {
    // the test binary lives in target/<profile>/deps, the CLI two levels up
    let mut path = std::env::current_exe().expect("test binary path");
    path.pop();
    path.pop();
    path.push(if cfg!(windows) {
        "branchy.exe"
    } else {
        "branchy"
    });
    path
}

/// Runs the binary, expecting success.
fn run(scratch: &Scratch, args: &[&str]) -> String {
    let output = Command::new(binary())
        .arg("--file")
        .arg(scratch.path())
        .args(args)
        .output()
        .expect("binary runs");
    assert!(
        output.status.success(),
        "`branchy {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 output")
}

/// Runs the binary, expecting failure, and returns stderr.
fn fails(scratch: &Scratch, args: &[&str]) -> String {
    let output = Command::new(binary())
        .arg("--file")
        .arg(scratch.path())
        .args(args)
        .output()
        .expect("binary runs");
    assert!(
        !output.status.success(),
        "`branchy {}` unexpectedly succeeded",
        args.join(" ")
    );
    String::from_utf8(output.stderr).expect("utf-8 output")
}

/// One area and a small maths chain.
fn seeded(tag: &str) -> Scratch {
    let scratch = Scratch::new(tag);
    run(&scratch, &["area", "Hard skills", "#4fd1c5"]);
    run(&scratch, &["add", "School algebra", "pri", "3"]);
    run(
        &scratch,
        &["add", "Calculus", "after", "school", "pri", "6"],
    );
    run(
        &scratch,
        &["add", "Probability theory", "after", "calculus", "pri", "8"],
    );
    scratch
}

#[test]
fn a_missing_file_reads_as_an_empty_graph() {
    let scratch = Scratch::new("empty");
    assert!(run(&scratch, &["tree"]).contains("The graph is empty"));
    assert!(run(&scratch, &["queue"]).contains("Nothing is available"));
}

#[test]
fn work_survives_between_invocations() {
    let scratch = seeded("persist");
    let tree = run(&scratch, &["tree"]);
    assert!(tree.contains("School algebra"));
    assert!(tree.contains("Probability theory"));
    assert!(scratch.path().exists(), "the document was written");
}

#[test]
fn a_multi_word_name_stays_one_task() {
    let scratch = seeded("names");
    let listing = run(&scratch, &["list"]);
    assert!(listing.contains("School algebra"));
    // and not split into "School" and "algebra"
    assert_eq!(listing.matches("School").count(), 1);
}

#[test]
fn the_queue_holds_only_what_is_reachable() {
    let scratch = seeded("queue");
    let queue = run(&scratch, &["queue"]);
    assert!(queue.contains("School algebra"));
    assert!(!queue.contains("Calculus"), "calculus is still blocked");

    run(&scratch, &["done", "school"]);
    let queue = run(&scratch, &["queue"]);
    assert!(queue.contains("Calculus"));
    assert!(
        !queue.contains("School algebra"),
        "done tasks leave the queue"
    );
}

#[test]
fn why_lists_the_work_in_order() {
    let scratch = seeded("why");
    let answer = run(&scratch, &["why", "Probability theory"]);
    assert!(answer.contains("2 task(s) stand in the way"));
    let algebra = answer.find("School algebra").expect("lists algebra");
    let calculus = answer.find("Calculus").expect("lists calculus");
    assert!(algebra < calculus, "deepest prerequisite comes first");
}

#[test]
fn why_says_so_when_nothing_is_in_the_way() {
    let scratch = seeded("why-clear");
    assert!(run(&scratch, &["why", "school"]).contains("available right now"));
}

#[test]
fn a_cycle_is_refused_by_name() {
    let scratch = seeded("cycle");
    let complaint = fails(
        &scratch,
        &["link", "School algebra", "after", "Probability theory"],
    );
    assert!(complaint.contains("cycle"));
    assert!(
        complaint.contains("School algebra"),
        "the loop is named, not numbered: {complaint}"
    );
    // and nothing was written
    assert!(run(&scratch, &["cycles"]).contains("No cycles"));
}

#[test]
fn an_ambiguous_name_is_refused() {
    let scratch = Scratch::new("ambiguous");
    run(&scratch, &["area", "Hard skills", "#fff"]);
    run(&scratch, &["add", "Calculus"]);
    run(&scratch, &["add", "Calculus of variations"]);
    assert!(fails(&scratch, &["done", "calc"]).contains("matches"));
}

#[test]
fn undo_reverses_the_last_change() {
    let scratch = seeded("undo");
    run(&scratch, &["done", "school"]);
    assert!(run(&scratch, &["queue"]).contains("Calculus"));

    run(&scratch, &["undo"]);
    let queue = run(&scratch, &["queue"]);
    assert!(queue.contains("School algebra"));
    assert!(!queue.contains("Calculus"));
}

#[test]
fn undo_is_a_stack_and_not_a_toggle() {
    let scratch = seeded("undo-stack");
    run(&scratch, &["add", "Alpha"]);
    run(&scratch, &["add", "Beta"]);
    run(&scratch, &["add", "Gamma"]);
    assert!(run(&scratch, &["list"]).contains("Gamma"));

    run(&scratch, &["undo"]);
    assert!(!run(&scratch, &["list"]).contains("Gamma"));

    // a toggle would bring Gamma back here; a stack keeps going backwards
    run(&scratch, &["undo"]);
    let listing = run(&scratch, &["list"]);
    assert!(!listing.contains("Gamma"), "still gone: {listing}");
    assert!(!listing.contains("Beta"), "Beta went too: {listing}");
    assert!(listing.contains("Alpha"));

    run(&scratch, &["undo"]);
    assert!(!run(&scratch, &["list"]).contains("Alpha"));
}

#[test]
fn undo_runs_out_rather_than_wrapping_around() {
    let scratch = seeded("undo-exhaust");
    run(&scratch, &["add", "Only"]);

    // unwind everything, however many saves that turns out to be
    let mut steps = 0;
    loop {
        let output = std::process::Command::new(binary())
            .arg("--file")
            .arg(scratch.path())
            .arg("undo")
            .output()
            .expect("binary runs");
        if !output.status.success() {
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("nothing to undo"),
                "ran out for the wrong reason"
            );
            break;
        }
        steps += 1;
        assert!(steps < 30, "undo never ran out, so it is wrapping around");
    }
    assert!(steps > 0, "there was something to undo");
}

#[test]
fn branchy_file_decides_the_document() {
    let scratch = seeded("env-file");
    let output = std::process::Command::new(binary())
        .env("BRANCHY_FILE", scratch.path())
        .arg("where")
        .output()
        .expect("binary runs");
    let printed = String::from_utf8(output.stdout).expect("utf-8");
    assert_eq!(
        printed.trim(),
        scratch.path().to_string_lossy(),
        "the environment variable has to win, or a sandboxed HOME silently          opens a second empty graph"
    );
}

#[test]
fn undo_with_no_history_says_so() {
    let scratch = Scratch::new("undo-empty");
    assert!(fails(&scratch, &["undo"]).contains("nothing to undo"));
}

#[test]
fn removing_a_task_strips_it_from_dependents() {
    let scratch = seeded("remove");
    run(&scratch, &["rm", "calculus"]);
    let tree = run(&scratch, &["tree"]);
    assert!(!tree.contains("Calculus"));
    assert!(
        run(&scratch, &["queue"]).contains("Probability theory"),
        "probability lost its blocker and became available"
    );
}

#[test]
fn a_corrupt_file_is_reported_not_ignored() {
    let scratch = Scratch::new("corrupt");
    std::fs::write(scratch.path(), "{ not json").expect("write");
    assert!(fails(&scratch, &["queue"]).contains("not valid Branchy JSON"));
}

#[test]
fn a_newer_format_is_refused() {
    let scratch = Scratch::new("future");
    std::fs::write(
        scratch.path(),
        r#"{"version": 99, "areas": [], "nodes": []}"#,
    )
    .expect("write");
    assert!(fails(&scratch, &["queue"]).contains("newer Branchy"));
}

#[test]
fn list_filters_by_status() {
    let scratch = seeded("filter");
    run(&scratch, &["done", "school"]);
    let done = run(&scratch, &["list", "--status", "done"]);
    assert!(done.contains("School algebra"));
    assert!(!done.contains("Probability"));

    assert!(fails(&scratch, &["list", "--status", "nonsense"]).contains("unknown status"));
}

#[test]
fn snapshot_carries_the_derived_picture() {
    let scratch = seeded("snapshot");
    run(&scratch, &["done", "school"]);
    let json = run(&scratch, &["snapshot"]);

    // the frontend must never have to recompute any of this
    assert!(json.contains("\"status\": \"done\""));
    assert!(json.contains("\"status\": \"available\""));
    assert!(json.contains("\"status\": \"locked\""));
    assert!(json.contains("\"tier\": 2"));
    assert!(json.contains("\"dependents\""));
    assert!(json.contains("\"queue\""));
    assert!(json.contains("\"cycles\": []"));
    assert!(json.contains("\"available\": 1"));
}

#[test]
fn snapshot_of_an_empty_graph_is_still_valid() {
    let scratch = Scratch::new("snapshot-empty");
    let json = run(&scratch, &["snapshot"]);
    assert!(json.contains("\"nodes\": []"));
    assert!(json.contains("\"total\": 0"));
}
