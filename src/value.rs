//! Type-erased, cloneable values carried by [`Change`](crate::Change)s.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

/// Requirements on [`Egg::Snapshot`](crate::Egg::Snapshot) types.
///
/// Snapshots must be cloneable, comparable (so patches can detect conflicts), printable
/// (for diagnostics only) and thread-safe, and must not borrow. It is implemented
/// automatically for every type that satisfies those bounds.
pub trait SnapshotData: Clone + fmt::Debug + PartialEq + Send + Sync + 'static {}

impl<T> SnapshotData for T where T: Clone + fmt::Debug + PartialEq + Send + Sync + 'static {}

trait Erased: Send + Sync + fmt::Debug {
    fn as_any(&self) -> &dyn Any;
    fn eq_erased(&self, other: &dyn Erased) -> bool;
    fn type_name(&self) -> &'static str;
}

impl<T: SnapshotData> Erased for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn eq_erased(&self, other: &dyn Erased) -> bool {
        other.as_any().downcast_ref::<T>().map_or(false, |o| o == self)
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }
}

/// An immutable, cheaply cloneable, typed value (a snapshot fragment).
///
/// `Value` is how [`Change`](crate::Change)s keep exact before/after data without resorting
/// to `Debug` strings. Use [`Value::downcast_ref`] to recover the concrete type; its
/// `Debug` output is for display only and is not a stable format.
#[derive(Clone)]
pub struct Value(Arc<dyn Erased>);

impl Value {
    /// Wraps a snapshot value.
    #[must_use]
    pub fn new<T: SnapshotData>(value: T) -> Self {
        Value(Arc::new(value))
    }

    /// Borrows the value as `T`, if that is its concrete type.
    #[must_use]
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        (*self.0).as_any().downcast_ref::<T>()
    }

    /// The concrete type's name, for diagnostics.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        (*self.0).type_name()
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        (*self.0).eq_erased(&*other.0)
    }
}
