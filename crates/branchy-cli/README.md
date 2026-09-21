# branchy-cli

A dependency-aware task tree, in the terminal.

Break a goal into tasks, wire up what blocks what, and always have an answer to "what can I actually start right now?"

```sh
cargo install branchy-cli
```

```sh
branchy vault new Plans           # a folder holding one graph, created here
branchy area "Hard skills" "#4fd1c5"
branchy add "School algebra" pri 3
branchy add "Calculus" after school pri 6
branchy add "Probability theory" after calculus pri 8
branchy due "Probability theory" 2027-03-01
branchy done school
```

```console
$ branchy queue
2 available:
  1. Calculus                                p6   Hard skills    in 5 months   unlocks 1

$ branchy why "Probability theory"
Probability theory is locked. 1 task(s) stand in the way:
  1. Calculus                                p6   Hard skills
```

## Commands

`add` · `done` · `undone` · `rm` · `pri` · `due` · `rename` · `note` · `move` · `link` · `unlink` · `area` · `rename-area` · `recolor-area` · `rmarea` · `run`

`queue` · `list` · `tree` · `why` · `show` · `calendar` · `cycles` · `snapshot` · `undo` · `where`

`vault list` · `vault new` · `vault open` · `vault forget`

`after` and `needs` both mean "blocked by"; `before` and `blocks` say the same edge from the other end. Tasks are named by any unambiguous part of their name.

## Vaults

A vault is a folder holding one graph, the way an Obsidian vault holds notes, so separate jobs get separate graphs.

- `branchy vault new <name> [--in <folder>]` creates one, in the current directory unless told otherwise, and works in it from then on.
- `branchy vault open <name|folder>` switches to a recent vault, or makes any folder one.
- `branchy vault` lists the five opened most recently, the current one marked.
- `branchy vault forget <name>` takes one off the list. The folder is left alone.
- `--vault <name|folder>` reaches another vault for a single command.

The window shares the list, so the terminal always works in the vault the window opened last.

`branchy where` prints the document's path. `BRANCHY_FILE` pins every command to one file, bypassing vaults, and `--file` overrides even that. The document is plain JSON and worth keeping in version control. The `.branchy` folder beside it holds the undo stack and can be left out.

## There is a window too

The same graph drawn as a skill tree, in the [main repository](https://github.com/jqnfxa/branchy-rs).

## License

MIT or Apache-2.0, at your option.
