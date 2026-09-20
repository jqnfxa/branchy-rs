//! Turning a line of text into a [`Command`].
//!
//! One grammar serves the in-app command line and the `branchy` binary, so
//! muscle memory transfers and anything scriptable in one is scriptable in the
//! other.
//!
//! ```text
//! add <name> [in <area>] [after a, b] [before c] [pri n]
//! done <task>            undone <task>
//! link <a> after <b>     link <a> before <b>     unlink <a> after <b>
//! rm <task>              pri <task> <n>          rename <task> <name>
//! note <task> <text>     move <task> in <area>
//! area <name> [colour]   rmarea <area>
//! ```
//!
//! `after` and `needs` are the same word for "blocked by"; `before` and
//! `blocks` describe the same edge from the other end. Tasks and areas are
//! named by id or by any unambiguous part of their name.

use std::collections::BTreeSet;

use crate::command::Command;
use crate::graph::Graph;
use crate::id::{AreaId, NodeId};

/// Why a line could not be turned into a command.
///
/// Separate from [`Error`](crate::Error) on purpose: this is about what someone
/// typed, not about what the graph refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The line was blank.
    Empty,
    /// The first word is not a command.
    UnknownVerb(String),
    /// `add` was given nothing to call the new task.
    MissingName,
    /// A command that needs a task was given none.
    MissingTask,
    /// `link` or `unlink` was written without `after` or `before`.
    MissingSeparator,
    /// A priority was expected and the word was not a number in range.
    BadPriority(String),
    /// Nothing matched this task reference.
    NoSuchTask(String),
    /// Several tasks matched this reference.
    AmbiguousTask {
        /// What was typed.
        query: String,
        /// Everything it matched, in id order.
        matches: Vec<NodeId>,
    },
    /// Nothing matched this area reference.
    NoSuchArea(String),
    /// Several areas matched this reference.
    AmbiguousArea {
        /// What was typed.
        query: String,
        /// Everything it matched, in id order.
        matches: Vec<AreaId>,
    },
    /// The graph holds no areas, so a node cannot be created yet.
    NoAreas,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "nothing to do"),
            Self::UnknownVerb(word) => write!(f, "unknown command: {word}"),
            Self::MissingName => write!(f, "give the task a name"),
            Self::MissingTask => write!(f, "name a task"),
            Self::MissingSeparator => write!(f, "needs two tasks: link A after B"),
            Self::BadPriority(word) => write!(f, "priority has to be 0-255, not {word}"),
            Self::NoSuchTask(query) => write!(f, "no task matches {query}"),
            Self::AmbiguousTask { query, matches } => {
                write!(f, "{query} matches {} tasks", matches.len())
            }
            Self::NoSuchArea(query) => write!(f, "no direction matches {query}"),
            Self::AmbiguousArea { query, matches } => {
                write!(f, "{query} matches {} directions", matches.len())
            }
            Self::NoAreas => write!(f, "create a direction first: area \"Hard skills\""),
        }
    }
}

impl std::error::Error for ParseError {}

const KEYWORDS: [&str; 7] = [
    "in", "after", "needs", "before", "blocks", "pri", "priority",
];

fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word.to_ascii_lowercase().as_str())
}

/// Splits a line into words, keeping anything inside double quotes together.
///
/// An empty pair of quotes produces an empty word, which is how a note or name
/// is deliberately cleared.
fn tokenize(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut open = false;

    for ch in input.chars() {
        if ch == '"' {
            open = !open;
            quoted = true;
        } else if !open && ch.is_whitespace() {
            if !current.is_empty() || quoted {
                out.push(std::mem::take(&mut current));
                quoted = false;
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() || quoted {
        out.push(current);
    }
    out
}

/// Finds the one node a reference names.
///
/// An exact id (`n12`) or an exact name wins outright; otherwise it is a
/// case-insensitive substring match, which has to land on exactly one node.
///
/// # Errors
///
/// [`ParseError::NoSuchTask`] or [`ParseError::AmbiguousTask`].
pub fn resolve_node(graph: &Graph, query: &str) -> Result<NodeId, ParseError> {
    if query.is_empty() {
        return Err(ParseError::MissingTask);
    }
    let lower = query.to_lowercase();

    let exact: Vec<NodeId> = graph
        .nodes()
        .filter(|(id, node)| id.to_string() == lower || node.name.to_lowercase() == lower)
        .map(|(id, _)| id)
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0]);
    }

    let hits: Vec<NodeId> = graph
        .nodes()
        .filter(|(_, node)| node.name.to_lowercase().contains(&lower))
        .map(|(id, _)| id)
        .collect();

    match hits.len() {
        0 => Err(ParseError::NoSuchTask(query.to_string())),
        1 => Ok(hits[0]),
        _ => Err(ParseError::AmbiguousTask {
            query: query.to_string(),
            matches: hits,
        }),
    }
}

