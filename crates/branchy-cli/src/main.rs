//! `branchy`: the command line over a Branchy graph.
//!
//! Every mutating subcommand reassembles its arguments into one line and hands
//! it to `branchy_core::parse`, so the grammar lives in exactly one place and
//! the terminal and the in-app command line can never drift apart.

mod render;

use branchy_app::{StoreError, Vault, Vaults, snapshot, store};

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use branchy_core::{Graph, Status};
use clap::{Parser, Subcommand};

/// Dependency-aware task tree.
#[derive(Debug, Parser)]
#[command(name = "branchy", version, about, long_about = None)]
struct Cli {
    /// Document to work on, bypassing vaults altogether.
    #[arg(long, short, global = true, conflicts_with = "vault")]
    file: Option<PathBuf>,

    /// Vault to work in for this one command, by name or folder. Defaults to
    /// the vault opened last, here or in the window.
    #[arg(long, global = true, value_name = "NAME|FOLDER")]
    vault: Option<String>,

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
    /// Set or clear a deadline: due <task> <YYYY-MM-DD | none>
    Due(Rest),
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
    /// Rename a direction: rename-area <direction> "New name"
    #[command(name = "rename-area")]
    RenameArea(Rest),
    /// Recolour a direction: recolor-area <direction> #4fd1c5
    #[command(name = "recolor-area", alias = "recolour-area")]
    RecolorArea(Rest),
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
    /// Everything with a deadline, grouped by how soon it is
    Calendar,
    /// Report dependency cycles and how to break them
    Cycles,
    /// Print the whole graph as JSON, exactly as a user interface receives it
    Snapshot,
    /// Undo the last change
    Undo,
    /// Print where the document lives
    Where,

    /// List, create, open and forget vaults. On its own, lists them.
    Vault {
        #[command(subcommand)]
        action: Option<VaultCmd>,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum VaultCmd {
    /// The recent vaults, newest first; the current one is marked
    List,
    /// Create a vault and work in it: vault new <name> [--in <folder>]
    New {
        /// The vault's name, which is also its folder's name
        name: String,
        /// Where to create it. Defaults to the current directory.
        #[arg(long = "in", value_name = "FOLDER")]
        parent: Option<PathBuf>,
    },
    /// Work in a vault from now on, here and in the window's recent list
    Open {
        /// A recent vault's name, or any folder
        #[arg(value_name = "NAME|FOLDER")]
        vault: String,
    },
    /// Take a vault off the recent list. Its folder is left alone.
    Forget {
        /// A recent vault's name or folder
        #[arg(value_name = "NAME|FOLDER")]
        vault: String,
    },
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
    if let Cmd::Vault { action } = &cli.command {
        return vault(action.clone().unwrap_or(VaultCmd::List)).map_err(|e| e.to_string());
    }
    let path = document(cli).map_err(|e| e.to_string())?;

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
        Cmd::Calendar => return Ok(render::calendar(&graph)),
        Cmd::Snapshot => {
            let json = serde_json::to_string_pretty(&snapshot::Snapshot::of(&graph))
                .map_err(|e| e.to_string())?;
            return Ok(format!("{json}\n"));
        }
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
        Cmd::Due(rest) => line("due", &rest.args),
        Cmd::Rename(rest) => line("rename", &rest.args),
        Cmd::Note(rest) => line("note", &rest.args),
        Cmd::Move(rest) => line("move", &rest.args),
        Cmd::Link(rest) => line("link", &rest.args),
        Cmd::Unlink(rest) => line("unlink", &rest.args),
        Cmd::Area(rest) => line("area", &rest.args),
        Cmd::RenameArea(rest) => line("rename-area", &rest.args),
        Cmd::RecolorArea(rest) => line("recolor-area", &rest.args),
        Cmd::Rmarea(rest) => line("rmarea", &rest.args),
        Cmd::Run(rest) => join(&rest.args),
        Cmd::Queue
        | Cmd::List { .. }
        | Cmd::Tree
        | Cmd::Why(_)
        | Cmd::Show(_)
        | Cmd::Cycles
        | Cmd::Calendar
        | Cmd::Snapshot
        | Cmd::Undo
        | Cmd::Where
        | Cmd::Vault { .. } => unreachable!("handled above"),
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

/// Which document a command works on.
///
/// An explicit `--file` or `--vault` wins, then `BRANCHY_FILE`, then the vault
/// opened last.
fn document(cli: &Cli) -> Result<PathBuf, StoreError> {
    if let Some(given) = &cli.file {
        return Ok(given.clone());
    }
    if let Some(named) = &cli.vault {
        return Ok(Vaults::user()?.resolve(named)?.document());
    }
    store::default_path()
}

fn vault(action: VaultCmd) -> Result<String, StoreError> {
    let mut vaults = Vaults::user()?;
    let mut out = String::new();
    match action {
        VaultCmd::List => return Ok(render::vaults(&vaults)),
        VaultCmd::New { name, parent } => {
            let parent = match parent {
                Some(given) => given,
                None => std::env::current_dir()?,
            };
            let vault = Vault::create(&parent, &name)?;
            vaults.opened(&vault);
            vaults.save()?;
            let _ = writeln!(
                out,
                "Created {} at {}. It is the current vault now.",
                vault.name(),
                vault.dir().display()
            );
        }
        VaultCmd::Open { vault: given } => {
            let vault = vaults.resolve(&given)?;
            vaults.opened(&vault);
            vaults.save()?;
            let _ = writeln!(
                out,
                "Working in {} ({}).",
                vault.name(),
                vault.dir().display()
            );
        }
        VaultCmd::Forget { vault: given } => {
            let dir = vaults.forget(&given)?;
            vaults.save()?;
            let _ = writeln!(
                out,
                "Took {} off the list. The folder is still there.",
                dir.display()
            );
        }
    }
    if let Some(file) = std::env::var_os("BRANCHY_FILE") {
        let _ = writeln!(
            out,
            "Note: BRANCHY_FILE is set, so every other command still uses {}.",
            Path::new(&file).display()
        );
    }
    Ok(out)
}

fn load(path: &Path) -> Result<Graph, String> {
    store::load(path).map_err(|e| format!("{} ({})", e, path.display()))
}

fn tally(graph: &Graph) -> String {
    let done = graph.nodes().filter(|(_, n)| n.done).count();
    let now = branchy_app::today();
    let overdue = graph
        .effective_due()
        .iter()
        .filter(|(id, date)| **date < now && graph.node(**id).is_some_and(|node| !node.done))
        .count();
    let mut line = format!(
        "{done}/{} done, {} available",
        graph.node_count(),
        graph.queue().len()
    );
    if overdue > 0 {
        let _ = write!(line, ", {overdue} overdue");
    }
    line
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
    if arg.is_empty() || arg.chars().any(char::is_whitespace) || arg.contains(['"', '\\']) {
        branchy_core::quote(arg)
    } else {
        arg.to_string()
    }
}
