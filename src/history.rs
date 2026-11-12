//! Undo/redo history.

use crate::config::{EggConfig, DEFAULT_HISTORY_LIMIT};
use crate::error::{EggError, EggResult};
use crate::snapshot::{Snapshot, SnapshotId};
use crate::traits::Egg;

/// Identifies a history entry marked with [`History::checkpoint`].
///
/// A `CheckpointId` is a handle to the *entry*, not to the name: it stays valid while that entry is
/// retained, even after [`History::remove_checkpoint`] drops the name (and two names marking the
/// same entry yield equal IDs). It becomes invalid once the entry is evicted by the retention limit
/// or removed by [`History::truncate`]/[`History::clear`]. Process-local, like [`SnapshotId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CheckpointId(SnapshotId);

/// A linear sequence of recorded states with a cursor, for undo/redo.
///
/// The history always holds at least one entry (the initial state), and `current()` is the
/// entry the cursor points at.
///
/// # Semantics
///
/// * [`History::record`] appends the new state; if the cursor is not at the end, the forward
///   ("redo") branch is discarded first.
/// * Retention is bounded: [`DEFAULT_HISTORY_LIMIT`] (128) entries by default. When the limit
///   is exceeded the oldest entries are evicted. A limit of `0` is **rejected** with
///   [`EggError::Validation`].
/// * [`History::undo`]/[`History::redo`] restore atomically and return `Ok(false)` when there is
///   nothing to move to. If a restore fails the cursor does not move.
/// * Checkpoint names are unique; creating a checkpoint with an existing name **replaces** it.
///
/// Restoring writes only fields that participate in snapshots; `#[egg(skip)]` fields are never
/// rewound.
///
/// ```
/// use eggs::{Egg, History};
///
/// #[derive(Egg, Debug, PartialEq)]
/// struct Doc { text: String }
///
/// let mut doc = Doc { text: "a".into() };
/// let mut history = History::new(&doc);
/// doc.text.push('b');
/// history.record(&doc);
///
/// assert!(history.undo(&mut doc).unwrap());
/// assert_eq!(doc.text, "a");
/// assert!(history.redo(&mut doc).unwrap());
/// assert_eq!(doc.text, "ab");
/// ```
pub struct History<T: Egg> {
    entries: Vec<Snapshot<T>>,
    cursor: usize,
    limit: usize,
    metadata: bool,
    checkpoints: Vec<(String, SnapshotId)>,
}

impl<T: Egg> History<T> {
    /// Starts a history whose first entry is `initial`, with the default limit.
    #[must_use]
    pub fn new(initial: &T) -> Self {
        History {
            entries: vec![Snapshot::capture(initial)],
            cursor: 0,
            limit: DEFAULT_HISTORY_LIMIT,
            metadata: false,
            checkpoints: Vec::new(),
        }
    }

    /// Starts a history with a custom retention limit.
    ///
    /// # Errors
    ///
    /// [`EggError::Validation`] if `limit` is zero.
    pub fn with_limit(initial: &T, limit: usize) -> EggResult<Self> {
        let mut history = Self::new(initial);
        history.set_limit(limit)?;
        Ok(history)
    }

    /// Starts a history using an [`EggConfig`].
    ///
    /// # Errors
    ///
    /// [`EggError::Validation`] if the configured limit is zero.
    pub fn with_config(initial: &T, config: &EggConfig) -> EggResult<Self> {
        let mut history = Self::with_limit(initial, config.history_limit())?;
        history.metadata = config.metadata();
        if history.metadata {
            history.entries[0] = Snapshot::capture_with_metadata(initial);
        }
        Ok(history)
    }

    /// Records `value` as the new current state, discarding any redo branch.
    pub fn record(&mut self, value: &T) {
        self.entries.truncate(self.cursor + 1);
        self.entries.push(if self.metadata {
            Snapshot::capture_with_metadata(value)
        } else {
            Snapshot::capture(value)
        });
        self.cursor = self.entries.len() - 1;
        self.enforce_limit();
    }