/// Finds the one area a reference names. See [`resolve_node`].
///
/// # Errors
///
/// [`ParseError::NoSuchArea`] or [`ParseError::AmbiguousArea`].
pub fn resolve_area(graph: &Graph, query: &str) -> Result<AreaId, ParseError> {
    let lower = query.to_lowercase();

    let exact: Vec<AreaId> = graph
        .areas()
        .filter(|(id, area)| id.to_string() == lower || area.name.to_lowercase() == lower)
        .map(|(id, _)| id)
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0]);
    }

    let hits: Vec<AreaId> = graph
        .areas()
        .filter(|(_, area)| area.name.to_lowercase().contains(&lower))
        .map(|(id, _)| id)
        .collect();

    match hits.len() {
        0 => Err(ParseError::NoSuchArea(query.to_string())),
        1 => Ok(hits[0]),
        _ => Err(ParseError::AmbiguousArea {
            query: query.to_string(),
            matches: hits,
        }),
    }
}

/// Turns one line into a command, resolving names against `graph`.
///
/// # Errors
///
/// A [`ParseError`] describing what was wrong with the line. Nothing is
/// changed: applying the command is a separate step.
pub fn parse(graph: &Graph, input: &str) -> Result<Command, ParseError> {
    let tokens = tokenize(input.trim());
    let Some((verb, rest)) = tokens.split_first() else {
        return Err(ParseError::Empty);
    };

    match verb.to_ascii_lowercase().as_str() {
        "add" => parse_add(graph, rest),
        "done" => Ok(Command::SetDone {
            node: resolve_node(graph, &join(rest))?,
            done: true,
        }),
        "undone" => Ok(Command::SetDone {
            node: resolve_node(graph, &join(rest))?,
            done: false,
        }),
        "rm" | "del" | "delete" => Ok(Command::RemoveNode(resolve_node(graph, &join(rest))?)),
        "pri" | "priority" => parse_priority(graph, rest),
        "rename" => parse_two_part(rest).map_or(Err(ParseError::MissingTask), |(who, what)| {
            Ok(Command::SetName {
                node: resolve_node(graph, &who)?,
                name: what,
            })
        }),
        "note" => parse_two_part(rest).map_or(Err(ParseError::MissingTask), |(who, what)| {
            Ok(Command::SetNote {
                node: resolve_node(graph, &who)?,
                note: what,
            })
        }),
        "move" => parse_move(graph, rest),
        "link" | "unlink" => parse_link(graph, verb == "link", rest),
        "area" => parse_area(rest),
        "rmarea" => Ok(Command::RemoveArea(resolve_area(graph, &join(rest))?)),
        other => Err(ParseError::UnknownVerb(other.to_string())),
    }
}

fn join(tokens: &[String]) -> String {
    tokens.join(" ").trim().to_string()
}

/// Splits `<task> <rest>` where the task is the first token.
fn parse_two_part(tokens: &[String]) -> Option<(String, String)> {
    let (first, rest) = tokens.split_first()?;
    Some((first.clone(), join(rest)))
}

fn parse_priority(graph: &Graph, tokens: &[String]) -> Result<Command, ParseError> {
    let Some((last, head)) = tokens.split_last() else {
        return Err(ParseError::MissingTask);
    };
    if head.is_empty() {
        return Err(ParseError::MissingTask);
    }
    let priority = last
        .parse::<u8>()
        .map_err(|_| ParseError::BadPriority(last.clone()))?;
    Ok(Command::SetPriority {
        node: resolve_node(graph, &join(head))?,
        priority,
    })
}

