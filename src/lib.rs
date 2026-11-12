//! # Eggs
//!
//! Type-safe snapshots, diffs, patches and undo/redo history for application state.
//!
//! ```
//! use eggs::{Egg, EggExt, History, Patch};
//!
//! #[derive(Egg, Clone, Debug, PartialEq)]
//! struct Player {
//!     name: String,
//!     health: u32,
//!     #[egg(skip)]
//!     scratch: Vec<u8>, // never captured, restored or diffed
//! }
//!
//! let mut player = Player { name: "Ann".into(), health: 100, scratch: vec![] };
//!
//! // Snapshot and restore.
//! let before = player.capture();
//! player.health = 40;
//! player.scratch.push(1);
//! before.try_restore(&mut player).unwrap();
//! assert_eq!(player.health, 100);
//! assert_eq!(player.scratch, vec![1]); // skipped fields are untouched
//!
//! // Diff.
//! let mut hurt = player.clone();
//! hurt.health = 10;
//! let diff = player.diff(&hurt);
//! assert!(diff.contains_change("health"));
//!
//! // Patch and reverse.
//! let patch = Patch::from_diff(&diff).unwrap();
//! patch.apply(&mut player).unwrap();
//! assert_eq!(player.health, 10);
//! patch.reverse().apply(&mut player).unwrap();
//! assert_eq!(player.health, 100);
//!
//! // Undo/redo.
//! let mut history = History::new(&player);
//! player.health = 55;
//! history.record(&player);
//! history.undo(&mut player).unwrap();
//! assert_eq!(player.health, 100);
//! ```
//!
//! ## What Eggs does not do
//!
//! Eggs cannot observe ordinary field writes. State is captured when you ask
//! ([`Snapshot::capture`], [`History::record`]) and changes are found by comparing states
//! ([`Egg::diff`]); the optional `tracking` module wraps that pattern but does not intercept
//! mutations. Fields marked `#[egg(skip)]` are excluded from every operation, including rollback.
//!
//! ## Feature flags
//!
//! | Feature | Default | Purpose |
//! |---|---|---|
//! | `derive` | yes | Re-exports `#[derive(Egg)]` |
//! | `history` | yes | [`History`] undo/redo |
//! | `serde` | no | Serde for [`Snapshot`], [`EggPath`], [`ChangeKind`] and derived snapshot types |
//! | `json` | no | `Snapshot::to_json` / `Snapshot::from_json` (implies `serde`) |
//! | `tracking` | no | `tracking::Tracker` |
//! | `transaction` | no | `Transaction` |
//! | `store` | no | `SnapshotStore`, `MemoryStore` |
//! | `sync` | no | `SharedSnapshot` |

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// Lets the derive macro's `::eggs::` paths resolve inside this crate's own tests and docs.
extern crate self as eggs;

// Compiles every Rust block in the README as a doctest so it cannot drift from the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

mod config;
mod diff;
mod error;
mod impls;
mod patch;
mod path;
mod snapshot;
mod traits;
mod value;

#[cfg(feature = "history")]
mod history;
#[cfg(feature = "store")]
pub mod store;
#[cfg(feature = "sync")]
pub mod sync;
#[cfg(feature = "tracking")]
pub mod tracking;
#[cfg(feature = "transaction")]
mod transaction;

pub use config::{EggConfig, EggMetadata, SnapshotOptions, Strategy, DEFAULT_HISTORY_LIMIT};
pub use diff::{Change, ChangeKind, Diff};
pub use error::{EggError, EggResult};
pub use impls::MapEntry;
pub use patch::Patch;
pub use path::{EggPath, PathSegment, ToEggPath};
pub use snapshot::{EggExt, Snapshot, SnapshotBuilder, SnapshotId};
pub use traits::{apply_whole, Egg};
pub use value::{SnapshotData, Value};

#[cfg(feature = "history")]
pub use history::{CheckpointId, History};
#[cfg(feature = "transaction")]
pub use transaction::{transaction, Transaction, TxCheckpoint};

#[cfg(feature = "derive")]
pub use eggs_derive::Egg;

#[cfg(feature = "serde")]
#[doc(hidden)]
pub use serde;
