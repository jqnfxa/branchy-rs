//! Printing a graph as text.
//!
//! Everything here writes into one buffer rather than building a string per
//! row, which is what `clippy::format_push_string` is after.

use std::fmt::Write as _;

use branchy_app::today::{Urgency, today};
use branchy_core::{Date, Graph, NodeId, Status};

/// One marker per status, so a list stays scannable down the left edge.
fn mark(status: Status) -> &'static str {
    match status {
        Status::Done => "[x]",
        Status::Available => "[ ]",
        Status::Locked => "[-]",
        Status::Cyclic => "[!]",
    }
}

fn name_of(graph: &Graph, id: NodeId) -> String {
    graph
        .node(id)
        .map_or_else(|| format!("<{id}>"), |node| node.name.clone())
}

fn area_of(graph: &Graph, id: NodeId) -> String {
    graph
        .node(id)
        .and_then(|node| graph.area(node.area))
        .map_or_else(|| "?".to_string(), |area| area.name.clone())
}

/// Writes a row, dropping any trailing padding.
fn row(out: &mut String, text: &str) {
    out.push_str(text.trim_end());
    out.push('\n');
}

/// The available frontier, highest priority first.
#[must_use]
pub fn queue(graph: &Graph) -> String {
    let queue = graph.queue();
    let deadlines = graph.effective_due();
    let now = today();
    if queue.is_empty() {
        return "Nothing is available. Try `branchy cycles`, or finish something first.\n".into();
    }
    let mut out = format!("{} available:\n", queue.len());
    for (rank, id) in queue.iter().enumerate() {
        let Some(node) = graph.node(*id) else {
            continue;
        };
        let unlocks = graph.dependents(*id).len();
        let deadline = deadlines.get(id).map_or_else(String::new, |date| {
            format!("{:<14}", when(now.days_until(*date)))
        });
        row(
            &mut out,
            &format!(
                "{:>3}. {:<38} p{:<3} {:<14} {deadline}{}",
                rank + 1,
                truncate(&node.name, 38),
                node.priority,
                truncate(&area_of(graph, *id), 14),
                if unlocks == 0 {
                    String::new()
                } else {
                    format!("unlocks {unlocks}")
                },
            ),
        );
    }
    out
}

/// Every task, grouped by direction, optionally filtered.
#[must_use]
pub fn list(graph: &Graph, area_filter: Option<&str>, status_filter: Option<Status>) -> String {
    let statuses = graph.statuses();
    let mut out = String::new();

    for (area_id, area) in graph.areas() {
        if let Some(wanted) = area_filter {
            if !area.name.to_lowercase().contains(&wanted.to_lowercase()) {
                continue;
            }
        }
        let rows: Vec<NodeId> = graph
            .nodes()
            .filter(|(id, node)| {
                node.area == area_id
                    && status_filter.is_none_or(|want| statuses.get(id) == Some(&want))
            })
            .map(|(id, _)| id)
            .collect();
        if rows.is_empty() {
            continue;
        }

        let done = graph
            .nodes()
            .filter(|(_, n)| n.area == area_id && n.done)
            .count();
        let total = graph.nodes().filter(|(_, n)| n.area == area_id).count();
        let _ = writeln!(out, "\n{}  {done}/{total}", area.name);

        for id in rows {
            let Some(node) = graph.node(id) else { continue };
            let status = statuses.get(&id).copied().unwrap_or(Status::Locked);
            row(
                &mut out,
                &format!(
                    "  {} {:<40} p{:<3} {id}",
                    mark(status),
                    truncate(&node.name, 40),
                    node.priority,
                ),
            );
        }
    }

    if out.is_empty() {
        "Nothing matches.\n".into()
    } else {
        out.trim_start_matches('\n').to_string()
    }
}

