//! Diffs: immutable, inspectable collections of [`Change`]s.

use std::collections::HashSet;

use crate::error::{EggError, EggResult};
use crate::path::{EggPath, PathSegment, ToEggPath};
use crate::value::Value;

/// What happened at a [`Change`]'s path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ChangeKind {
    /// A value was replaced. Both `before` and `after` are present.
    Modified,
    /// An element (sequence item, map entry, set element) appeared. Only `after` is present.
    Added,
    /// An element disappeared. Only `before` is present.
    Removed,
    /// A value switched variant (`None` ⇄ `Some`, `Ok` ⇄ `Err`). Both sides are present.
    VariantChanged,
}

/// One path-level difference between two states.
///
/// `before`/`after` hold exact typed snapshot fragments (see [`Value`]), never
/// `Debug` strings, which is what makes a [`Diff`] executable as a [`Patch`](crate::Patch).
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// Where the change happened.
    pub path: EggPath,
    /// What kind of change it is.
    pub kind: ChangeKind,
    before: Option<Value>,
    after: Option<Value>,
}

impl Change {
    /// A value replaced at `path`.
    #[must_use]
    pub fn modified(path: EggPath, before: Value, after: Value) -> Self {
        Change { path, kind: ChangeKind::Modified, before: Some(before), after: Some(after) }
    }

    /// An element added at `path`.
    #[must_use]
    pub fn added(path: EggPath, after: Value) -> Self {
        Change { path, kind: ChangeKind::Added, before: None, after: Some(after) }
    }

    /// An element removed from `path`.
    #[must_use]
    pub fn removed(path: EggPath, before: Value) -> Self {
        Change { path, kind: ChangeKind::Removed, before: Some(before), after: None }
    }

    /// A variant switch at `path`.
    #[must_use]
    pub fn variant_changed(path: EggPath, before: Value, after: Value) -> Self {
        Change { path, kind: ChangeKind::VariantChanged, before: Some(before), after: Some(after) }
    }

    /// The value before the change, if the kind has one.
    #[must_use]
    pub fn before(&self) -> Option<&Value> {
        self.before.as_ref()
    }

    /// The value after the change, if the kind has one.
    #[must_use]
    pub fn after(&self) -> Option<&Value> {
        self.after.as_ref()
    }

