//! Snapshot repositories.

use std::collections::BTreeMap;

use crate::error::EggResult;
use crate::snapshot::Snapshot;
use crate::traits::Egg;

/// A repository that retains snapshots (or anything else) and hands out IDs.
pub trait SnapshotStore<S> {
    /// The identifier type the store hands out.
    type Id;

    /// Stores `snapshot`.
    ///
    /// # Errors
    ///
    /// [`EggError::Storage`](crate::EggError) if the store cannot retain it.
    fn insert(&mut self, snapshot: S) -> EggResult<Self::Id>;

    /// Fetches by ID. `Ok(None)` if the ID is unknown.
    ///
    /// # Errors
    ///
    /// [`EggError::Storage`](crate::EggError) if the store fails.
    fn get(&self, id: &Self::Id) -> EggResult<Option<&S>>;

    /// Removes by ID. Returns whether it existed.
    ///
    /// # Errors
    ///
    /// [`EggError::Storage`](crate::EggError) if the store fails.
    fn remove(&mut self, id: &Self::Id) -> EggResult<bool>;

    /// Whether `id` exists.
    ///
    /// # Errors
    ///
    /// [`EggError::Storage`](crate::EggError) if the store fails.
    fn contains(&self, id: &Self::Id) -> EggResult<bool> {
        Ok(self.get(id)?.is_some())
    }
}

/// Identifies an entry in a [`MemoryStore`]. Process-local; IDs ascend in insertion order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StoreId(u64);

/// An in-memory [`SnapshotStore`].
#[derive(Clone, Debug)]
pub struct MemoryStore<S> {
    items: BTreeMap<StoreId, S>,
    next: u64,
}

impl<S> Default for MemoryStore<S> {
    fn default() -> Self {
        MemoryStore { items: BTreeMap::new(), next: 0 }
    }
}

impl<S> MemoryStore<S> {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Stored IDs, oldest first.
    #[must_use]
    pub fn list(&self) -> Vec<StoreId> {
        self.items.keys().copied().collect()
    }

    /// Removes everything.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Keeps only the newest `keep` items; returns how many were removed.
    pub fn prune_oldest(&mut self, keep: usize) -> usize {
        let mut removed = 0;
        while self.items.len() > keep {
            self.items.pop_first();
            removed += 1;
        }
        removed
    }
}

impl<S> SnapshotStore<S> for MemoryStore<S> {
    type Id = StoreId;

    fn insert(&mut self, snapshot: S) -> EggResult<StoreId> {
        let id = StoreId(self.next);
        self.next += 1;
        self.items.insert(id, snapshot);
        Ok(id)
    }

    fn get(&self, id: &StoreId) -> EggResult<Option<&S>> {
        Ok(self.items.get(id))
    }

    fn remove(&mut self, id: &StoreId) -> EggResult<bool> {
        Ok(self.items.remove(id).is_some())
    }
}

impl<T: Egg> MemoryStore<Snapshot<T>> {
    /// Sum of the stored snapshots' [`Snapshot::estimate_size`]. An estimate.
    #[must_use]
    pub fn estimate_size(&self) -> usize {
        self.items.values().map(Snapshot::estimate_size).sum()
    }
}
