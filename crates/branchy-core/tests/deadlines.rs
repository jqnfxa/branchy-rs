//! Deadlines, and the way they reach backwards through the graph.

use branchy_core::{Command, Date, Graph, NewArea, NewNode, NodeId, Status};

fn d(text: &str) -> Date {
    text.parse().expect("a real date")
}

fn fixture(names: &[&str]) -> (Graph, Vec<NodeId>) {
    let mut graph = Graph::new();
    let area = graph.add_area(NewArea::new("Work", "#4fd1c5"));
    let ids = names
        .iter()
        .map(|n| graph.add_node(NewNode::new(*n, area)).expect("area exists"))
        .collect();
    (graph, ids)
}

fn parse_apply(graph: &mut Graph, line: &str) {
    let command = branchy_core::parse(graph, line).expect("line parses");
    graph.apply(command).expect("command applies");
}

#[test]
fn a_deadline_does_not_change_status() {
    let (mut graph, ids) = fixture(&["a", "b"]);
    graph.add_prerequisite(ids[1], ids[0]).expect("acyclic");
    graph
        .set_due(ids[1], Some(d("2026-01-01")))
        .expect("exists");

    // a date on a task does not unblock it, and does not block it either
    assert_eq!(graph.status(ids[1]), Some(Status::Locked));
    assert_eq!(graph.status(ids[0]), Some(Status::Available));
}

#[test]
fn a_deadline_reaches_back_to_prerequisites() {
    // goal needs middle needs base; only goal has a date
    let (mut graph, ids) = fixture(&["base", "middle", "goal"]);
    graph.add_prerequisite(ids[1], ids[0]).expect("acyclic");
    graph.add_prerequisite(ids[2], ids[1]).expect("acyclic");
    graph
        .set_due(ids[2], Some(d("2027-03-01")))
        .expect("exists");

    let due = graph.effective_due();
    assert_eq!(due.get(&ids[2]), Some(&d("2027-03-01")));
    assert_eq!(due.get(&ids[1]), Some(&d("2027-03-01")), "inherited");
    assert_eq!(due.get(&ids[0]), Some(&d("2027-03-01")), "inherited twice");

    // and nothing was written down: the nodes themselves are untouched
    assert_eq!(graph.node(ids[0]).expect("exists").due, None);
    assert_eq!(graph.node(ids[1]).expect("exists").due, None);
}

#[test]
fn the_earliest_deadline_wins_when_dependents_disagree() {
    // both goals need base; base inherits whichever comes first
    let (mut graph, ids) = fixture(&["base", "march goal", "june goal"]);
    graph.add_prerequisite(ids[1], ids[0]).expect("acyclic");
    graph.add_prerequisite(ids[2], ids[0]).expect("acyclic");
    graph
        .set_due(ids[1], Some(d("2027-03-01")))
        .expect("exists");
    graph
        .set_due(ids[2], Some(d("2027-06-01")))
        .expect("exists");

    assert_eq!(graph.effective_due().get(&ids[0]), Some(&d("2027-03-01")));
}

#[test]
fn an_own_deadline_wins_over_an_inherited_later_one() {
    let (mut graph, ids) = fixture(&["base", "goal"]);
    graph.add_prerequisite(ids[1], ids[0]).expect("acyclic");
    graph
        .set_due(ids[1], Some(d("2027-06-01")))
        .expect("exists");
    graph
        .set_due(ids[0], Some(d("2027-01-01")))
        .expect("exists");

    assert_eq!(graph.effective_due().get(&ids[0]), Some(&d("2027-01-01")));
}

#[test]
fn a_deadline_does_not_reach_forwards() {
    // finishing the base early says nothing about when the goal is due
    let (mut graph, ids) = fixture(&["base", "goal"]);
    graph.add_prerequisite(ids[1], ids[0]).expect("acyclic");
    graph
        .set_due(ids[0], Some(d("2026-01-01")))
        .expect("exists");

    assert_eq!(graph.effective_due().get(&ids[1]), None);
}

#[test]
fn propagation_crosses_a_whole_chain_in_one_pass() {
    let (mut graph, ids) = fixture(&["t0", "t1", "t2", "t3", "t4", "t5"]);
    for step in 1..ids.len() {
        graph
            .add_prerequisite(ids[step], ids[step - 1])
            .expect("acyclic");
    }
    graph
        .set_due(ids[ids.len() - 1], Some(d("2027-12-31")))
        .expect("exists");

    let due = graph.effective_due();
    for id in &ids {
        assert_eq!(due.get(id), Some(&d("2027-12-31")), "{id} missed it");
    }
}

