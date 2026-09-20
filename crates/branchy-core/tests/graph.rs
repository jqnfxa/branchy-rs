//! Behaviour of the graph, exercised through the public API only.

use std::collections::BTreeMap;

use branchy_core::{Error, Graph, NewArea, NewNode, NodeId, Status};

/// A graph with one area and the named nodes, none of them done.
fn fixture(names: &[&str]) -> (Graph, BTreeMap<String, NodeId>) {
    let mut graph = Graph::new();
    let area = graph.add_area(NewArea::new("Hard skills", "#4fd1c5"));
    let mut ids = BTreeMap::new();
    for name in names {
        let id = graph
            .add_node(NewNode::new(*name, area))
            .expect("area exists");
        ids.insert((*name).to_string(), id);
    }
    (graph, ids)
}

// ── construction and lookup ──────────────────────────────────────────────

#[test]
fn new_graph_is_empty() {
    let graph = Graph::new();
    assert!(graph.is_empty());
    assert_eq!(graph.node_count(), 0);
    assert_eq!(graph.area_count(), 0);
    assert!(graph.queue().is_empty());
    assert!(graph.find_cycles().is_empty());
}

#[test]
fn added_node_is_retrievable_and_starts_locked_free() {
    let (graph, ids) = fixture(&["a"]);
    let a = ids["a"];
    let node = graph.node(a).expect("just added");
    assert_eq!(node.name, "a");
    assert_eq!(node.priority, 5);
    assert!(!node.done);
    assert!(node.prereqs.is_empty());
    // nothing to wait for, so it is immediately available
    assert_eq!(graph.status(a), Some(Status::Available));
    assert_eq!(graph.tier(a), Some(0));
}

#[test]
fn missing_node_is_none_not_an_error() {
    let graph = Graph::new();
    assert!(graph.node(NodeId::new(42)).is_none());
    assert!(graph.status(NodeId::new(42)).is_none());
    assert!(graph.tier(NodeId::new(42)).is_none());
}

#[test]
fn node_in_unknown_area_is_refused() {
    let mut graph = Graph::new();
    let orphan = graph.add_area(NewArea::new("gone", "#000"));
    graph.remove_area(orphan).expect("empty area");
    let err = graph.add_node(NewNode::new("x", orphan)).unwrap_err();
    assert_eq!(err, Error::NoSuchArea(orphan));
}

#[test]
fn area_holding_nodes_cannot_be_removed() {
    let (mut graph, ids) = fixture(&["a"]);
    let area = graph.node(ids["a"]).expect("exists").area;
    let err = graph.remove_area(area).unwrap_err();
    assert_eq!(err, Error::AreaNotEmpty { area, nodes: 1 });

    graph.remove_node(ids["a"]).expect("exists");
    assert!(graph.remove_area(area).is_ok());
}

#[test]
fn builder_sets_note_and_priority() {
    let mut graph = Graph::new();
    let area = graph.add_area(NewArea::new("a", "#fff"));
    let id = graph
        .add_node(NewNode::new("x", area).with_note("why").with_priority(9))
        .expect("area exists");
    let node = graph.node(id).expect("just added");
    assert_eq!(node.note, "why");
    assert_eq!(node.priority, 9);
}

// ── edges ────────────────────────────────────────────────────────────────

#[test]
fn prerequisite_is_recorded_once() {
    let (mut graph, ids) = fixture(&["a", "b"]);
    graph.add_prerequisite(ids["b"], ids["a"]).expect("acyclic");
    graph
        .add_prerequisite(ids["b"], ids["a"])
        .expect("adding twice is not an error");
    assert_eq!(graph.node(ids["b"]).expect("exists").prereqs.len(), 1);
    assert_eq!(graph.dependents(ids["a"]), vec![ids["b"]]);
}

#[test]
fn prerequisite_on_missing_node_is_refused() {
    let (mut graph, ids) = fixture(&["a"]);
    let ghost = NodeId::new(999);
    assert_eq!(
        graph.add_prerequisite(ids["a"], ghost).unwrap_err(),
        Error::NoSuchNode(ghost)
    );
    assert_eq!(
        graph.add_prerequisite(ghost, ids["a"]).unwrap_err(),
        Error::NoSuchNode(ghost)
    );
}

