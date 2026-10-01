//! `branchy`: the command line over a Branchy graph.
//!
//! Every mutating subcommand reassembles its arguments into one line and hands
//! it to `branchy_core::parse`, so the grammar lives in exactly one place and
//! the terminal and the in-app command line can never drift apart.

mod plain;
mod render;

use branchy_app::{Made, StoreError, Vault, Vaults, snapshot, store};

use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use branchy_core::{Graph, Status};
use clap::{Parser, Subcommand};

/// The page `branchy guide` prints. Every `$ branchy` line in it is run by
/// the test suite, so it cannot advertise something the binary does not do.
const GUIDE: &str = include_str!("guide.txt");

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

    /// Print for a program or an agent: one tab-separated line per task,
    /// names never shortened, other tasks and directions referred to by id.
    #[arg(long, global = true)]
    plain: bool,

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
    /// Mark a task done, which unlocks whatever needs it
    Done(Rest),
    /// Mark a task not done; it goes back to the backlog
    Undone(Rest),
    /// Plan a task to be done next. A locked task may be planned ahead.
    Todo(Rest),
    /// Start working on a task. Only an available task can be started.
    Start(Rest),
    /// Mark a task finished but waiting to be checked. It unlocks nothing yet.
    Review(Rest),
    /// Move a task to a stage: stage <task> <backlog|todo|doing|review|done>
    Stage(Rest),
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
    /// Run a raw command line, exactly as the in-app command line would.
    /// `run -` reads many lines from stdin and applies them as one change:
    /// all or nothing, one save, one undo. A line may start with `$x =` to
    /// name what it creates, and later lines refer to it as `$x`.
    Run(Rest),

    /// How to drive Branchy from a script or an agent, in one page
    Guide,
    /// Where things stand, in one screen. The first thing to run.
    Brief {
        /// How many tasks to show from the queue, and from what is due soon
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Tasks whose name holds some text, ignoring case: find <text>
    Find {
        /// What to look for
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
        /// Look in notes too, after the names
        #[arg(long)]
        notes: bool,
        /// Only the first this many
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Everything available now, most urgent first
    Queue {
        /// Only the first this many
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Every task, grouped by direction
    List {
        /// Only this direction
        #[arg(long)]
        area: Option<String>,
        /// Only this status: done, available, locked, cyclic
        #[arg(long)]
        status: Option<String>,
        /// Only this stage: backlog, todo, doing, review, done
        #[arg(long)]
        stage: Option<String>,
        /// Only the first this many
        #[arg(long)]
        limit: Option<usize>,
    },
    /// The board: backlog, todo, doing, review and done
    Board {
        /// How many to show of the backlog and of done, which run long
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// The whole graph, by direction and tier
    Tree,
    /// What stands between you and a task
    Why(Rest),
    /// One task in full
    Show(Rest),
    /// Everything with a deadline, grouped by how soon it is
    Calendar {
        /// Only the first this many
        #[arg(long)]
        limit: Option<usize>,
    },
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
        Ok(output) => emit(&output),
        Err(message) => {
            eprintln!("branchy: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Writes the output, treating a reader that stopped listening as success.
///
/// `print!` panics when stdout is a pipe whose reader has gone, which is what
/// `branchy queue | head` does as soon as it has its lines. Nothing went wrong
/// there: the reader got everything it asked for.
fn emit(output: &str) -> ExitCode {
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(output.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("branchy: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<String, String> {
    if let Cmd::Guide = &cli.command {
        return Ok(GUIDE.to_string());
    }
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

    let graph = load(&path)?;

    let plain = cli.plain;
    if let Cmd::Brief { limit } = &cli.command {
        let title = vault_title(&path);
        return Ok(if plain {
            plain::brief(&graph, &title, *limit)
        } else {
            render::brief(&graph, &title, *limit)
        });
    }
    if let Some(shown) = view(&cli.command, &graph, plain) {
        return shown;
    }

    // everything else is a mutation, expressed in the shared grammar
    if let Cmd::Run(rest) = &cli.command {
        if rest.args == ["-"] {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| format!("could not read the batch from stdin: {e}"))?;
            return batch(&path, graph, &text, plain);
        }
    }

    let line = match &cli.command {
        Cmd::Add(rest) => line("add", &rest.args),
        Cmd::Done(rest) => line("done", &rest.args),
        Cmd::Undone(rest) => line("undone", &rest.args),
        Cmd::Todo(rest) => line("todo", &rest.args),
        Cmd::Start(rest) => line("start", &rest.args),
        Cmd::Review(rest) => line("review", &rest.args),
        Cmd::Stage(rest) => line("stage", &rest.args),
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
        Cmd::Guide
        | Cmd::Brief { .. }
        | Cmd::Queue { .. }
        | Cmd::List { .. }
        | Cmd::Board { .. }
        | Cmd::Find { .. }
        | Cmd::Tree
        | Cmd::Why(_)
        | Cmd::Show(_)
        | Cmd::Cycles
        | Cmd::Calendar { .. }
        | Cmd::Snapshot
        | Cmd::Undo
        | Cmd::Where
        | Cmd::Vault { .. } => unreachable!("handled above"),
    };

    let edit = branchy_app::apply_lines(graph, &[line]).map_err(|e| e.message)?;
    let graph = edit.graph;
    store::save_change(&path, &graph, &edit.undo).map_err(|e| e.to_string())?;
    if plain {
        return Ok(plain::changed(&graph, edit.node));
    }

    let mut out = String::new();
    if let Some(id) = edit.node {
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

/// The read-only views, or `None` for a command that changes something.
fn view(command: &Cmd, graph: &Graph, plain: bool) -> Option<Result<String, String>> {
    let shown = match command {
        Cmd::Queue { limit } if plain => Ok(plain::queue(graph, *limit)),
        Cmd::Queue { limit } => Ok(render::queue(graph, *limit)),
        Cmd::Tree if plain => Ok(plain::tree(graph)),
        Cmd::Tree => Ok(render::tree(graph)),
        Cmd::Cycles => Ok(render::cycles(graph)),
        Cmd::Calendar { limit } if plain => Ok(plain::calendar(graph, *limit)),
        Cmd::Calendar { limit } => Ok(render::calendar(graph, *limit)),
        Cmd::Snapshot => serde_json::to_string_pretty(&snapshot::Snapshot::of(graph))
            .map(|json| format!("{json}\n"))
            .map_err(|e| e.to_string()),
        Cmd::Find { text, notes, limit } => {
            let query = text.join(" ");
            if query.trim().is_empty() {
                Err("give find something to look for".to_string())
            } else if plain {
                Ok(plain::find(graph, &query, *notes, *limit))
            } else {
                Ok(render::find(graph, &query, *notes, *limit))
            }
        }
        Cmd::Board { limit } if plain => Ok(plain::board(graph, *limit)),
        Cmd::Board { limit } => Ok(render::board(graph, *limit)),
        Cmd::List {
            area,
            status,
            stage,
            limit,
        } => filter(area.as_deref(), status.as_deref(), stage.as_deref()).map(|filter| {
            if plain {
                plain::list(graph, &filter, *limit)
            } else {
                render::list(graph, &filter, *limit)
            }
        }),
        Cmd::Why(rest) => resolve(graph, &rest.args).map(|id| {
            if plain {
                plain::why(graph, id)
            } else {
                render::why(graph, id)
            }
        }),
        Cmd::Show(rest) => resolve(graph, &rest.args).map(|id| {
            if plain {
                plain::show(graph, id)
            } else {
                render::show(graph, id)
            }
        }),
        _ => return None,
    };
    Some(shown)
}

/// Applies a whole batch as one change.
///
/// A refusal names its line, because in a batch of two hundred the message
/// alone does not say where to look. Nothing is saved unless every line was
/// accepted.
fn batch(path: &Path, graph: Graph, text: &str, plain: bool) -> Result<String, String> {
    let lines: Vec<&str> = text.lines().collect();
    let edit = branchy_app::apply_lines(graph, &lines)
        .map_err(|e| format!("line {}: {}", e.line, e.message))?;
    store::save_change(path, &edit.graph, &edit.undo).map_err(|e| e.to_string())?;
    if plain {
        return Ok(plain::made(&edit.made));
    }

    let mut out = format!(
        "Applied {} command{}.\n",
        edit.applied,
        if edit.applied == 1 { "" } else { "s" }
    );
    for (label, made) in &edit.made {
        let named = match made {
            Made::Node(id) => edit.graph.node(*id).map(|node| node.name.as_str()),
            Made::Area(id) => edit.graph.area(*id).map(|area| area.name.as_str()),
        };
        let _ = write!(out, "  {made}  {}", named.unwrap_or("?"));
        if let Some(label) = label {
            let _ = write!(out, "  ${label}");
        }
        out.push('\n');
    }
    let _ = writeln!(out, "{}", tally(&edit.graph));
    Ok(out)
}

/// The one task a reference names, explained in words when it names several.
fn resolve(graph: &Graph, args: &[String]) -> Result<branchy_core::NodeId, String> {
    branchy_core::resolve_node(graph, &args.join(" "))
        .map_err(|e| branchy_app::edit::describe_parse(graph, &e))
}

/// What to call the document in a heading: its vault's folder name.
fn vault_title(path: &Path) -> String {
    path.parent().and_then(Path::file_name).map_or_else(
        || "branchy".to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
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
    let done = graph.nodes().filter(|(_, n)| n.is_done()).count();
    let now = branchy_app::today();
    let overdue = graph
        .effective_due()
        .iter()
        .filter(|(id, date)| **date < now && graph.node(**id).is_some_and(|node| !node.is_done()))
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

/// What `list --area --status --stage` asked for.
fn filter<'a>(
    area: Option<&'a str>,
    status: Option<&str>,
    stage: Option<&str>,
) -> Result<render::Filter<'a>, String> {
    Ok(render::Filter {
        area,
        status: status.map(parse_status).transpose()?,
        stage: stage
            .map(|word| {
                branchy_core::Stage::from_name(word).ok_or_else(|| {
                    format!("unknown stage {word}, try backlog, todo, doing, review or done")
                })
            })
            .transpose()?,
    })
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
