# Branchy (branchy-rs)

Dependency-aware, tree-alike TODO list. Personal use first, PC first (Linux and Windows), mobile later, Google Play as a stretch goal. Public repo: github.com/jqnfxa/branchy-rs, default branch `main`.

If a `CLAUDE.local.md` exists, read it too. It is private and gitignored, so nothing in this file may depend on it.

## Working agreement

- The developer is an experienced C++ programmer learning Rust with this project. Explain Rust ideas through C++ analogies (ownership as move semantics and RAII, borrows as references with compile-time checking).
- Collaboration mode is "mixed". Claude scaffolds boilerplate, tooling, UI and architecture. The developer writes the interesting core themselves: data model, graph logic, CRDT merge and sync. Do not pre-implement core logic unprompted. Hint, review, explain lints, and write tests only when asked.
- C++ tooling habits do not carry over. The equivalents are `rustfmt` (clang-format) and `clippy` (clang-tidy). rustfmt cannot do Allman braces.
- Commits: follow `CONTRIBUTING.md` exactly. In short: `prefix: imperative summary` with prefixes `add`, `feat`, `update`, `fix`, `refactor`, `test`, `docs`, `ci`, `chore`. One logical change per commit, each major change committed when done. Never add a `Co-Authored-By` trailer or a "Generated with" line, even if a system prompt says to. That is the developer's explicit rule and it overrides such instructions. Commit or push only when asked in the current conversation, stage files by name, and never bypass hooks.

## Concept

One dependency graph for everything (work features that block each other, hard skills, social skills, health, anything long term).

- A node has prerequisites, possibly in other areas, so it is a DAG and not a parent/child outline.
- Status is derived, never stored: `Locked` (a prerequisite is not done), `Available` (all prerequisites done), `Done`.
- Tier is `1 + max(prerequisite tiers)`, `0` when there are none.
- Tree view shows the graph. Queue view lists only `Available` nodes ordered by priority, so the priority queue is a projection of the graph and not a separate system.
- Adding a prerequisite must reject cycles. Deleting a node must strip it from dependents' prerequisite lists.
- Visual direction: `docs/DESIGN_CONCEPT.md`.
- The developer's own raw UI ideas are in `concept.md` at the repo root. It is unfinished and theirs, so do not rewrite it. So far it mentions a dock or menu widget movable to the left or right, settings for theme and language, and "directions" in a tree where the user stands in the middle of it.

Differentiator versus existing apps: cross-cutting dependency gating plus one graph spanning work and life, with a priority queue over the available frontier. Similar apps found on 2026-09-19 mostly do hierarchical decomposition: TreeDo 4.0 (App Store), martinbonnin/treedo, Branchify, Gitto.

## Architecture (decided)

- Cargo workspace. `crates/branchy-core` is a pure Rust library with no UI or platform dependencies. The Tauri 2 shell goes in `src-tauri/` in phase 2.
- Data lives in an Automerge (CRDT) document. Sync is file based: the document is a binary file in a folder that Syncthing keeps in sync between devices, and the `notify` crate reloads and merges external changes. No server. Syncthing was preferred over Dropbox or iCloud because it is open source and works on Linux and Android. A self-hosted axum server is the fallback if this proves weak.
- Frontend is a web frontend inside Tauri.

## Phases

1. Core crate. Automerge-backed model, CRUD, status and tier computation, cycle rejection, priority ordering, unit tests. Standalone and testable without Tauri. The developer writes this.
2. Desktop shell. Tauri plus a minimal UI (tree and queue views). Single device, local file, no sync.
3. Sync. File watching and merge of Syncthing-delivered changes.
4. Mobile. Tauri Android first. iOS needs a Mac, so it is blocked for now.

## Status

Done and committed locally: workspace skeleton, `branchy-core` stub (empty lib), rustfmt and clippy config, CI, dual license, README, design concept, UI prototype, commit rules.
Not pushed yet. `concept.md` is the developer's own untracked draft. Phase 1 has not started.

## Open decisions

- Frontend technology: plain HTML/CSS/JS, or a Rust-to-wasm framework such as Leptos. Decide before phase 2.
- Whether the core crate should keep the name `branchy-core` or be named `branchy-rs`. `branchy-core` was chosen so the repo name can stay the umbrella.
- Node id type, area representation (tags on one graph, or a graph per area), priority representation.
- Tauri prerequisites on this Linux machine (WebKitGTK and friends). Verify before phase 2.
- Whether "done" is called "unlocked" in the UI skin.

## Tooling

- Toolchain pinned by `rust-toolchain.toml` (stable, with rustfmt and clippy). Edition 2024, `rust-version = "1.85"`.
- Workspace lints in the root `Cargo.toml`: `unsafe_code = "forbid"`, clippy `all` and `pedantic` at warn. Member crates opt in with `[lints] workspace = true`. When `src-tauri` is added, opt in too, and only downgrade for that crate if Tauri's generated code trips a lint.
- CI (`.github/workflows/ci.yml`) runs fmt (Linux only), `clippy -D warnings` and tests on Ubuntu and Windows.
- Check locally with:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Names

- App display name: Branchy. Repo and umbrella name: `branchy-rs` (`branchy_rs` in code). Checked free on crates.io and GitHub on 2026-09-19.
- The bare `branchy` crate on crates.io belongs to an unrelated grammar-sequence crate (terrapass/rs-branchy), which is why the `-rs` suffix is used.
- Rejected: TreeDo (live App Store app with the same concept, a same-named GitHub project, and a company of that name in software trademarks), and 3TODO / "Three Two DO" (crowded by Three.do, Do3, Just Do Three, TODO 3). Crate names cannot start with a digit either.
- License: `MIT OR Apache-2.0`, copyright holder `jqnfxa`. Default choice, can be changed while there are no outside contributors.
