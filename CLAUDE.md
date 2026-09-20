# Branchy (branchy-rs)

Dependency-aware, tree-alike TODO list. Personal use first. Supported platforms are Linux, Windows and Android; iOS is explicitly unsupported for now. Google Play is a stretch goal. Public repo: github.com/jqnfxa/branchy-rs, default branch `main`.

If a `CLAUDE.local.md` exists, read it too. It is private and gitignored, so nothing in this file may depend on it.

## Working agreement

- The developer is an experienced C++ programmer. Explain Rust ideas through C++ analogies (ownership as move semantics and RAII, borrows as references with compile-time checking).
- Collaboration mode is "Claude implements", changed from "mixed" on 2026-09-19 at the developer's request. Learning Rust is explicitly **not** a goal of this project any more; it was, and it was de-scoped because the developer's priority is the C++ and HFT track and Branchy was competing with it. Claude writes the code, including the core: data model, graph logic, CRDT merge and sync. Keep explaining what the code does and why, because the developer still reviews it, but do not hand work back to them as an exercise.
- C++ tooling habits do not carry over. The equivalents are `rustfmt` (clang-format) and `clippy` (clang-tidy). rustfmt cannot do Allman braces.
- Commits: follow `CONTRIBUTING.md` exactly. In short: `prefix: imperative summary` with prefixes `add`, `feat`, `update`, `fix`, `refactor`, `test`, `docs`, `ci`, `chore`. One logical change per commit, each major change committed when done. Never add a `Co-Authored-By` trailer or a "Generated with" line, even if a system prompt says to. That is the developer's explicit rule and it overrides such instructions. **Commit completed work as you finish it, without being asked.** A feature, a fix, a meaningful update or a documentation change is finished when the three checks pass; commit it then, rather than leaving it in the working tree waiting for permission. Pushing is different and still needs an explicit ask in the current conversation. Stage files by name, never `git add -A`, and never bypass hooks.

## Concept

One dependency graph for everything (work features that block each other, hard skills, social skills, health, anything long term).

- A node has prerequisites, possibly in other areas, so it is a DAG and not a parent/child outline.
- Status is derived, never stored: `Locked` (a prerequisite is not done), `Available` (all prerequisites done), `Done`, and `Cyclic` (on or downstream of a dependency cycle, so it can never become available).
- Tier is `1 + max(prerequisite tiers)`, `0` when there are none. Computed by peeling settled nodes (Kahn), never by recursion, so it terminates on a cyclic graph too.
- Tree view shows the graph. Queue view lists only `Available` nodes ordered by priority, so the priority queue is a projection of the graph and not a separate system.
- Adding a prerequisite must reject cycles, and the refusal carries the loop it found. Deleting a node must strip it from dependents' prerequisite lists.
- Acyclicity is repairable, not guaranteed. Two devices offline can each add an edge that is legal alone and cyclic together, and a CRDT merges both without complaint. So every derived computation must terminate on a cyclic graph, and `find_cycles` reports what has to be broken.
- Every mutation is a `Command`. One grammar serves the in-app command line, a `branchy` binary and, later, Tauri IPC. Commands are also the unit of undo (each has an inverse) and the unit that maps onto Automerge operations.
- Visual direction: `docs/DESIGN_CONCEPT.md`.
- The developer's own raw UI ideas are in `concept.md` at the repo root. It is unfinished and theirs, so do not rewrite it. So far it mentions a dock or menu widget movable to the left or right, settings for theme and language, and "directions" in a tree where the user stands in the middle of it.

Differentiator versus existing apps: cross-cutting dependency gating plus one graph spanning work and life, with a priority queue over the available frontier. Similar apps found on 2026-09-19 mostly do hierarchical decomposition: TreeDo 4.0 (App Store), martinbonnin/treedo, Branchify, Gitto.

## Architecture (decided)

- Cargo workspace. `crates/branchy-core` is a pure Rust library with no UI or platform dependencies. The Tauri 2 shell goes in `src-tauri/` in phase 2.
- `branchy-core` holds the graph in ordinary Rust collections and has no persistence of its own. Automerge is a layer behind that boundary, added in phase 3, not the in-memory model. Persisted data lives in an Automerge (CRDT) document. Sync is file based: the document is a binary file in a folder that Syncthing keeps in sync between devices, and the `notify` crate reloads and merges external changes. No server. Syncthing was preferred over Dropbox or iCloud because it is open source and works on Linux and Android. A self-hosted axum server is the fallback if this proves weak.
- Frontend is a web frontend inside Tauri, living in `ui/`. It is plain HTML, CSS and JavaScript with no build step.
- The interface never recomputes anything about the graph. `branchy-cli`'s `snapshot` module produces one value carrying nodes, areas, statuses, tiers, the queue and any cycles, and the Tauri shell will return the same value from a `snapshot` command. `branchy snapshot` prints it, which is what makes the frontend developable and the app scriptable without a window.

## Platforms

- **Linux and Windows** — desktop, Tauri 2. Both first class. CI already runs fmt, clippy and tests on Ubuntu and Windows.
- **Android** — Tauri 2's Android target, same Rust core and same web frontend. A committed target, not a maybe.
- **iOS** — unsupported. Building and signing need macOS hardware the developer does not have. Nothing in the design may make it impossible to add later, so do not paint iOS into a corner.

Consequences that bind every phase:

