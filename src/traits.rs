//! The [`Egg`] trait.

use crate::diff::{Change, ChangeKind, Diff};
use crate::error::{EggError, EggResult};
use crate::path::{EggPath, PathSegment};
use crate::value::{SnapshotData, Value};

/// A type whose state Eggs can capture, restore, diff and patch.
///
/// Implement it with `#[derive(Egg)]` (feature `derive`), with
/// [`impl_egg_for_value!`](crate::impl_egg_for_value) for plain value types, or by hand.
/// Implementations are provided for primitives, `String`, `Option`, `Result`, `Box`,
/// `Vec`, arrays, tuples (up to 6), `HashMap`, `BTreeMap`, `HashSet` and `BTreeSet`.
///
/// # Differences from the proposed trait in the specification
///
/// The specification sketches `snapshot`, `restore`, `diff` and `fields`. Those are all
/// here, plus the minimum extra surface needed to make nested values, collections and
/// patches work:
///
/// * [`Egg::restore_in_place`] is the required restore primitive. [`Egg::restore`] (with
///   the spec's `&self` *model* receiver) has a default that forwards to it; override it
///   only if your type really needs the model.
/// * [`Egg::from_snapshot`] builds a new value from a snapshot, which collections need when
///   a restore must re-create elements.
/// * [`Egg::diff_into`] is the required diff primitive; [`Egg::diff`] wraps it.
/// * [`Egg::apply_change`] and [`Egg::restore_path`] address nested locations for patches
///   and per-field restore.
/// * `Snapshot` must satisfy [`SnapshotData`] (clone, compare, debug-print, `Send + Sync`,
///   `'static`) rather than only `Clone`; comparison is what lets patches detect conflicts.
pub trait Egg: Sized {
    /// The captured representation of a value. Must not borrow from the value.
    type Snapshot: SnapshotData;

    /// Captures the participating state.
    fn snapshot(&self) -> Self::Snapshot;

    /// Restores participating fields from `snapshot` into `self`.
    ///
    /// Fields excluded from capture are left untouched. On error the value may be partially
    /// restored; use [`Snapshot::try_restore`](crate::Snapshot::try_restore) for atomic restore.
    ///
    /// # Errors
    ///
    /// Returns an error if the snapshot cannot be applied (schema mismatch, validation, …).
    fn restore_in_place(&mut self, snapshot: &Self::Snapshot) -> EggResult<()>;

    /// Restores `snapshot` into `target`, with `self` acting as the *model* instance.
    ///
    /// The default ignores the model and calls `target.restore_in_place(snapshot)`.
    ///
    /// # Errors
    ///
    /// See [`Egg::restore_in_place`].
    fn restore(&self, target: &mut Self, snapshot: &Self::Snapshot) -> EggResult<()> {
        target.restore_in_place(snapshot)
    }

    /// Builds a fresh value from a snapshot.
    ///
    /// Needed when restoring a collection must re-create elements. Derived types can only do
    /// this when every `#[egg(skip)]` field is also `#[egg(default)]`; otherwise (and by
    /// default) it returns [`EggError::UnsupportedOperation`].
    ///
    /// # Errors
    ///
    /// Returns an error if a value cannot be built from the snapshot alone.
    fn from_snapshot(snapshot: &Self::Snapshot) -> EggResult<Self> {
        let _ = snapshot;
        Err(EggError::unsupported(
            "from_snapshot",
            format!("`{}` cannot be built from a snapshot alone", std::any::type_name::<Self>()),
        ))
    }

    /// Appends the changes that turn `self` into `other` to `out`. `path` is where `self`
    /// lives inside the value being diffed.
    fn diff_into(&self, other: &Self, path: &EggPath, out: &mut Vec<Change>);

    /// Describes the differences from `self` (before) to `other` (after).
    fn diff(&self, other: &Self) -> Diff {
        let mut out = Vec::new();
        self.diff_into(other, &EggPath::root(), &mut out);
        Diff::from_changes(out)
    }