#[test]
fn removing_an_absent_edge_is_an_error() {
    let (mut graph, ids) = fixture(&["a", "b"]);
    assert_eq!(
        graph.remove_prerequisite(ids["b"], ids["a"]).unwrap_err(),
        Error::NotAPrerequisite {
            dependent: ids["b"],
            prerequisite: ids["a"],
        }
    );
}

#[test]
fn removing_a_node_strips_it_from_dependents() {
    let (mut graph, ids) = fixture(&["a", "b", "c"]);
    graph.add_prerequisite(ids["b"], ids["a"]).expect("acyclic");
    graph.add_prerequisite(ids["c"], ids["a"]).expect("acyclic");

    let gone = graph.remove_node(ids["a"]).expect("exists");
    assert_eq!(gone.name, "a");
    assert!(graph.node(ids["b"]).expect("exists").prereqs.is_empty());
    assert!(graph.node(ids["c"]).expect("exists").prereqs.is_empty());
    // with nothing left to wait for, both become available
    assert_eq!(graph.status(ids["b"]), Some(Status::Available));
    assert_eq!(
        graph.remove_node(ids["a"]).unwrap_err(),
        Error::NoSuchNode(ids["a"])
    );
}

// ── cycles ───────────────────────────────────────────────────────────────

#[test]
fn a_node_cannot_need_itself() {
    let (mut graph, ids) = fixture(&["a"]);
    let a = ids["a"];
    match graph.add_prerequisite(a, a).unwrap_err() {
        Error::WouldCycle { path, .. } => assert_eq!(path, vec![a, a]),
        other => panic!("expected a cycle, got {other:?}"),
    }
}

#[test]
fn direct_cycle_is_refused_with_its_path() {
    // b needs a; a must then not be allowed to need b
    let (mut graph, ids) = fixture(&["a", "b"]);
    graph.add_prerequisite(ids["b"], ids["a"]).expect("acyclic");

    match graph.add_prerequisite(ids["a"], ids["b"]).unwrap_err() {
        Error::WouldCycle {
            dependent,
            prerequisite,
            path,
        } => {
            assert_eq!(dependent, ids["a"]);
            assert_eq!(prerequisite, ids["b"]);
            // reads as "needs", and closes back on itself
            assert_eq!(path, vec![ids["b"], ids["a"], ids["b"]]);
        }
        other => panic!("expected a cycle, got {other:?}"),
    }
    assert!(graph.node(ids["a"]).expect("exists").prereqs.is_empty());
}

#[test]
fn long_cycle_is_refused() {
    // d needs c needs b needs a; a must not be allowed to need d
    let (mut graph, ids) = fixture(&["a", "b", "c", "d"]);
    graph.add_prerequisite(ids["b"], ids["a"]).expect("acyclic");
    graph.add_prerequisite(ids["c"], ids["b"]).expect("acyclic");
    graph.add_prerequisite(ids["d"], ids["c"]).expect("acyclic");

    match graph.add_prerequisite(ids["a"], ids["d"]).unwrap_err() {
        Error::WouldCycle { path, .. } => {
            assert_eq!(path, vec![ids["d"], ids["c"], ids["b"], ids["a"], ids["d"]]);
        }
        other => panic!("expected a cycle, got {other:?}"),
    }
}

#[test]
fn a_diamond_is_not_a_cycle() {
    // b and c both need a; d needs both. Perfectly legal.
    let (mut graph, ids) = fixture(&["a", "b", "c", "d"]);
    graph.add_prerequisite(ids["b"], ids["a"]).expect("acyclic");
    graph.add_prerequisite(ids["c"], ids["a"]).expect("acyclic");
    graph.add_prerequisite(ids["d"], ids["b"]).expect("acyclic");
    graph.add_prerequisite(ids["d"], ids["c"]).expect("acyclic");

    assert!(graph.find_cycles().is_empty());
    assert_eq!(graph.tier(ids["d"]), Some(2));
}

