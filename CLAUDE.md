# Branchy (branchy-rs)

Dependency-aware, tree-alike TODO list. Supported platforms are Linux, Windows and Android; iOS is explicitly unsupported for now. Repo: github.com/jqnfxa/branchy-rs, default branch `main`.

This file is public. It describes the project, not whoever is working on it. Anything about a particular person — their background, what they want explained, how they like being worked with — belongs in `.claude/context.local.md`, which is gitignored. **If that file exists, read it too**, and never copy its contents here.

## Working agreement

- Explain what the code does and why. Changes get reviewed, so a commit that cannot be explained is not finished.
- Commits: follow `CONTRIBUTING.md` exactly. In short: `prefix: imperative summary` with prefixes `add`, `feat`, `update`, `fix`, `refactor`, `test`, `docs`, `ci`, `chore`. One logical change per commit. **Commit completed work as you finish it, without being asked**: a feature, a fix, a meaningful update or a documentation change is finished when the three checks pass, so commit it then rather than leaving it in the working tree waiting for permission. Pushing is different and still needs an explicit ask in the current conversation. Stage files by name, never `git add -A`, and never bypass hooks.

## Concept

One dependency graph for everything (work features that block each other, hard skills, social skills, health, anything long term).

- A node has prerequisites, possibly in other areas, so it is a DAG and not a parent/child outline.
- Status is derived, never stored: `Locked` (a prerequisite is not done), `Available` (all prerequisites done), `Done`, and `Cyclic` (on or downstream of a dependency cycle, so it can never become available).
- Tier is `1 + max(prerequisite tiers)`, `0` when there are none. Computed by peeling settled nodes (Kahn), never by recursion, so it terminates on a cyclic graph too.
- Tree view shows the graph. Queue view lists only `Available` nodes, deadline first and then priority, so the queue is a projection of the graph and not a separate system.
- A deadline on one node is inherited by everything it depends on, earliest wins (`effective_due`). A date never changes a status, only urgency. With no dates anywhere the queue is pure priority order.
- Adding a prerequisite must reject cycles, and the refusal carries the loop it found. Deleting a node must strip it from dependents' prerequisite lists.
- Acyclicity is repairable, not guaranteed. Two devices offline can each add an edge that is legal alone and cyclic together, and a CRDT merges both without complaint. So every derived computation must terminate on a cyclic graph, and `find_cycles` reports what has to be broken.
- Every mutation is a `Command`. One grammar serves the in-app command line, the `branchy` binary and Tauri's IPC — including the editing forms, which build command lines rather than calling a second API. Commands are also the unit of undo (each has an inverse) and the unit that maps onto Automerge operations.
- A form's worth of commands goes through `execute_all`, which applies them together and rolls back if any is refused. Half-applied edits are worse than refused ones.
- The frontend may compute what to *draw* (positions, which checkbox to grey out) but never what is *true*. Status, tier, the queue and cycle detection come from `branchy-core` in the snapshot, and the graph refuses anything illegal regardless of what the interface allowed.
- Visual direction: `docs/DESIGN_CONCEPT.md`.
- Raw UI ideas live in `concept.md` at the repo root. It is an unfinished personal draft belonging to the maintainer, so do not rewrite it. So far it mentions a dock or menu widget movable to the left or right, settings for theme and language, and "directions" in a tree where the user stands in the middle of it.

Differentiator versus existing apps: cross-cutting dependency gating plus one graph spanning work and life, with a priority queue over the available frontier. Similar apps found on 2026-09-19 mostly do hierarchical decomposition: TreeDo 4.0 (App Store), martinbonnin/treedo, Branchify, Gitto.

## Architecture (decided)

