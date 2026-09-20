//! The command layer: applying, undoing, and parsing text into commands.

use branchy_core::{AreaId, Command, Error, Graph, NewArea, NewNode, NodeId, ParseError, Status};

/// A graph with one area named "Hard skills" and the given nodes.
fn fixture(names: &[&str]) -> (Graph, AreaId, Vec<NodeId>) {
    let mut graph = Graph::new();
    let area = graph.add_area(NewArea::new("Hard skills", "#4fd1c5"));
    let ids = names
        .iter()
        .map(|n| graph.add_node(NewNode::new(*n, area)).expect("area exists"))
        .collect();
    (graph, area, ids)
}

/// Everything the graph holds, ignoring the id counters.
///
/// Undo restores content but never rewinds the counters: an id that was handed
/// out must not be handed out again, because another device may already have
/// seen it.
type Contents = (
    Vec<(NodeId, branchy_core::Node)>,
    Vec<(AreaId, branchy_core::Area)>,
);

fn contents(graph: &Graph) -> Contents {
    (
        graph.nodes().map(|(id, n)| (id, n.clone())).collect(),
        graph.areas().map(|(id, a)| (id, a.clone())).collect(),
    )
}

fn parse(graph: &Graph, line: &str) -> Command {
    branchy_core::parse(graph, line).expect("line should parse")
}

fn run(graph: &mut Graph, line: &str) -> NodeId {
    let command = parse(graph, line);
    graph
        .apply(command)
        .expect("command should apply")
        .node
        .expect("touches a node")
}

// ── applying ─────────────────────────────────────────────────────────────

#[test]
fn add_node_command_reports_the_new_id() {
    let (mut graph, area, _) = fixture(&[]);
    let applied = graph
        .apply(Command::AddNode {
            name: "Calculus".into(),
            note: "limits".into(),
            area,
            priority: 7,
            prereqs: [].into(),
            dependents: [].into(),
        })
        .expect("area exists");

    let id = applied.node.expect("a node was created");
    let node = graph.node(id).expect("exists");
    assert_eq!(node.name, "Calculus");
    assert_eq!(node.note, "limits");
    assert_eq!(node.priority, 7);
}

#[test]
fn add_node_can_wire_itself_up_in_one_step() {
    let (mut graph, area, ids) = fixture(&["algebra", "book"]);
    let applied = graph
        .apply(Command::AddNode {
            name: "Calculus".into(),
            note: String::new(),
            area,
            priority: 5,
            prereqs: [ids[0]].into(),
            dependents: [ids[1]].into(),
        })
        .expect("no cycle");

    let id = applied.node.expect("created");
    assert!(graph.node(id).expect("exists").prereqs.contains(&ids[0]));
    assert!(graph.node(ids[1]).expect("exists").prereqs.contains(&id));
}

#[test]
fn add_node_that_would_close_a_loop_creates_nothing() {
    // b already needs a. A new node needing b and needed by a closes the loop.
    let (mut graph, area, ids) = fixture(&["a", "b"]);
    graph.add_prerequisite(ids[1], ids[0]).expect("acyclic");
    let before = graph.node_count();

    let err = graph
        .apply(Command::AddNode {
            name: "middle".into(),
            note: String::new(),
            area,
            priority: 5,
            prereqs: [ids[1]].into(),
            dependents: [ids[0]].into(),
        })
        .unwrap_err();

    assert!(matches!(err, Error::WouldCycle { .. }));
    assert_eq!(graph.node_count(), before, "nothing was half-created");
}

#[test]
fn add_node_referring_to_a_missing_node_is_refused() {
    let (mut graph, area, _) = fixture(&[]);
    let ghost = NodeId::new(77);
    let err = graph
        .apply(Command::AddNode {
            name: "x".into(),
            note: String::new(),
            area,
            priority: 5,
            prereqs: [ghost].into(),
            dependents: [].into(),
        })
        .unwrap_err();
    assert_eq!(err, Error::NoSuchNode(ghost));
}

// ── undo ─────────────────────────────────────────────────────────────────