#[test]
fn a_merged_cycle_is_tolerated_and_reported() {
    // what two offline devices produce: each edge legal alone, cyclic together
    let (mut graph, ids) = fixture(&["a", "b", "c"]);
    graph.add_prerequisite(ids["b"], ids["a"]).expect("acyclic");
    graph
        .add_prerequisite_unchecked(ids["a"], ids["b"])
        .expect("both nodes exist");

    let cycles = graph.find_cycles();
    assert_eq!(cycles.len(), 1);
    let mut expected = vec![ids["a"], ids["b"]];
    expected.sort_unstable();
    assert_eq!(cycles[0], expected);

    // derived values still terminate, and say what is wrong
    assert_eq!(graph.status(ids["a"]), Some(Status::Cyclic));
    assert_eq!(graph.status(ids["b"]), Some(Status::Cyclic));
    assert_eq!(graph.tier(ids["a"]), None);
    // c is untouched
    assert_eq!(graph.status(ids["c"]), Some(Status::Available));
    assert_eq!(graph.tier(ids["c"]), Some(0));

    // breaking one edge repairs it
    graph
        .remove_prerequisite(ids["a"], ids["b"])
        .expect("edge is there");
    assert!(graph.find_cycles().is_empty());
    assert_eq!(graph.status(ids["a"]), Some(Status::Available));
}

#[test]
fn nodes_downstream_of_a_cycle_are_cyclic_too() {
    let (mut graph, ids) = fixture(&["a", "b", "downstream"]);
    graph.add_prerequisite(ids["b"], ids["a"]).expect("acyclic");
    graph
        .add_prerequisite_unchecked(ids["a"], ids["b"])
        .expect("exists");
    graph
        .add_prerequisite(ids["downstream"], ids["b"])
        .expect("does not itself close a loop");

    assert_eq!(graph.status(ids["downstream"]), Some(Status::Cyclic));
    assert_eq!(graph.tier(ids["downstream"]), None);
    // the cycle itself is only a and b
    assert_eq!(graph.find_cycles().len(), 1);
    assert_eq!(graph.find_cycles()[0].len(), 2);
}

#[test]
fn self_loop_from_a_merge_is_found() {
    let (mut graph, ids) = fixture(&["a"]);
    graph
        .add_prerequisite_unchecked(ids["a"], ids["a"])
        .expect("exists");
    assert_eq!(graph.find_cycles(), vec![vec![ids["a"]]]);
    assert_eq!(graph.status(ids["a"]), Some(Status::Cyclic));
}

// ── derived values ───────────────────────────────────────────────────────

#[test]
fn tier_is_one_more_than_the_deepest_prerequisite() {
    let (mut graph, ids) = fixture(&["a", "b", "c", "d"]);
    // d needs c (tier 1 via b) and a (tier 0) => d is tier 2
    graph.add_prerequisite(ids["b"], ids["a"]).expect("acyclic");
    graph.add_prerequisite(ids["c"], ids["b"]).expect("acyclic");
    graph.add_prerequisite(ids["d"], ids["c"]).expect("acyclic");
    graph.add_prerequisite(ids["d"], ids["a"]).expect("acyclic");

    assert_eq!(graph.tier(ids["a"]), Some(0));
    assert_eq!(graph.tier(ids["b"]), Some(1));
    assert_eq!(graph.tier(ids["c"]), Some(2));
    assert_eq!(graph.tier(ids["d"]), Some(3));
}

#[test]
fn status_follows_the_prerequisites() {
    let (mut graph, ids) = fixture(&["a", "b"]);
    graph.add_prerequisite(ids["b"], ids["a"]).expect("acyclic");

    assert_eq!(graph.status(ids["a"]), Some(Status::Available));
    assert_eq!(graph.status(ids["b"]), Some(Status::Locked));

    graph.set_done(ids["a"], true).expect("exists");
    assert_eq!(graph.status(ids["a"]), Some(Status::Done));
    assert_eq!(graph.status(ids["b"]), Some(Status::Available));

    graph.set_done(ids["a"], false).expect("exists");
    assert_eq!(graph.status(ids["b"]), Some(Status::Locked));
}

#[test]
fn done_wins_over_unmet_prerequisites() {
    // importing history has to be able to say "this happened", out of order
    let (mut graph, ids) = fixture(&["a", "b"]);
    graph.add_prerequisite(ids["b"], ids["a"]).expect("acyclic");
    graph.set_done(ids["b"], true).expect("exists");
    assert_eq!(graph.status(ids["b"]), Some(Status::Done));
}

