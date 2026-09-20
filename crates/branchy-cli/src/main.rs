//! `branchy`: the command line over a Branchy graph.
//!
//! Every mutating subcommand reassembles its arguments into one line and hands
//! it to `branchy_core::parse`, so the grammar lives in exactly one place and
//! the terminal and the in-app command line can never drift apart.

mod render;
mod store;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use branchy_core::{Graph, Status};
use clap::{Parser, Subcommand};

/// Dependency-aware task tree.
#[derive(Debug, Parser)]
#[command(name = "branchy", version, about, long_about = None)]
struct Cli {
    /// Document to work on. Defaults to the per-user data directory.
    #[arg(long, short, global = true)]
    file: Option<PathBuf>,

    #[command(subcommand)]
    command: Cmd,
}

/// Arguments collected verbatim, to be rejoined into one command line.
#[derive(Debug, clap::Args)]
struct Rest {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 1..)]
    args: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Create a task: add "Name" [in <direction>] [after a, b] [before c] [pri n]
    Add(Rest),
    /// Mark a task done
    Done(Rest),
    /// Mark a task not done
    Undone(Rest),
    /// Delete a task and strip it from everything that needed it
    Rm(Rest),
    /// Set a task's priority: pri <task> <0-255>
    Pri(Rest),
    /// Rename a task: rename <task> "New name"
    Rename(Rest),
    /// Replace a task's note: note <task> "text"
    Note(Rest),
    /// Move a task to another direction: move <task> in <direction>
    Move(Rest),
    /// Record a dependency: link <a> after <b>, or link <a> before <b>
    Link(Rest),
    /// Remove a dependency: unlink <a> after <b>
    Unlink(Rest),
    /// Create a direction: area "Hard skills" [#4fd1c5]
    Area(Rest),
    /// Remove an empty direction
    Rmarea(Rest),
    /// Run a raw command line, exactly as the in-app command line would
    Run(Rest),

    /// Everything available now, highest priority first
    Queue,
    /// Every task, grouped by direction
    List {
        /// Only this direction
        #[arg(long)]
        area: Option<String>,
        /// Only this status: done, available, locked, cyclic
        #[arg(long)]
        status: Option<String>,
    },
    /// The whole graph, by direction and tier
    Tree,
    /// What stands between you and a task
    Why(Rest),
    /// One task in full
    Show(Rest),
    /// Report dependency cycles and how to break them
    Cycles,
    /// Undo the last change
    Undo,
    /// Print where the document lives
    Where,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("branchy: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<String, String> {
    let path = match &cli.file {
        Some(given) => given.clone(),
        None => store::default_path().map_err(|e| e.to_string())?,
    };

    // read-only views and file-level operations first
    match &cli.command {
        Cmd::Where => return Ok(format!("{}\n", path.display())),
        Cmd::Undo => {
            store::undo(&path).map_err(|e| e.to_string())?;
            let graph = load(&path)?;
            return Ok(format!("Undone. {}\n", tally(&graph)));
        }
        _ => {}
    }

    let mut graph = load(&path)?;

    match &cli.command {
        Cmd::Queue => return Ok(render::queue(&graph)),
        Cmd::Tree => return Ok(render::tree(&graph)),
        Cmd::Cycles => return Ok(render::cycles(&graph)),
        Cmd::List { area, status } => {
            let wanted = match status.as_deref() {
                None => None,
                Some(word) => Some(parse_status(word)?),
            };
            return Ok(render::list(&graph, area.as_deref(), wanted));
        }
        Cmd::Why(rest) => {
            let id = branchy_core::resolve_node(&graph, &rest.args.join(" "))
                .map_err(|e| e.to_string())?;
            return Ok(render::why(&graph, id));
        }
        Cmd::Show(rest) => {
            let id = branchy_core::resolve_node(&graph, &rest.args.join(" "))
                .map_err(|e| e.to_string())?;
            return Ok(render::show(&graph, id));
        }
        _ => {}
    }

    // everything else is a mutation, expressed in the shared grammar
    let line = match &cli.command {
        Cmd::Add(rest) => line("add", &rest.args),
        Cmd::Done(rest) => line("done", &rest.args),
        Cmd::Undone(rest) => line("undone", &rest.args),
        Cmd::Rm(rest) => line("rm", &rest.args),
        Cmd::Pri(rest) => line("pri", &rest.args),
        Cmd::Rename(rest) => line("rename", &rest.args),
        Cmd::Note(rest) => line("note", &rest.args),
        Cmd::Move(rest) => line("move", &rest.args),
        Cmd::Link(rest) => line("link", &rest.args),
        Cmd::Unlink(rest) => line("unlink", &rest.args),
        Cmd::Area(rest) => line("area", &rest.args),
        Cmd::Rmarea(rest) => line("rmarea", &rest.args),
        Cmd::Run(rest) => join(&rest.args),
        Cmd::Queue
        | Cmd::List { .. }
        | Cmd::Tree
        | Cmd::Why(_)
        | Cmd::Show(_)
        | Cmd::Cycles
        | Cmd::Undo
        | Cmd::Where => unreachable!("handled above"),
    };

    let command = branchy_core::parse(&graph, &line).map_err(|e| e.to_string())?;
    let applied = graph.apply(command).map_err(|e| describe(&graph, &e))?;
    store::save(&path, &graph).map_err(|e| e.to_string())?;

    let mut out = String::new();
    if let Some(id) = applied.node {
        if let Some(node) = graph.node(id) {
            let status = graph.status(id).unwrap_or(Status::Locked);
            let _ = writeln!(
                out,
                "{}  {id}  {}",
                node.name,
                format!("{status:?}").to_lowercase()
            );
        }
    }
    let _ = writeln!(out, "{}", tally(&graph));
    Ok(out)
}

