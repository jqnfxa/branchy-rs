//! Printing a graph as text.
//!
//! Everything here writes into one buffer rather than building a string per
//! row, which is what `clippy::format_push_string` is after.

use std::fmt::Write as _;

use branchy_app::Vaults;
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
pub fn queue(graph: &Graph, limit: Option<usize>) -> String {
    let queue = graph.queue();
    let deadlines = graph.effective_due();
    let now = today();
    if queue.is_empty() {
        // An empty graph and a blocked one both have an empty queue, but the
        // first is a new user and the second is a problem, and they need
        // opposite advice.
        if graph.is_empty() {
            return if graph.area_count() == 0 {
                "The graph is empty. Start with a direction:\n  branchy area \"Hard skills\" \"#4fd1c5\"\n".into()
            } else {
                "No tasks yet. Add one:\n  branchy add \"My first task\"\n".into()
            };
        }
        return "Nothing is available. Try `branchy cycles`, or finish something first.\n".into();
    }
    let shown = limit.map_or(queue.len(), |n| n.min(queue.len()));
    let mut out = if shown < queue.len() {
        format!("{shown} of {} available:\n", queue.len())
    } else {
        format!("{} available:\n", queue.len())
    };
    for (rank, id) in queue.iter().take(shown).enumerate() {
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
                "{:>3}. {:<6} {:<38} p{:<3} {:<14} {deadline}{}",
                rank + 1,
                id.to_string(),
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

/// Where things stand, in one screen: the counts, the directions, the top of
/// the queue, whatever is due within a week, and any cycles.
///
/// The first thing to run in a session, and the one call an agent needs to
/// orient itself before it touches anything.
#[must_use]
pub fn brief(graph: &Graph, title: &str, limit: usize) -> String {
    if graph.is_empty() {
        return queue(graph, None);
    }
    let now = today();
    let counts = Counts::of(graph);
    let mut out = format!("{title}, today {now}\n");
    let _ = writeln!(out, "{}", counts.sentence());

    out.push_str("\nDirections:\n");
    for (id, area) in graph.areas() {
        let (done, total) = area_progress(graph, id);
        row(
            &mut out,
            &format!("  {:<5} {:<30} {done}/{total}", id.to_string(), area.name),
        );
    }

    out.push('\n');
    out.push_str(&queue(graph, Some(limit)));

    let soon = due_soon(graph, now);
    if !soon.is_empty() {
        let statuses = graph.statuses();
        let _ = writeln!(out, "\nDue within a week ({}):", soon.len());
        for (date, id) in soon.iter().take(limit) {
            let Some(node) = graph.node(*id) else {
                continue;
            };
            row(
                &mut out,
                &format!(
                    "  {} {date}  {:<6} {:<38} {}",
                    mark(statuses.get(id).copied().unwrap_or(Status::Locked)),
                    id.to_string(),
                    truncate(&node.name, 38),
                    when(now.days_until(*date)),
                ),
            );
        }
    }

    let cycles = graph.find_cycles().len();
    if cycles > 0 {
        let _ = writeln!(
            out,
            "\n{cycles} cycle(s). `branchy cycles` shows how to break them."
        );
    }
    out
}

/// How many tasks stand where.
pub struct Counts {
    /// Every task.
    pub total: usize,
    /// Finished.
    pub done: usize,
    /// Ready to start.
    pub available: usize,
    /// Waiting on a prerequisite.
    pub locked: usize,
    /// On or after a cycle.
    pub cyclic: usize,
    /// Unfinished and past their deadline.
    pub overdue: usize,
}

impl Counts {
    /// Counts a graph.
    #[must_use]
    pub fn of(graph: &Graph) -> Self {
        let statuses = graph.statuses();
        let of = |want: Status| statuses.values().filter(|s| **s == want).count();
        let now = today();
        Self {
            total: graph.node_count(),
            done: of(Status::Done),
            available: of(Status::Available),
            locked: of(Status::Locked),
            cyclic: of(Status::Cyclic),
            overdue: graph
                .effective_due()
                .iter()
                .filter(|(id, date)| {
                    **date < now && statuses.get(id).is_some_and(|s| *s != Status::Done)
                })
                .count(),
        }
    }

    fn sentence(&self) -> String {
        let mut line = format!(
            "{} tasks: {} done, {} available, {} locked",
            self.total, self.done, self.available, self.locked
        );
        if self.cyclic > 0 {
            let _ = write!(line, ", {} cyclic", self.cyclic);
        }
        if self.overdue > 0 {
            let _ = write!(line, ". {} overdue", self.overdue);
        }
        line.push('.');
        line
    }
}

/// How many of a direction's tasks are done, out of how many.
#[must_use]
pub fn area_progress(graph: &Graph, area: branchy_core::AreaId) -> (usize, usize) {
    let held: Vec<bool> = graph
        .nodes()
        .filter(|(_, node)| node.area == area)
        .map(|(_, node)| node.is_done())
        .collect();
    (held.iter().filter(|done| **done).count(), held.len())
}

/// Unfinished tasks due within a week or already late, soonest first.
#[must_use]
pub fn due_soon(graph: &Graph, now: Date) -> Vec<(Date, NodeId)> {
    let statuses = graph.statuses();
    let mut soon: Vec<(Date, NodeId)> = graph
        .effective_due()
        .into_iter()
        .filter(|(id, date)| {
            statuses.get(id) != Some(&Status::Done) && Urgency::of(*date, now) != Urgency::Later
        })
        .map(|(id, date)| (date, id))
        .collect();
    soon.sort_unstable();
    soon
}

/// Every task, grouped by direction, optionally filtered.
#[must_use]
pub fn list(
    graph: &Graph,
    area_filter: Option<&str>,
    status_filter: Option<Status>,
    limit: Option<usize>,
) -> String {
    let statuses = graph.statuses();
    let mut out = String::new();
    let mut left = limit.unwrap_or(usize::MAX);

    for (area_id, area) in graph.areas() {
        if left == 0 {
            break;
        }
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
            .filter(|(_, n)| n.area == area_id && n.is_done())
            .count();
        let total = graph.nodes().filter(|(_, n)| n.area == area_id).count();
        let _ = writeln!(out, "\n{}  {done}/{total}", area.name);

        for id in rows {
            if left == 0 {
                break;
            }
            left -= 1;
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
            .filter(|id| graph.node(**id).is_some_and(branchy_core::Node::is_done))
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
                        "{:>3}. {:<6} {:<38} p{:<3} {}",
                        step + 1,
                        blocker.to_string(),
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
            let met = graph.node(*prereq).is_some_and(branchy_core::Node::is_done);
            let _ = writeln!(
                out,
                "    [{}] {:<6} {}",
                if met { "x" } else { " " },
                prereq.to_string(),
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
            let _ = writeln!(
                out,
                "    {:<6} {}",
                dependent.to_string(),
                name_of(graph, dependent)
            );
        }
    }
    out
}

/// Everything with a deadline, grouped by how soon it is.
///
/// Inherited deadlines are shown alongside written ones and marked, because a
/// task is just as due whether the date is on it or on the thing it unblocks.
#[must_use]
pub fn calendar(graph: &Graph, limit: Option<usize>) -> String {
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
    rows.truncate(limit.unwrap_or(usize::MAX));

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
                "  {} {}  {:<6} {:<36} {:<12} {}",
                mark(statuses.get(&id).copied().unwrap_or(Status::Locked)),
                date,
                id.to_string(),
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

/// The recent vaults, the current one marked.
pub fn vaults(vaults: &Vaults) -> String {
    let entries = vaults.entries();
    if entries.is_empty() {
        return "No vaults yet. Create one with `branchy vault new <name>`, \
                or open a folder with `branchy vault open <folder>`.\n"
            .to_string();
    }
    let width = entries
        .iter()
        .map(|entry| entry.name.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for (index, entry) in entries.iter().enumerate() {
        let mark = if index == 0 { '*' } else { ' ' };
        let gone = if entry.missing {
            "  (folder not found)"
        } else {
            ""
        };
        let _ = writeln!(out, "{mark} {:width$}  {}{gone}", entry.name, entry.path);
    }
    out
}
