//! Changing a graph by command lines: the one path every front end's edits
//! take.
//!
//! The terminal, the window and a batch fed to `branchy run -` all hand over
//! lines in the shared grammar. Applying them, collecting what they created,
//! and turning a refusal into something a person can act on happened in each
//! front end separately before; it lives here so they cannot drift apart.

use branchy_core::{AreaId, Command, Error, Graph, Labels, NodeId};

/// Something a line created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Made {
    /// A task.
    Node(NodeId),
    /// A direction.
    Area(AreaId),
}

impl std::fmt::Display for Made {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Node(id) => write!(f, "{id}"),
            Self::Area(id) => write!(f, "{id}"),
        }
    }
}

/// What a run of lines did.
#[derive(Debug)]
pub struct Edit {
    /// The graph afterwards.
    pub graph: Graph,
    /// The last task any line created or changed.
    pub node: Option<NodeId>,
    /// The last direction any line created or changed.
    pub area: Option<AreaId>,
    /// Everything the lines created, in order, with the label the line gave
    /// it, if any.
    pub made: Vec<(Option<String>, Made)>,
    /// Commands that take the whole run back, in the order to apply them.
    pub undo: Vec<Command>,
}

/// A line the graph or the parser would not accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// Which line, counting from 1.
    pub line: usize,
    /// Why, in words, naming tasks rather than ids where that helps.
    pub message: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Refused {}

/// Applies lines in order, as one change.
///
/// Takes the graph by value and hands it back only on success. A refusal at
/// the fourth line leaves the first three applied to a graph that nobody gets
/// back, so a caller cannot save a half-applied edit by mistake: refusing the
/// lot is better than keeping part of it.
///
/// Lines may label what they create (`$x = add ...`) and refer to it later,
/// see [`branchy_core::parse_labelled`]. Blank lines and lines starting with
/// `#` are skipped, so a batch file can carry comments.
///
/// # Errors
///
/// [`Refused`] for the first line that did not parse or was not accepted.
pub fn apply_lines<S: AsRef<str>>(mut graph: Graph, lines: &[S]) -> Result<Edit, Refused> {
    let mut labels = Labels::new();
    let mut node = None;
    let mut area = None;
    let mut made = Vec::new();
    let mut undo: Vec<Command> = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let line = line.as_ref().trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let refuse = |message: String| Refused {
            line: index + 1,
            message,
        };
        let (label, command) = branchy_core::parse_labelled(&graph, line, &labels)
            .map_err(|e| refuse(e.to_string()))?;
        let creates = matches!(command, Command::AddNode { .. } | Command::AddArea { .. });
        let applied = graph
            .apply(command)
            .map_err(|e| refuse(describe(&graph, &e)))?;

        if creates {
            let thing = match (applied.node, applied.area) {
                (Some(id), _) => Some(Made::Node(id)),
                (None, Some(id)) => Some(Made::Area(id)),
                (None, None) => None,
            };
            if let Some(thing) = thing {
                if let Some(label) = &label {
                    match thing {
                        Made::Node(id) => labels.name_node(label.clone(), id),
                        Made::Area(id) => labels.name_area(label.clone(), id),
                    }
                }
                made.push((label, thing));
            }
        }
        node = applied.node.or(node);
        area = applied.area.or(area);
        // later undos must run first
        let mut next = applied.undo;
        next.extend(undo);
        undo = next;
    }

    Ok(Edit {
        graph,
        node,
        area,
        made,
        undo,
    })
}

/// A refusal from the graph, in words. The graph speaks in ids; a person
/// wants names.
#[must_use]
pub fn describe(graph: &Graph, error: &Error) -> String {
    match error {
        Error::WouldCycle { path, .. } => {
            let names: Vec<String> = path.iter().map(|id| name(graph, *id)).collect();
            format!(
                "Refused: that would create a cycle. {}",
                names.join(" \u{2192} ")
            )
        }
        Error::NotAPrerequisite {
            dependent,
            prerequisite,
        } => format!(
            "{} does not need {}",
            name(graph, *dependent),
            name(graph, *prerequisite)
        ),
        other => other.to_string(),
    }
}

fn name(graph: &Graph, id: NodeId) -> String {
    graph
        .node(id)
        .map_or_else(|| id.to_string(), |node| node.name.clone())
}
