# Branchy

A dependency-aware, tree-alike TODO list. Break a goal into branches, unlock the next step as its prerequisites get done, and always see what is worth doing next.

> Status: early scaffolding. Nothing usable yet.

## Idea

- Any item can depend on any other item, across branches, not only parent to child.
- An item is **locked** until its prerequisites are done, **available** once they are, and **done** when finished.
- The **tree view** shows the whole graph like a game skill tree. The **queue view** lists only what is available, ordered by priority.
- One graph for everything: work features that block each other, hard skills, health, any long-term plan.

Personal use first. PC first (Linux and Windows), mobile later.

## Planned stack

- Rust, Cargo workspace.
- `crates/branchy-core`: graph, status and priority logic on top of an Automerge document. No UI dependencies.
- Tauri desktop shell with a web frontend, then file-based sync between devices.

## Roadmap

1. Core crate.
2. Desktop shell.
3. Sync between devices.
4. Mobile (Android first).

The visual direction is in [docs/DESIGN_CONCEPT.md](docs/DESIGN_CONCEPT.md).

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the same three checks. Commit message rules are in [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