/// The whole graph, by direction and then tier, with each task's prerequisites.
#[must_use]
pub fn tree(graph: &Graph) -> String {
    if graph.is_empty() {
        return "The graph is empty. Start with `branchy area \"Hard skills\"`.\n".into();
    }
    let tiers = graph.tiers();
    let statuses = graph.statuses();
    let mut out = String::new();

    for (area_id, area) in graph.areas() {
        let mut rows: Vec<NodeId> = graph
            .nodes()
            .filter(|(_, node)| node.area == area_id)
            .map(|(id, _)| id)
            .collect();
        if rows.is_empty() {
            continue;
        }
        // nodes with no tier are tangled in a cycle; they sort last
        rows.sort_by_key(|id| (tiers.get(id).copied().unwrap_or(u32::MAX), *id));

        let done = rows
            .iter()
            .filter(|id| graph.node(**id).is_some_and(|n| n.done))
            .count();
        let _ = writeln!(
            out,
            "\n{}  {}  {done}/{}",
            area.name,
            bar(done, rows.len()),
            rows.len()
        );

        let mut shown: Option<Option<u32>> = None;
        for id in rows {
            let Some(node) = graph.node(id) else { continue };
            let tier = tiers.get(&id).copied();
            if shown != Some(tier) {
                match tier {
                    Some(t) => {
                        let _ = writeln!(out, "  tier {t}");
                    }
                    None => out.push_str("  tangled in a cycle\n"),
                }
                shown = Some(tier);
            }
            let status = statuses.get(&id).copied().unwrap_or(Status::Locked);
            let needs: Vec<String> = node.prereqs.iter().map(|p| name_of(graph, *p)).collect();
            row(
                &mut out,
                &format!(
                    "    {} {:<38} {:<6} {}",
                    mark(status),
                    truncate(&node.name, 38),
                    id.to_string(),
                    if needs.is_empty() {
                        String::new()
                    } else {
                        format!("needs {}", needs.join(", "))
                    }
                ),
            );
        }
    }
    out.trim_start_matches('\n').to_string()
}

/// What stands between the user and one task.
#[must_use]
pub fn why(graph: &Graph, id: NodeId) -> String {
    let Some(node) = graph.node(id) else {
        return format!("No such task: {id}\n");
    };
    match graph.status(id) {
        Some(Status::Done) => return format!("{} is already done.\n", node.name),
        Some(Status::Available) => {
            return format!(
                "{} is available right now. Nothing is in the way.\n",
                node.name
            );
        }
        _ => {}
    }

    match graph.path_to_unlock(id) {
        Ok(path) => {
            let mut out = format!(
                "{} is locked. {} task(s) stand in the way:\n",
                node.name,
                path.len()
            );
            for (step, blocker) in path.iter().enumerate() {
                let Some(blocking) = graph.node(*blocker) else {
                    continue;
                };
                row(
                    &mut out,
                    &format!(
                        "{:>3}. {:<38} p{:<3} {}",
                        step + 1,
                        truncate(&blocking.name, 38),
                        blocking.priority,
                        truncate(&area_of(graph, *blocker), 16),
                    ),
                );
            }
            out
        }
        Err(error) => format!("{}: {error}\n", node.name),
    }
}

/// One task in full.
#[must_use]
pub fn show(graph: &Graph, id: NodeId) -> String {
    let Some(node) = graph.node(id) else {
        return format!("No such task: {id}\n");
    };
    let status = graph.status(id).unwrap_or(Status::Locked);
    let mut out = format!("{}  {id}\n", node.name);
    let _ = writeln!(
        out,
        "  {:<10} {:<16} priority {}  tier {}",
        format!("{status:?}").to_lowercase(),
        area_of(graph, id),
        node.priority,
        graph
            .tier(id)
            .map_or_else(|| "-".to_string(), |t| t.to_string())
    );
    if let Some(date) = graph.effective_due().get(&id) {
        let now = today();
        let _ = writeln!(
            out,
            "  due {date}  {}{}",
            when(now.days_until(*date)),
            if node.due.is_none() {
                ", inherited from what it unblocks"
            } else {
                ""
            }
        );
    }
    if !node.note.is_empty() {
        let _ = writeln!(out, "  {}", node.note);
    }

    out.push_str("\n  needs:\n");
    if node.prereqs.is_empty() {
        out.push_str("    nothing, this is a starting point\n");
    } else {
        for prereq in &node.prereqs {
            let met = graph.node(*prereq).is_some_and(|n| n.done);
            let _ = writeln!(
                out,
                "    [{}] {}",
                if met { "x" } else { " " },
                name_of(graph, *prereq)
            );
        }
    }

    let dependents = graph.dependents(id);
    out.push_str("\n  unlocks:\n");
    if dependents.is_empty() {
        out.push_str("    nothing yet\n");
    } else {
        for dependent in dependents {
            let _ = writeln!(out, "    {}", name_of(graph, dependent));
        }
    }
    out
}

