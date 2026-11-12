//! Thread-safe shared snapshots.

use std::ops::Deref;
use std::sync::Arc;

use crate::snapshot::Snapshot;
use crate::traits::Egg;

/// A reference-counted, immutable [`Snapshot`] that can be shared across threads.
///
/// Cloning is a pointer copy. This shares *ownership of one capture*; it is not
/// copy-on-write structural sharing between captures.
pub struct SharedSnapshot<T: Egg>(Arc<Snapshot<T>>);

impl<T: Egg> SharedSnapshot<T> {
    /// Shares an existing snapshot.
    #[must_use]
    pub fn new(snapshot: Snapshot<T>) -> Self {
        SharedSnapshot(Arc::new(snapshot))
    }

    /// Captures `value` and shares the result.
    #[must_use]
    pub fn capture(value: &T) -> Self {
        Self::new(Snapshot::capture(value))
    }

    /// Whether both handles point at the same capture.
    #[must_use]
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }
}

impl<T: Egg> Clone for SharedSnapshot<T> {
    fn clone(&self) -> Self {
        SharedSnapshot(Arc::clone(&self.0))
    }
}

impl<T: Egg> Deref for SharedSnapshot<T> {
    type Target = Snapshot<T>;

    fn deref(&self) -> &Snapshot<T> {
        &self.0
    }
}

impl<T: Egg> From<Snapshot<T>> for SharedSnapshot<T> {
    fn from(snapshot: Snapshot<T>) -> Self {
        SharedSnapshot::new(snapshot)
    }
}