    /// Stable field labels in declaration order, where available.
    #[must_use]
    fn fields() -> &'static [&'static str] {
        &[]
    }

    /// Schema version declared with `#[egg(version = N)]`; `0` otherwise.
    #[must_use]
    fn schema_version() -> u32 {
        0
    }

    /// Restores only the location `path` (relative to `self`) from `snapshot`.
    ///
    /// The default handles the empty path (restore everything) and rejects any other.
    ///
    /// # Errors
    ///
    /// Returns [`EggError::InvalidFieldPath`] if `path` does not exist or is not restorable.
    fn restore_path(&mut self, path: &[PathSegment], snapshot: &Self::Snapshot) -> EggResult<()> {
        if path.is_empty() {
            self.restore_in_place(snapshot)
        } else {
            Err(EggError::invalid_path(render(path), "type has no addressable children"))
        }
    }

    /// Applies one [`Change`] located at `path` (relative to `self`), verifying that the
    /// current value matches the change's `before` side.
    ///
    /// The default handles `Modified`/`VariantChanged` at the empty path by comparing and
    /// replacing the whole value. Containers override it to route to children.
    ///
    /// # Errors
    ///
    /// * [`EggError::PatchConflict`] if the current state does not match the change;
    /// * [`EggError::InvalidFieldPath`] if `path` does not exist;
    /// * [`EggError::IncompatibleSchema`] if the change carries values of another type.
    fn apply_change(&mut self, path: &[PathSegment], change: &Change) -> EggResult<()> {
        if path.is_empty() {
            apply_whole(self, change)
        } else {
            Err(EggError::invalid_path(&change.path, "type has no addressable children"))
        }
    }

    /// Whether `snapshot` holds no participating data.
    #[must_use]
    fn snapshot_is_empty(snapshot: &Self::Snapshot) -> bool {
        let _ = snapshot;
        false
    }

    /// Estimated heap bytes owned by `snapshot` (not counting `size_of::<Self::Snapshot>()`).
    /// An estimate, not an allocator measurement.
    #[must_use]
    fn snapshot_heap_size(snapshot: &Self::Snapshot) -> usize {
        let _ = snapshot;
        0
    }
}

pub(crate) fn render(path: &[PathSegment]) -> String {
    EggPath::from_segments(path.to_vec()).to_string()
}

pub(crate) fn key_string<K: std::fmt::Debug>(key: &K) -> String {
    format!("{key:?}")
}

/// Extracts a typed value from a change side, with an actionable error.
pub(crate) fn downcast_value<'a, S: SnapshotData>(
    change: &Change,
    value: Option<&'a Value>,
) -> EggResult<&'a S> {
    let value = value.ok_or_else(|| {
        EggError::incompatible(
            format!("a {:?} change with values for `{}`", change.kind, change.path),
            "a change missing its before/after value",
        )
    })?;
    value
        .downcast_ref::<S>()
        .ok_or_else(|| EggError::incompatible(std::any::type_name::<S>(), value.type_name()))
}

/// Applies a whole-value `Modified`/`VariantChanged` change to `target`.
///
/// Checks that `target`'s current snapshot equals the change's `before`, then restores the
/// `after` snapshot. This is what the default [`Egg::apply_change`] does at the empty path.
///
/// # Errors
///
/// See [`Egg::apply_change`].
pub fn apply_whole<T: Egg>(target: &mut T, change: &Change) -> EggResult<()> {
    match change.kind {
        ChangeKind::Modified | ChangeKind::VariantChanged => {
            let before = downcast_value::<T::Snapshot>(change, change.before())?;
            let after = downcast_value::<T::Snapshot>(change, change.after())?;
            if target.snapshot() != *before {
                return Err(EggError::conflict(
                    &change.path,
                    "current value differs from the value the patch expects",
                ));
            }
            target.restore_in_place(after)
        }
        ChangeKind::Added | ChangeKind::Removed => Err(EggError::unsupported(
            "apply_change",
            format!("{:?} is not applicable at `{}`: the parent is not a collection", change.kind, change.path),
        )),
    }
}

/// Implements [`Egg`] for plain value types that are captured and compared as a whole.
///
/// The types must be `Clone + Debug + PartialEq + Send + Sync + 'static`. A changed value is
/// reported as a single [`ChangeKind::Modified`] change.
///
/// ```
/// #[derive(Clone, Debug, PartialEq)]
/// struct Rgb(u8, u8, u8);
/// eggs::impl_egg_for_value!(Rgb);
///
/// use eggs::Egg;
/// assert!(Rgb(1, 2, 3).diff(&Rgb(1, 2, 4)).changed());
/// ```
#[macro_export]
macro_rules! impl_egg_for_value {
    ($($t:ty),+ $(,)?) => {$(
        impl $crate::Egg for $t {
            type Snapshot = $t;

            fn snapshot(&self) -> Self::Snapshot {
                ::core::clone::Clone::clone(self)
            }

            fn restore_in_place(&mut self, snapshot: &Self::Snapshot) -> $crate::EggResult<()> {
                *self = ::core::clone::Clone::clone(snapshot);
                ::core::result::Result::Ok(())
            }

            fn from_snapshot(snapshot: &Self::Snapshot) -> $crate::EggResult<Self> {
                ::core::result::Result::Ok(::core::clone::Clone::clone(snapshot))
            }

            fn diff_into(
                &self,
                other: &Self,
                path: &$crate::EggPath,
                out: &mut ::std::vec::Vec<$crate::Change>,
            ) {
                if self != other {
                    out.push($crate::Change::modified(
                        ::core::clone::Clone::clone(path),
                        $crate::Value::new(::core::clone::Clone::clone(self)),
                        $crate::Value::new(::core::clone::Clone::clone(other)),
                    ));
                }
            }
        }
    )+};
}
