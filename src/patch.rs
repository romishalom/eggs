//! Patches: executable, reversible edit scripts built from [`Diff`]s.

use std::fmt;
use std::marker::PhantomData;

use crate::diff::{Change, Diff};
use crate::error::EggResult;
use crate::traits::Egg;

/// An executable edit script for values of type `T`.
///
/// A patch is built from a [`Diff`] (whose changes carry exact typed before/after values) and
/// can be applied to any `T` whose current state matches the patch's expectations, or reversed.
/// Diffs from a different type fail with [`EggError::IncompatibleSchema`](crate::EggError) or
/// [`EggError::PatchConflict`](crate::EggError) rather than corrupting state.
///
/// ```
/// use eggs::{Egg, Patch};
///
/// #[derive(Egg, Clone, PartialEq, Debug)]
/// struct Config { volume: u8, title: String }
///
/// let old = Config { volume: 3, title: "a".into() };
/// let new = Config { volume: 7, title: "a".into() };
/// let patch = Patch::between(&old, &new).unwrap();
///
/// let mut state = old.clone();
/// patch.apply(&mut state).unwrap();
/// assert_eq!(state, new);
/// patch.reverse().apply(&mut state).unwrap();
/// assert_eq!(state, old);
/// ```
pub struct Patch<T> {
    changes: Vec<Change>,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Clone for Patch<T> {
    fn clone(&self) -> Self {
        Patch { changes: self.changes.clone(), _marker: PhantomData }
    }
}

impl<T> fmt::Debug for Patch<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Patch").field("changes", &self.changes).finish()
    }
}

impl<T: Egg> Patch<T> {
    /// Builds a patch from a diff.
    ///
    /// # Errors
    ///
    /// [`EggError::Validation`](crate::EggError) if the diff fails [`Diff::validate`].
    pub fn from_diff(diff: &Diff) -> EggResult<Self> {
        diff.validate()?;
        Ok(Patch { changes: diff.changes().to_vec(), _marker: PhantomData })
    }

    /// Builds the patch that turns `before` into `after`.
    ///
    /// # Errors
    ///
    /// See [`Patch::from_diff`].
    pub fn between(before: &T, after: &T) -> EggResult<Self> {
        Self::from_diff(&before.diff(after))
    }

    /// The patch as a [`Diff`].
    #[must_use]
    pub fn diff(&self) -> Diff {
        Diff::from_changes(self.changes.clone())
    }

    /// The operations, in application order.
    #[must_use]
    pub fn operations(&self) -> &[Change] {
        &self.changes
    }

    /// Number of operations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// `true` if the patch has no operations.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// The patch that undoes this one (operations reversed and inverted).
    #[must_use]
    pub fn reverse(&self) -> Patch<T> {
        Patch { changes: self.changes.iter().rev().map(Change::reversed).collect(), _marker: PhantomData }
    }

    /// Applies operations one by one **without** staging.
    ///
    /// If an operation fails, earlier operations stay applied. Use [`Patch::apply`] for atomic
    /// application.
    ///
    /// # Errors
    ///
    /// The first failing operation's error ([`EggError::PatchConflict`](crate::EggError) when
    /// the state does not match).
    pub fn apply_in_place(&self, target: &mut T) -> EggResult<()> {
        for change in &self.changes {
            target.apply_change(change.path.segments(), change)?;
        }
        Ok(())
    }
}

impl<T: Egg + Clone> Patch<T> {
    /// Applies the patch atomically: operations run on a staged copy and replace `target`
    /// only if all succeed. On error `target` is untouched.
    ///
    /// # Errors
    ///
    /// See [`Patch::apply_in_place`].
    pub fn apply(&self, target: &mut T) -> EggResult<()> {
        let mut staged = target.clone();
        self.apply_in_place(&mut staged)?;
        *target = staged;
        Ok(())
    }

    /// Dry-runs the patch against a copy of `target`.
    ///
    /// # Errors
    ///
    /// The error [`Patch::apply`] would return.
    pub fn validate(&self, target: &T) -> EggResult<()> {
        let mut scratch = target.clone();
        self.apply_in_place(&mut scratch)
    }

    /// Whether [`Patch::apply`] would succeed on `target`.
    #[must_use]
    pub fn is_applicable(&self, target: &T) -> bool {
        self.validate(target).is_ok()
    }
}
