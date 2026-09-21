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
    assert!(run(&scratch, &["queue"]).contains("The graph is empty"));
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

// ── deadlines ────────────────────────────────────────────────────────────

#[test]
fn a_deadline_survives_a_save_and_reload() {
    let scratch = seeded("due-persist");
    run(&scratch, &["due", "calculus", "2027-03-01"]);
    assert!(run(&scratch, &["show", "calculus"]).contains("2027-03-01"));
    // a fresh process, so this came off disk
    assert!(run(&scratch, &["calendar"]).contains("2027-03-01"));
}

#[test]
fn a_deadline_can_be_cleared() {
    let scratch = seeded("due-clear");
    run(&scratch, &["due", "calculus", "2027-03-01"]);
    run(&scratch, &["due", "calculus", "none"]);
    assert!(run(&scratch, &["calendar"]).contains("Nothing has a deadline"));
}

#[test]
fn an_impossible_date_is_refused_before_anything_is_written() {
    let scratch = seeded("due-bad");
    assert!(fails(&scratch, &["due", "calculus", "2027-02-30"]).contains("not a date"));
    assert!(run(&scratch, &["calendar"]).contains("Nothing has a deadline"));
}

#[test]
fn the_calendar_marks_an_inherited_deadline() {
    let scratch = seeded("due-inherit");
    // probability needs calculus needs school algebra
    run(&scratch, &["due", "probability", "2027-03-01"]);
    let calendar = run(&scratch, &["calendar"]);

    assert!(calendar.contains("Probability theory"));
    assert!(calendar.contains("Calculus"));
    assert!(calendar.contains("School algebra"));
    assert_eq!(
        calendar.matches("inherited").count(),
        2,
        "the two prerequisites inherited it, the goal did not: {calendar}"
    );
}

#[test]
fn an_inherited_deadline_reorders_the_queue() {
    let scratch = seeded("due-queue");
    run(&scratch, &["add", "Unrelated", "pri", "9"]);
    run(&scratch, &["pri", "school", "1"]);

    // nothing urgent: priority decides
    let before = run(&scratch, &["queue"]);
    let unrelated_first = before.find("Unrelated").expect("listed");
    let school_first = before.find("School algebra").expect("listed");
    assert!(unrelated_first < school_first);

    // now the chain school algebra feeds has a date
    run(&scratch, &["due", "probability", "2026-10-01"]);
    let after = run(&scratch, &["queue"]);
    let unrelated_then = after.find("Unrelated").expect("listed");
    let school_then = after.find("School algebra").expect("listed");
    assert!(
        school_then < unrelated_then,
        "priority 1 outranks priority 9 once a deadline reaches it: {after}"
    );
}

#[test]
fn overdue_work_is_counted_in_the_tally() {
    let scratch = seeded("due-overdue");
    assert!(
        !run(&scratch, &["done", "school"]).contains("overdue"),
        "nothing has a date yet"
    );

    run(&scratch, &["due", "calculus", "2000-01-01"]);
    assert!(run(&scratch, &["pri", "calculus", "6"]).contains("overdue"));

    // finishing it takes it out of the count
    assert!(!run(&scratch, &["done", "calculus"]).contains("overdue"));
}

#[test]
fn a_document_written_before_deadlines_still_loads() {
    let scratch = Scratch::new("due-old-format");
    std::fs::write(
        scratch.path(),
        r##"{"version":1,
            "areas":[{"id":0,"name":"Work","color":"#4fd1c5"}],
            "nodes":[{"id":0,"name":"Old task","area":0,"priority":5,"done":false}]}"##,
    )
    .expect("write");
    assert!(run(&scratch, &["queue"]).contains("Old task"));
    assert!(run(&scratch, &["calendar"]).contains("Nothing has a deadline"));
}

#[test]
fn a_brand_new_graph_says_how_to_start_not_to_look_for_cycles() {
    let scratch = Scratch::new("first-run");
    let first = run(&scratch, &["queue"]);
    assert!(first.contains("branchy area"), "{first}");
    assert!(
        !first.contains("cycles"),
        "a new user has no cycles: {first}"
    );

    run(&scratch, &["area", "Work", "#4fd1c5"]);
    assert!(run(&scratch, &["queue"]).contains("branchy add"));
}

