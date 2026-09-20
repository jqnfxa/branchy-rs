# branchy-cli

A dependency-aware task tree, in the terminal.

Break a goal into tasks, wire up what blocks what, and always have an answer to "what can I actually start right now?"

```sh
cargo install branchy-cli
```

```sh
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

`after` and `needs` both mean "blocked by"; `before` and `blocks` say the same edge from the other end. Tasks are named by any unambiguous part of their name.

## Where the document lives

`branchy where` prints it. `BRANCHY_FILE` overrides it, and `--file` overrides that. The default comes from the per-user data directory, which is derived from `HOME` — so if you run from inside a sandbox such as a snap, set `BRANCHY_FILE` or you will silently open a second, empty graph.

The format is plain JSON, and worth keeping in version control.

## There is a window too

The same graph drawn as a skill tree, in the [main repository](https://github.com/jqnfxa/branchy-rs).

## License

MIT or Apache-2.0, at your option.