    /// The change that undoes this one: same path, `before`/`after` swapped,
    /// `Added` ⇄ `Removed`.
    #[must_use]
    pub fn reversed(&self) -> Change {
        let kind = match self.kind {
            ChangeKind::Added => ChangeKind::Removed,
            ChangeKind::Removed => ChangeKind::Added,
            other => other,
        };
        Change {
            path: self.path.clone(),
            kind,
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

/// An immutable collection of [`Change`]s between two states.
///
/// Produced by [`Egg::diff`](crate::Egg::diff). Change order is deterministic: struct
/// fields in declaration order, map and set entries sorted by their key rendering,
/// sequence removals from the highest index down, then additions from the lowest up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diff {
    changes: Vec<Change>,
}

fn overlaps(a: &EggPath, b: &EggPath) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

impl Diff {
    /// An empty diff.
    #[must_use]
    pub fn new() -> Self {
        Diff::default()
    }

    /// Builds a diff from changes (no validation; see [`Diff::validate`]).
    #[must_use]
    pub fn from_changes(changes: Vec<Change>) -> Self {
        Diff { changes }
    }

    /// All changes, in deterministic order.
    #[must_use]
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    /// `true` if any change exists.
    #[must_use]
    pub fn changed(&self) -> bool {
        !self.changes.is_empty()
    }

    /// `true` if no change exists.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Number of changes.
    #[must_use]
    pub fn change_count(&self) -> usize {
        self.changes.len()
    }

    /// Iterates the changed paths without allocating.
    pub fn changed_fields(&self) -> impl Iterator<Item = &EggPath> {
        self.changes.iter().map(|c| &c.path)
    }

    /// Whether a change exists at exactly `path`. Malformed string paths yield `false`.
    pub fn contains_change<P: ToEggPath>(&self, path: P) -> bool {
        self.field_diff(path).is_some()
    }

    /// The change at exactly `path`, if any. Malformed string paths yield `None`.
    pub fn field_diff<P: ToEggPath>(&self, path: P) -> Option<&Change> {
        let path = path.to_egg_path().ok()?;
        self.changes.iter().find(|c| c.path == path)
    }

    /// Keeps only changes at or below one of `paths`.
    ///
    /// # Errors
    ///
    /// Returns [`EggError::InvalidFieldPath`] if a path is malformed.
    pub fn filter_fields<P: ToEggPath>(&self, paths: impl IntoIterator<Item = P>) -> EggResult<Diff> {
        let paths = collect_paths(paths)?;
        Ok(Diff {
            changes: self
                .changes
                .iter()
                .filter(|c| paths.iter().any(|p| c.path.starts_with(p)))
                .cloned()
                .collect(),
        })
    }

    /// Removes changes at or below any of `paths`.
    ///
    /// # Errors
    ///
    /// Returns [`EggError::InvalidFieldPath`] if a path is malformed.
    pub fn exclude_fields<P: ToEggPath>(&self, paths: impl IntoIterator<Item = P>) -> EggResult<Diff> {
        let paths = collect_paths(paths)?;
        Ok(Diff {
            changes: self
                .changes
                .iter()
                .filter(|c| !paths.iter().any(|p| c.path.starts_with(p)))
                .cloned()
                .collect(),
        })
    }

    /// The diff that undoes this one: every change is [`Change::reversed`] and the order
    /// is reversed, so applying it after this diff walks back through the same steps.
    #[must_use]
    pub fn reverse(&self) -> Diff {
        Diff { changes: self.changes.iter().rev().map(Change::reversed).collect() }
    }

    /// Alias of [`Diff::reverse`].
    #[must_use]
    pub fn invert(&self) -> Diff {
        self.reverse()
    }

    /// Unions two diffs that describe edits to the same base state.
    ///
    /// Identical changes are kept once; changes at overlapping paths that differ are a conflict.
    ///
    /// # Errors
    ///
    /// Returns [`EggError::PatchConflict`] on conflicting changes.
    pub fn merge(&self, other: &Diff) -> EggResult<Diff> {
        let mut changes = self.changes.clone();
        for change in &other.changes {
            match self.changes.iter().find(|c| overlaps(&c.path, &change.path)) {
                Some(existing) if existing == change => {}
                Some(existing) => {
                    return Err(EggError::conflict(
                        &change.path,
                        format!("both diffs change overlapping path `{}` differently", existing.path),
                    ));
                }
                None => changes.push(change.clone()),
            }
        }
        Ok(Diff { changes })
    }

    /// Composes `self` followed by `next` into one diff.
    ///
    /// Continuity is verified: where both touch the same path, `self`'s `after` must equal
    /// `next`'s `before`. Supported: disjoint paths, `Modified ∘ Modified` (a net no-op is
    /// dropped), and `Added ∘ Removed` / `Removed ∘ Added` with identical values (which cancel).
    ///
    /// # Errors
    ///
    /// * [`EggError::PatchConflict`] if continuity is broken;
    /// * [`EggError::UnsupportedOperation`] for overlapping paths of different depth, other
    ///   kind pairs, or `Added`/`Removed` at sequence indices (whose positions shift, so
    ///   continuity cannot be verified path by path).
    pub fn compose(&self, next: &Diff) -> EggResult<Diff> {
        let shifts = |c: &Change| {
            matches!(c.kind, ChangeKind::Added | ChangeKind::Removed)
                && matches!(c.path.segments().last(), Some(PathSegment::Index(_)))
        };
        if self.changes.iter().chain(&next.changes).any(shifts) {
            return Err(EggError::unsupported(
                "compose",
                "additions/removals at sequence indices shift positions; compose cannot verify them",
            ));
        }

        let mut out = self.changes.clone();
        for n in &next.changes {
            let Some(i) = out.iter().position(|e| overlaps(&e.path, &n.path)) else {
                out.push(n.clone());
                continue;
            };
            let e = &out[i];
            if e.path != n.path {
                return Err(EggError::unsupported(
                    "compose",
                    format!("changes at `{}` and `{}` overlap at different depths", e.path, n.path),
                ));
            }
            use ChangeKind::{Added, Modified, Removed, VariantChanged};
            match (e.kind, n.kind) {
                (Modified | VariantChanged, Modified | VariantChanged) => {
                    if e.after != n.before {
                        return Err(EggError::conflict(
                            &n.path,
                            "second diff does not start from the first diff's result",
                        ));
                    }
                    let kind = if e.kind == VariantChanged || n.kind == VariantChanged {
                        VariantChanged
                    } else {
                        Modified
                    };
                    if e.before == n.after {
                        out.remove(i);
                    } else {
                        out[i] = Change {
                            path: n.path.clone(),
                            kind,
                            before: e.before.clone(),
                            after: n.after.clone(),
                        };
                    }
                }
                (Added, Removed) | (Removed, Added) => {
                    let (left, right) =
                        if e.kind == Added { (&e.after, &n.before) } else { (&e.before, &n.after) };
                    if left != right {
                        return Err(EggError::unsupported(
                            "compose",
                            format!("`{}` is re-created with a different value", n.path),
                        ));
                    }
                    out.remove(i);
                }
                (a, b) => {
                    return Err(EggError::unsupported(
                        "compose",
                        format!("cannot compose {a:?} followed by {b:?} at `{}`", n.path),
                    ));
                }
            }
        }
        Ok(Diff { changes: out })
    }

    /// Checks internal invariants: each kind carries exactly the values it should,
    /// `Modified` changes are not no-ops, and no path appears twice.
    ///
    /// # Errors
    ///
    /// Returns [`EggError::Validation`] describing the first violation.
    pub fn validate(&self) -> EggResult<()> {
        let mut seen = HashSet::new();
        for c in &self.changes {
            let (want_before, want_after) = match c.kind {
                ChangeKind::Modified | ChangeKind::VariantChanged => (true, true),
                ChangeKind::Added => (false, true),
                ChangeKind::Removed => (true, false),
            };
            if c.before.is_some() != want_before || c.after.is_some() != want_after {
                return Err(EggError::validation(format!(
                    "{:?} change at `{}` has the wrong before/after values",
                    c.kind, c.path
                )));
            }
            if c.kind == ChangeKind::Modified && c.before == c.after {
                return Err(EggError::validation(format!("no-op Modified change at `{}`", c.path)));
            }
            if !seen.insert(&c.path) {
                return Err(EggError::validation(format!("duplicate change at `{}`", c.path)));
            }
        }
        Ok(())
    }
}

fn collect_paths<P: ToEggPath>(paths: impl IntoIterator<Item = P>) -> EggResult<Vec<EggPath>> {
    paths.into_iter().map(|p| p.to_egg_path()).collect()
}