- Cargo workspace. `crates/branchy-core` is a pure Rust library with no UI or platform dependencies. The Tauri 2 shell goes in `src-tauri/` in phase 2.
- **Vaults**, as in Obsidian: a vault is a folder holding one `graph.json`, named by the folder itself, plus a `.branchy/` folder of device-local state (the undo stack). Separate jobs get separate vaults. The list of recent vaults (at most 5) is per device, in `vaults.json` in the configuration directory, never inside a vault, and it lives in `branchy-app::vault` rather than the frontend so the terminal reads the same list: `branchy` works in the vault the window opened last. Removing a vault only takes it off the list; nothing in the app deletes a folder, because the undo stack lives inside it.
- `branchy-core` holds the graph in ordinary Rust collections and has no persistence of its own. Automerge is a layer behind that boundary, added in phase 3, not the in-memory model. Persisted data lives in an Automerge (CRDT) document. Sync is file based: the document is a binary file in a vault folder that Syncthing keeps in sync between devices, leaving `.branchy/` out, and the `notify` crate reloads and merges external changes. No server. Syncthing was preferred over Dropbox or iCloud because it is open source and works on Linux and Android. A self-hosted axum server is the fallback if this proves weak.
- Frontend is a web frontend inside Tauri, living in `ui/`. It is plain HTML, CSS and JavaScript with no build step.
- The interface never recomputes anything about the graph. `branchy-app`'s `snapshot` module produces one value carrying nodes, areas, statuses, tiers, deadlines, the queue and any cycles; the Tauri shell returns it from its `snapshot` command and `branchy snapshot` prints it. That is what makes the frontend developable and the app scriptable without a window.
- The clock lives in `branchy-app::today`, never in `branchy-core`. A graph engine that reads the clock stops being a pure function of its input.

## Platforms

- **Linux and Windows** — desktop, Tauri 2. Both first class. CI already runs fmt, clippy and tests on Ubuntu and Windows.
- **Android** — Tauri 2's Android target, same Rust core and same web frontend. A committed target, not a maybe. Not started, and deliberately so: the app is desktop-only for now.

  What was settled on 2026-09-27, before any code moved:

  - **The build is the easy part; storage is the problem.** A vault is a folder and the recent list is absolute paths. Android has no user-visible filesystem of that shape.
  - **Distribution decides storage.** The eventual target is Google Play, and Play restricts `MANAGE_EXTERNAL_STORAGE` to file managers and backup tools, so a direct path under `/sdcard` is not available. That forces the Storage Access Framework: the user picks a folder and the app gets a `content://` URI and a `ContentResolver`, never a path.
  - **SAF breaks the atomic save.** There is no reliable rename-over-an-existing-document, so the write-a-sibling-and-rename invariant has no direct equivalent and needs a design of its own. This is the single hardest part and it should not be discovered late.
  - `directories` has no sensible Android answer and would fail with `NoHome`. Solved ahead of time by the `Dirs` injection above, which is desktop-only work and already shipped.
  - Testing will be on an emulator the maintainer sets up; there is no device here, so nothing about Android may be called supported on the strength of it compiling.

  Order when it starts: a storage abstraction behind `load` and `save`, then the toolchain, then the build.
- **iOS** — unsupported. Building and signing need macOS hardware that is not available here. Nothing in the design may make it impossible to add later, so do not paint iOS into a corner.

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

**Phases 1 and 2 are done, and vaults (0.2.0) on top of them.** First published on crates.io as v0.1.0 on 2026-09-21; 0.1.1 and 0.1.2 followed the same day with fixes, and 0.2.0 added vaults. Each `v*` tag also publishes the desktop shell on GitHub Releases (`.github/workflows/release.yml`): a `.deb` for the Debian family, an AppImage for every other Linux, and an `.msi` for Windows, built on three runners and published by one final job so a half-finished release never appears. Both Linux artifacts are built on Ubuntu 22.04 so they run on anything newer. All of it is x86_64; there is no arm64 build and no macOS build.

| Crate | Where | What |
| --- | --- | --- |
| `branchy-core` | crates.io | The graph. Zero dependencies. |
| `branchy-app` | crates.io | Persistence, vaults and their recent list, the snapshot view model, the clock. Internal glue; published only so `branchy-cli` could be. |
| `branchy-cli` | crates.io | The `branchy` binary. `cargo install branchy-cli`. |
| `branchy-desktop` | `src-tauri/`, not published | The window. Its own workspace and CI job. |
| — | `ui/` | The frontend. Plain HTML, CSS and JavaScript, no build step. |

Verified on Windows 11 on 2026-09-27: the `.msi` from the v0.2.4 release installs and the window runs. The terminal is checked on Windows semantics from Linux under Wine, see Tooling. Verified end to end on 2026-09-20 by driving the real window, and on 2026-09-21 by installing `branchy-cli` from crates.io into a clean location and running it. Vaults were driven in the window on 2026-09-21 on a nested X server: the vault screen, opening, creating through the folder picker, opening a folder, closing, forgetting, reopening the last vault on start, and switching language.