    /// Moves one step back, restoring `target` atomically. `Ok(false)` if already at the start.
    ///
    /// # Errors
    ///
    /// The restore error; the cursor does not move.
    pub fn undo(&mut self, target: &mut T) -> EggResult<bool> {
        if !self.can_undo() {
            return Ok(false);
        }
        self.move_to(self.cursor - 1, |s| s.try_restore(target)).map(|()| true)
    }

    /// Like [`History::undo`], with `model` acting as the schema/model instance
    /// (see [`Egg::restore`]). Not atomic.
    ///
    /// # Errors
    ///
    /// The restore error; the cursor does not move.
    pub fn undo_with_model(&mut self, target: &mut T, model: &T) -> EggResult<bool> {
        if !self.can_undo() {
            return Ok(false);
        }
        self.move_to(self.cursor - 1, |s| s.restore(target, model)).map(|()| true)
    }

    /// Moves one step forward, restoring `target` atomically. `Ok(false)` if already at the end.
    ///
    /// # Errors
    ///
    /// The restore error; the cursor does not move.
    pub fn redo(&mut self, target: &mut T) -> EggResult<bool> {
        if !self.can_redo() {
            return Ok(false);
        }
        self.move_to(self.cursor + 1, |s| s.try_restore(target)).map(|()| true)
    }

    /// Like [`History::redo`], with a model instance. Not atomic.
    ///
    /// # Errors
    ///
    /// The restore error; the cursor does not move.
    pub fn redo_with_model(&mut self, target: &mut T, model: &T) -> EggResult<bool> {
        if !self.can_redo() {
            return Ok(false);
        }
        self.move_to(self.cursor + 1, |s| s.restore(target, model)).map(|()| true)
    }

    /// Moves back to `checkpoint`. `Ok(false)` if it is the current entry.
    ///
    /// # Errors
    ///
    /// [`EggError::Validation`] if the checkpoint is unknown/evicted or lies ahead of the cursor
    /// (use [`History::redo_to`]); otherwise the restore error.
    pub fn undo_to(&mut self, checkpoint: CheckpointId, target: &mut T) -> EggResult<bool> {
        let index = self.index_of(checkpoint.0)?;
        if index > self.cursor {
            return Err(EggError::validation("checkpoint is ahead of the current position; use redo_to"));
        }
        self.go(index, target)
    }

    /// Moves forward to `checkpoint`. `Ok(false)` if it is the current entry.
    ///
    /// # Errors
    ///
    /// [`EggError::Validation`] if the checkpoint is unknown/evicted or lies behind the cursor
    /// (use [`History::undo_to`]); otherwise the restore error.
    pub fn redo_to(&mut self, checkpoint: CheckpointId, target: &mut T) -> EggResult<bool> {
        let index = self.index_of(checkpoint.0)?;
        if index < self.cursor {
            return Err(EggError::validation("checkpoint is behind the current position; use undo_to"));
        }
        self.go(index, target)
    }

    /// The state at the cursor.
    #[must_use]
    pub fn current(&self) -> &Snapshot<T> {
        &self.entries[self.cursor]
    }

    /// The entry before the cursor, if any.
    #[must_use]
    pub fn previous(&self) -> Option<&Snapshot<T>> {
        self.cursor.checked_sub(1).map(|i| &self.entries[i])
    }

    /// The entry after the cursor, if any.
    #[must_use]
    pub fn next(&self) -> Option<&Snapshot<T>> {
        self.entries.get(self.cursor + 1)
    }

    /// The oldest retained entry.
    #[must_use]
    pub fn first(&self) -> &Snapshot<T> {
        &self.entries[0]
    }

    /// The newest entry (the end of the redo branch, if any).
    #[must_use]
    pub fn last(&self) -> &Snapshot<T> {
        &self.entries[self.entries.len() - 1]
    }

    /// Number of retained entries (always at least one).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Always `false`: a history holds at least its current entry. Provided for API symmetry.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The cursor's zero-based position among the retained entries.
    #[must_use]
    pub fn position(&self) -> usize {
        self.cursor
    }

