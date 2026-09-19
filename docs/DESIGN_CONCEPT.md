# Design concept

Branchy should feel like the skill tree of a video game, not like a list with indentation. The goal is to see at a glance what is done, what is unlocked and worth doing now, and what is still out of reach.

## Two views over one graph

- **Tree view**: the full dependency graph. One lane per area, tiers running left to right, curved connectors from prerequisite to dependent.
- **Queue view**: a flat list of only the available items (prerequisites done, item not done), ordered by priority. It answers "what do I do next?" across every area at once.

## Node states

| State | Meaning | Look |
| --- | --- | --- |
| Locked | At least one prerequisite is not done | Dimmed, lock icon, muted connector |
| Available | All prerequisites done, item not done | Area-colored border with a slow pulsing glow, diamond icon |
| Done | Finished | Filled with a tint of the area color, check icon, bright connector |

The tier of a node is `1 + max(tier of prerequisites)`, or `0` when it has none, so a dependent always sits to the right of everything it needs. In a game-flavored skin "done" can be labeled "unlocked".

## Areas

An area is a user-defined group with its own accent color (work project, hard skills, social skills, health, anything else). Prerequisites may cross areas. A connector is only drawn inside a lane, so cross-area dependencies are listed in the detail panel instead.

## Palette

Dark canvas by design, in the manner of game tech trees. The tints lean blue-violet rather than neutral grey.

| Token | Value | Use |
| --- | --- | --- |
| bg | `#10131c` | Page background |
| bg-elevated | `#171c2b` | Area panels, modals |
| bg-card | `#1c2233` | Node cards, inputs |
| line | `#2a3050` | Borders |
| line-soft | `#232842` | Panel borders |
| locked | `#363c56` | Locked borders and connectors |
| text | `#e9ebf5` | Primary text |
| text-dim | `#8991ac` | Secondary text |
| accent teal | `#4fd1c5` | First area, primary action |
| accent rose | `#ef8093` | Second area |
| accent green | `#8fd17c` | Third area |
| accent amber | `#eab64d` | Fourth area |
| danger | `#e2666f` | Delete, unmet prerequisite |

## Typography

- Display: Chakra Petch, for titles and node names. Angular, slightly technical.
- Body: Manrope, for descriptions and controls.
- Numbers: JetBrains Mono with tabular figures, for counters and progress.

## Interaction

- Click a node to open its detail: description, prerequisites with met and unmet marks, and one action (unlock, undo, or a disabled locked button).
- Adding an item picks its area and its prerequisites from existing items. A prerequisite that would create a cycle must be refused.
- Deleting an item removes it from every dependent's prerequisite list.
- Unlocking gets a brief pop, and nothing more. Respect reduced-motion.

## Layout notes

- Each area is a full-width panel with a progress bar and an `unlocked/total` counter.
- Tiers scroll horizontally inside their panel, which keeps the layout usable on a phone later.
- A top bar shows the overall counter and the add button.

## Reference prototype

[prototype/perk-tree-artifact.html](prototype/perk-tree-artifact.html) is a single-file web prototype of this look with the tier layout, connector drawing and node states. It was written for a hosted sandbox, so its persistence calls (`window.claude.use("db")`) do nothing outside it and it falls back to one example node. Use it as a styling and rendering reference only.

## Open questions

- Light theme, or dark only.
- Areas as tags on one graph, or one graph per area.
- Drag to reorder the queue, or a numeric priority.