**A change to a published crate reaches nobody until its version is bumped and it is published again.** crates.io refuses a version it already holds, and versions can be yanked but never deleted. Bump deliberately, and publish in dependency order: core, app, cli.

No automated tests cover the frontend yet. It is checked by opening `ui/index.html` in a browser, or by driving the shell. Bugs found that way and not by any test: a shortcut key leaking into the field its own dialog had just focused, `Enter` saving from only one of the form's inputs, a language `<select>` changed by a scroll wheel passing over it, an empty graph leaving the camera off-centre with the hub behind the empty state's text, and **clicking a node never selecting it in the Tauri window** (the canvas took pointer capture on every press, and WebKit then sends the click to the capturing element). The last one shipped in every release up to 0.2.0.

Scale was tested on 2026-09-21 by importing a real project's planning docs, 817 tasks in 10 directions and mostly flat. Two things broke and were fixed in the layout rather than the data: a tier with more nodes than its ring holds now wraps onto further rings (a tall layered column into several columns), and labels are placed greedily by importance so none overlaps another, in a layer above every node. It was legible at that size but not fast; what that cost and what fixed it is under "Rendering at scale" below.

Invariants worth not breaking:

- Ids are never reused, including after an undone removal. A withdrawn id may already have been seen by another device.
- Every derived computation terminates on a cyclic graph.
- The on-disk format lives in `branchy-app/src/store.rs`, written by hand and versioned, never derived from the core's internal layout. New fields are optional with a default, so older documents keep loading. Phase 3 swaps its body for Automerge behind `load` and `save`.
- Saves are written to a sibling file and renamed over the target. A half-written document is exactly what a sync tool would propagate everywhere. The target is resolved through symlinks first: renaming over a link replaces the link, and the link and its file then silently fork.
- The window keeps no copy of the graph. Every command reads the file, applies, and saves, exactly as the terminal does, so neither front end can save over a change the other made. A copy held since startup did exactly that. The window also rereads the file when it regains focus, so a change typed in the terminal shows up on switching back.
- Which document a command works on, in every front end: `--file`, then `--vault`, then `BRANCHY_FILE`, then the vault opened last. `BRANCHY_FILE` pins a front end to one file and leaves the vault list alone; the window opens that file directly instead of the vault screen.
- Anything that holds the vault list for longer than one command (the window) rereads it before changing it, for the same reason the window rereads the graph.
- A vault whose folder has gone is reported, never recreated: a save would otherwise make the folder again, silently.
- **`branchy-app` does not discover its own directories.** `Dirs::user()` derives them from the platform's conventions, and anything whose host tells it instead builds a `Dirs` and passes it in. The forms that take one — `Vaults::for_dirs`, `store::default_path_in` — read no environment variable and are pure functions of it, which is what lets them be tested without touching the environment the rest of the process shares. Discovery and the `BRANCHY_*` overrides live only at the edge, in `Dirs::user`, `Vaults::user` and `store::default_path`. The reason is Android, which has no home directory to derive anything from, while the crate still has to work with no Tauri at all because the CLI links it.
- **The desktop shell keeps using `Dirs::user()` on purpose.** Tauri's own path API would answer `~/.config/dev.jqnfxa.branchy` where `directories` answers `~/.config/branchy`, so switching would orphan an existing vault list. The injection exists for a platform that has no other answer, not to replace the one that works.
- `BRANCHY_VAULTS` names the list file and turns off adopting a pre-vault graph from the old data directory. **Every test that reaches the vault list sets it and clears `BRANCHY_FILE`**, or it reads the developer's real list or follows their environment into their real graph.

`concept.md` is an untracked personal draft.

## Open decisions