#[test]
fn undo_of_set_done_restores_the_old_value() {
    let (mut graph, _, ids) = fixture(&["a"]);
    let applied = graph
        .apply(Command::SetDone {
            node: ids[0],
            done: true,
        })
        .expect("exists");
    assert_eq!(graph.status(ids[0]), Some(Status::Done));

    graph.apply_all(applied.undo).expect("undo applies");
    assert_eq!(graph.status(ids[0]), Some(Status::Available));
}

#[test]
fn undo_of_remove_node_brings_back_its_edges() {
    let (mut graph, _, ids) = fixture(&["a", "b", "c"]);
    graph.add_prerequisite(ids[1], ids[0]).expect("acyclic");
    graph.add_prerequisite(ids[2], ids[1]).expect("acyclic");
    let before = contents(&graph);

    let applied = graph.apply(Command::RemoveNode(ids[1])).expect("exists");
    assert!(graph.node(ids[1]).is_none());
    assert!(graph.node(ids[2]).expect("exists").prereqs.is_empty());

    graph.apply_all(applied.undo).expect("undo applies");
    assert_eq!(
        contents(&graph),
        before,
        "the graph holds exactly what it did"
    );
}

#[test]
fn undo_of_add_node_removes_it_again() {
    let (mut graph, area, ids) = fixture(&["a"]);
    let before = contents(&graph);

    let applied = graph
        .apply(Command::AddNode {
            name: "new".into(),
            note: String::new(),
            area,
            priority: 5,
            prereqs: [ids[0]].into(),
            dependents: [].into(),
        })
        .expect("no cycle");
    let created = applied.node.expect("created");

    graph.apply_all(applied.undo).expect("undo applies");
    assert_eq!(contents(&graph), before);

    // the id is retired, not recycled
    let next = graph
        .apply(Command::AddNode {
            name: "later".into(),
            note: String::new(),
            area,
            priority: 5,
            prereqs: [].into(),
            dependents: [].into(),
        })
        .expect("applies")
        .node
        .expect("created");
    assert_ne!(next, created);
}

#[test]
fn undo_of_a_prerequisite_that_was_already_there_does_nothing() {
    let (mut graph, _, ids) = fixture(&["a", "b"]);
    graph.add_prerequisite(ids[1], ids[0]).expect("acyclic");

    let applied = graph
        .apply(Command::AddPrerequisite {
            dependent: ids[1],
            prerequisite: ids[0],
        })
        .expect("already present is not an error");
    assert!(
        applied.undo.is_empty(),
        "undo must not remove a pre-existing edge"
    );

    graph.apply_all(applied.undo).expect("nothing to do");
    assert!(
        graph
            .node(ids[1])
            .expect("exists")
            .prereqs
            .contains(&ids[0])
    );
}

#[test]
fn a_sequence_undoes_in_reverse() {
    let (mut graph, area, _) = fixture(&[]);
    let before = contents(&graph);

    let undo = graph
        .apply_all([
            Command::AddNode {
                name: "one".into(),
                note: String::new(),
                area,
                priority: 1,
                prereqs: [].into(),
                dependents: [].into(),
            },
            Command::AddNode {
                name: "two".into(),
                note: String::new(),
                area,
                priority: 2,
                prereqs: [].into(),
                dependents: [].into(),
            },
        ])
        .expect("both apply");
    assert_eq!(graph.node_count(), 2);

    graph.apply_all(undo).expect("undo applies");
    assert_eq!(contents(&graph), before);
}

#[test]
fn a_failing_sequence_reports_what_had_already_happened() {
    let (mut graph, _, ids) = fixture(&["a"]);
    let ghost = NodeId::new(404);

    let (error, undo) = graph
        .apply_all([
            Command::SetPriority {
                node: ids[0],
                priority: 9,
            },
            Command::SetDone {
                node: ghost,
                done: true,
            },
        ])
        .unwrap_err();

    assert_eq!(error, Error::NoSuchNode(ghost));
    assert_eq!(graph.node(ids[0]).expect("exists").priority, 9);
    graph.apply_all(undo).expect("undo applies");
    assert_eq!(graph.node(ids[0]).expect("exists").priority, 5);
}

// ── parsing ──────────────────────────────────────────────────────────────

