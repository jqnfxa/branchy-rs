//! Printing a graph for a program to read, or for an agent paying per token.
//!
//! The human views in `render` truncate names to fit a terminal, spell out
//! prerequisites by name, and repeat a direction's name on every row. That
//! reads well and costs a lot: the tree of an 800-task graph runs to 70 KB.
//! Here every task is one line of tab-separated fields, never truncated,
//! referring to other tasks and to directions by id only, under a header that
//! says what the fields are. Fields are separated by tabs, shown as spaces
//! here. Directions are named once, in a legend of only
//! those the rows mention.
//!
//! ```text
//! # a0  Goals
//! # id  status  pri  area  tier  due  needs  name
//! n5  locked  9  a0  5  -  n4,n26  Tier 3 · PPP: solve one station on its own
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use branchy_app::Made;
use branchy_app::today::today;
use branchy_core::{AreaId, Date, Graph, NodeId, Status};

/// What every row's fields are, in order.
const HEADER: &str = "# id\tstatus\tstage\tpri\tarea\ttier\tdue\tneeds\tname\n";

/// Everything a row needs that is derived from the whole graph, worked out
/// once per view rather than once per row.
struct Derived {
    statuses: BTreeMap<NodeId, Status>,
    tiers: BTreeMap<NodeId, u32>,
    due: BTreeMap<NodeId, Date>,
}

impl Derived {
    fn of(graph: &Graph) -> Self {
        Self {
            statuses: graph.statuses(),
            tiers: graph.tiers(),
            due: graph.effective_due(),
        }
    }
}

fn status_word(status: Status) -> &'static str {
    match status {
        Status::Done => "done",
        Status::Available => "available",
        Status::Locked => "locked",
        Status::Cyclic => "cyclic",
    }
}