fn load(path: &std::path::Path) -> Result<Graph, String> {
    store::load(path).map_err(|e| format!("{} ({})", e, path.display()))
}

fn tally(graph: &Graph) -> String {
    let done = graph.nodes().filter(|(_, n)| n.done).count();
    format!(
        "{done}/{} done, {} available",
        graph.node_count(),
        graph.queue().len()
    )
}

/// Errors from the graph carry ids; a person wants names.
fn describe(graph: &Graph, error: &branchy_core::Error) -> String {
    let name = |id: branchy_core::NodeId| {
        graph
            .node(id)
            .map_or_else(|| id.to_string(), |node| node.name.clone())
    };
    match error {
        branchy_core::Error::WouldCycle { path, .. } => {
            let loop_text: Vec<String> = path.iter().map(|id| name(*id)).collect();
            format!(
                "refused, that would create a cycle: {}",
                loop_text.join(" -> ")
            )
        }
        branchy_core::Error::NotAPrerequisite {
            dependent,
            prerequisite,
        } => format!("{} does not need {}", name(*dependent), name(*prerequisite)),
        other => other.to_string(),
    }
}

fn parse_status(word: &str) -> Result<Status, String> {
    match word.to_ascii_lowercase().as_str() {
        "done" => Ok(Status::Done),
        "available" | "free" => Ok(Status::Available),
        "locked" => Ok(Status::Locked),
        "cyclic" => Ok(Status::Cyclic),
        other => Err(format!(
            "unknown status {other}, try done, available, locked or cyclic"
        )),
    }
}

/// Rebuilds a command line from arguments the shell already split, for the one
/// subcommand that passes a whole line through.
///
/// Anything holding whitespace is quoted again, so `branchy add "Linear
/// algebra" after algebra` reaches the parser as one name and not two words.
fn line(verb: &str, args: &[String]) -> String {
    let mut out = String::from(verb);
    for arg in args {
        out.push(' ');
        out.push_str(&requote(arg));
    }
    out
}

fn join(args: &[String]) -> String {
    args.iter()
        .map(|a| requote(a))
        .collect::<Vec<_>>()
        .join(" ")
}

fn requote(arg: &str) -> String {
    if arg.is_empty() || arg.chars().any(char::is_whitespace) {
        format!("\"{}\"", arg.replace('"', "'"))
    } else {
        arg.to_string()
    }
}
