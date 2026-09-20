//! The graph itself: storage, editing, and everything derived from it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::date::Date;
use crate::error::Error;
use crate::id::{AreaId, NodeId};
use crate::node::{Area, NewArea, NewNode, Node, Status};

/// Marker for a node Tarjan's algorithm has not reached yet.
const UNVISITED: usize = usize::MAX;

/// A dependency graph of tasks.
///
/// The graph stores only what cannot be worked out: the nodes, the areas, and
/// which node needs which. Status, tier, the available queue and the route to
/// unlocking something are all computed on demand, so they can never disagree
/// with the edges.
///
/// Iteration order is stable, because both collections are ordered maps. That
/// is deliberate: tests compare against a fixed order, and a hash map would
/// randomise it per process.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Graph {
    nodes: BTreeMap<NodeId, Node>,
    areas: BTreeMap<AreaId, Area>,
    next_node: u64,
    next_area: u64,
}

impl Graph {
    /// An empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // ── areas ────────────────────────────────────────────────────────────

    /// Creates an area and returns its id.
    pub fn add_area(&mut self, draft: NewArea) -> AreaId {
        let id = AreaId::new(self.next_area);
        self.next_area += 1;
        self.areas.insert(
            id,
            Area {
                name: draft.name,
                color: draft.color,
            },
        );
        id
    }

    /// The area with this id, if it exists.
    #[must_use]
    pub fn area(&self, id: AreaId) -> Option<&Area> {
        self.areas.get(&id)
    }

    /// Every area, in id order.
    pub fn areas(&self) -> impl Iterator<Item = (AreaId, &Area)> {
        self.areas.iter().map(|(id, area)| (*id, area))
    }

    /// How many areas there are.
    #[must_use]
    pub fn area_count(&self) -> usize {
        self.areas.len()
    }

    /// Renames an area.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchArea`] if there is no such area.
    pub fn set_area_name(&mut self, id: AreaId, name: impl Into<String>) -> Result<(), Error> {
        self.area_mut(id)?.name = name.into();
        Ok(())
    }

    /// Changes an area's accent colour.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchArea`] if there is no such area.
    pub fn set_area_color(&mut self, id: AreaId, color: impl Into<String>) -> Result<(), Error> {
        self.area_mut(id)?.color = color.into();
        Ok(())
    }

    fn area_mut(&mut self, id: AreaId) -> Result<&mut Area, Error> {
        self.areas.get_mut(&id).ok_or(Error::NoSuchArea(id))
    }

    /// Removes an empty area.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchArea`] if there is no such area, or
    /// [`Error::AreaNotEmpty`] if nodes still belong to it. Moving those
    /// elsewhere is a decision for the caller, not for the graph.
    pub fn remove_area(&mut self, id: AreaId) -> Result<Area, Error> {
        if !self.areas.contains_key(&id) {
            return Err(Error::NoSuchArea(id));
        }
        let held = self.nodes.values().filter(|n| n.area == id).count();
        if held > 0 {
            return Err(Error::AreaNotEmpty {
                area: id,
                nodes: held,
            });
        }
        self.areas.remove(&id).ok_or(Error::NoSuchArea(id))
    }

    // ── nodes ────────────────────────────────────────────────────────────

    /// Creates a node with no prerequisites and returns its id.
    ///
    /// Ids are never reused, including after a node is removed and the removal
    /// undone. A handed-out id may already have been seen by another device, so
    /// giving it to a different node later would silently rewrite history.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchArea`] if the draft names an area that does not exist.
    pub fn add_node(&mut self, draft: NewNode) -> Result<NodeId, Error> {
        if !self.areas.contains_key(&draft.area) {
            return Err(Error::NoSuchArea(draft.area));
        }
        let id = NodeId::new(self.next_node);
        self.next_node += 1;
        self.nodes.insert(
            id,
            Node {
                name: draft.name,
                note: draft.note,
                area: draft.area,
                priority: draft.priority,
                done: false,
                due: draft.due,
                prereqs: BTreeSet::new(),
            },
        );
        Ok(id)
    }

