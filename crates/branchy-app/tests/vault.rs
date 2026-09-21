//! Vaults, the recent list, and where the undo stack lives.

use std::fs;
use std::path::{Path, PathBuf};

use branchy_app::store::{self, StoreError};
use branchy_app::vault::{DOCUMENT, RECENT_LIMIT, Vault, Vaults};
use branchy_core::{Graph, NewArea, NewNode};

/// A throwaway folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("branchy-vault-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        // resolved the way the library resolves a vault's folder, so paths
        // compare equal to what the list stores on every platform
        Self(Vault::open(&dir).expect("opens").dir().to_path_buf())
    }

    fn dir(&self) -> &Path {
        &self.0
    }

    fn list(&self) -> PathBuf {
        self.0.join("vaults.json")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn graph_with(names: &[&str]) -> Graph {
    let mut graph = Graph::new();
    let area = graph.add_area(NewArea::new("Work", "#4fd1c5"));
    for name in names {
        graph
            .add_node(NewNode::new(*name, area))
            .expect("area exists");
    }
    graph
}

fn names(path: &Path) -> Vec<String> {
    store::load(path)
        .expect("loads")
        .nodes()
        .map(|(_, node)| node.name.clone())
        .collect()
}

// ── one vault ───────────────────────────────────────────────────────────

#[test]
fn a_new_vault_is_a_folder_holding_an_empty_graph() {
    let scratch = Scratch::new("create");
    let vault = Vault::create(scratch.dir(), "Work").expect("created");

    assert_eq!(vault.name(), "Work");
    assert_eq!(vault.dir(), scratch.dir().join("Work"));
    assert!(
        vault.document().is_file(),
        "the graph is written straight away"
    );
    assert_eq!(
        store::load(&vault.document()).expect("loads").node_count(),
        0
    );
}

#[test]
fn creating_a_vault_makes_the_folders_it_needs() {
    let scratch = Scratch::new("create-deep");
    let vault = Vault::create(&scratch.dir().join("Documents/Branchy"), "Work").expect("created");
    assert!(vault.document().is_file());
}

#[test]
fn an_empty_folder_can_become_a_vault() {
    let scratch = Scratch::new("create-empty");
    fs::create_dir(scratch.dir().join("Work")).expect("mkdir");
    assert!(Vault::create(scratch.dir(), "Work").is_ok());
}

#[test]
fn a_vault_is_not_created_among_someone_elses_files() {
    let scratch = Scratch::new("create-busy");
    fs::create_dir(scratch.dir().join("Work")).expect("mkdir");
    fs::write(scratch.dir().join("Work/notes.txt"), "mine").expect("write");

    let refused = Vault::create(scratch.dir(), "Work");
    assert!(
        matches!(refused, Err(StoreError::VaultExists(_))),
        "{refused:?}"
    );
    assert!(!scratch.dir().join("Work").join(DOCUMENT).exists());
}

#[test]
fn a_file_in_the_way_is_refused_too() {
    let scratch = Scratch::new("create-file");
    fs::write(scratch.dir().join("Work"), "a file").expect("write");
    let refused = Vault::create(scratch.dir(), "Work");
    assert!(
        matches!(refused, Err(StoreError::VaultExists(_))),
        "{refused:?}"
    );
}

#[test]
fn a_name_that_cannot_be_a_folder_everywhere_is_refused() {
    let scratch = Scratch::new("names");
    for name in [
        "", "   ", ".", "..", "a/b", r"a\b", "a:b", "why?", "dots.", "CON", "com1", "nul.txt",
    ] {
        let refused = Vault::create(scratch.dir(), name);
        assert!(
            matches!(refused, Err(StoreError::BadVaultName(_))),
            "{name:?} was accepted: {refused:?}"
        );
    }
    // and ordinary names, including ones outside ASCII, are fine
    for name in ["Работа", "仕事", "Side project", "v2.0 plan", "Console"] {
        assert!(
            Vault::create(scratch.dir(), name).is_ok(),
            "{name:?} was refused"
        );
    }
}

#[test]
fn surrounding_spaces_are_dropped_from_the_name() {
    let scratch = Scratch::new("trim");
    let vault = Vault::create(scratch.dir(), "  Work  ").expect("created");
    assert_eq!(vault.name(), "Work");
}

#[test]
fn any_folder_opens_as_a_vault_and_writes_nothing_until_a_change() {
    let scratch = Scratch::new("open-any");
    let vault = Vault::open(scratch.dir()).expect("opens");
    assert_eq!(
        store::load(&vault.document()).expect("loads").node_count(),
        0
    );
    assert!(!vault.document().exists(), "opening alone must not write");
}

#[test]
fn a_file_or_a_missing_folder_is_not_a_vault() {
    let scratch = Scratch::new("open-bad");
    let file = scratch.dir().join("plain.txt");
    fs::write(&file, "text").expect("write");
    assert!(matches!(Vault::open(&file), Err(StoreError::NotAFolder(_))));
    assert!(matches!(
        Vault::open(&scratch.dir().join("nowhere")),
        Err(StoreError::NotAFolder(_))
    ));
}

// ── the recent list ─────────────────────────────────────────────────────

#[test]
fn a_missing_list_reads_as_empty() {
    let scratch = Scratch::new("list-empty");
    let list = Vaults::load(&scratch.list()).expect("loads");
    assert!(list.recent().is_empty());
    assert!(list.current().is_none());
    assert!(
        !list.open_last(),
        "the window offers the list unless told otherwise"
    );
}

#[test]
fn the_list_keeps_the_five_most_recent_newest_first() {
    let scratch = Scratch::new("list-five");
    let mut list = Vaults::load(&scratch.list()).expect("loads");
    let vaults: Vec<Vault> = (1..=7)
        .map(|i| Vault::create(scratch.dir(), &format!("V{i}")).expect("created"))
        .collect();
    for vault in &vaults {
        list.opened(vault);
    }

    assert_eq!(list.recent().len(), RECENT_LIMIT);
    let names: Vec<String> = list.entries().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["V7", "V6", "V5", "V4", "V3"]);
    assert_eq!(list.current(), Some(vaults[6].dir()));
    assert!(
        vaults[0].dir().is_dir(),
        "falling off the list leaves the folder"
    );
}

