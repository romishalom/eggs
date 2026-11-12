//! Explicit change tracking by comparison with a baseline.
//!
//! Rust cannot observe ordinary field writes, and this module does not pretend to. A
//! [`Tracker`] owns a value plus a baseline copy; "what changed" is answered by diffing the
//! two when asked ([`Tracker::changes`]), and [`Tracker::mark_changed`] lets callers record
//! changes the diff cannot see.

use std::collections::BTreeSet;
use std::ops::{Deref, DerefMut};

use crate::diff::Diff;
use crate::error::EggResult;
use crate::path::{EggPath, ToEggPath};
use crate::traits::Egg;

/// Identifies a watcher registered with [`Tracker::watch`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WatchId(u64);

/// Starts tracking `value`: the current state becomes the baseline.
pub fn track<T: Egg + Clone>(value: T) -> Tracker<T> {
    Tracker::new(value)
}

/// A value plus the baseline it is compared against.
///
/// Watchers registered with [`Tracker::watch`] are called synchronously, in registration order,
/// from [`Tracker::take_changes`] and [`Tracker::flush_changes`] — only when there are changes. They
/// receive `&Diff` and cannot reenter the tracker (it is mutably borrowed during the call). A
/// panicking watcher propagates to the caller. There are no global observers.
///
/// ```
/// use eggs::{Egg, tracking::track};
///
/// #[derive(Egg, Clone)]
/// struct Settings { volume: u8, muted: bool }
///
/// let mut t = track(Settings { volume: 3, muted: false });
/// t.volume = 9;
/// assert!(t.has_changed_field("volume"));
/// assert!(!t.has_changed_field("muted"));
/// let diff = t.take_changes();
/// assert_eq!(diff.change_count(), 1);
/// assert!(!t.has_changed());
/// ```
pub struct Tracker<T: Egg + Clone> {
    value: T,
    baseline: T,
    only: Vec<EggPath>,
    marked: BTreeSet<EggPath>,
    watchers: Vec<(WatchId, Box<dyn FnMut(&Diff)>)>,
    next_watch: u64,
}

impl<T: Egg + Clone> Tracker<T> {
    /// Starts tracking `value`.
    pub fn new(value: T) -> Self {
        Tracker {
            baseline: value.clone(),
            value,
            only: Vec::new(),
            marked: BTreeSet::new(),
            watchers: Vec::new(),
            next_watch: 0,
        }
    }

    /// Stops tracking and returns the value.
    pub fn untrack(self) -> T {
        self.value
    }

    /// Borrows the value.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Mutably borrows the value. Edits are detected by comparison, not intercepted.
    pub fn value_mut(&mut self) -> &mut T {
        &mut self.value
    }

    /// Restricts change reporting to this path (and below). If never called, every path is tracked.
    ///
    /// # Errors
    ///
    /// [`EggError::InvalidFieldPath`](crate::EggError) if `path` is malformed.
    pub fn track_field<P: ToEggPath>(&mut self, path: P) -> EggResult<()> {
        let path = path.to_egg_path()?;
        if !self.only.contains(&path) {
            self.only.push(path);
        }
        Ok(())
    }

    /// Removes a path from the tracked set. Returns whether it was present.
    pub fn untrack_field<P: ToEggPath>(&mut self, path: P) -> bool {
        let Ok(path) = path.to_egg_path() else { return false };
        let before = self.only.len();
        self.only.retain(|p| *p != path);
        self.only.len() != before
    }

    /// The explicitly tracked paths; empty means "everything".
    pub fn tracked_fields(&self) -> &[EggPath] {
        &self.only
    }

    /// Records that `path` changed even if a diff cannot show it (for example interior
    /// mutability or `#[egg(ignore_diff)]` fields).
    ///
    /// # Errors
    ///
    /// [`EggError::InvalidFieldPath`](crate::EggError) if `path` is malformed.
    pub fn mark_changed<P: ToEggPath>(&mut self, path: P) -> EggResult<()> {
        self.marked.insert(path.to_egg_path()?);
        Ok(())
    }

    /// Paths recorded with [`Tracker::mark_changed`] since the last reset.
    pub fn marked_paths(&self) -> impl Iterator<Item = &EggPath> {
        self.marked.iter()
    }

    /// The differences between the baseline and the current value, within the tracked paths.
    #[must_use]
    pub fn changes(&self) -> Diff {
        let diff = self.baseline.diff(&self.value);
        if self.only.is_empty() {
            diff
        } else {
            Diff::from_changes(
                diff.changes()
                    .iter()
                    .filter(|c| self.only.iter().any(|p| c.path.starts_with(p)))
                    .cloned()
                    .collect(),
            )
        }
    }

    /// Whether anything changed (by diff or by [`Tracker::mark_changed`]).
    #[must_use]
    pub fn has_changed(&self) -> bool {
        !self.marked.is_empty() || self.changes().changed()
    }

    /// Whether `path` (or something below it) changed. Malformed string paths yield `false`.
    #[must_use]
    pub fn has_changed_field<P: ToEggPath>(&self, path: P) -> bool {
        let Ok(path) = path.to_egg_path() else { return false };
        self.marked.iter().any(|m| m.starts_with(&path))
            || self.changes().changes().iter().any(|c| c.path.starts_with(&path))
    }

    /// Returns the changes, notifies watchers, and makes the current value the new baseline.
    pub fn take_changes(&mut self) -> Diff {
        let diff = self.changes();
        if diff.changed() {
            for (_, watcher) in &mut self.watchers {
                watcher(&diff);
            }
        }
        self.reset_tracking();
        diff
    }

    /// Like [`Tracker::take_changes`] but discards the diff.
    pub fn flush_changes(&mut self) {
        let _ = self.take_changes();
    }

    /// Makes the current value the baseline and clears marks, without notifying watchers.
    pub fn reset_tracking(&mut self) {
        self.baseline = self.value.clone();
        self.marked.clear();
    }

    /// Registers a watcher called from [`Tracker::take_changes`] / [`Tracker::flush_changes`].
    pub fn watch(&mut self, callback: impl FnMut(&Diff) + 'static) -> WatchId {
        let id = WatchId(self.next_watch);
        self.next_watch += 1;
        self.watchers.push((id, Box::new(callback)));
        id
    }

    /// Removes a watcher. Returns whether it existed.
    pub fn unwatch(&mut self, id: WatchId) -> bool {
        let before = self.watchers.len();
        self.watchers.retain(|(w, _)| *w != id);
        self.watchers.len() != before
    }
}

impl<T: Egg + Clone> Deref for Tracker<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: Egg + Clone> DerefMut for Tracker<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}