#[test]
fn add_parses_every_clause() {
    let (mut graph, _, ids) = fixture(&["algebra", "linear algebra", "book"]);
    let command = parse(
        &graph,
        r#"add "Probability theory" in hard after algebra, linear before book pri 8"#,
    );

    match command {
        Command::AddNode {
            ref name,
            priority,
            ref prereqs,
            ref dependents,
            ..
        } => {
            assert_eq!(name, "Probability theory");
            assert_eq!(priority, 8);
            assert_eq!(prereqs, &[ids[0], ids[1]].into());
            assert_eq!(dependents, &[ids[2]].into());
        }
        other => panic!("expected AddNode, got {other:?}"),
    }
    graph.apply(command).expect("applies");
    assert_eq!(graph.node_count(), 4);
}

#[test]
fn needs_and_blocks_are_the_other_spellings() {
    let (graph, _, ids) = fixture(&["a", "b"]);
    assert_eq!(
        parse(&graph, "link a after b"),
        parse(&graph, "link a needs b")
    );
    // "a before b" is the same edge as "b after a"
    assert_eq!(
        parse(&graph, "link a before b"),
        Command::AddPrerequisite {
            dependent: ids[1],
            prerequisite: ids[0],
        }
    );
    assert_eq!(
        parse(&graph, "link a blocks b"),
        parse(&graph, "link a before b")
    );
}

