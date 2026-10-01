//! Turning a line of text into a [`Command`].
//!
//! One grammar serves the in-app command line and the `branchy` binary, so
//! muscle memory transfers and anything scriptable in one is scriptable in the
//! other.
//!
//! ```text
//! add <name> [in <area>] [after a, b] [before c] [pri n] [note "..."] [due YYYY-MM-DD]
//! due <task> <YYYY-MM-DD | none>
//! done <task>            undone <task>
//! link <a> after <b>     link <a> before <b>     unlink <a> after <b>
//! rm <task>              pri <task> <n>          rename <task> <name>
//! note <task> <text>     move <task> in <area>
//! area <name> [colour]   rmarea <area>
//! rename-area <area> <name>                       recolor-area <area> <colour>
//! ```
//!
//! `after` and `needs` are the same word for "blocked by"; `before` and
//! `blocks` describe the same edge from the other end. Tasks and areas are
//! named by id or by any unambiguous part of their name.
//!
//! A batch of lines (see [`parse_labelled`]) may also name what a line
//! creates, and refer to it from later lines, before it has an id:
//!
//! ```text
//! $calc = add Calculus in Maths
//! add "Linear algebra" after $calc
//! ```

use std::collections::{BTreeMap, BTreeSet};

use crate::command::Command;
use crate::date::Date;
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
    /// A deadline was expected and the word was not a date.
    BadDate(String),
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
            Self::BadDate(word) => {
                write!(f, "{word} is not a date, expected YYYY-MM-DD or none")
            }
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

const KEYWORDS: [&str; 9] = [
    "in", "after", "needs", "before", "blocks", "pri", "priority", "note", "due",
];

fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word.to_ascii_lowercase().as_str())
}

/// Splits a line into words, keeping anything inside double quotes together.
///
/// An empty pair of quotes produces an empty word, which is how a note or name
/// is deliberately cleared. A backslash escapes the character after it, so a
/// name may contain a quote or a backslash of its own — which matters because
/// a user interface builds these lines out of whatever someone typed into a
/// form, and must be able to do so without the line falling apart.
fn tokenize(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut open = false;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
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

/// Wraps a value so [`tokenize`] gives it back unchanged.
///
/// Front ends building a command line from form fields use this rather than
/// inventing their own quoting.
#[must_use]
pub fn quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Names a batch gave to what its earlier lines created.
///
/// A line in a batch cannot refer to a task an earlier line created by id,
/// because nobody writing the batch knows that id yet. So a line may begin
/// with `$name =`, and later lines say `$name` wherever a task or direction
/// goes. The caller records what each labelled line created, since only it
/// sees the command applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Labels {
    nodes: BTreeMap<String, NodeId>,
    areas: BTreeMap<String, AreaId>,
}

impl Labels {
    /// No names yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that `label` (without its `$`) means this task.
    pub fn name_node(&mut self, label: impl Into<String>, id: NodeId) {
        self.nodes.insert(label.into(), id);
    }

    /// Records that `label` (without its `$`) means this direction.
    pub fn name_area(&mut self, label: impl Into<String>, id: AreaId) {
        self.areas.insert(label.into(), id);
    }

    /// The task `label` names, if any.
    #[must_use]
    pub fn node(&self, label: &str) -> Option<NodeId> {
        self.nodes.get(label).copied()
    }

    /// The direction `label` names, if any.
    #[must_use]
    pub fn area(&self, label: &str) -> Option<AreaId> {
        self.areas.get(label).copied()
    }
}

/// What a reference is resolved against: the graph, and in a batch the
/// labels its earlier lines gave out.
struct Scope<'a> {
    graph: &'a Graph,
    labels: Option<&'a Labels>,
}

impl Scope<'_> {
    fn node(&self, query: &str) -> Result<NodeId, ParseError> {
        if let (Some(labels), Some(label)) = (self.labels, query.strip_prefix('$')) {
            return labels
                .node(label)
                .ok_or_else(|| ParseError::NoSuchTask(query.to_string()));
        }
        resolve_node(self.graph, query)
    }

    fn area(&self, query: &str) -> Result<AreaId, ParseError> {
        if let (Some(labels), Some(label)) = (self.labels, query.strip_prefix('$')) {
            return labels
                .area(label)
                .ok_or_else(|| ParseError::NoSuchArea(query.to_string()));
        }
        resolve_area(self.graph, query)
    }
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
    let scope = Scope {
        graph,
        labels: None,
    };
    parse_tokens(&scope, &tokenize(input.trim()))
}

/// Turns one line of a batch into a command, along with the label the line
/// gives to whatever it creates.
///
/// Unlike [`parse`], a line may begin with `$name =`, and `$name` anywhere a
/// task or direction goes is looked up in `labels` rather than in the graph.
///
/// # Errors
///
/// As [`parse`]. A `$name` nothing was given is [`ParseError::NoSuchTask`] or
/// [`ParseError::NoSuchArea`].
pub fn parse_labelled(
    graph: &Graph,
    input: &str,
    labels: &Labels,
) -> Result<(Option<String>, Command), ParseError> {
    let scope = Scope {
        graph,
        labels: Some(labels),
    };
    let mut tokens = tokenize(input.trim());
    // `$x = add`, `$x= add` and `$x=add` all name the line `x`
    let label = match tokens.first() {
        Some(first) if first.len() > 1 && first.starts_with('$') => {
            let first = tokens.remove(0);
            let (name, after) = first[1..].split_once('=').unwrap_or((&first[1..], ""));
            if !after.is_empty() {
                tokens.insert(0, after.to_string());
            } else if tokens.first().is_some_and(|next| next == "=") {
                tokens.remove(0);
            }
            Some(name.to_string())
        }
        _ => None,
    };
    let rest = tokens.as_slice();
    Ok((label, parse_tokens(&scope, rest)?))
}