/// Everything with a deadline, grouped by how soon it is.
///
/// Inherited deadlines are shown alongside written ones and marked, because a
/// task is just as due whether the date is on it or on the thing it unblocks.
#[must_use]
pub fn calendar(graph: &Graph) -> String {
    let now = today();
    let deadlines = graph.effective_due();
    let statuses = graph.statuses();

    let mut rows: Vec<(Date, NodeId)> = deadlines
        .iter()
        .filter(|(id, _)| statuses.get(id) != Some(&Status::Done))
        .map(|(id, date)| (*date, *id))
        .collect();
    if rows.is_empty() {
        return "Nothing has a deadline. Try `branchy due <task> 2026-12-31`.\n".into();
    }
    rows.sort_unstable();

    let mut out = format!("Today is {now}.\n");
    let mut heading: Option<&'static str> = None;

    for (date, id) in rows {
        let Some(node) = graph.node(id) else { continue };
        let urgency = Urgency::of(date, now);
        if heading != Some(urgency.name()) {
            let _ = writeln!(out, "\n{}", label(urgency, now));
            heading = Some(urgency.name());
        }
        let days = now.days_until(date);
        let inherited = node.due.is_none();
        row(
            &mut out,
            &format!(
                "  {} {}  {:<36} {:<12} {}",
                mark(statuses.get(&id).copied().unwrap_or(Status::Locked)),
                date,
                truncate(&node.name, 36),
                truncate(&area_of(graph, id), 12),
                if inherited {
                    format!("{}  inherited", when(days))
                } else {
                    when(days)
                }
            ),
        );
    }
    out
}

fn label(urgency: Urgency, now: Date) -> String {
    match urgency {
        Urgency::Overdue => "OVERDUE".to_string(),
        Urgency::Today => format!("TODAY, {now}"),
        Urgency::Soon => "THIS WEEK".to_string(),
        Urgency::Later => "LATER".to_string(),
    }
}

fn when(days: i64) -> String {
    match days {
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        -1 => "1 day late".to_string(),
        d if d < 0 => format!("{} days late", -d),
        d if d < 14 => format!("in {d} days"),
        d if d < 60 => format!("in {} weeks", d / 7),
        d => format!("in {} months", d / 30),
    }
}

/// Any tangles in the graph, with the commands that would break them.
#[must_use]
pub fn cycles(graph: &Graph) -> String {
    let found = graph.find_cycles();
    if found.is_empty() {
        return "No cycles. Every task can be reached.\n".into();
    }
    let mut out = format!(
        "{} tangle(s). Each needs one prerequisite removed:\n",
        found.len()
    );
    for group in found {
        let names: Vec<String> = group.iter().map(|id| name_of(graph, *id)).collect();
        let _ = writeln!(out, "\n  {}", names.join(" <-> "));
        for id in &group {
            let Some(node) = graph.node(*id) else {
                continue;
            };
            for prereq in &node.prereqs {
                if group.contains(prereq) {
                    let _ = writeln!(out, "    branchy unlink {id} after {prereq}");
                }
            }
        }
    }
    out
}

fn bar(done: usize, total: usize) -> String {
    const WIDTH: usize = 10;
    let filled = (done * WIDTH).checked_div(total).unwrap_or(0);
    format!("{}{}", "#".repeat(filled), ".".repeat(WIDTH - filled))
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}\u{2026}")
}
