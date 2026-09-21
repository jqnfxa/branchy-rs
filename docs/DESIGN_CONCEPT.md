# Design concept

Branchy should feel like the skill tree of a video game, not like a list with indentation. The goal is to see at a glance what is done, what is unlocked and worth doing now, and what is still out of reach.

## Two views over one graph

- **Tree view**: the full dependency graph, drawn on a pannable, zoomable canvas.
- **Queue view**: a flat list of only the available items (prerequisites done, item not done), ordered by priority. It answers "what do I do next?" across every area at once.

The queue is a projection of the graph, not a separate system. The tree is for planning and for seeing progress; the queue is what you open on an ordinary weekday.

## Node states

| State | Meaning | Look |
| --- | --- | --- |
| Locked | At least one prerequisite is not done | Dimmed, lock icon, muted connector |
| Available | All prerequisites done, item not done | Area-colored border with a slow pulsing glow, diamond icon |
| Done | Finished | Filled with a tint of the area color, check icon, bright connector |

The tier of a node is `1 + max(tier of prerequisites)`, or `0` when it has none. Status and tier are both derived on every read and never stored. In a game-flavored skin "done" can be labeled "unlocked".

## Layouts

Three layouts render the same graph. Tier is the ordering in all three, but each projects it differently, so **layout is purely a rendering concern and the core crate stays layout-agnostic**. Node positions are computed, never stored.

| Layout | Projection | Use |
| --- | --- | --- |
| **Radial** | You at the center, one angular sector per direction, one ring per tier | Default. The whole life in one frame |
| **Layered** | Tiers left to right, one horizontal band per direction | Opening a single direction, and any direction that has outgrown the disc |
| **Constellation** | Radial with seeded jitter | The most game-like look; the least precise to navigate |

The user picks the default in settings and switches live with `1`, `2` and `3`. Clicking a direction's name isolates it and switches to Layered; clicking it again returns to the default.

In Radial, a direction is given only the rings it actually occupies, not one ring per absolute tier. A direction whose lowest node is at tier 3 would otherwise sit alone far out and force the whole disc to zoom out.

## Directions

A direction is a user-defined group with its own accent color (work project, hard skills, social skills, health, anything else). Prerequisites may cross directions, and **cross-direction connectors are drawn**, dashed and colored by their *source* direction. This is the point of the app: a career task visibly pulling from mathematics is what makes it a skill tree rather than four separate lists. The detail panel additionally names the source direction on each cross-direction prerequisite.

## Palette

Dark is the product default, in the manner of game tech trees, with a light skin available in settings. The theme is an application setting rather than something inherited from the OS, because it is part of the skin.

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| bg | `#10131c` | `#eef0f7` | Page background |
| bg-elevated | `#171c2b` | `#ffffff` | Panels, modals |
| bg-card | `#1c2233` | `#ffffff` | Node cards, inputs |
| bg-sunken | `#0b0e16` | `#e4e7f1` | Inset controls |
| line | `#2a3050` | `#c3c9dd` | Borders |
| line-soft | `#232842` | `#d9dded` | Panel borders |
| locked | `#363c56` | `#a9b0c6` | Locked borders and connectors |
| text | `#e9ebf5` | `#161a27` | Primary text |
| text-dim | `#8991ac` | `#666e88` | Secondary text |
| accent teal | `#4fd1c5` | `#12897e` | First direction, primary action |
| accent amber | `#eab64d` | `#9a6f10` | Second direction |
| accent green | `#8fd17c` | `#3f8a2c` | Third direction |
| accent rose | `#ef8093` | `#c94a63` | Fourth direction |
| danger | `#e2666f` | `#c4404f` | Delete, unmet prerequisite |

Accents are darkened in the light skin so they keep contrast on a white ground. Every color is a token; nothing is a literal.

## Typography

- Display: **Exo 2**, for titles and node names. Angular and technical.
- Body: **Manrope**, for descriptions and controls.
- Numbers: **JetBrains Mono** with tabular figures, for counters and progress.