fn parse_tokens(scope: &Scope<'_>, tokens: &[String]) -> Result<Command, ParseError> {
    let Some((verb, rest)) = tokens.split_first() else {
        return Err(ParseError::Empty);
    };

    match verb.to_ascii_lowercase().as_str() {
        "add" => parse_add(scope, rest),
        "done" => Ok(Command::SetDone {
            node: scope.node(&join(rest))?,
            done: true,
        }),
        "undone" => Ok(Command::SetDone {
            node: scope.node(&join(rest))?,
            done: false,
        }),
        "rm" | "del" | "delete" => Ok(Command::RemoveNode(scope.node(&join(rest))?)),
        "pri" | "priority" => parse_priority(scope, rest),
        "due" => parse_due(scope, rest),
        "rename" => parse_two_part(rest).map_or(Err(ParseError::MissingTask), |(who, what)| {
            Ok(Command::SetName {
                node: scope.node(&who)?,
                name: what,
            })
        }),
        "note" => parse_two_part(rest).map_or(Err(ParseError::MissingTask), |(who, what)| {
            Ok(Command::SetNote {
                node: scope.node(&who)?,
                note: what,
            })
        }),
        "move" => parse_move(scope, rest),
        "link" | "unlink" => parse_link(scope, verb == "link", rest),
        "area" => parse_area(rest),
        "rename-area" => {
            parse_two_part(rest).map_or(Err(ParseError::MissingTask), |(who, what)| {
                Ok(Command::SetAreaName {
                    area: scope.area(&who)?,
                    name: what,
                })
            })
        }
        "recolor-area" | "recolour-area" => {
            parse_two_part(rest).map_or(Err(ParseError::MissingTask), |(who, what)| {
                Ok(Command::SetAreaColor {
                    area: scope.area(&who)?,
                    color: what,
                })
            })
        }
        "rmarea" => Ok(Command::RemoveArea(scope.area(&join(rest))?)),
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

fn parse_priority(scope: &Scope<'_>, tokens: &[String]) -> Result<Command, ParseError> {
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
        node: scope.node(&join(head))?,
        priority,
    })
}

/// `due <task> 2026-12-31`, or `due <task> none` to take the date off.
fn parse_due(scope: &Scope<'_>, tokens: &[String]) -> Result<Command, ParseError> {
    let Some((last, head)) = tokens.split_last() else {
        return Err(ParseError::MissingTask);
    };
    if head.is_empty() {
        return Err(ParseError::MissingTask);
    }
    let due = if last.eq_ignore_ascii_case("none") || last.eq_ignore_ascii_case("never") {
        None
    } else {
        Some(
            last.parse::<Date>()
                .map_err(|_| ParseError::BadDate(last.clone()))?,
        )
    };
    Ok(Command::SetDue {
        node: scope.node(&join(head))?,
        due,
    })
}

fn parse_move(scope: &Scope<'_>, tokens: &[String]) -> Result<Command, ParseError> {
    let at = tokens
        .iter()
        .position(|t| t.eq_ignore_ascii_case("in"))
        .ok_or(ParseError::MissingSeparator)?;
    if at == 0 {
        return Err(ParseError::MissingTask);
    }
    Ok(Command::SetArea {
        node: scope.node(&join(&tokens[..at]))?,
        area: scope.area(&join(&tokens[at + 1..]))?,
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

fn parse_link(scope: &Scope<'_>, adding: bool, tokens: &[String]) -> Result<Command, ParseError> {
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

    let left = scope.node(&join(&tokens[..at]))?;
    let right = scope.node(&join(&tokens[at + 1..]))?;
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

fn parse_add(scope: &Scope<'_>, tokens: &[String]) -> Result<Command, ParseError> {
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
    let mut note = String::new();
    let mut due: Option<Date> = None;

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
            "note" => note = values.join(" "),
            "due" => {
                let word = values.first().cloned().unwrap_or_default();
                due = Some(
                    word.parse::<Date>()
                        .map_err(|_| ParseError::BadDate(word))?,
                );
            }
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
        scope.area(&reference)?
    } else {
        let mut areas = scope.graph.areas().map(|(id, _)| id);
        let only = areas.next().ok_or(ParseError::NoAreas)?;
        if areas.next().is_some() {
            return Err(ParseError::NoSuchArea("in <direction>".to_string()));
        }
        only
    };

    let mut prereqs = BTreeSet::new();
    for reference in &after {
        prereqs.insert(scope.node(reference)?);
    }
    let mut dependents = BTreeSet::new();
    for reference in &before {
        dependents.insert(scope.node(reference)?);
    }

    Ok(Command::AddNode {
        name,
        note,
        area,
        priority,
        due,
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