// ── a document reached through a symlink ────────────────────────────────

#[cfg(unix)]
fn link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("symlink");
}

#[cfg(unix)]
#[test]
fn a_save_through_a_symlink_keeps_the_link_and_updates_the_real_file() {
    let real = Scratch::new("link-real");
    run(&real, &["area", "Work", "#4fd1c5"]);
    run(&real, &["add", "Original task"]);

    let via = Scratch::new("link-via");
    link(real.path(), via.path());

    // write through the link, the way a launcher using the default path would
    run(&via, &["add", "Added through the link"]);

    let meta = std::fs::symlink_metadata(via.path()).expect("link still there");
    assert!(
        meta.file_type().is_symlink(),
        "the save replaced the link with a regular file"
    );
    assert!(
        run(&real, &["list"]).contains("Added through the link"),
        "the real file never saw the change: the two have forked"
    );
}

#[cfg(unix)]
#[test]
fn undo_through_a_symlink_keeps_the_link_too() {
    let real = Scratch::new("link-undo-real");
    run(&real, &["area", "Work", "#4fd1c5"]);
    run(&real, &["add", "Keep"]);

    let via = Scratch::new("link-undo-via");
    link(real.path(), via.path());
    run(&via, &["add", "Take back"]);
    run(&via, &["undo"]);

    assert!(
        std::fs::symlink_metadata(via.path())
            .expect("exists")
            .file_type()
            .is_symlink(),
        "undo replaced the link"
    );
    let listing = run(&real, &["list"]);
    assert!(listing.contains("Keep"));
    assert!(!listing.contains("Take back"));
}

#[cfg(unix)]
#[test]
fn a_link_to_a_document_that_does_not_exist_yet_creates_the_target() {
    let real = Scratch::new("link-dangling-real");
    let via = Scratch::new("link-dangling-via");
    link(real.path(), via.path());
    assert!(!real.path().exists(), "precondition: nothing there yet");

    run(&via, &["area", "Work", "#4fd1c5"]);

    assert!(real.path().is_file(), "the target was not created");
    assert!(
        std::fs::symlink_metadata(via.path())
            .expect("exists")
            .file_type()
            .is_symlink()
    );
}

// ── vaults ──────────────────────────────────────────────────────────────

/// A scratch folder and a vault list of its own, and nothing else.
///
/// Every run sets `BRANCHY_VAULTS` and clears `BRANCHY_FILE`. Without both, a
/// test would read the real user's list, or follow their environment straight
/// into their real graph.
struct Desk(Scratch);

impl Desk {
    fn new(tag: &str) -> Self {
        Self(Scratch::new(tag))
    }

    fn dir(&self) -> &Path {
        self.0.path().parent().expect("scratch folder")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(binary());
        command
            .env("BRANCHY_VAULTS", self.dir().join("vaults.json"))
            .env_remove("BRANCHY_FILE")
            .current_dir(self.dir())
            .args(args);
        command
    }