#[test]
fn reopening_a_vault_moves_it_to_the_front_instead_of_repeating_it() {
    let scratch = Scratch::new("list-move");
    let mut list = Vaults::load(&scratch.list()).expect("loads");
    let work = Vault::create(scratch.dir(), "Work").expect("created");
    let life = Vault::create(scratch.dir(), "Life").expect("created");
    list.opened(&work);
    list.opened(&life);

    // the same folder, spelled differently
    let again = Vault::open(&scratch.dir().join("Life/../Work/")).expect("opens");
    list.opened(&again);

    let names: Vec<String> = list.entries().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["Work", "Life"]);
}

#[test]
fn the_list_survives_a_save_and_reload() {
    let scratch = Scratch::new("list-save");
    let mut list = Vaults::load(&scratch.list()).expect("loads");
    list.opened(&Vault::create(scratch.dir(), "Work").expect("created"));
    list.opened(&Vault::create(scratch.dir(), "Life").expect("created"));
    list.set_open_last(true);
    list.save().expect("saves");

    let again = Vaults::load(&scratch.list()).expect("reloads");
    assert_eq!(again, list);
}

#[test]
fn a_list_from_a_newer_version_is_refused_rather_than_overwritten() {
    let scratch = Scratch::new("list-newer");
    fs::write(scratch.list(), r#"{"version": 99, "recent": []}"#).expect("write");
    assert!(matches!(
        Vaults::load(&scratch.list()),
        Err(StoreError::Version(99))
    ));
}

#[test]
fn a_moved_vault_stays_listed_as_missing() {
    let scratch = Scratch::new("list-missing");
    let mut list = Vaults::load(&scratch.list()).expect("loads");
    let vault = Vault::create(scratch.dir(), "Work").expect("created");
    list.opened(&vault);
    fs::rename(vault.dir(), scratch.dir().join("Elsewhere")).expect("move");

    let entries = list.entries();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].missing);
    assert!(matches!(
        list.resolve("Work"),
        Err(StoreError::VaultMissing(_))
    ));
}

#[test]
fn forgetting_a_vault_takes_it_off_the_list_and_leaves_its_folder() {
    let scratch = Scratch::new("forget");
    let mut list = Vaults::load(&scratch.list()).expect("loads");
    let work = Vault::create(scratch.dir(), "Work").expect("created");
    let life = Vault::create(scratch.dir(), "Life").expect("created");
    list.opened(&work);
    list.opened(&life);

    assert_eq!(list.forget("Life").expect("forgets"), life.dir());
    assert_eq!(list.current(), Some(work.dir()));
    assert!(life.document().is_file(), "the folder was left alone");

    // by folder as well as by name, which is what the window sends
    let path = work.dir().display().to_string();
    list.forget(&path).expect("forgets by path");
    assert!(list.recent().is_empty());
    assert!(matches!(
        list.forget("Work"),
        Err(StoreError::UnknownVault(_))
    ));
}