All three cover Latin, Latin Extended, Cyrillic and Greek. **Chakra Petch was the original display face and had to be replaced**: it covers only Latin, Thai and Vietnamese, so every title in a Cyrillic locale fell back silently to a system font. Any future face has to be checked for the scripts the UI is translated into. CJK is not covered by any of them, so a Noto Sans face for the script is appended to each stack and loaded on demand; because it sits after the Latin faces it only ever resolves CJK codepoints.

## Localization

The interface is translatable; **the user's own data is not**. Task names, notes and direction names stay in whatever language they were typed in, even when the UI is switched. Translation therefore lives entirely in the UI layer and `branchy-core` never sees a locale.

Three constraints follow:

- **Plural forms.** Russian and Polish need `one` / `few` / `many`; Japanese needs no plural at all. A string like "3 prerequisites left" cannot be built by concatenation. The web prototype uses `Intl.PluralRules`; on the Rust side the equivalent is the `fluent` crate, which handles plural categories and gender. `rust-i18n` is simpler but does not.
- **Text expansion.** German runs roughly 30% longer than English. Nothing may be fixed-width.
- **Direction of writing.** All directional CSS is written with logical properties (`padding-inline-start`, `inset-inline-end`), so a right-to-left language needs `dir="rtl"` and little else.

Numbers and dates go through `Intl` (and, in Rust, through an equivalent) rather than being formatted by hand.

## Interaction

- Drag to pan, scroll to zoom, `F` to fit the graph to the screen.
- Click a node to open its detail: description, tier, priority, prerequisites with met and unmet marks, what it unlocks, and one action (unlock, undo, or a disabled locked button naming how many prerequisites remain).
- Selecting a node traces its dependency path — every transitive prerequisite and everything it leads to — and dims the rest.
- Search filters by substring, or by regular expression when the `.*` toggle is on. An invalid expression marks the field rather than throwing anything away.
- `/` focuses search, `Q` toggles the queue, `S` opens settings, `Esc` clears or closes, `1`–`3` switch layout.
- Clicking a direction's swatch shows or hides it; clicking its name isolates it.

## Motion

Motion is a setting (Full or Reduced) and also yields to the OS `prefers-reduced-motion`.

- Selecting a node flies the camera to it, easing, and interpolating scale geometrically rather than linearly, which is what reads as smooth.
- Available nodes carry a slow pulsing halo.
- Unlocking fires one expanding ring at the node, and any node that *became* available flashes once. This is the only celebratory moment in the app and it should stay the only one.

## Settings

The open vault first, with "Close vault" and "Open the last vault on start". Then language, default layout, theme, dock side and motion. Settings are per-device preferences stored locally; they are not part of the graph and must never enter the synced document.

## Vaults

A vault is a folder holding one graph, the way an Obsidian vault holds notes, so separate jobs get separate graphs. Its name is the folder's name and nothing else.

With no vault open, the window shows the vault screen instead of the tree:

- **Recent vaults**, at most five, newest first. Each has a button that takes it off the list and never touches the folder. A vault whose folder has moved or gone stays listed, greyed and marked, until it is removed.
- **Open the last vault on start**, off by default, so the app offers the list unless told otherwise.
- **Create a new vault**: a name and a location picked with the system's folder dialog. The folder it will become is spelled out before anything is created.
- **Open a folder as a vault**: any folder, and one without a graph starts empty.
- The language, as a row of buttons, since nothing else on the screen reaches settings.

Inside a vault, its name sits in the top bar next to the brand and switches vaults when clicked.

The list is per device and shared with the terminal: `branchy` works in the vault the window opened last. Nothing in the interface deletes a vault. The undo stack lives inside the folder, so deleting it is the one action that could never be taken back, and it stays the file manager's job.

On Android, picking arbitrary folders is restricted, so creating a vault has to work with a default location and no dialog.

## Command layer

Every mutation is a command, and text is one of the ways to produce one. The grammar is shared:

```
add <name> [in <direction>] [after a, b] [before c] [pri n] [note "..."] [due YYYY-MM-DD]
done <task>          undone <task>         due <task> <YYYY-MM-DD | none>
link <a> after <b>   link <a> before <b>   unlink <a> after <b>
rm <task>            pri <task> <n>        rename <task> <name>
note <task> <text>   move <task> in <direction>
area <name> [colour] rename-area <d> <name> recolor-area <d> <colour>   rmarea <d>
```