    fn run(&self, args: &[&str]) -> String {
        let output = self.command(args).output().expect("binary runs");
        assert!(
            output.status.success(),
            "`branchy {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("utf-8 output")
    }

    fn fails(&self, args: &[&str]) -> String {
        let output = self.command(args).output().expect("binary runs");
        assert!(
            !output.status.success(),
            "`branchy {}` unexpectedly succeeded",
            args.join(" ")
        );
        String::from_utf8(output.stderr).expect("utf-8 output")
    }

    /// Creates a vault under the scratch folder.
    fn vault(&self, name: &str) -> PathBuf {
        let parent = self.dir().display().to_string();
        self.run(&["vault", "new", name, "--in", &parent]);
        self.where_is()
            .parent()
            .expect("vault folder")
            .to_path_buf()
    }

    fn where_is(&self) -> PathBuf {
        PathBuf::from(self.run(&["where"]).trim())
    }
}

#[test]
fn with_no_vault_the_terminal_says_how_to_start() {
    let desk = Desk::new("vault-none");
    assert!(desk.fails(&["queue"]).contains("branchy vault new"));
    assert!(desk.run(&["vault"]).contains("No vaults yet"));
}

#[test]
fn a_new_vault_is_where_the_next_commands_go() {
    let desk = Desk::new("vault-new");
    let work = desk.vault("Work");
    desk.run(&["area", "Jobs"]);
    desk.run(&["add", "First task"]);

    assert_eq!(desk.where_is(), work.join("graph.json"));
    assert!(desk.run(&["list"]).contains("First task"));
    assert!(work.join("graph.json").is_file());
}

#[test]
fn a_new_vault_goes_in_the_current_directory_unless_told_otherwise() {
    let desk = Desk::new("vault-here");
    desk.run(&["vault", "new", "Here"]);
    assert!(desk.dir().join("Here").join("graph.json").is_file());
}

#[test]
fn opening_a_vault_moves_the_terminal_to_it() {
    let desk = Desk::new("vault-open");
    let work = desk.vault("Work");
    let life = desk.vault("Life");
    assert_eq!(desk.where_is(), life.join("graph.json"));

    desk.run(&["vault", "open", "Work"]);
    assert_eq!(desk.where_is(), work.join("graph.json"));

    // any folder opens, and joins the list
    let plans = desk.dir().join("Plans");
    std::fs::create_dir(&plans).expect("mkdir");
    desk.run(&["vault", "open", &plans.display().to_string()]);
    assert!(desk.run(&["vault", "list"]).contains("* Plans"));
}

#[test]
fn the_vault_option_reaches_another_vault_for_one_command_only() {
    let desk = Desk::new("vault-flag");
    let work = desk.vault("Work");
    desk.run(&["area", "Jobs"]);
    desk.vault("Life");
    desk.run(&["vault", "open", "Work"]);

    desk.run(&["--vault", "Life", "area", "Health"]);
    desk.run(&["--vault", "Life", "add", "Sleep more"]);

    assert!(
        desk.run(&["--vault", "Life", "list"])
            .contains("Sleep more")
    );
    assert!(!desk.run(&["list"]).contains("Sleep more"));
    assert_eq!(desk.where_is(), work.join("graph.json"), "still in Work");
}

#[test]
fn the_list_marks_the_current_vault_and_keeps_five() {
    let desk = Desk::new("vault-list");
    for name in ["V1", "V2", "V3", "V4", "V5", "V6"] {
        desk.vault(name);
    }
    let listing = desk.run(&["vault", "list"]);
    assert!(listing.lines().next().expect("a line").starts_with("* V6"));
    assert_eq!(listing.lines().count(), 5);
    assert!(!listing.contains("V1"));
    assert!(
        desk.dir().join("V1").is_dir(),
        "dropped off the list, not deleted"
    );
}

#[test]
fn forgetting_a_vault_leaves_its_folder_alone() {
    let desk = Desk::new("vault-forget");
    desk.vault("Work");
    let life = desk.vault("Life");

    desk.run(&["vault", "forget", "Life"]);
    assert!(!desk.run(&["vault"]).contains("Life"));
    assert!(life.join("graph.json").is_file());
}

#[test]
fn a_deleted_vault_is_reported_rather_than_recreated() {
    let desk = Desk::new("vault-gone");
    let work = desk.vault("Work");
    std::fs::remove_dir_all(&work).expect("delete");

    assert!(desk.fails(&["area", "Jobs"]).contains("is gone"));
    assert!(!work.exists(), "a save recreated the deleted folder");
    assert!(desk.run(&["vault"]).contains("folder not found"));
}

#[test]
fn branchy_file_still_wins_over_the_current_vault() {
    let desk = Desk::new("vault-env");
    desk.vault("Work");
    let pinned = desk.dir().join("pinned.json");
    let output = desk
        .command(&["where"])
        .env("BRANCHY_FILE", &pinned)
        .output()
        .expect("binary runs");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        pinned.display().to_string()
    );
}

#[test]
fn a_file_and_a_vault_cannot_both_be_named() {
    let desk = Desk::new("vault-conflict");
    desk.vault("Work");
    assert!(
        desk.fails(&["--file", "x.json", "--vault", "Work", "list"])
            .contains("cannot be used with")
    );
}