#[test]
fn a_vault_is_named_by_its_folder_name_or_by_its_folder() {
    let scratch = Scratch::new("resolve");
    let mut list = Vaults::load(&scratch.list()).expect("loads");
    let work = Vault::create(scratch.dir(), "Work").expect("created");
    list.opened(&work);

    assert_eq!(list.resolve("Work").expect("by name"), work);
    let path = work.dir().display().to_string();
    assert_eq!(list.resolve(&path).expect("by folder"), work);

    // a folder not on the list is fine too; a name that is neither is not
    let other = scratch.dir().join("Other");
    fs::create_dir(&other).expect("mkdir");
    assert!(list.resolve(&other.display().to_string()).is_ok());
    assert!(matches!(
        list.resolve("Nothing"),
        Err(StoreError::UnknownVault(_))
    ));
}

#[test]
fn two_recent_vaults_with_one_name_have_to_be_told_apart_by_folder() {
    let scratch = Scratch::new("ambiguous");
    let mut list = Vaults::load(&scratch.list()).expect("loads");
    let one = Vault::create(&scratch.dir().join("a"), "Work").expect("created");
    let two = Vault::create(&scratch.dir().join("b"), "Work").expect("created");
    list.opened(&one);
    list.opened(&two);

    assert!(matches!(
        list.resolve("Work"),
        Err(StoreError::AmbiguousVault(_))
    ));
    assert_eq!(
        list.resolve(&one.dir().display().to_string())
            .expect("by folder"),
        one
    );
}

// ── upgrading from before vaults ────────────────────────────────────────

#[test]
fn the_first_run_adopts_the_folder_older_versions_kept_the_graph_in() {
    let scratch = Scratch::new("adopt");
    let legacy = scratch.dir().join("data");
    store::save(&legacy.join(DOCUMENT), &graph_with(&["Old task"])).expect("saves");

    let list = Vaults::load_or_adopt(&scratch.list(), &legacy).expect("loads");
    assert_eq!(list.current(), Some(legacy.as_path()));
}

#[test]
fn nothing_is_adopted_from_a_folder_without_a_graph() {
    let scratch = Scratch::new("adopt-none");
    let legacy = scratch.dir().join("data");
    fs::create_dir(&legacy).expect("mkdir");
    let list = Vaults::load_or_adopt(&scratch.list(), &legacy).expect("loads");
    assert!(list.recent().is_empty());
}

#[test]
fn adoption_happens_once_so_a_forgotten_vault_stays_forgotten() {
    let scratch = Scratch::new("adopt-once");
    let legacy = scratch.dir().join("data");
    store::save(&legacy.join(DOCUMENT), &graph_with(&["Old task"])).expect("saves");

    let mut list = Vaults::load_or_adopt(&scratch.list(), &legacy).expect("loads");
    list.forget("data").expect("forgets");
    list.save().expect("saves");

    let again = Vaults::load_or_adopt(&scratch.list(), &legacy).expect("reloads");
    assert!(again.recent().is_empty());
}

#[test]
fn undo_copies_live_in_the_vaults_own_folder() {
    let scratch = Scratch::new("undo-place");
    let document = scratch.dir().join(DOCUMENT);
    store::save(&document, &graph_with(&["One"])).expect("saves");
    store::save(&document, &graph_with(&["One", "Two"])).expect("saves");

    assert!(scratch.dir().join(".branchy/undo/graph.json.1").is_file());
    let loose: Vec<String> = fs::read_dir(scratch.dir())
        .expect("lists")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        {
            let mut loose = loose;
            loose.sort();
            loose
        },
        [".branchy", "graph.json"],
        "nothing but the document and the private folder"
    );
    assert_eq!(store::undo_depth(&document), 1);
}

#[test]
fn an_undo_stack_left_by_an_older_version_moves_in_and_still_works() {
    let scratch = Scratch::new("undo-loose");
    let document = scratch.dir().join(DOCUMENT);
    // what 0.1 left behind: the document and its stack side by side
    fs::write(
        &document,
        serde_json_text(&scratch, &graph_with(&["One", "Two", "Three"])),
    )
    .expect("write");
    fs::write(
        scratch.dir().join("graph.json.undo1"),
        serde_json_text(&scratch, &graph_with(&["One", "Two"])),
    )
    .expect("write");
    fs::write(
        scratch.dir().join("graph.json.undo2"),
        serde_json_text(&scratch, &graph_with(&["One"])),
    )
    .expect("write");

    assert_eq!(store::undo_depth(&document), 2);
    store::undo(&document).expect("undoes");
    assert_eq!(names(&document), ["One", "Two"]);
    store::undo(&document).expect("undoes again");
    assert_eq!(names(&document), ["One"]);
    assert!(!scratch.dir().join("graph.json.undo1").exists());
}

/// A document's text as the store writes it, produced through the store so the
/// test does not restate the format.
fn serde_json_text(scratch: &Scratch, graph: &Graph) -> String {
    let staging = scratch.dir().join("staging").join(DOCUMENT);
    store::save(&staging, graph).expect("saves");
    let text = fs::read_to_string(&staging).expect("reads");
    fs::remove_dir_all(scratch.dir().join("staging")).expect("cleans up");
    text
}