A backslash escapes the next character, so a name may hold a quotation mark. Front ends build lines with `branchy_core::quote` rather than inventing their own quoting.

`after` and `needs` are the same word for "blocked by"; `before` and `blocks` express the same edge from the other end. Tasks are referred to by id or by any unambiguous part of their name, and an ambiguous reference is an error that lists the candidates rather than guessing.

There are two front ends over one parser:

- **In-app**, a command line opened with `:` or `Ctrl`+`K`. It previews what the command will do before it runs, suggests task names as you type, and refuses an illegal command while naming the reason.
- **A `branchy` binary**, so the same wiring can be scripted, kept under version control, or driven by an agent. It also means the core crate is usable on its own, before any GUI exists.

Pointing and clicking stays the primary way to work. Typing is what makes bulk wiring bearable: laying out twenty prerequisites is a minute of typing and an afternoon of dragging. Seeing the result drawn is what makes the structure understandable, so the two are complementary rather than alternatives.

Commands are also the unit of synchronization and of undo: each one maps to an Automerge operation, and each one has an inverse.

## Refusing cycles

The graph is a DAG and has to stay one. A cycle is not merely untidy: it makes both derived values undefined. Nothing inside a cycle can ever become available, because each member waits on another, and `1 + max(tier of prerequisites)` does not terminate.

Adding "A needs B" closes a loop exactly when B already reaches A through its own prerequisites, which is one search over the graph. The refusal must show the loop it found, as a chain of names, not merely say no.

A cycle usually means the nodes are too coarse. "Mathematics needs programming, programming needs mathematics" is not a real dependency loop; it is two different granularities wearing one name. Splitting it into school algebra and linear algebra dissolves it. The error message should suggest that.

**Rejecting at insert time is not sufficient once devices sync.** Two devices, each offline, can add one edge apiece that is legal on its own and cyclic together, and Automerge will merge both without complaint, because it knows nothing about this invariant. The merged document can therefore hold a cycle that neither device created. The core must be built for that:

- tier and status computation must terminate on cyclic input rather than recursing forever;
- loading a document must look for cycles;
- a found cycle has to be surfaced to the user, who breaks one edge to resolve it.

This makes acyclicity a repairable condition rather than a guaranteed one, and it is the reason the graph code cannot assume it.

## Layout notes

- A dock holds the directions with a progress bar and an `unlocked/total` counter each. It is movable to either edge, per the raw notes in `concept.md`.
- The hub at the center of the radial layout carries the overall completion ring.
- At phone width the dock and the detail panel become overlays; the canvas keeps the full width.

## Reference prototypes

- [prototype/skill-tree.html](prototype/skill-tree.html) — the current one. Standalone, opens in a browser, no build step. Covers all three layouts, cross-direction connectors, search, the queue, settings, five languages, both themes and the motion described above. Sample data is a stand-in for real tasks.
- [prototype/perk-tree-artifact.html](prototype/perk-tree-artifact.html) — the earlier swimlane study. Superseded, kept for its per-area panel treatment. Its persistence calls do nothing outside the sandbox it was written for.

Both are styling and rendering references only. Neither is the shape of the real frontend.

## Open questions

- Frontend technology: plain HTML/CSS/JS, or a Rust-to-wasm framework such as Leptos. The prototype is plain JS and is already large enough that this needs deciding before phase 2.
- How time enters the model: due dates, recurring items, partial progress. `concept.md` asks for calendar planning, and none of it exists in the model yet.
- Priority: a numeric field, or drag-to-reorder within the queue.
- Authoring by pointing: the command line covers creating and wiring, but there is still no click-driven add dialog or prerequisite picker, and no way to create, rename or recolor a direction.
- Whether the tree stays SVG. It is comfortable to a few hundred nodes; a graph grown over years may need viewport culling or a Canvas renderer, and a level-of-detail rule for what to draw when zoomed out.
