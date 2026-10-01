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
fn an_ambiguous_name_lists_what_it_matched_with_ids() {
    let scratch = Scratch::new("ambiguous-list");
    run(&scratch, &["area", "Hard skills", "#fff"]);
    run(&scratch, &["add", "Calculus"]);
    run(&scratch, &["add", "Calculus of variations"]);
    let complaint = fails(&scratch, &["done", "calc"]);
    assert!(complaint.contains("n0 Calculus"), "{complaint}");
    assert!(
        complaint.contains("n1 Calculus of variations"),
        "{complaint}"
    );
}

#[test]
fn a_long_list_of_matches_is_cut_short() {
    let scratch = Scratch::new("ambiguous-many");
    run(&scratch, &["area", "Work"]);
    for i in 0..12 {
        run(&scratch, &["add", &format!("Task {i}")]);
    }
    let complaint = fails(&scratch, &["done", "task"]);
    assert!(complaint.contains("12 tasks"), "{complaint}");
    assert!(complaint.contains("and 4 more"), "{complaint}");
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

// ── ids are never handed out twice ──────────────────────────────────────

#[test]
fn a_removed_task_keeps_its_id_after_a_reload() {
    let scratch = Scratch::new("spent-node");
    run(&scratch, &["area", "Work"]);
    run(&scratch, &["add", "First"]);
    assert!(run(&scratch, &["add", "Second"]).contains("n1"));
    run(&scratch, &["rm", "Second"]);
    let third = run(&scratch, &["add", "Third"]);
    assert!(third.contains("n2"), "n1 was spent on Second: {third}");
}

#[test]
fn an_undone_addition_keeps_its_id_spent() {
    let scratch = Scratch::new("spent-undo");
    run(&scratch, &["area", "Work"]);
    run(&scratch, &["add", "First"]);
    assert!(run(&scratch, &["add", "Second"]).contains("n1"));
    run(&scratch, &["undo"]);
    let third = run(&scratch, &["add", "Third"]);
    assert!(third.contains("n2"), "n1 was spent on Second: {third}");
}

#[test]
fn a_removed_direction_keeps_its_id_after_a_reload() {
    let scratch = Scratch::new("spent-area");
    run(&scratch, &["area", "Work"]);
    run(&scratch, &["area", "Spare"]);
    run(&scratch, &["rmarea", "Spare"]);
    run(&scratch, &["area", "Health"]);
    let snapshot = run(&scratch, &["snapshot"]);
    assert!(snapshot.contains(r#""id": "a2""#), "{snapshot}");
    assert!(!snapshot.contains(r#""id": "a1""#), "{snapshot}");
}

#[test]
fn a_document_without_counters_continues_from_its_highest_id() {
    let scratch = Scratch::new("no-counters");
    std::fs::write(
        scratch.path(),
        r##"{"version":1,
            "areas":[{"id":0,"name":"Work","color":"#4fd1c5"}],
            "nodes":[{"id":4,"name":"Old task","area":0,"priority":5,"done":false}]}"##,
    )
    .expect("write");
    assert!(run(&scratch, &["add", "New task"]).contains("n5"));
}

// ── output to a reader that stops early ─────────────────────────────────

#[test]
fn a_reader_closing_the_pipe_is_not_a_crash() {
    use std::process::Stdio;

    // far more output than a pipe buffers, so the write has to meet the
    // closed end rather than fitting into the buffer before it closes
    let scratch = Scratch::new("broken-pipe");
    let nodes: Vec<String> = (0..2000)
        .map(|i| {
            format!(r#"{{"id":{i},"name":"Task number {i}","area":0,"priority":5,"done":false}}"#)
        })
        .collect();
    std::fs::write(
        scratch.path(),
        format!(
            r##"{{"version":1,"areas":[{{"id":0,"name":"Work","color":"#4fd1c5"}}],"nodes":[{}]}}"##,
            nodes.join(",")
        ),
    )
    .expect("write");

    let mut child = Command::new(binary())
        .arg("--file")
        .arg(scratch.path())
        .arg("snapshot")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    // what `branchy snapshot | head -1` does once it has its line
    drop(child.stdout.take());
    let output = child.wait_with_output().expect("binary exits");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
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

// ── batches ─────────────────────────────────────────────────────────────

/// Runs the binary with `input` on stdin, returning stdout and stderr and
/// whether it succeeded.
fn run_with_input(scratch: &Scratch, args: &[&str], input: &str) -> (bool, String, String) {
    use std::io::Write as _;
    use std::process::Stdio;

    let mut child = Command::new(binary())
        .arg("--file")
        .arg(scratch.path())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(input.as_bytes())
        .expect("input written");
    let output = child.wait_with_output().expect("binary exits");
    (
        output.status.success(),
        String::from_utf8(output.stdout).expect("utf-8 output"),
        String::from_utf8(output.stderr).expect("utf-8 output"),
    )
}

#[test]
fn a_batch_wires_up_tasks_that_had_no_id_when_it_was_written() {
    let scratch = Scratch::new("batch");
    let (ok, out, err) = run_with_input(
        &scratch,
        &["run", "-"],
        "# a small maths chain\n\
         $m = area Maths\n\
         $calc = add Calculus in $m\n\
         \n\
         $alg = add Algebra in $m pri 7\n\
         add \"Linear algebra\" in $m after $calc, $alg\n",
    );
    assert!(ok, "{err}");
    assert!(out.contains("Applied 4 commands"), "{out}");
    assert!(out.contains("n0  Calculus  $calc"), "{out}");
    let why = run(&scratch, &["why", "Linear algebra"]);
    assert!(why.contains("Calculus") && why.contains("Algebra"), "{why}");
}

#[test]
fn a_batch_is_one_change_to_undo() {
    let scratch = Scratch::new("batch-undo");
    run(&scratch, &["area", "Work"]);
    let (ok, _, err) = run_with_input(&scratch, &["run", "-"], "add One\nadd Two\nadd Three\n");
    assert!(ok, "{err}");
    run(&scratch, &["undo"]);
    let list = run(&scratch, &["list"]);
    assert!(!list.contains("One") && !list.contains("Three"), "{list}");
}

#[test]
fn a_refused_line_leaves_the_whole_batch_unapplied() {
    let scratch = Scratch::new("batch-refused");
    run(&scratch, &["area", "Work"]);
    let before = std::fs::read_to_string(scratch.path()).expect("saved");
    let (ok, _, err) = run_with_input(
        &scratch,
        &["run", "-"],
        "add One\nadd Two\nlink One after Nowhere\nadd Four\n",
    );
    assert!(!ok);
    assert!(err.contains("line 3:"), "{err}");
    assert_eq!(
        std::fs::read_to_string(scratch.path()).expect("still there"),
        before
    );
}

#[test]
fn a_batch_of_comments_is_refused_as_empty() {
    let scratch = Scratch::new("batch-empty");
    run(&scratch, &["area", "Work"]);
    let (ok, _, err) = run_with_input(&scratch, &["run", "-"], "# nothing yet\n\n");
    assert!(!ok);
    assert!(err.contains("nothing to do"), "{err}");
}

// ── plain output, for programs and agents ───────────────────────────────

#[test]
fn plain_rows_carry_ids_and_whole_names() {
    let scratch = seeded("plain-queue");
    let long = "A task whose name is far too long to fit any column of the human view";
    run(&scratch, &["add", long, "pri", "9"]);
    let out = run(&scratch, &["--plain", "queue"]);
    assert!(
        out.contains("# id\tstatus\tstage\tpri\tarea\ttier\tdue\tneeds\tname"),
        "{out}"
    );
    assert!(
        out.contains(&format!("n3\tavailable\tbacklog\t9\ta0\t0\t-\t-\t{long}")),
        "{out}"
    );
    assert!(out.contains("# a0\tHard skills"), "{out}");
}

#[test]
fn plain_rows_refer_to_prerequisites_by_id() {
    let scratch = seeded("plain-tree");
    let out = run(&scratch, &["--plain", "tree"]);
    assert!(
        out.contains("n2\tlocked\tbacklog\t8\ta0\t2\t-\tn1\tProbability theory"),
        "{out}"
    );
}

#[test]
fn a_limit_says_how_much_was_left_out() {
    let scratch = Scratch::new("limit");
    run(&scratch, &["area", "Work"]);
    for name in ["One", "Two", "Three"] {
        run(&scratch, &["add", name]);
    }
    let plain = run(&scratch, &["--plain", "queue", "--limit", "2"]);
    assert!(plain.starts_with("# 2 of 3 available"), "{plain}");
    assert_eq!(plain.lines().filter(|l| !l.starts_with('#')).count(), 2);
    let human = run(&scratch, &["queue", "--limit", "1"]);
    assert!(human.starts_with("1 of 3 available"), "{human}");
    assert_eq!(
        run(&scratch, &["list", "--limit", "2"])
            .lines()
            .filter(|l| l.contains("[ ]"))
            .count(),
        2
    );
}

#[test]
fn plain_show_ends_with_the_note_as_written() {
    let scratch = seeded("plain-show");
    run(
        &scratch,
        &["note", "calculus", "Limits first, then series."],
    );
    let out = run(&scratch, &["--plain", "show", "calculus"]);
    assert!(out.contains("unlocks\tn2"), "{out}");
    assert!(out.ends_with("note\tLimits first, then series.\n"), "{out}");
}

#[test]
fn a_plain_change_prints_the_row_it_changed() {
    let scratch = seeded("plain-change");
    let out = run(&scratch, &["--plain", "done", "school"]);
    assert_eq!(out, "n0\tdone\tdone\t3\ta0\t0\t-\t-\tSchool algebra\n");
}

#[test]
fn a_plain_batch_prints_what_it_made_with_labels() {
    let scratch = Scratch::new("plain-batch");
    let (ok, out, err) = run_with_input(
        &scratch,
        &["--plain", "run", "-"],
        "$w = area Work\n$a = add Alpha in $w\nadd Beta in $w after $a\n",
    );
    assert!(ok, "{err}");
    assert_eq!(out, "a0\t$w\nn0\t$a\nn1\n");
}

#[test]
fn brief_gives_counts_directions_and_the_top_of_the_queue() {
    let scratch = seeded("brief");
    run(&scratch, &["done", "school"]);
    let human = run(&scratch, &["brief"]);
    assert!(
        human.contains("3 tasks: 1 done, 1 available, 1 locked."),
        "{human}"
    );
    assert!(human.contains("a0    Hard skills"), "{human}");
    assert!(human.contains("n1     Calculus"), "{human}");

    let plain = run(&scratch, &["--plain", "brief", "--limit", "1"]);
    assert!(
        plain.contains("# tasks 3\tdone 1\tavailable 1\tlocked 1\tcyclic 0\toverdue 0"),
        "{plain}"
    );
    assert!(plain.contains("# a0\tHard skills\t1/3"), "{plain}");
    assert!(
        plain.contains("n1\tavailable\tbacklog\t6\ta0\t1\t-\tn0\tCalculus"),
        "{plain}"
    );
    assert!(plain.ends_with("# cycles 0\n"), "{plain}");
}

#[test]
fn brief_on_an_empty_graph_says_how_to_start() {
    let scratch = Scratch::new("brief-empty");
    assert!(run(&scratch, &["brief"]).contains("branchy area"));
}

// ── the guide ───────────────────────────────────────────────────────────

/// Splits a command line the way a shell would for the guide's examples:
/// on spaces, keeping double-quoted words together.
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut open = false;
    for ch in line.chars() {
        match ch {
            '"' => {
                open = !open;
                quoted = true;
            }
            ' ' if !open => {
                if !current.is_empty() || quoted {
                    words.push(std::mem::take(&mut current));
                    quoted = false;
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() || quoted {
        words.push(current);
    }
    words
}

#[test]
fn every_example_in_the_guide_runs() {
    let scratch = Scratch::new("guide");
    let guide = run(&scratch, &["guide"]);
    let mut lines = guide.lines();
    let mut ran = 0;
    while let Some(line) = lines.next() {
        let Some(command) = line.strip_prefix("$ branchy ") else {
            continue;
        };
        // a comment after the command starts at two spaces and a #
        let command = command.split("  #").next().unwrap_or("").trim();
        if let Some(head) = command.strip_suffix("<<'EOF'") {
            let batch: Vec<&str> = lines.by_ref().take_while(|l| *l != "EOF").collect();
            let args = shell_words(head.trim());
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let (ok, out, err) = run_with_input(&scratch, &args, &batch.join("\n"));
            assert!(ok, "the guide's batch failed: {err}");
            assert_eq!(
                out, "a2\t$m\nn4\t$mech\nn5\n",
                "the guide says what this prints"
            );
        } else {
            let args = shell_words(command);
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            run(&scratch, &args);
        }
        ran += 1;
    }
    assert!(
        ran > 25,
        "only {ran} examples found; is the format still `$ branchy`?"
    );
}

#[test]
fn the_guide_needs_no_vault() {
    let output = Command::new(binary())
        .arg("guide")
        .env(
            "BRANCHY_VAULTS",
            std::env::temp_dir().join("branchy-no-such-list.json"),
        )
        .env_remove("BRANCHY_FILE")
        .output()
        .expect("binary runs");
    assert!(output.status.success());
}

// ── the board ───────────────────────────────────────────────────────────

#[test]
fn the_stage_verbs_move_a_task_across_the_board() {
    let scratch = seeded("stages");
    run(&scratch, &["todo", "school"]);
    assert!(run(&scratch, &["--plain", "show", "school"]).contains("\ttodo\t"));
    run(&scratch, &["start", "school"]);
    run(&scratch, &["review", "school"]);
    // review does not unlock calculus
    assert!(run(&scratch, &["--plain", "show", "calculus"]).contains("locked\tbacklog"));
    run(&scratch, &["stage", "school", "done"]);
    assert!(run(&scratch, &["--plain", "show", "calculus"]).contains("available\tbacklog"));
}

#[test]
fn starting_a_locked_task_is_refused_by_name() {
    let scratch = seeded("start-locked");
    let complaint = fails(&scratch, &["start", "calculus"]);
    assert!(
        complaint.contains("Calculus cannot be started"),
        "{complaint}"
    );
    // planning it is fine
    run(&scratch, &["todo", "calculus"]);
}

#[test]
fn the_board_shows_every_column() {
    let scratch = seeded("board");
    run(&scratch, &["start", "school"]);
    run(&scratch, &["todo", "probability"]);
    let human = run(&scratch, &["board"]);
    for heading in [
        "BACKLOG  0 ready, 1 locked",
        "TODO  1",
        "DOING  1",
        "REVIEW  0",
        "DONE  0",
    ] {
        assert!(human.contains(heading), "{heading}: {human}");
    }
    let plain = run(&scratch, &["--plain", "board"]);
    assert!(
        plain.starts_with("# board\tbacklog 1\ttodo 1\tdoing 1\treview 0\tdone 0\n"),
        "{plain}"
    );
    assert!(
        plain.contains("# 1 doing\nn0\tavailable\tdoing\t3"),
        "{plain}"
    );
}

#[test]
fn list_filters_by_stage() {
    let scratch = seeded("list-stage");
    run(&scratch, &["start", "school"]);
    let out = run(&scratch, &["--plain", "list", "--stage", "doing"]);
    assert!(out.starts_with("# 1 tasks"), "{out}");
    assert!(fails(&scratch, &["list", "--stage", "closed"]).contains("unknown stage"));
}

#[test]
fn the_queue_says_which_tasks_are_already_under_way() {
    let scratch = seeded("queue-stage");
    run(&scratch, &["start", "school"]);
    assert!(run(&scratch, &["queue"]).contains("doing"));
}

#[test]
fn a_format_two_document_is_written_and_read_back() {
    let scratch = seeded("format-two");
    run(&scratch, &["review", "school"]);
    let text = std::fs::read_to_string(scratch.path()).expect("saved");
    assert!(text.contains(r#""version": 2"#), "{text}");
    assert!(run(&scratch, &["--plain", "show", "school"]).contains("\treview\t"));
}