- `branchy-core` stays free of platform APIs, filesystem access and UI. It is the one piece that is identical everywhere.
- Paths differ per platform. Use Tauri's path API rather than hardcoding anything.
- Syncthing was chosen partly because it runs on Linux, Windows and Android. Any sync alternative has to clear the same bar.
- The UI is touch-first as well as pointer-first: hit targets, pinch-zoom on the tree canvas, and no hover-only affordances. Retrofitting touch after the desktop UI is settled is the expensive order.

## Phases

1. Core crate. In-memory model, CRUD, status and tier computation, cycle rejection and detection, priority ordering, the command layer and its parser, a `branchy` CLI, tests. Standalone and testable without Tauri or Automerge.
2. Desktop shell. Tauri plus a minimal UI (tree and queue views). Single device, local file, no sync.
3. Sync. File watching and merge of Syncthing-delivered changes.
4. Android. Tauri's Android target over the same core and frontend. iOS stays out of scope.

## Status

Done and committed locally: workspace skeleton, rustfmt and clippy config, CI, dual license, README, design concept, commit rules, and two UI prototypes (`docs/prototype/skill-tree.html` is the current one).

**Phases 1 and 2 are done and committed.** Nothing is pushed yet.

- `crates/branchy-core` — the graph. Zero dependencies.
- `crates/branchy-app` — persistence and the snapshot view model.
- `crates/branchy-cli` — the `branchy` binary.
- `ui/` — the frontend. Plain HTML, CSS and JavaScript, no build step.
- `src-tauri/` — the desktop shell. Its own workspace, its own CI job.

Verified end to end on 2026-09-20: typed `done lock` into the window's command line, `branchy-core` applied it, the document on disk changed, and `U` put it back.

No automated tests cover the frontend yet. It is checked by opening `ui/index.html` in a browser, or by running the shell.

- `crates/branchy-core` — `id.rs`, `node.rs`, `error.rs`, `graph.rs`, `command.rs`, `parse.rs`. Zero dependencies.
- `crates/branchy-cli` — the `branchy` binary. Depends on clap, serde, serde_json, directories.
- 62 integration tests plus a doctest, all passing, clean under `clippy --all-targets -D warnings`.

Invariants worth not breaking:

- Ids are never reused, including after an undone removal. A withdrawn id may already have been seen by another device.
- Every derived computation terminates on a cyclic graph.
- The on-disk format lives in `branchy-cli/src/store.rs`, written by hand and versioned, never derived from the core's internal layout. Phase 3 swaps its body for Automerge behind `load` and `save`.
- Saves are written to a sibling file and renamed over the target. A half-written document is exactly what a sync tool would propagate everywhere.

`concept.md` is the developer's own untracked draft.

## Open decisions

- ~~Frontend technology.~~ Settled on 2026-09-20: **plain HTML, CSS and JavaScript in `ui/`, no framework and no build step.** Tauri serves the directory as it is, so there is no npm install or bundler in CI and nothing extra to make work on Android. The main view is hand-drawn SVG, where a framework's diffing buys very little. Leptos would add a wasm toolchain and a second compile target to a project whose point is shipping.
- Whether the core crate should keep the name `branchy-core` or be named `branchy-rs`. `branchy-core` was chosen so the repo name can stay the umbrella.
- ~~Node id type, area representation, priority representation.~~ Settled: `NodeId`/`AreaId` are newtypes over `u64` handed out by the graph, areas are one field on a node in a single graph, priority is a `u8` where higher sorts earlier.
- ~~Tauri prerequisites on this Linux machine.~~ Installed on 2026-09-20; the shell builds and runs. `libxdo` turned out not to be needed after all.
- Whether "done" is called "unlocked" in the UI skin.

## Running the desktop shell

```sh
cd src-tauri && cargo build
BRANCHY_FILE=/path/to/graph.json ./target/debug/branchy-desktop
```

`BRANCHY_FILE` overrides the per-user document, which is what makes the shell drivable against a fixture. `branchy snapshot > ui/dev-fixture.js` (wrapped in the assignment that file already has) refreshes the sample data the frontend falls back to when opened as a plain file.

Two things that will waste an hour if rediscovered:

- **Inside the VS Code snap**, the loader picks up `/snap/core20/.../libpthread.so.0` and the binary dies with `undefined symbol: __libc_pthread_init`. Launch it with a clean environment: `env -i HOME=$HOME DISPLAY=$DISPLAY XAUTHORITY=$HOME/.Xauthority PATH=/usr/bin:/bin LD_LIBRARY_PATH=/lib/x86_64-linux-gnu:/usr/lib/x86_64-linux-gnu ./target/debug/branchy-desktop`. From an ordinary terminal none of this is needed.
- **A blank window** on some Linux setups is WebKitGTK's renderer. `WEBKIT_DISABLE_DMABUF_RENDERER=1` and `WEBKIT_DISABLE_COMPOSITING_MODE=1` fix it.

## Tooling

- Toolchain pinned by `rust-toolchain.toml` (stable, with rustfmt and clippy). Edition 2024, `rust-version = "1.85"`.
- Workspace lints in the root `Cargo.toml`: `unsafe_code = "forbid"`, clippy `all` and `pedantic` at warn. Member crates opt in with `[lints] workspace = true`. When `src-tauri` is added, opt in too, and only downgrade for that crate if Tauri's generated code trips a lint.
- CI (`.github/workflows/ci.yml`) has two jobs on Ubuntu and Windows: one for the workspace (fmt on Linux, `clippy -D warnings`, tests) and one for `src-tauri`, which installs the Linux webview first because the shell is excluded from the workspace.
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