#[test]
fn queue_holds_available_nodes_by_priority() {
    let (mut graph, ids) = fixture(&["low", "high", "blocked", "middle"]);
    graph.set_priority(ids["low"], 1).expect("exists");
    graph.set_priority(ids["middle"], 5).expect("exists");
    graph.set_priority(ids["high"], 9).expect("exists");
    graph
        .add_prerequisite(ids["blocked"], ids["low"])
        .expect("acyclic");

    assert_eq!(
        graph.queue(),
        vec![ids["high"], ids["middle"], ids["low"]],
        "blocked is absent, the rest run highest priority first"
    );

    graph.set_done(ids["low"], true).expect("exists");
    assert!(graph.queue().contains(&ids["blocked"]));
    assert!(!graph.queue().contains(&ids["low"]));
}

#[test]
fn queue_breaks_ties_by_name_so_the_order_is_stable() {
    let (mut graph, ids) = fixture(&["zebra", "apple", "mango"]);
    for name in ["zebra", "apple", "mango"] {
        graph.set_priority(ids[name], 5).expect("exists");
    }
    assert_eq!(
        graph.queue(),
        vec![ids["apple"], ids["mango"], ids["zebra"]]
    );
}

#[test]
fn path_to_unlock_lists_the_work_in_order() {
    let (mut graph, ids) = fixture(&["algebra", "calculus", "probability", "book"]);
    graph
        .add_prerequisite(ids["calculus"], ids["algebra"])
        .expect("acyclic");
    graph
        .add_prerequisite(ids["probability"], ids["calculus"])
        .expect("acyclic");
    graph
        .add_prerequisite(ids["book"], ids["probability"])
        .expect("acyclic");

    let path = graph.path_to_unlock(ids["book"]).expect("no cycle");
    assert_eq!(
        path,
        vec![ids["algebra"], ids["calculus"], ids["probability"]],
        "deepest prerequisite first, so the list can be worked top to bottom"
    );

    // finishing part of it shortens the list
    graph.set_done(ids["algebra"], true).expect("exists");
    assert_eq!(
        graph.path_to_unlock(ids["book"]).expect("no cycle"),
        vec![ids["calculus"], ids["probability"]]
    );
}

#[test]
fn path_to_unlock_is_empty_for_something_already_available() {
    let (graph, ids) = fixture(&["a"]);
    assert!(graph.path_to_unlock(ids["a"]).expect("no cycle").is_empty());
}

#[test]
fn path_to_unlock_does_not_walk_past_finished_work() {
    // buried needs done_one, which needs deep. done_one is finished, so what it
    // needed is no longer anybody's problem.
    let (mut graph, ids) = fixture(&["deep", "done_one", "buried"]);
    graph
        .add_prerequisite(ids["done_one"], ids["deep"])
        .expect("acyclic");
    graph
        .add_prerequisite(ids["buried"], ids["done_one"])
        .expect("acyclic");
    graph.set_done(ids["done_one"], true).expect("exists");

    assert!(
        graph
            .path_to_unlock(ids["buried"])
            .expect("no cycle")
            .is_empty()
    );
}

#[test]
fn path_to_unlock_refuses_a_node_behind_a_cycle() {
    let (mut graph, ids) = fixture(&["a", "b", "wanted"]);
    graph.add_prerequisite(ids["b"], ids["a"]).expect("acyclic");
    graph
        .add_prerequisite_unchecked(ids["a"], ids["b"])
        .expect("exists");
    graph
        .add_prerequisite(ids["wanted"], ids["b"])
        .expect("does not itself close a loop");

    assert_eq!(
        graph.path_to_unlock(ids["wanted"]).unwrap_err(),
        Error::CycleBlocks(ids["wanted"])
    );
}

#[test]
fn iteration_order_is_stable_across_runs() {
    let (graph, _) = fixture(&["c", "a", "b"]);
    let once: Vec<&str> = graph.nodes().map(|(_, n)| n.name.as_str()).collect();
    let twice: Vec<&str> = graph.nodes().map(|(_, n)| n.name.as_str()).collect();
    assert_eq!(once, twice);
    // id order, which is insertion order here, not name order
    assert_eq!(once, vec!["c", "a", "b"]);
}

#[test]
fn errors_render_readably() {
    let (mut graph, ids) = fixture(&["a", "b"]);
    graph.add_prerequisite(ids["b"], ids["a"]).expect("acyclic");
    let err = graph.add_prerequisite(ids["a"], ids["b"]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "n0 cannot need n1: that closes a cycle n1 -> n0 -> n1"
    );
}