/// A name or note with anything that would break the line format flattened.
fn flat(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// One task as one line. `due` is the deadline it is really working to,
/// inherited from what it unblocks when it has none of its own.
fn row(out: &mut String, graph: &Graph, derived: &Derived, id: NodeId) {
    let Some(node) = graph.node(id) else { return };
    let status = derived.statuses.get(&id).copied().unwrap_or(Status::Locked);
    let tier = derived
        .tiers
        .get(&id)
        .map_or_else(|| "-".to_string(), ToString::to_string);
    let due = derived
        .due
        .get(&id)
        .map_or_else(|| "-".to_string(), ToString::to_string);
    let needs = if node.prereqs.is_empty() {
        "-".to_string()
    } else {
        node.prereqs
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    let _ = writeln!(
        out,
        "{id}\t{}\t{}\t{}\t{}\t{tier}\t{due}\t{needs}\t{}",
        status_word(status),
        node.stage,
        node.priority,
        node.area,
        flat(&node.name)
    );
}

/// The directions these rows belong to, each named once.
fn legend(out: &mut String, graph: &Graph, ids: &[NodeId]) {
    let used: BTreeSet<AreaId> = ids
        .iter()
        .filter_map(|id| graph.node(*id).map(|node| node.area))
        .collect();
    for area in used {
        if let Some(found) = graph.area(area) {
            let _ = writeln!(out, "# {area}\t{}", flat(&found.name));
        }
    }
}

/// A legend, the header, and one row per task.
fn table(out: &mut String, graph: &Graph, ids: &[NodeId]) {
    let derived = Derived::of(graph);
    legend(out, graph, ids);
    out.push_str(HEADER);
    for id in ids {
        row(out, graph, &derived, *id);
    }
}

/// `# 10 of 515 available`, or `# 515 available` when nothing was cut.
fn count(out: &mut String, shown: usize, total: usize, what: &str) {
    if shown < total {
        let _ = writeln!(out, "# {shown} of {total} {what}");
    } else {
        let _ = writeln!(out, "# {total} {what}");
    }
}

fn limited(ids: Vec<NodeId>, limit: Option<usize>) -> Vec<NodeId> {
    match limit {
        Some(n) => ids.into_iter().take(n).collect(),
        None => ids,
    }
}

/// Where things stand: counts, every direction, the top of the queue, what is
/// due within a week, and how many cycles. See `render::brief`.
#[must_use]
pub fn brief(graph: &Graph, title: &str, limit: usize) -> String {
    let now = today();
    let counts = crate::render::Counts::of(graph);
    let mut out = format!("# vault {}\ttoday {now}\n", flat(title));
    let _ = writeln!(
        out,
        "# tasks {}\tdone {}\tavailable {}\tlocked {}\tcyclic {}\toverdue {}",
        counts.total, counts.done, counts.available, counts.locked, counts.cyclic, counts.overdue
    );
    for (id, area) in graph.areas() {
        let (done, total) = crate::render::area_progress(graph, id);
        let _ = writeln!(out, "# {id}\t{}\t{done}/{total}", flat(&area.name));
    }
    let derived = Derived::of(graph);

    let board = graph.board();
    let _ = writeln!(
        out,
        "# board\ttodo {}\tdoing {}\treview {}",
        board.todo.len(),
        board.doing.len(),
        board.review.len()
    );
    let started: Vec<NodeId> = board.doing.iter().chain(&board.review).copied().collect();
    if !started.is_empty() {
        count(&mut out, started.len(), started.len(), "in progress");
        out.push_str(HEADER);
        for id in &started {
            row(&mut out, graph, &derived, *id);
        }
    }

    let queue = graph.queue();
    count(&mut out, queue.len().min(limit), queue.len(), "available");
    out.push_str(HEADER);
    for id in queue.iter().take(limit) {
        row(&mut out, graph, &derived, *id);
    }

    let soon = crate::render::due_soon(graph, now);
    if !soon.is_empty() {
        count(
            &mut out,
            soon.len().min(limit),
            soon.len(),
            "due within a week",
        );
        for (_, id) in soon.iter().take(limit) {
            row(&mut out, graph, &derived, *id);
        }
    }
    let _ = writeln!(out, "# cycles {}", graph.find_cycles().len());
    out
}

/// The board's columns, each a section of rows. The backlog and done are cut
/// to `limit`; todo, doing and review are shown whole.
#[must_use]
pub fn board(graph: &Graph, limit: usize) -> String {
    let board = graph.board();
    let mut out = format!(
        "# board\tbacklog {}\ttodo {}\tdoing {}\treview {}\tdone {}\n",
        board.backlog_ready.len() + board.backlog_locked.len(),
        board.todo.len(),
        board.doing.len(),
        board.review.len(),
        board.done.len()
    );
    let cut = |ids: &[NodeId], limit: Option<usize>| -> Vec<NodeId> {
        ids.iter()
            .copied()
            .take(limit.unwrap_or(usize::MAX))
            .collect()
    };
    let sections = [
        (
            "backlog ready",
            cut(&board.backlog_ready, Some(limit)),
            board.backlog_ready.len(),
        ),
        (
            "backlog locked",
            cut(&board.backlog_locked, Some(limit)),
            board.backlog_locked.len(),
        ),
        ("todo", cut(&board.todo, None), board.todo.len()),
        ("doing", cut(&board.doing, None), board.doing.len()),
        ("review", cut(&board.review, None), board.review.len()),
        ("done", cut(&board.done, Some(limit)), board.done.len()),
    ];
    let shown: Vec<NodeId> = sections
        .iter()
        .flat_map(|(_, ids, _)| ids.iter().copied())
        .collect();
    legend(&mut out, graph, &shown);
    out.push_str(HEADER);
    let derived = Derived::of(graph);
    for (title, ids, total) in &sections {
        count(&mut out, ids.len(), *total, title);
        for id in ids {
            row(&mut out, graph, &derived, *id);
        }
    }
    out
}

/// The available frontier, most urgent first.
#[must_use]
pub fn queue(graph: &Graph, limit: Option<usize>) -> String {
    let all = graph.queue();
    let total = all.len();
    let ids = limited(all, limit);
    let mut out = String::new();
    count(&mut out, ids.len(), total, "available");
    table(&mut out, graph, &ids);
    out
}

/// Every task, optionally filtered, in id order.
#[must_use]
pub fn list(graph: &Graph, filter: &crate::render::Filter<'_>, limit: Option<usize>) -> String {
    let statuses = graph.statuses();
    let wanted_area = filter.area.map(str::to_lowercase);
    let all: Vec<NodeId> = graph
        .nodes()
        .filter(|(id, node)| {
            filter
                .status
                .is_none_or(|want| statuses.get(id) == Some(&want))
                && filter.stage.is_none_or(|want| node.stage == want)
                && wanted_area.as_ref().is_none_or(|wanted| {
                    graph
                        .area(node.area)
                        .is_some_and(|area| area.name.to_lowercase().contains(wanted))
                })
        })
        .map(|(id, _)| id)
        .collect();
    let total = all.len();
    let ids = limited(all, limit);
    let mut out = String::new();
    count(&mut out, ids.len(), total, "tasks");
    table(&mut out, graph, &ids);
    out
}

/// Every task, by direction and then tier. The same rows as [`list`] in the
/// order the tree view reads them.
#[must_use]
pub fn tree(graph: &Graph) -> String {
    let tiers = graph.tiers();
    let mut ids: Vec<NodeId> = graph.nodes().map(|(id, _)| id).collect();
    ids.sort_by_key(|id| {
        (
            graph.node(*id).map(|node| node.area),
            tiers.get(id).copied().unwrap_or(u32::MAX),
            *id,
        )
    });
    let mut out = String::new();
    count(&mut out, ids.len(), ids.len(), "tasks");
    table(&mut out, graph, &ids);
    out
}

/// Unfinished tasks with a deadline, soonest first.
#[must_use]
pub fn calendar(graph: &Graph, limit: Option<usize>) -> String {
    let statuses = graph.statuses();
    let mut dated: Vec<(Date, NodeId)> = graph
        .effective_due()
        .into_iter()
        .filter(|(id, _)| statuses.get(id) != Some(&Status::Done))
        .map(|(id, date)| (date, id))
        .collect();
    dated.sort_unstable();
    let total = dated.len();
    let ids = limited(dated.into_iter().map(|(_, id)| id).collect(), limit);
    let mut out = format!("# today {}\n", today());
    count(&mut out, ids.len(), total, "with a deadline");
    table(&mut out, graph, &ids);
    out
}

/// What stands between the user and one task, in an order that can be worked
/// through from the top.
#[must_use]
pub fn why(graph: &Graph, id: NodeId) -> String {
    match graph.path_to_unlock(id) {
        Ok(path) => {
            let mut out = String::new();
            count(&mut out, path.len(), path.len(), "in the way");
            table(&mut out, graph, &path);
            out
        }
        Err(error) => format!("# {error}\n"),
    }
}

/// One task in full: its row, what it unlocks, and its note.
#[must_use]
pub fn show(graph: &Graph, id: NodeId) -> String {
    let Some(node) = graph.node(id) else {
        return format!("# no such task: {id}\n");
    };
    let mut out = String::new();
    table(&mut out, graph, &[id]);
    let unlocks: Vec<String> = graph
        .dependents(id)
        .into_iter()
        .map(|dependent| dependent.to_string())
        .collect();
    if !unlocks.is_empty() {
        let _ = writeln!(out, "unlocks\t{}", unlocks.join(","));
    }
    if let Some(written) = node.due {
        let _ = writeln!(out, "due written\t{written}");
    }
    if !node.note.is_empty() {
        // last, and kept as written, because it is the one free-form field
        let _ = writeln!(out, "note\t{}", node.note);
    }
    out
}

/// The task a command just created or changed, as one row.
#[must_use]
pub fn changed(graph: &Graph, id: Option<NodeId>) -> String {
    let mut out = String::new();
    if let Some(id) = id {
        row(&mut out, graph, &Derived::of(graph), id);
    }
    if out.is_empty() {
        out.push_str("ok\n");
    }
    out
}

/// What a batch created, one per line, with the label the batch gave it.
#[must_use]
pub fn made(made: &[(Option<String>, Made)]) -> String {
    let mut out = String::new();
    for (label, thing) in made {
        match label {
            Some(label) => {
                let _ = writeln!(out, "{thing}\t${label}");
            }
            None => {
                let _ = writeln!(out, "{thing}");
            }
        }
    }
    if out.is_empty() {
        out.push_str("ok\n");
    }
    out
}