#[test]
fn simple_verbs_parse() {
    let (graph, _, ids) = fixture(&["algebra"]);
    assert_eq!(
        parse(&graph, "done algebra"),
        Command::SetDone {
            node: ids[0],
            done: true
        }
    );
    assert_eq!(
        parse(&graph, "undone algebra"),
        Command::SetDone {
            node: ids[0],
            done: false
        }
    );
    assert_eq!(parse(&graph, "rm algebra"), Command::RemoveNode(ids[0]));
    assert_eq!(
        parse(&graph, "pri algebra 9"),
        Command::SetPriority {
            node: ids[0],
            priority: 9
        }
    );
    assert_eq!(
        parse(&graph, r#"rename algebra "School algebra""#),
        Command::SetName {
            node: ids[0],
            name: "School algebra".into()
        }
    );
}

#[test]
fn a_node_is_found_by_its_printed_id() {
    let (graph, _, ids) = fixture(&["algebra"]);
    let reference = ids[0].to_string();
    assert_eq!(
        branchy_core::resolve_node(&graph, &reference).expect("resolves"),
        ids[0]
    );
}

#[test]
fn an_ambiguous_reference_lists_the_candidates() {
    let (graph, _, ids) = fixture(&["calculus", "calculus of variations"]);
    match branchy_core::parse(&graph, "done calc").unwrap_err() {
        ParseError::AmbiguousTask { query, matches } => {
            assert_eq!(query, "calc");
            assert_eq!(matches, vec![ids[0], ids[1]]);
        }
        other => panic!("expected ambiguity, got {other:?}"),
    }
    // the exact name still wins outright
    assert_eq!(
        branchy_core::resolve_node(&graph, "calculus").expect("exact match wins"),
        ids[0]
    );
}

#[test]
fn bad_lines_are_refused_clearly() {
    use ParseError as P;

    let (graph, _, _) = fixture(&["a"]);
    assert_eq!(branchy_core::parse(&graph, "   ").unwrap_err(), P::Empty);
    assert_eq!(
        branchy_core::parse(&graph, "frobnicate a").unwrap_err(),
        P::UnknownVerb("frobnicate".into())
    );
    assert_eq!(
        branchy_core::parse(&graph, "add").unwrap_err(),
        P::MissingName
    );
    assert_eq!(
        branchy_core::parse(&graph, "link a").unwrap_err(),
        P::MissingSeparator
    );
    assert_eq!(
        branchy_core::parse(&graph, "pri a lots").unwrap_err(),
        P::BadPriority("lots".into())
    );
    assert_eq!(
        branchy_core::parse(&graph, "done nowhere").unwrap_err(),
        P::NoSuchTask("nowhere".into())
    );
}

#[test]
fn area_commands_parse() {
    let mut graph = Graph::new();
    let command = parse(&graph, r#"area "Hard skills" #4fd1c5"#);
    assert_eq!(
        command,
        Command::AddArea {
            name: "Hard skills".into(),
            color: "#4fd1c5".into()
        }
    );
    graph.apply(command).expect("applies");
    assert_eq!(graph.area_count(), 1);
}

#[test]
fn creating_a_node_before_any_area_says_so() {
    let graph = Graph::new();
    assert_eq!(
        branchy_core::parse(&graph, "add something").unwrap_err(),
        ParseError::NoAreas
    );
}

#[test]
fn a_typed_cycle_is_refused_with_its_loop() {
    let (mut graph, _, ids) = fixture(&["a", "b"]);
    run(&mut graph, "link b after a");
    let command = parse(&graph, "link a after b");
    match graph.apply(command).unwrap_err() {
        Error::WouldCycle { path, .. } => assert_eq!(path, vec![ids[1], ids[0], ids[1]]),
        other => panic!("expected a cycle, got {other:?}"),
    }
}

#[test]
fn a_whole_session_typed_out() {
    let mut graph = Graph::new();
    for line in [
        r#"area "Hard skills" #4fd1c5"#,
        r#"add "School algebra" pri 3"#,
        r#"add "Calculus" after school pri 6"#,
        r#"add "Probability theory" after calculus pri 8"#,
        "done school",
    ] {
        let command = parse(&graph, line);
        graph.apply(command).expect("applies");
    }

    assert_eq!(graph.node_count(), 3);
    let queue = graph.queue();
    assert_eq!(queue.len(), 1, "only calculus is available");
    assert_eq!(graph.node(queue[0]).expect("exists").name, "Calculus");

    let probability = branchy_core::resolve_node(&graph, "probability").expect("resolves");
    let path = graph.path_to_unlock(probability).expect("no cycle");
    assert_eq!(path.len(), 1);
    assert_eq!(graph.node(path[0]).expect("exists").name, "Calculus");
}

// ── directions can be edited, not only created ───────────────────────────

#[test]
fn an_area_can_be_renamed_and_recoloured() {
    let (mut graph, area, _) = fixture(&[]);

    let command = parse(&graph, r#"rename-area hard "Deep work""#);
    let applied = graph.apply(command).expect("area exists");
    assert_eq!(graph.area(area).expect("exists").name, "Deep work");

    let command = parse(&graph, "recolor-area deep #ef8093");
    graph.apply(command).expect("area exists");
    assert_eq!(graph.area(area).expect("exists").color, "#ef8093");

    // and the rename undoes
    graph.apply_all(applied.undo).expect("undo applies");
    assert_eq!(graph.area(area).expect("exists").name, "Hard skills");
}

// ── quoting: a front end builds lines out of form fields ─────────────────

#[test]
fn a_name_holding_a_quote_survives_the_round_trip() {
    let (mut graph, area, _) = fixture(&[]);
    let awkward = r#"Read "Flash Boys" \ notes"#;

    let line = format!("add {} in hard", branchy_core::quote(awkward));
    let command = parse(&graph, &line);
    let id = graph
        .apply(command)
        .expect("applies")
        .node
        .expect("created");

    assert_eq!(graph.node(id).expect("exists").name, awkward);
    assert_eq!(graph.node(id).expect("exists").area, area);
}

#[test]
fn quoting_handles_the_empty_string() {
    let (mut graph, _, ids) = fixture(&["a"]);
    let line = format!("note {} {}", ids[0], branchy_core::quote(""));
    let command = parse(&graph, &line);
    graph.apply(command).expect("applies");
    assert_eq!(graph.node(ids[0]).expect("exists").note, "");
}

#[test]
fn a_note_with_spaces_and_punctuation_round_trips() {
    let (mut graph, _, ids) = fixture(&["a"]);
    let note = "Two a day, until they stop hurting -- then three.";
    let line = format!("note {} {}", ids[0], branchy_core::quote(note));
    let command = parse(&graph, &line);
    graph.apply(command).expect("applies");
    assert_eq!(graph.node(ids[0]).expect("exists").note, note);
}
