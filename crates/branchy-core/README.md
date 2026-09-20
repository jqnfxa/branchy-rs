# branchy-core

A dependency graph of tasks, with everything interesting derived rather than stored.

No dependencies. No filesystem, no UI, no clock — which is what lets the same code run on a desktop, in a terminal and on Android unchanged.

```rust
use branchy_core::{Graph, NewArea, NewNode, Status};

fn main() -> Result<(), branchy_core::Error> {
    let mut graph = Graph::new();
    let maths = graph.add_area(NewArea::new("Hard skills", "#4fd1c5"));

    let algebra = graph.add_node(NewNode::new("School algebra", maths))?;
    let calculus = graph.add_node(NewNode::new("Calculus", maths).with_priority(7))?;
    graph.add_prerequisite(calculus, algebra)?;

    assert_eq!(graph.status(calculus), Some(Status::Locked));
    graph.set_done(algebra, true)?;
    assert_eq!(graph.status(calculus), Some(Status::Available));
    assert_eq!(graph.queue(), vec![calculus]);

    // the reverse edge would close a loop, so it is refused, and the
    // refusal carries the loop it found
    assert!(graph.add_prerequisite(algebra, calculus).is_err());
    Ok(())
}
```

## What it computes

- **Status** — `Locked`, `Available`, `Done`, or `Cyclic`. Never stored, so it cannot disagree with the edges.
- **Tier** — `1 + max(tier of prerequisites)`. Computed by peeling settled nodes, not by recursion, so it terminates on any input.
- **Queue** — the available frontier, most urgent first.
- **`path_to_unlock`** — everything still standing between you and a locked task, in an order you can work through.
- **`find_cycles`** — groups of mutually blocking tasks, for the day a merge produces one.

## Deadlines reach backwards

A deadline recorded on one task is inherited by everything that task depends on, earliest wins. Dating a goal dates the whole chain leading to it, and nothing is written down twice. A date never changes a status; it changes urgency, and the queue sorts by deadline before priority.

## Cycles are refused, and survivable

`add_prerequisite` rejects any edge that would close a loop and hands back the loop. But refusing at insert time is not enough once devices sync: two devices offline can each add an edge that is legal alone and cyclic together, and a CRDT merges both without complaint. So every derived computation terminates on a cyclic graph, `Status::Cyclic` exists, and `find_cycles` reports what has to be broken.

## Commands

Every mutation is a `Command`. Applying one returns the commands that undo it, and `parse` turns a line of text into one, so a terminal, a GUI and an IPC boundary can share a single grammar:

```text
add <name> [in <direction>] [after a, b] [pri n] [due YYYY-MM-DD]
done <task>   link <a> after <b>   due <task> <date | none>
```

## License

MIT or Apache-2.0, at your option.