#[test]
fn a_cycle_does_not_stop_the_calculation() {
    let (mut graph, ids) = fixture(&["a", "b", "clean"]);
    graph.add_prerequisite(ids[1], ids[0]).expect("acyclic");
    graph
        .add_prerequisite_unchecked(ids[0], ids[1])
        .expect("both exist");
    graph
        .set_due(ids[0], Some(d("2027-01-01")))
        .expect("exists");
    graph
        .set_due(ids[2], Some(d("2027-02-01")))
        .expect("exists");

    let due = graph.effective_due();
    assert_eq!(due.get(&ids[0]), Some(&d("2027-01-01")), "kept its own");
    assert_eq!(due.get(&ids[2]), Some(&d("2027-02-01")));
}

// ── the queue ────────────────────────────────────────────────────────────

#[test]
fn a_deadline_outranks_a_priority() {
    let (mut graph, ids) = fixture(&["urgent but dull", "important but open"]);
    graph.set_priority(ids[0], 1).expect("exists");
    graph.set_priority(ids[1], 10).expect("exists");
    graph
        .set_due(ids[0], Some(d("2026-10-01")))
        .expect("exists");

    assert_eq!(
        graph.queue(),
        vec![ids[0], ids[1]],
        "a date the world imposed beats a number somebody picked"
    );
}

#[test]
fn the_nearest_deadline_comes_first() {
    let (mut graph, ids) = fixture(&["late", "soon", "middle"]);
    graph
        .set_due(ids[0], Some(d("2027-01-01")))
        .expect("exists");
    graph
        .set_due(ids[1], Some(d("2026-10-01")))
        .expect("exists");
    graph
        .set_due(ids[2], Some(d("2026-12-01")))
        .expect("exists");
    assert_eq!(graph.queue(), vec![ids[1], ids[2], ids[0]]);
}

#[test]
fn without_any_deadline_the_queue_is_unchanged() {
    let (mut graph, ids) = fixture(&["low", "high", "middle"]);
    graph.set_priority(ids[0], 1).expect("exists");
    graph.set_priority(ids[1], 9).expect("exists");
    graph.set_priority(ids[2], 5).expect("exists");
    assert_eq!(graph.queue(), vec![ids[1], ids[2], ids[0]]);
}

#[test]
fn an_inherited_deadline_moves_a_prerequisite_up_the_queue() {
    // base has nothing on it, but the goal it feeds is due next month
    let (mut graph, ids) = fixture(&["base", "goal", "unrelated"]);
    graph.add_prerequisite(ids[1], ids[0]).expect("acyclic");
    graph.set_priority(ids[0], 1).expect("exists");
    graph.set_priority(ids[2], 9).expect("exists");
    graph
        .set_due(ids[1], Some(d("2026-10-01")))
        .expect("exists");

    assert_eq!(
        graph.queue(),
        vec![ids[0], ids[2]],
        "the base is urgent because of what it blocks, not what it is"
    );
}

// ── the grammar ──────────────────────────────────────────────────────────

#[test]
fn a_deadline_can_be_typed_set_and_cleared() {
    let (mut graph, ids) = fixture(&["ship it"]);
    parse_apply(&mut graph, &format!("due {} 2027-03-01", ids[0]));
    assert_eq!(
        graph.node(ids[0]).expect("exists").due,
        Some(d("2027-03-01"))
    );

    parse_apply(&mut graph, &format!("due {} none", ids[0]));
    assert_eq!(graph.node(ids[0]).expect("exists").due, None);
}

#[test]
fn add_can_carry_a_deadline() {
    let (mut graph, _) = fixture(&[]);
    let command = branchy_core::parse(&graph, r#"add "Interviews" in work due 2027-03-01 pri 9"#)
        .expect("parses");
    let id = graph
        .apply(command)
        .expect("applies")
        .node
        .expect("created");
    assert_eq!(graph.node(id).expect("exists").due, Some(d("2027-03-01")));
    assert_eq!(graph.node(id).expect("exists").priority, 9);
}

#[test]
fn a_date_that_does_not_exist_is_refused() {
    let (graph, ids) = fixture(&["a"]);
    for bad in ["2026-02-30", "2026-13-01", "next tuesday", "01-01-2026"] {
        let line = format!("due {} {bad}", ids[0]);
        assert!(
            branchy_core::parse(&graph, &line).is_err(),
            "{bad} should not parse"
        );
    }
}

#[test]
fn setting_a_deadline_undoes() {
    let (mut graph, ids) = fixture(&["a"]);
    graph
        .set_due(ids[0], Some(d("2026-01-01")))
        .expect("exists");

    let applied = graph
        .apply(Command::SetDue {
            node: ids[0],
            due: Some(d("2027-01-01")),
        })
        .expect("applies");
    assert_eq!(
        graph.node(ids[0]).expect("exists").due,
        Some(d("2027-01-01"))
    );

    graph.apply_all(applied.undo).expect("undo applies");
    assert_eq!(
        graph.node(ids[0]).expect("exists").due,
        Some(d("2026-01-01"))
    );
}
