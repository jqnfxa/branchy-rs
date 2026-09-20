# branchy-app

Persistence and the view model behind [Branchy](https://github.com/jqnfxa/branchy-rs)'s front ends.

This is internal glue. If you are looking for the dependency graph itself, that is [`branchy-core`](https://crates.io/crates/branchy-core), which has no dependencies and is the crate worth depending on.

What lives here is the part `branchy-core` deliberately refuses to hold:

- **`store`** — reading and writing the document, atomically, with an undo stack. A save is written beside the target and renamed over it, because a half-written document is exactly what a sync tool would propagate everywhere.
- **`snapshot`** — the whole derived picture in one value, so a user interface never recomputes status, tier or the queue for itself.
- **`today`** — the clock, which a graph engine must not read if its tests are to mean anything.

The API is not stable and follows whatever the Branchy front ends need.

## License

MIT or Apache-2.0, at your option.
