//! Transactional edits with abort-on-drop.

use std::thread;

use crate::error::{EggError, EggResult};
use crate::snapshot::{Snapshot, SnapshotId};
use crate::traits::Egg;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Active,
    Committed,
    Aborted,
    /// An abort failed; the error was returned to the caller, so `Drop` must not retry.
    Failed,
}

/// Identifies a point inside a [`Transaction`], created by [`Transaction::checkpoint`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TxCheckpoint(SnapshotId);

/// Starts a [`Transaction`] over `target`. See [`Transaction`] for semantics.
///
/// # Errors
///
/// Currently infallible; returns a `Result` so future capture strategies can fail.
pub fn transaction<T: Egg>(target: &mut T) -> EggResult<Transaction<'_, T>> {
    Transaction::begin(target)
}

/// A group of edits that is either committed or rolled back as a unit.
///
/// * [`Transaction::abort`] restores the state captured when the transaction began.
/// * [`Transaction::commit`] keeps the edited state.
/// * **Dropping an active transaction aborts it.** `Drop` cannot return an error, so a failed
///   restore there panics (unless the thread is already panicking) instead of being swallowed.
///   Call [`Transaction::abort`] explicitly to handle the error.
/// * Nested transactions are not supported (the borrow checker already prevents starting a
///   second transaction on the same value while one is alive).
///
/// Rolling back restores participating fields only; edits to `#[egg(skip)]` fields are not undone.
///
/// ```
/// use eggs::{Egg, transaction};
///
/// #[derive(Egg)]
/// struct Counter { count: u32, enabled: bool }
///
/// let mut state = Counter { count: 0, enabled: false };
/// let mut tx = transaction(&mut state).unwrap();
/// tx.edit(|s| { s.count += 1; s.enabled = true; }).unwrap();
/// tx.abort().unwrap();
/// drop(tx); // a transaction borrows the value until it is dropped
/// assert_eq!(state.count, 0);
/// ```
pub struct Transaction<'a, T: Egg> {
    target: &'a mut T,
    backup: Snapshot<T>,
    marks: Vec<Snapshot<T>>,
    state: State,
}

impl<'a, T: Egg> Transaction<'a, T> {
    /// Begins a transaction.
    ///
    /// # Errors
    ///
    /// Currently infallible.
    pub fn begin(target: &'a mut T) -> EggResult<Self> {
        let backup = Snapshot::capture(target);
        Ok(Transaction { target, backup, marks: Vec::new(), state: State::Active })
    }

    /// The value as currently edited.
    #[must_use]
    pub fn value(&self) -> &T {
        self.target
    }

    /// Runs `edit` on the value.
    ///
    /// # Errors
    ///
    /// [`EggError::TransactionState`] if the transaction is no longer active.
    pub fn edit(&mut self, edit: impl FnOnce(&mut T)) -> EggResult<()> {
        self.require_active("edit")?;
        edit(self.target);
        Ok(())
    }

    /// Keeps the edited state and ends the transaction.
    ///
    /// # Errors
    ///
    /// [`EggError::TransactionState`] if the transaction is no longer active.
    pub fn commit(&mut self) -> EggResult<()> {
        self.require_active("commit")?;
        self.state = State::Committed;
        Ok(())
    }

    /// Commits and records the committed state in `history`.
    ///
    /// # Errors
    ///
    /// [`EggError::TransactionState`] if the transaction is no longer active.
    #[cfg(feature = "history")]
    pub fn commit_and_record(&mut self, history: &mut crate::history::History<T>) -> EggResult<()> {
        self.commit()?;
        history.record(self.target);
        Ok(())
    }

    /// Restores the state from when the transaction began and ends the transaction.
    ///
    /// # Errors
    ///
    /// [`EggError::TransactionState`] if the transaction is no longer active, or the restore
    /// error (the transaction then counts as finished; `Drop` will not retry).
    pub fn abort(&mut self) -> EggResult<()> {
        self.require_active("abort")?;
        match self.backup.try_restore(self.target) {
            Ok(()) => {
                self.state = State::Aborted;
                Ok(())
            }
            Err(e) => {
                self.state = State::Failed;
                Err(e)
            }
        }
    }

    /// Restores the starting state but keeps the transaction open for further edits.
    ///
    /// # Errors
    ///
    /// [`EggError::TransactionState`] if the transaction is no longer active, or the restore error.
    pub fn rollback_transaction(&mut self) -> EggResult<()> {
        self.require_active("rollback_transaction")?;
        self.backup.try_restore(self.target)
    }

    /// Marks the current edited state so [`Transaction::rollback_to`] can return to it.
    ///
    /// # Errors
    ///
    /// [`EggError::TransactionState`] if the transaction is no longer active.
    pub fn checkpoint(&mut self) -> EggResult<TxCheckpoint> {
        self.require_active("checkpoint")?;
        let mark = Snapshot::capture(self.target);
        let id = mark.id();
        self.marks.push(mark);
        Ok(TxCheckpoint(id))
    }

    /// Restores a checkpoint created by this transaction; later checkpoints are discarded.
    ///
    /// # Errors
    ///
    /// [`EggError::TransactionState`] if inactive or the checkpoint is unknown (for example
    /// already discarded by an earlier rollback); otherwise the restore error.
    pub fn rollback_to(&mut self, checkpoint: TxCheckpoint) -> EggResult<()> {
        self.require_active("rollback_to")?;
        let index = self
            .marks
            .iter()
            .position(|m| m.id() == checkpoint.0)
            .ok_or_else(|| EggError::TransactionState("unknown or discarded checkpoint".into()))?;
        self.marks[index].try_restore(self.target)?;
        self.marks.truncate(index + 1);
        Ok(())
    }

    /// Whether [`Transaction::commit`] succeeded.
    #[must_use]
    pub fn is_committed(&self) -> bool {
        self.state == State::Committed
    }

    /// Whether the transaction was aborted (explicitly or on drop).
    #[must_use]
    pub fn is_aborted(&self) -> bool {
        self.state == State::Aborted
    }

    fn require_active(&self, operation: &str) -> EggResult<()> {
        match self.state {
            State::Active => Ok(()),
            other => Err(EggError::TransactionState(format!(
                "cannot {operation}: the transaction is already {}",
                match other {
                    State::Committed => "committed",
                    State::Aborted => "aborted",
                    _ => "finished after a failed abort",
                }
            ))),
        }
    }
}

impl<T: Egg> Drop for Transaction<'_, T> {
    fn drop(&mut self) {
        if self.state == State::Active {
            if let Err(error) = self.backup.try_restore(self.target) {
                if !thread::panicking() {
                    panic!("eggs: failed to abort an uncommitted transaction on drop: {error}");
                }
            }
        }
    }
}