fn parse_move(graph: &Graph, tokens: &[String]) -> Result<Command, ParseError> {
    let at = tokens
        .iter()
        .position(|t| t.eq_ignore_ascii_case("in"))
        .ok_or(ParseError::MissingSeparator)?;
    if at == 0 {
        return Err(ParseError::MissingTask);
    }
    Ok(Command::SetArea {
        node: resolve_node(graph, &join(&tokens[..at]))?,
        area: resolve_area(graph, &join(&tokens[at + 1..]))?,
    })
}

fn parse_area(tokens: &[String]) -> Result<Command, ParseError> {
    let Some((name, rest)) = tokens.split_first() else {
        return Err(ParseError::MissingName);
    };
    if name.is_empty() {
        return Err(ParseError::MissingName);
    }
    let color = if rest.is_empty() {
        "#8991ac".to_string()
    } else {
        join(rest)
    };
    Ok(Command::AddArea {
        name: name.clone(),
        color,
    })
}

fn parse_link(graph: &Graph, adding: bool, tokens: &[String]) -> Result<Command, ParseError> {
    let at = tokens
        .iter()
        .position(|t| {
            let w = t.to_ascii_lowercase();
            w == "after" || w == "needs" || w == "before" || w == "blocks"
        })
        .ok_or(ParseError::MissingSeparator)?;
    if at == 0 || at == tokens.len() - 1 {
        return Err(ParseError::MissingSeparator);
    }

    let left = resolve_node(graph, &join(&tokens[..at]))?;
    let right = resolve_node(graph, &join(&tokens[at + 1..]))?;
    let inverted = {
        let w = tokens[at].to_ascii_lowercase();
        w == "before" || w == "blocks"
    };

    // "A before B" and "B after A" are the same edge
    let (dependent, prerequisite) = if inverted {
        (right, left)
    } else {
        (left, right)
    };

    Ok(if adding {
        Command::AddPrerequisite {
            dependent,
            prerequisite,
        }
    } else {
        Command::RemovePrerequisite {
            dependent,
            prerequisite,
        }
    })
}

fn parse_add(graph: &Graph, tokens: &[String]) -> Result<Command, ParseError> {
    let mut at = 0;
    let mut name_parts: Vec<String> = Vec::new();
    while at < tokens.len() && !is_keyword(&tokens[at]) {
        name_parts.push(tokens[at].clone());
        at += 1;
    }
    let name = join(&name_parts);
    if name.is_empty() {
        return Err(ParseError::MissingName);
    }

    let mut area_ref: Option<String> = None;
    let mut after: Vec<String> = Vec::new();
    let mut before: Vec<String> = Vec::new();
    let mut priority: u8 = 5;

    while at < tokens.len() {
        let keyword = tokens[at].to_ascii_lowercase();
        at += 1;
        let start = at;
        while at < tokens.len() && !is_keyword(&tokens[at]) {
            at += 1;
        }
        let values = &tokens[start..at];

        match keyword.as_str() {
            "in" => area_ref = Some(join(values)),
            "after" | "needs" => after.extend(split_list(values)),
            "before" | "blocks" => before.extend(split_list(values)),
            _ => {
                let word = values.first().cloned().unwrap_or_default();
                priority = word
                    .parse::<u8>()
                    .map_err(|_| ParseError::BadPriority(word))?;
            }
        }
    }

    // with no `in`, fall back to the only area there is
    let area = if let Some(reference) = area_ref {
        resolve_area(graph, &reference)?
    } else {
        let mut areas = graph.areas().map(|(id, _)| id);
        let only = areas.next().ok_or(ParseError::NoAreas)?;
        if areas.next().is_some() {
            return Err(ParseError::NoSuchArea("in <direction>".to_string()));
        }
        only
    };

    let mut prereqs = BTreeSet::new();
    for reference in &after {
        prereqs.insert(resolve_node(graph, reference)?);
    }
    let mut dependents = BTreeSet::new();
    for reference in &before {
        dependents.insert(resolve_node(graph, reference)?);
    }

    Ok(Command::AddNode {
        name,
        note: String::new(),
        area,
        priority,
        prereqs,
        dependents,
    })
}

/// `after calculus, linalg` is two references, not one.
fn split_list(values: &[String]) -> Vec<String> {
    values
        .join(" ")
        .split(',')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}