- ~~Frontend technology.~~ Settled on 2026-09-20: **plain HTML, CSS and JavaScript in `ui/`, no framework and no build step.** Tauri serves the directory as it is, so there is no npm install or bundler in CI and nothing extra to make work on Android. The main view is hand-drawn SVG, where a framework's diffing buys very little. Leptos would add a wasm toolchain and a second compile target to a project whose point is shipping.
- Whether the core crate should keep the name `branchy-core` or be named `branchy-rs`. `branchy-core` was chosen so the repo name can stay the umbrella.
- ~~Node id type, area representation, priority representation.~~ Settled: `NodeId`/`AreaId` are newtypes over `u64` handed out by the graph, areas are one field on a node in a single graph, priority is a `u8` where higher sorts earlier.
- ~~Tauri prerequisites on this Linux machine.~~ Installed on 2026-09-20; the shell builds and runs. `libxdo` turned out not to be needed after all.
- Whether "done" is called "unlocked" in the UI skin.
- ~~Due dates.~~ Done on 2026-09-20: deadlines that propagate backwards, a calendar view, urgency in the queue.
- **Still open: partial progress and recurrence.** "150 problems, 40 done" and "gym three times a week" do not fit a done flag. Recurrence in particular does not fit a DAG node at all and may want to be a different kind of thing. Either changes the data model, so decide before phase 3 rather than after.
- ~~Rendering is slow at scale.~~ Measured and largely fixed on 2026-09-27. The window ran at about 5 fps on the 817-task vault above. Measuring it first was worth it, because two of the three suspects recorded here were wrong.

  Measured on that vault, before and after, on a nested X server:

  | | idle | pan | zoom | select |
  | --- | --- | --- | --- | --- |
  | before | 24.9 fps | 16.4 fps | 16.9 fps | 121 ms |
  | after | 76.8 fps | 55.0 fps | 65.8 fps | 29 ms |

  What actually cost, in order:

  - **A forced layout in the click handler.** `flyTo` read `stage.clientWidth` straight after `select` had toggled classes on 1 634 elements, so the read had to flush style and layout for the whole canvas: 50 ms of the 121. The stage's box is now cached behind a `ResizeObserver`, whose callback runs when layout has already settled. The wheel handler had the same bug with `getBoundingClientRect`. **Never read a layout property from an event handler on this canvas.**
  - **`opacity:0` is not free.** A hidden label is still laid out and composited. Taking hidden labels and haloes out of the render tree with `display:none`, and not building a halo for a task that can never show one, roughly tripled pan and zoom on its own. This was the single biggest win and it was not on the suspect list.
  - **The halo pulse**, as suspected: 488 infinite animations tripled idle cost. Gated off above `DENSE`.
  - **Level of detail:** below 36% zoom the status glyph is a couple of pixels of stroked path, so it is dropped there.

  Wrong suspects, both now disproven by measurement: the per-node `<title>` costs nothing (removing 817 of them moved pan by 0.3 fps, because it is never rendered), and the `.25s` opacity transition on a thousand fading elements was worth about 5 ms, not the 121.

  `DENSE = 150` in `ui/app.js` puts `body.dense` on the document and every concession hangs off that class, so a graph below it renders exactly as before. Verified by eye at both sizes.

  Still on the table if it is not enough on real hardware: cull nodes and links outside the viewport (no help at fit zoom, where the whole graph is on screen), and draw the bulk on a Canvas with SVG only for what is interactive. The numbers above come from Xephyr with software rendering, where the baseline measured 16 fps against the 5 fps reported on a real display, so treat the ratios as the result and not the absolute figures.

- **Sync is not built.** Two devices each have their own independent document today. Putting the JSON in Syncthing before phase 3 is unsafe: concurrent edits produce `*.sync-conflict-*` files and nothing merges them.

## Running the desktop shell

```sh
cd src-tauri && cargo build
BRANCHY_FILE=/path/to/graph.json ./target/debug/branchy-desktop
```

`BRANCHY_FILE` opens one document directly, which is what makes the shell drivable against a fixture. Without it the window starts on the vault screen; set `BRANCHY_VAULTS` to a scratch file so that trying vaults out never touches the real list. `branchy snapshot > ui/dev-fixture.js` (wrapped in the assignment that file already has) refreshes the sample data the frontend falls back to when opened as a plain file.

Two things that will waste an hour if rediscovered:

- **Inside a snap-confined terminal** (the VS Code snap, for instance), the loader picks up `/snap/core20/.../libpthread.so.0` and the binary dies with `undefined symbol: __libc_pthread_init`. Launch it with a clean environment: `env -i HOME=$HOME DISPLAY=$DISPLAY XAUTHORITY=$HOME/.Xauthority PATH=/usr/bin:/bin LD_LIBRARY_PATH=/lib/x86_64-linux-gnu:/usr/lib/x86_64-linux-gnu ./target/debug/branchy-desktop`. From an ordinary terminal none of this is needed.
- **A blank window** on some Linux setups is WebKitGTK's renderer. `WEBKIT_DISABLE_DMABUF_RENDERER=1` and `WEBKIT_DISABLE_COMPOSITING_MODE=1` fix it.
- **Checking Windows behaviour without Windows.** `sudo apt install mingw-w64 lld`, `rustup target add x86_64-pc-windows-gnu`, then run the suites under Wine:

  ```sh
  export WINEPREFIX=/tmp/scratch-prefix WINEDLLOVERRIDES="mscoree,mshtml=" WINEDEBUG=-all
  CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine \
    cargo test --workspace --target x86_64-pc-windows-gnu
  ```

  Point `WINEPREFIX` at a scratch folder so the developer's own prefix is left alone. On 2026-09-27 this ran 148 tests green. The three the Linux run has and this one does not are the `#[cfg(unix)]` symlink tests, which cannot exist on Windows. Driving the binary by hand there also confirmed what the suite cannot, because every test sets `BRANCHY_VAULTS`: `Dirs::user()` resolves to `%APPDATA%\jqnfxa\branchy\config\vaults.json`, paths print as `C:\vaults\Work` with no `\\?\` prefix leaking, and the undo stack lands in `.branchy\undo\` inside the vault.

  What this cannot check: the `.msi`, because Tauri's Linux bundler offers only `deb`, `rpm` and `appimage` — WiX is Windows-only, so there is no installer to hand to Wine. Nor the window, which needs WebView2. Only the terminal is checkable this way.
- **Measuring the frontend**: temporarily add a script to `ui/` that drives the real handlers with synthetic `PointerEvent` and `WheelEvent`, counts `requestAnimationFrame` callbacks over a fixed window, and reports by painting the numbers into a `position:fixed` overlay, which `xwd` can then capture. `document.title` is a dead end: Tauri does not propagate it to the window title, so `xdotool getwindowname` never sees it. Nothing in `ui/` is exposed on `window`, which is what makes driving real events the honest way to measure anyway.
- **Driving the window from a script** (xdotool plus screenshots): do it on a nested X server, `Xephyr :5 -screen 1280x800 -ac` and `DISPLAY=:5`, not on the display someone is working at. There, the scripted clicks move their real pointer and their scroll wheel reaches the window, and screenshots capture their notifications. Also run the app with a scratch `HOME`: the webview keeps its storage (the interface preferences) under `HOME`, shared with any installed copy of the app, and the folder picker opens on the real Documents folder otherwise. Xephyr with no window manager delivers clicks but no keystrokes, so typed input has to be checked another way.

## Tooling

- Toolchain pinned by `rust-toolchain.toml` (stable, with rustfmt and clippy). Edition 2024, `rust-version = "1.85"`.
- Workspace lints in the root `Cargo.toml`: `unsafe_code = "forbid"`, clippy `all` and `pedantic` at warn. Member crates opt in with `[lints] workspace = true`. When `src-tauri` is added, opt in too, and only downgrade for that crate if Tauri's generated code trips a lint.
- **`upload-artifact` given several path patterns keeps the directory structure they share.** Two patterns under `bundle/deb/` and `bundle/appimage/` arrive as `deb/...` and `appimage/...`, a level below a single-pattern upload. In 0.2.4 that put both Linux artifacts out of reach of the publishing step's `artifacts/*/*`, and the release went out with only the `.msi` while all four jobs reported success. `fail_on_unmatched_files` does not catch it: it only proves the pattern matched *something*. The fix is to gather the bundles into one flat folder before uploading, and to check the exact pattern that will be published rather than a recursive `find`.
- **A workflow runs as it exists at the commit the tag points at.** Changing `release.yml` does nothing for a tag that already exists; the change reaches a release only from the next tag made after it, or by moving a tag that has not been pushed yet.
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
- The icon's source is `src-tauri/icons/icon.svg`, and `cargo tauri icon icons/icon.svg` regenerates the set. Only the five files `tauri.conf.json` names are kept; the generator also emits Android and iOS sets that nothing here uses. Before 2026-09-27 there was no source at all, so the icons could not be adjusted, only replaced.
- The mark is a tree: a root node, a branch left and lower, a branch right and higher, and an open ring at the top. That is the tree view's own vocabulary, where a filled node is done and a ring is available. Drafts made of three symmetric connected circles were tried first and rejected: that is the Material Design share glyph, so it collides with a symbol everyone already knows, and at 32 px a symmetric three-node mark reads as a person. Asymmetry is what stops both readings. Check any new mark at 16 px, not at 512: that is where a design stops working, and several did.
- License: `MIT OR Apache-2.0`, copyright holder `jqnfxa`. Default choice, can be changed while there are no outside contributors.