    /// Drops every entry except the current one, and all checkpoints.
    pub fn clear(&mut self) {
        let current = self.entries.swap_remove(self.cursor);
        self.entries.clear();
        self.entries.push(current);
        self.cursor = 0;
        self.checkpoints.clear();
    }

    /// Drops the entries after `position` (an index into the retained entries) and moves the
    /// cursor back to `position` if it was beyond it. The state of `target` is not changed.
    ///
    /// # Errors
    ///
    /// [`EggError::Validation`] if `position` is out of range.
    pub fn truncate(&mut self, position: usize) -> EggResult<()> {
        if position >= self.entries.len() {
            return Err(EggError::validation(format!(
                "position {position} is out of range (history has {} entries)",
                self.entries.len()
            )));
        }
        self.entries.truncate(position + 1);
        self.cursor = self.cursor.min(position);
        self.prune_checkpoints();
        Ok(())
    }

    /// The retention limit.
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Changes the retention limit, evicting old entries if necessary.
    ///
    /// # Errors
    ///
    /// [`EggError::Validation`] if `limit` is zero.
    pub fn set_limit(&mut self, limit: usize) -> EggResult<()> {
        if limit == 0 {
            return Err(EggError::validation("history limit must be at least 1"));
        }
        self.limit = limit;
        self.enforce_limit();
        Ok(())
    }

    /// Whether [`History::undo`] can move.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    /// Whether [`History::redo`] can move.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }

    /// Names the current entry. An existing checkpoint with the same name is **replaced**.
    pub fn checkpoint(&mut self, name: impl Into<String>) -> CheckpointId {
        let name = name.into();
        let id = self.entries[self.cursor].id();
        self.checkpoints.retain(|(n, _)| *n != name);
        self.checkpoints.push((name, id));
        CheckpointId(id)
    }

    /// Moves to the entry named `name` (in either direction), restoring `target` atomically.
    /// `Ok(false)` if it is the current entry.
    ///
    /// # Errors
    ///
    /// [`EggError::Validation`] if no such checkpoint exists; otherwise the restore error.
    pub fn restore_checkpoint(&mut self, name: &str, target: &mut T) -> EggResult<bool> {
        let id = self
            .checkpoints
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
            .ok_or_else(|| EggError::validation(format!("no checkpoint named `{name}`")))?;
        let index = self.index_of(id)?;
        self.go(index, target)
    }

    /// Removes the checkpoint name `name`. Returns whether it existed. Existing
    /// [`CheckpointId`]s for that entry remain usable (see [`CheckpointId`]).
    pub fn remove_checkpoint(&mut self, name: &str) -> bool {
        let before = self.checkpoints.len();
        self.checkpoints.retain(|(n, _)| n != name);
        self.checkpoints.len() != before
    }

    /// The live checkpoints as `(name, id)`, oldest first.
    pub fn checkpoints(&self) -> impl Iterator<Item = (&str, CheckpointId)> {
        self.checkpoints.iter().map(|(n, id)| (n.as_str(), CheckpointId(*id)))
    }

    fn index_of(&self, id: SnapshotId) -> EggResult<usize> {
        self.entries
            .iter()
            .position(|s| s.id() == id)
            .ok_or_else(|| EggError::validation("unknown checkpoint (it may have been evicted or removed)"))
    }

    fn go(&mut self, index: usize, target: &mut T) -> EggResult<bool> {
        if index == self.cursor {
            return Ok(false);
        }
        self.move_to(index, |s| s.try_restore(target)).map(|()| true)
    }

    fn move_to(&mut self, index: usize, restore: impl FnOnce(&Snapshot<T>) -> EggResult<()>) -> EggResult<()> {
        restore(&self.entries[index])?;
        self.cursor = index;
        Ok(())
    }

    fn enforce_limit(&mut self) {
        while self.entries.len() > self.limit {
            if self.cursor > 0 {
                self.entries.remove(0);
                self.cursor -= 1;
            } else {
                self.entries.pop();
            }
        }
        self.prune_checkpoints();
    }

    fn prune_checkpoints(&mut self) {
        let entries = &self.entries;
        self.checkpoints.retain(|(_, id)| entries.iter().any(|s| s.id() == *id));
    }
}