    /// The node with this id, if it exists.
    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// Every node, in id order.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter().map(|(id, node)| (*id, node))
    }

    /// How many nodes there are.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph holds no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Removes a node and strips it from every node that needed it.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchNode`] if there is no such node.
    pub fn remove_node(&mut self, id: NodeId) -> Result<Node, Error> {
        let removed = self.nodes.remove(&id).ok_or(Error::NoSuchNode(id))?;
        for node in self.nodes.values_mut() {
            node.prereqs.remove(&id);
        }
        Ok(removed)
    }

    /// Marks a node done or not done.
    ///
    /// Deliberately not restricted to available nodes: importing history and
    /// correcting a mistake both need to set this freely. A node that is done
    /// while its prerequisites are not simply reports [`Status::Done`].
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchNode`] if there is no such node.
    pub fn set_done(&mut self, id: NodeId, done: bool) -> Result<(), Error> {
        self.node_mut(id)?.done = done;
        Ok(())
    }

    /// Sets or clears a node's deadline.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchNode`] if there is no such node.
    pub fn set_due(&mut self, id: NodeId, due: Option<Date>) -> Result<(), Error> {
        self.node_mut(id)?.due = due;
        Ok(())
    }

    /// Sets a node's priority.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchNode`] if there is no such node.
    pub fn set_priority(&mut self, id: NodeId, priority: u8) -> Result<(), Error> {
        self.node_mut(id)?.priority = priority;
        Ok(())
    }

    /// Renames a node.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchNode`] if there is no such node.
    pub fn set_name(&mut self, id: NodeId, name: impl Into<String>) -> Result<(), Error> {
        self.node_mut(id)?.name = name.into();
        Ok(())
    }

    /// Replaces a node's note.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchNode`] if there is no such node.
    pub fn set_note(&mut self, id: NodeId, note: impl Into<String>) -> Result<(), Error> {
        self.node_mut(id)?.note = note.into();
        Ok(())
    }

    /// Moves a node to another area.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchNode`] or [`Error::NoSuchArea`].
    pub fn set_area(&mut self, id: NodeId, area: AreaId) -> Result<(), Error> {
        if !self.areas.contains_key(&area) {
            return Err(Error::NoSuchArea(area));
        }
        self.node_mut(id)?.area = area;
        Ok(())
    }

    fn node_mut(&mut self, id: NodeId) -> Result<&mut Node, Error> {
        self.nodes.get_mut(&id).ok_or(Error::NoSuchNode(id))
    }

    // ── edges ────────────────────────────────────────────────────────────

    /// Records that `dependent` needs `prerequisite`.
    ///
    /// Adding the same edge twice is not an error; the prerequisites are a set.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchNode`] if either node is missing, or
    /// [`Error::WouldCycle`] if the edge would close a dependency loop — which
    /// includes a node depending on itself. The error carries the loop.
    pub fn add_prerequisite(
        &mut self,
        dependent: NodeId,
        prerequisite: NodeId,
    ) -> Result<(), Error> {
        if !self.nodes.contains_key(&dependent) {
            return Err(Error::NoSuchNode(dependent));
        }
        if !self.nodes.contains_key(&prerequisite) {
            return Err(Error::NoSuchNode(prerequisite));
        }
        if let Some(path) = self.closing_path(dependent, prerequisite) {
            return Err(Error::WouldCycle {
                dependent,
                prerequisite,
                path,
            });
        }
        self.node_mut(dependent)?.prereqs.insert(prerequisite);
        Ok(())
    }

    /// Records that `dependent` needs `prerequisite`, **without** the cycle
    /// check.
    ///
    /// This exists for one reason: merging. Two devices can each add an edge
    /// that is legal on its own and cyclic together, and a CRDT will merge both
    /// without complaint, so a document can arrive already holding a cycle.
    /// Code that applies such a document uses this and then calls
    /// [`Graph::find_cycles`] to report what needs breaking.
    ///
    /// Ordinary editing uses [`Graph::add_prerequisite`].
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchNode`] if either node is missing.
    pub fn add_prerequisite_unchecked(
        &mut self,
        dependent: NodeId,
        prerequisite: NodeId,
    ) -> Result<(), Error> {
        if !self.nodes.contains_key(&prerequisite) {
            return Err(Error::NoSuchNode(prerequisite));
        }
        self.node_mut(dependent)?.prereqs.insert(prerequisite);
        Ok(())
    }

    /// Removes the edge saying `dependent` needs `prerequisite`.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchNode`] if `dependent` is missing, or
    /// [`Error::NotAPrerequisite`] if the edge was not there.
    pub fn remove_prerequisite(
        &mut self,
        dependent: NodeId,
        prerequisite: NodeId,
    ) -> Result<(), Error> {
        let node = self.node_mut(dependent)?;
        if node.prereqs.remove(&prerequisite) {
            Ok(())
        } else {
            Err(Error::NotAPrerequisite {
                dependent,
                prerequisite,
            })
        }
    }

    /// Everything that needs this node directly, in id order.
    #[must_use]
    pub fn dependents(&self, id: NodeId) -> Vec<NodeId> {
        self.nodes
            .iter()
            .filter(|(_, node)| node.prereqs.contains(&id))
            .map(|(other, _)| *other)
            .collect()
    }

    /// Walks prerequisite edges from `prerequisite` looking for `dependent`.
    ///
    /// A step from `a` to `b` means "a needs b", so the chain returned reads
    /// left to right as "needs". Adding `dependent -> prerequisite` would close
    /// it, so the chain is returned with `prerequisite` appended: it starts and
    /// ends at the same node and can be shown to a user as-is.
    pub(crate) fn closing_path(
        &self,
        dependent: NodeId,
        prerequisite: NodeId,
    ) -> Option<Vec<NodeId>> {
        let mut parent: BTreeMap<NodeId, NodeId> = BTreeMap::new();
        let mut seen: BTreeSet<NodeId> = BTreeSet::new();
        let mut queue: VecDeque<NodeId> = VecDeque::new();

        seen.insert(prerequisite);
        queue.push_back(prerequisite);

        while let Some(current) = queue.pop_front() {
            if current == dependent {
                let mut path = vec![dependent];
                let mut at = dependent;
                while let Some(previous) = parent.get(&at) {
                    path.push(*previous);
                    at = *previous;
                }
                path.reverse();
                path.push(prerequisite);
                return Some(path);
            }
            if let Some(node) = self.nodes.get(&current) {
                for next in &node.prereqs {
                    if seen.insert(*next) {
                        parent.insert(*next, current);
                        queue.push_back(*next);
                    }
                }
            }
        }
        None
    }

    // ── derived ──────────────────────────────────────────────────────────

    /// The tier of every node whose tier is defined, in id order.
    ///
    /// A node's tier is `1 + max(tier of its prerequisites)`, or `0` when it has
    /// none, so a dependent always sits strictly after everything it needs.
    ///
    /// Nodes on, or downstream of, a cycle have no defined tier and are absent
    /// from the result. This is computed by peeling nodes whose prerequisites
    /// are all resolved (Kahn's algorithm), which terminates on any input —
    /// the naive recursive definition would not.
    #[must_use]
    pub fn tiers(&self) -> BTreeMap<NodeId, u32> {
        let dependents = self.dependents_index();
        let mut remaining: BTreeMap<NodeId, usize> = self
            .nodes
            .iter()
            .map(|(id, node)| {
                let live = node
                    .prereqs
                    .iter()
                    .filter(|p| self.nodes.contains_key(p))
                    .count();
                (*id, live)
            })
            .collect();

        let mut tier: BTreeMap<NodeId, u32> = BTreeMap::new();
        let mut ready: VecDeque<NodeId> = VecDeque::new();
        for (id, live) in &remaining {
            if *live == 0 {
                tier.insert(*id, 0);
                ready.push_back(*id);
            }
        }

        while let Some(id) = ready.pop_front() {
            let settled = tier[&id];
            for dependent in dependents.get(&id).into_iter().flatten() {
                let slot = tier.entry(*dependent).or_insert(0);
                *slot = (*slot).max(settled + 1);
                if let Some(live) = remaining.get_mut(dependent) {
                    *live -= 1;
                    if *live == 0 {
                        ready.push_back(*dependent);
                    }
                }
            }
        }

        // anything that never reached zero is tangled in a cycle
        tier.retain(|id, _| remaining.get(id) == Some(&0));
        tier
    }

    /// The tier of one node, or `None` if it does not exist or sits on or after
    /// a cycle. See [`Graph::tiers`].
    #[must_use]
    pub fn tier(&self, id: NodeId) -> Option<u32> {
        self.tiers().get(&id).copied()
    }

    /// The status of every node, in id order.
    #[must_use]
    pub fn statuses(&self) -> BTreeMap<NodeId, Status> {
        let tiers = self.tiers();
        self.nodes
            .iter()
            .map(|(id, node)| {
                let status = if node.done {
                    Status::Done
                } else if !tiers.contains_key(id) {
                    Status::Cyclic
                } else if node
                    .prereqs
                    .iter()
                    .all(|p| self.nodes.get(p).is_none_or(|n| n.done))
                {
                    Status::Available
                } else {
                    Status::Locked
                };
                (*id, status)
            })
            .collect()
    }

    /// The status of one node, or `None` if it does not exist.
    #[must_use]
    pub fn status(&self, id: NodeId) -> Option<Status> {
        self.statuses().get(&id).copied()
    }

    /// The deadline every node is really working to, in id order.
    ///
    /// A deadline reaches backwards. If a goal is due in March then everything
    /// it depends on is due in March too, whether or not anybody wrote that
    /// down, and the earliest such date wins when several dependents disagree.
    /// That propagation is the whole reason a deadline is worth recording on a
    /// graph rather than on a list.
    ///
    /// Nodes tangled in a cycle keep whatever date was set on them directly and
    /// propagate nothing, because there is no order in which to do it.
    #[must_use]
    pub fn effective_due(&self) -> BTreeMap<NodeId, Date> {
        let mut out: BTreeMap<NodeId, Date> = self
            .nodes
            .iter()
            .filter_map(|(id, node)| node.due.map(|due| (*id, due)))
            .collect();

        // deepest first, so a node is always settled before its prerequisites
        let tiers = self.tiers();
        let mut order: Vec<(u32, NodeId)> = tiers.iter().map(|(id, t)| (*t, *id)).collect();
        order.sort_unstable_by(|a, b| b.cmp(a));

        for (_, id) in order {
            let Some(due) = out.get(&id).copied() else {
                continue;
            };
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            for prereq in &node.prereqs {
                let slot = out.entry(*prereq).or_insert(due);
                if due < *slot {
                    *slot = due;
                }
            }
        }
        out
    }

    /// Every available node, most urgent first.
    ///
    /// This is the queue view: a projection of the graph rather than a separate
    /// list.
    ///
    /// A deadline outranks a priority, because a number somebody chose once
    /// should not outweigh a date the world imposed. Sorting by date ascending
    /// puts the most overdue first without the graph ever needing to know what
    /// day it is. Nodes with no deadline come after those that have one, and
    /// fall back to priority — so on a graph with no dates at all, this is
    /// exactly the priority order it always was.
    ///
    /// Ties break by name and then by id, so the order is stable.
    #[must_use]
    pub fn queue(&self) -> Vec<NodeId> {
        let due = self.effective_due();
        let mut out: Vec<NodeId> = self
            .statuses()
            .into_iter()
            .filter(|(_, status)| *status == Status::Available)
            .map(|(id, _)| id)
            .collect();

        out.sort_by(|a, b| {
            let left = &self.nodes[a];
            let right = &self.nodes[b];
            match (due.get(a), due.get(b)) {
                (Some(x), Some(y)) => x.cmp(y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
            .then_with(|| right.priority.cmp(&left.priority))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| a.cmp(b))
        });
        out
    }

    /// Everything still to be done before `id` becomes available, in an order
    /// that can be worked through from start to finish.
    ///
    /// Empty when the node is already available or already done. Prerequisites
    /// that are done are not walked into, since what they themselves needed no
    /// longer matters.
    ///
    /// # Errors
    ///
    /// [`Error::NoSuchNode`] if there is no such node, or
    /// [`Error::CycleBlocks`] if a cycle sits between this node and its
    /// prerequisites, in which case no amount of work would unlock it.
    pub fn path_to_unlock(&self, id: NodeId) -> Result<Vec<NodeId>, Error> {
        if !self.nodes.contains_key(&id) {
            return Err(Error::NoSuchNode(id));
        }
        let tiers = self.tiers();
        if !tiers.contains_key(&id) {
            return Err(Error::CycleBlocks(id));
        }

        let mut seen: BTreeSet<NodeId> = BTreeSet::new();
        let mut stack: Vec<NodeId> = vec![id];
        let mut out: Vec<NodeId> = Vec::new();

        while let Some(current) = stack.pop() {
            let Some(node) = self.nodes.get(&current) else {
                continue;
            };
            for prereq in &node.prereqs {
                if !seen.insert(*prereq) {
                    continue;
                }
                if self.nodes.get(prereq).is_some_and(|n| !n.done) {
                    out.push(*prereq);
                    stack.push(*prereq);
                }
            }
        }

        // tier is a valid execution order: a prerequisite always has a
        // strictly smaller one
        out.sort_by(|a, b| {
            tiers[a]
                .cmp(&tiers[b])
                .then_with(|| self.nodes[b].priority.cmp(&self.nodes[a].priority))
                .then_with(|| self.nodes[a].name.cmp(&self.nodes[b].name))
                .then_with(|| a.cmp(b))
        });
        Ok(out)
    }

    /// Every group of nodes that mutually block one another.
    ///
    /// Empty on a healthy graph, because [`Graph::add_prerequisite`] refuses to
    /// create a cycle. It is not empty when a document arrives with a cycle
    /// already merged into it, and then each group is a tangle the user has to
    /// break one edge of. Node ids within a group are sorted.
    #[must_use]
    pub fn find_cycles(&self) -> Vec<Vec<NodeId>> {
        // Tarjan's strongly connected components, with an explicit stack rather
        // than recursion so depth is bounded by the heap and not by the thread.
        let ids: Vec<NodeId> = self.nodes.keys().copied().collect();
        let position: BTreeMap<NodeId, usize> =
            ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        let successors: Vec<Vec<usize>> = self
            .nodes
            .values()
            .map(|node| {
                node.prereqs
                    .iter()
                    .filter_map(|p| position.get(p).copied())
                    .collect()
            })
            .collect();

        let count = ids.len();
        let mut index = vec![UNVISITED; count];
        let mut low = vec![0usize; count];
        let mut on_stack = vec![false; count];
        let mut stack: Vec<usize> = Vec::new();
        let mut next_index = 0usize;
        let mut calls: Vec<(usize, usize)> = Vec::new();
        let mut out: Vec<Vec<NodeId>> = Vec::new();

        for start in 0..count {
            if index[start] != UNVISITED {
                continue;
            }
            index[start] = next_index;
            low[start] = next_index;
            next_index += 1;
            stack.push(start);
            on_stack[start] = true;
            calls.push((start, 0));

            while let Some((at, child)) = calls.pop() {
                if child < successors[at].len() {
                    let next = successors[at][child];
                    calls.push((at, child + 1));
                    if index[next] == UNVISITED {
                        index[next] = next_index;
                        low[next] = next_index;
                        next_index += 1;
                        stack.push(next);
                        on_stack[next] = true;
                        calls.push((next, 0));
                    } else if on_stack[next] {
                        low[at] = low[at].min(index[next]);
                    }
                } else {
                    if low[at] == index[at] {
                        let mut group: Vec<NodeId> = Vec::new();
                        while let Some(popped) = stack.pop() {
                            on_stack[popped] = false;
                            group.push(ids[popped]);
                            if popped == at {
                                break;
                            }
                        }
                        let self_loop = group.len() == 1 && successors[at].contains(&at);
                        if group.len() > 1 || self_loop {
                            group.sort_unstable();
                            out.push(group);
                        }
                    }
                    if let Some((parent, _)) = calls.last() {
                        low[*parent] = low[*parent].min(low[at]);
                    }
                }
            }
        }
        out
    }

    /// Puts a node back at an id it previously held, keeping the id counter
    /// ahead of it so a later `add_node` cannot collide. Used only by undo.
    pub(crate) fn insert_node_at(&mut self, id: NodeId, node: Node) {
        self.next_node = self.next_node.max(id.raw() + 1);
        self.nodes.insert(id, node);
    }

    /// Puts an area back at an id it previously held. Used only by undo.
    pub(crate) fn insert_area_at(&mut self, id: AreaId, area: Area) {
        self.next_area = self.next_area.max(id.raw() + 1);
        self.areas.insert(id, area);
    }

    fn dependents_index(&self) -> BTreeMap<NodeId, Vec<NodeId>> {
        let mut index: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for (id, node) in &self.nodes {
            for prereq in &node.prereqs {
                index.entry(*prereq).or_default().push(*id);
            }
        }
        index
    }
}
