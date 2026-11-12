<div align="center">

# eggs

**Snapshots, diffs, patches and undo/redo for your Rust application state.**

[![crates.io](https://img.shields.io/crates/v/eggs)](https://crates.io/crates/eggs)
[![docs.rs](https://img.shields.io/docsrs/eggs)](https://docs.rs/eggs)
[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![MSRV 1.75](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](#minimum-supported-rust-version)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)

</div>

Capture the state of a value, change it freely, then see exactly what changed, undo it, or replay
the change somewhere else. Eggs is built for editors, games, simulations, configuration tools and
test infrastructure, anywhere "go back to how it was" matters.

- **Snapshots**: capture and atomically restore, with a derive macro and per-field control
- **Diffs**: field-level, nested, typed; paths like `items[3].qty` and `scores{"ann"}`
- **Patches**: executable, conflict-checked and reversible
- **History**: undo/redo with checkpoints and bounded memory
- **Transactions**: group edits, abort restores, drop aborts
- **100% safe Rust**, no runtime reflection, no global state

## Install

```toml
[dependencies]
eggs = "0.1.1"
```

## Quick start

```rust
use eggs::{Egg, EggExt};

#[derive(Egg, Clone, Debug)]
struct Player {
    name: String,
    health: u32,
    position: (f32, f32),
    #[egg(skip)] // never captured, diffed or restored
    frame_cache: Vec<u8>,
}

fn main() -> eggs::EggResult<()> {
    let mut player = Player {
        name: "Ann".into(),
        health: 100,
        position: (0.0, 0.0),
        frame_cache: Vec::new(),
    };

    let checkpoint = player.capture();      // 1. capture

    player.health = 35;                      // 2. change things
    player.position.0 = 12.5;

    for change in checkpoint.diff(&player)?.changes() {
        println!("{} ({:?})", change.path, change.kind);   // 3. what changed?
    }
    // health (Modified)
    // position.0 (Modified)

    checkpoint.try_restore(&mut player)?;    // 4. atomic rollback
    assert_eq!(player.health, 100);
    Ok(())
}
```

Run the full version with `cargo run --example snapshot`.

## Diff and patch

A diff keeps the exact *before* and *after* values, so it can be turned into a patch that is
checked before it is applied, and reversed.

```rust
use eggs::{Egg, Patch};

#[derive(Egg, Clone, Debug, PartialEq)]
struct Config {
    volume: u8,
    tags: Vec<String>,
}

let old = Config { volume: 3, tags: vec!["a".into()] };
let new = Config { volume: 8, tags: vec!["a".into(), "b".into()] };

let patch = Patch::between(&old, &new).unwrap();

let mut state = old.clone();
patch.apply(&mut state).unwrap();            // staged on a copy; all-or-nothing
assert_eq!(state, new);

patch.reverse().apply(&mut state).unwrap();
assert_eq!(state, old);

// A patch only applies to the state it was made from.
let mut other = Config { volume: 99, tags: vec![] };
assert!(patch.apply(&mut other).is_err());   // PatchConflict, `other` untouched
```

## Undo / redo

```rust
use eggs::{Egg, History};

#[derive(Egg, Debug)]
struct Doc {
    text: String,
}

let mut doc = Doc { text: "a".into() };
let mut history = History::new(&doc);

doc.text.push('b');
history.record(&doc);
history.checkpoint("saved");

assert!(history.undo(&mut doc).unwrap());    // `true`: it moved
assert_eq!(doc.text, "a");
assert!(history.redo(&mut doc).unwrap());
assert_eq!(doc.text, "ab");
```

Recording after an undo discards the redo branch. Retention is bounded (128 entries by default,
`History::with_limit` to change it).

## Transactions

Enable the `transaction` feature.

```rust,ignore
use eggs::{transaction, Egg};

#[derive(Egg)]
struct Counter { count: u32 }

let mut state = Counter { count: 0 };
let mut tx = transaction(&mut state)?;
tx.edit(|s| s.count += 1)?;
tx.abort()?;      // back to 0. Dropping an uncommitted transaction aborts it too.
```

## How it behaves

| | |
|---|---|
| **Eggs doesn't watch your writes.** | State is captured when you call `capture`/`record`, and changes are found by comparing. `tracking::Tracker` wraps that pattern; it doesn't intercept mutations. |
| **`#[egg(skip)]` means everywhere.** | Rollback, undo and transaction abort never touch a skipped field. |
| **Restore is atomic** via `try_restore`, and so is everything built on it (`History`, `Transaction`). | `restore_in_place` is the non-atomic primitive. |
| **Patches are checked.** | Each change carries its exact *before* value. A mismatch is a `PatchConflict`; a patch for a different type is `IncompatibleSchema`. |
| **Diffs are typed and ordered.** | `Debug` output is display-only. Order is deterministic: fields in declaration order, map/set entries by key, sequence removals high→low. |
| **IDs are process-local.** | Snapshot IDs aren't unique across runs and aren't serialized. |

## Derive attributes

| Attribute | Meaning |
|---|---|
| `#[egg(skip)]` | Exclude from capture, restore and diff |
| `#[egg(rename = "x")]` | Custom path label |
| `#[egg(default)]` | On `skip` fields: the value used when rebuilding from a snapshot |
| `#[egg(ignore_diff)]` | Capture and restore, but omit from diffs |
| `#[egg(ignore_restore)]` | Capture and diff, but never restore (patches still apply it) |
| `#[egg(compare_with = "path")]` | Custom equality `fn(&T, &T) -> bool` |
| `#[egg(with = "module")]` | Custom adapter module (`Snapshot`, `snapshot`, `restore`, `from_snapshot`, `diff`) |
| `#[egg(strategy = "clone")]` | Snapshot strategy (only `clone` exists today) |
| `#[egg(version = N)]` | On the struct: schema version, checked when deserializing |

Named, tuple and unit structs are supported, including generics and lifetimes. Enums are a
compile error for now. Bad or conflicting attributes are compile errors too.

Built-in support: integers, floats, `bool`, `char`, `String`, `&'static str`, `Option`, `Result`,
`Box`, `Vec`, arrays, tuples (up to 6), `HashMap`, `BTreeMap`, `HashSet`, `BTreeSet`. For your own
plain value types, use `eggs::impl_egg_for_value!`.

## Cargo features

| Feature | Default | What it adds |
|---|:---:|---|
| `derive` | ✅ | `#[derive(Egg)]` |
| `history` | ✅ | `History` undo/redo with checkpoints |
| `serde` | | Serde support for snapshots, paths and derived snapshot types |
| `json` | | `Snapshot::to_json` / `from_json` with a versioned envelope (implies `serde`) |
| `transaction` | | `Transaction`, abort-on-drop |
| `tracking` | | `tracking::Tracker`, baseline comparison with watchers |
| `store` | | `SnapshotStore` trait and an in-memory store |
| `sync` | | `SharedSnapshot`, an `Arc`-backed snapshot for sharing across threads |

## Not yet implemented

Compression, binary/file persistence helpers, observer events, enum derive, copy-on-write
structural sharing, and serde for `Diff`/`Patch` (their typed values are type-erased). Nothing in
the public API claims these. `with_capacity` on the snapshot builder is accepted but currently a
no-op. `cargo bench` prints baseline timings; diffing large *equal* values allocates more than
it needs to, which is the first planned optimization.

## Security

A deserialized snapshot becomes your application state when restored. Validate untrusted input and
enforce size limits before restoring it.

## Minimum supported Rust version

Rust **1.75**. Raising it is considered a minor-version change.

## License

Licensed under the [MIT license](LICENSE).
