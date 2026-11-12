//! Snapshots: captured state with identity, optional metadata and restore operations.

use std::fmt;
use std::mem::size_of;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use crate::config::{EggMetadata, SnapshotOptions, Strategy};
use crate::diff::Diff;
use crate::error::{EggError, EggResult};
use crate::path::{EggPath, PathSegment, ToEggPath};
use crate::traits::Egg;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// An opaque, process-local snapshot identifier.
///
/// IDs are unique within one process run only. They are **not** globally unique and are
/// not preserved by serialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SnapshotId(u64);

impl SnapshotId {
    fn next() -> Self {
        SnapshotId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// A captured state of a `T`.
///
/// With the default [`Strategy::Clone`], a snapshot owns a deep copy of the participating
/// state. A snapshot built with include/exclude options (see [`SnapshotBuilder`]) still holds the
/// whole copy, but *restore* and *diff* only consider the selected paths.
pub struct Snapshot<T: Egg> {
    id: SnapshotId,
    data: T::Snapshot,
    selection: Option<Vec<EggPath>>,
    metadata: Option<EggMetadata>,
}

impl<T: Egg> Clone for Snapshot<T> {
    /// Copies the snapshot, keeping its [`SnapshotId`]. Use [`Snapshot::clone_snapshot`] for a
    /// copy with a fresh ID.
    fn clone(&self) -> Self {
        Snapshot {
            id: self.id,
            data: self.data.clone(),
            selection: self.selection.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

impl<T: Egg> fmt::Debug for Snapshot<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Snapshot")
            .field("id", &self.id)
            .field("data", &self.data)
            .field("selection", &self.selection)
            .finish()
    }
}

impl<T: Egg> Snapshot<T> {
    /// Captures `value` into an owned snapshot.
    #[must_use]
    pub fn capture(value: &T) -> Self {
        Snapshot { id: SnapshotId::next(), data: value.snapshot(), selection: None, metadata: None }
    }

    /// Captures with options. Used by [`SnapshotBuilder::capture`].
    ///
    /// # Errors
    ///
    /// [`EggError::InvalidFieldPath`] for unknown or misspelled field names;
    /// [`EggError::UnsupportedOperation`] for nested paths in `exclude`.
    pub fn capture_with(value: &T, options: &SnapshotOptions) -> EggResult<Self> {
        let Strategy::Clone = options.strategy;
        let selection = resolve_selection::<T>(options)?;
        let metadata = options.metadata.then(|| EggMetadata {
            created_at: SystemTime::now(),
            label: options.label.clone(),
            schema_version: T::schema_version(),
        });
        Ok(Snapshot { id: SnapshotId::next(), data: value.snapshot(), selection, metadata })
    }

    #[cfg(feature = "history")]
    pub(crate) fn capture_with_metadata(value: &T) -> Self {
        let mut snapshot = Self::capture(value);
        snapshot.metadata = Some(EggMetadata {
            created_at: SystemTime::now(),
            label: None,
            schema_version: T::schema_version(),
        });
        snapshot
    }

    /// The process-local ID.
    #[must_use]
    pub fn id(&self) -> SnapshotId {
        self.id
    }

    /// When the snapshot was captured, if metadata was requested.
    #[must_use]
    pub fn created_at(&self) -> Option<SystemTime> {
        self.metadata.as_ref().map(EggMetadata::created_at)
    }

    /// The attached metadata, if any.
    #[must_use]
    pub fn metadata(&self) -> Option<&EggMetadata> {
        self.metadata.as_ref()
    }

    /// Borrows the underlying snapshot representation.
    #[must_use]
    pub fn value(&self) -> &T::Snapshot {
        &self.data
    }

    /// The paths restore/diff are limited to, or `None` if the whole snapshot participates.
    #[must_use]
    pub fn selection(&self) -> Option<&[EggPath]> {
        self.selection.as_deref()
    }

    /// Explicitly copies the snapshot and gives the copy a fresh [`SnapshotId`].
    #[must_use]
    pub fn clone_snapshot(&self) -> Self {
        let mut copy = self.clone();
        copy.id = SnapshotId::next();
        copy
    }

    /// `true` if no data participates: every field was excluded, or the type captures nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match &self.selection {
            Some(paths) => paths.is_empty(),
            None => T::snapshot_is_empty(&self.data),
        }
    }

    /// Estimated storage size in bytes. An estimate — not an allocator measurement.
    #[must_use]
    pub fn estimate_size(&self) -> usize {
        size_of::<Self>() + T::snapshot_heap_size(&self.data)
    }

    /// Alias of [`Snapshot::estimate_size`].
    #[must_use]
    pub fn size(&self) -> usize {
        self.estimate_size()
    }

    /// Restores into `target`, with `model` as the schema/model instance (see [`Egg::restore`]).
    /// Not atomic; see [`Snapshot::try_restore`].
    ///
    /// # Errors
    ///
    /// Propagates restore errors.
    pub fn restore(&self, target: &mut T, model: &T) -> EggResult<()> {
        match &self.selection {
            None => model.restore(target, &self.data),
            Some(_) => self.restore_into(target),
        }
    }

    /// Restores into `target`. Not atomic: on error `target` may be partially restored.
    ///
    /// # Errors
    ///
    /// Propagates restore errors.
    pub fn restore_into(&self, target: &mut T) -> EggResult<()> {
        match &self.selection {
            None => target.restore_in_place(&self.data),
            Some(paths) => {
                for path in paths {
                    target.restore_path(path.segments(), &self.data)?;
                }
                Ok(())
            }
        }
    }

    /// Atomic restore: on failure `target` is put back to its previous (participating) state.
    ///
    /// Fields excluded from capture are never touched by restore, so rolling back is exact.
    ///
    /// # Errors
    ///
    /// The restore error. If rolling back also fails, an [`EggError::RestoreFailed`] reporting both.
    pub fn try_restore(&self, target: &mut T) -> EggResult<()> {
        with_rollback(target, |t| self.restore_into(t))
    }

    /// Atomically restores a single location.
    ///
    /// # Errors
    ///
    /// [`EggError::InvalidFieldPath`] if `path` is malformed, does not exist, or is excluded
    /// from this snapshot's selection.
    pub fn restore_field<P: ToEggPath>(&self, target: &mut T, path: P) -> EggResult<()> {
        self.restore_fields(target, [path])
    }

    /// Atomically restores several locations. Nothing is changed if any path is invalid.
    ///
    /// # Errors
    ///
    /// See [`Snapshot::restore_field`].
    pub fn restore_fields<P: ToEggPath>(
        &self,
        target: &mut T,
        paths: impl IntoIterator<Item = P>,
    ) -> EggResult<()> {
        let paths: Vec<EggPath> = paths.into_iter().map(|p| p.to_egg_path()).collect::<EggResult<_>>()?;
        if let Some(selection) = &self.selection {
            for p in &paths {
                if !selection.iter().any(|s| p.starts_with(s)) {
                    return Err(EggError::invalid_path(p, "path is excluded from this snapshot's selection"));
                }
            }
        }
        with_rollback(target, |t| {
            for p in &paths {
                t.restore_path(p.segments(), &self.data)?;
            }
            Ok(())
        })
    }
}

impl<T: Egg + Clone> Snapshot<T> {
    /// Dry-runs a restore on a copy of `target`.
    #[must_use]
    pub fn can_restore(&self, target: &T) -> bool {
        let mut scratch = target.clone();
        self.restore_into(&mut scratch).is_ok()
    }

    /// The differences from this snapshot (before) to `current` (after), limited to the
    /// snapshot's selection.
    ///
    /// # Errors
    ///
    /// Fails if the snapshot cannot be materialized over a copy of `current`.
    pub fn diff(&self, current: &T) -> EggResult<Diff> {
        let mut before = current.clone();
        self.restore_into(&mut before)?;
        let diff = before.diff(current);
        match &self.selection {
            None => Ok(diff),
            Some(paths) => diff.filter_fields(paths.iter()),
        }
    }

    /// Ergonomic alias of [`Snapshot::diff`]; `diff` is the canonical name.
    ///
    /// # Errors
    ///
    /// See [`Snapshot::diff`].
    pub fn crack(&self, current: &T) -> EggResult<Diff> {
        self.diff(current)
    }
}

impl<T: Egg> From<&T> for Snapshot<T> {
    fn from(value: &T) -> Self {
        Snapshot::capture(value)
    }
}

pub(crate) fn with_rollback<T: Egg>(
    target: &mut T,
    op: impl FnOnce(&mut T) -> EggResult<()>,
) -> EggResult<()> {
    let backup = target.snapshot();
    match op(target) {
        Ok(()) => Ok(()),
        Err(error) => match target.restore_in_place(&backup) {
            Ok(()) => Err(error),
            Err(rollback) => Err(EggError::restore_failed(format!(
                "{error}; additionally, rolling back failed: {rollback}"
            ))),
        },
    }
}

fn resolve_selection<T: Egg>(options: &SnapshotOptions) -> EggResult<Option<Vec<EggPath>>> {
    if options.include.is_empty() && options.exclude.is_empty() {
        return Ok(None);
    }
    let fields = T::fields();
    let known = |path: &EggPath| -> EggResult<()> {
        match path.segments().first() {
            Some(PathSegment::Field(name)) if fields.contains(&name.as_str()) => Ok(()),
            _ => Err(EggError::invalid_path(
                path,
                if fields.is_empty() {
                    "this type exposes no field labels".to_string()
                } else {
                    format!("unknown field; available fields: {}", fields.join(", "))
                },
            )),
        }
    };

    let include: Vec<EggPath> =
        options.include.iter().map(|s| EggPath::parse(s)).collect::<EggResult<_>>()?;
    let exclude: Vec<EggPath> =
        options.exclude.iter().map(|s| EggPath::parse(s)).collect::<EggResult<_>>()?;
    for p in include.iter().chain(&exclude) {
        known(p)?;
    }
    if let Some(nested) = exclude.iter().find(|p| p.len() != 1) {
        return Err(EggError::unsupported(
            "exclude",
            format!("`{nested}` is nested; only top-level fields can be excluded"),
        ));
    }

    let base: Vec<EggPath> = if include.is_empty() {
        fields.iter().map(|f| EggPath::root().child_field(f)).collect()
    } else {
        include
    };
    Ok(Some(base.into_iter().filter(|p| !exclude.iter().any(|x| p.starts_with(x))).collect()))
}

/// Builder returned by [`EggExt::snapshot_with`]; configures one capture.
///
/// ```
/// use eggs::{Egg, EggExt};
///
/// #[derive(Egg)]
/// struct Player { health: u32, name: String }
///
/// let p = Player { health: 10, name: "Ann".into() };
/// let snapshot = p.snapshot_with().include("health").with_metadata().capture().unwrap();
/// assert!(snapshot.created_at().is_some());
/// assert!(p.snapshot_with().include("helth").capture().is_err()); // misspelled
/// ```
pub struct SnapshotBuilder<'a, T: Egg> {
    value: &'a T,
    options: SnapshotOptions,
}

impl<'a, T: Egg> SnapshotBuilder<'a, T> {
    pub(crate) fn new(value: &'a T) -> Self {
        SnapshotBuilder { value, options: SnapshotOptions::default() }
    }

    /// Restricts restore/diff to this field (or nested path). May be called repeatedly.
    #[must_use]
    pub fn include(mut self, field: impl Into<String>) -> Self {
        self.options.include.push(field.into());
        self
    }

    /// Removes a top-level field from restore/diff. May be called repeatedly.
    #[must_use]
    pub fn exclude(mut self, field: impl Into<String>) -> Self {
        self.options.exclude.push(field.into());
        self
    }

    /// Attaches [`EggMetadata`] (capture time and schema version).
    #[must_use]
    pub fn with_metadata(mut self) -> Self {
        self.options.metadata = true;
        self
    }

    /// Attaches metadata with a human-readable label.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.options.metadata = true;
        self.options.label = Some(label.into());
        self
    }

    /// A capacity hint. The clone strategy does not use it yet; it is accepted and recorded
    /// so callers need not change when a strategy that benefits from it is added.
    #[must_use]
    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.options.capacity = Some(capacity);
        self
    }

    /// Selects the snapshot strategy.
    #[must_use]
    pub fn with_strategy(mut self, strategy: Strategy) -> Self {
        self.options.strategy = strategy;
        self
    }

    /// Captures the snapshot.
    ///
    /// # Errors
    ///
    /// See [`Snapshot::capture_with`].
    pub fn capture(self) -> EggResult<Snapshot<T>> {
        Snapshot::capture_with(self.value, &self.options)
    }
}

/// Ergonomic methods available on every [`Egg`] type.
pub trait EggExt: Egg {
    /// Captures a [`Snapshot`] of `self`.
    fn capture(&self) -> Snapshot<Self> {
        Snapshot::capture(self)
    }

    /// Starts a configurable capture.
    fn snapshot_with(&self) -> SnapshotBuilder<'_, Self> {
        SnapshotBuilder::new(self)
    }

    /// Compares `snapshot` (before) with `self` (after). Alias of [`Snapshot::diff`].
    ///
    /// # Errors
    ///
    /// See [`Snapshot::diff`].
    fn crack(&self, snapshot: &Snapshot<Self>) -> EggResult<Diff>
    where
        Self: Clone,
    {
        snapshot.diff(self)
    }

    /// Atomically rolls `self` back to `snapshot`.
    ///
    /// # Errors
    ///
    /// See [`Snapshot::try_restore`].
    fn rollback_to(&mut self, snapshot: &Snapshot<Self>) -> EggResult<()> {
        snapshot.try_restore(self)
    }

    /// Atomically undoes `diff` (a diff that led *to* the current state).
    ///
    /// # Errors
    ///
    /// [`EggError::PatchConflict`] if the current state no longer matches the diff.
    fn rollback_changes(&mut self, diff: &Diff) -> EggResult<()>
    where
        Self: Clone,
    {
        crate::patch::Patch::<Self>::from_diff(diff)?.reverse().apply(self)
    }
}

impl<T: Egg> EggExt for T {}

#[cfg(feature = "serde")]
mod serde_impl {
    use super::Snapshot;
    use crate::error::EggError;
    use crate::path::EggPath;
    use crate::traits::Egg;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Version of the serialized snapshot envelope.
    pub(crate) const FORMAT_VERSION: u32 = 1;

    #[derive(Serialize)]
    struct EnvelopeRef<'a, S> {
        eggs_format: u32,
        schema_version: u32,
        selection: Option<Vec<String>>,
        data: &'a S,
    }

    #[derive(Deserialize)]
    struct Envelope<S> {
        eggs_format: u32,
        schema_version: u32,
        #[serde(default)]
        selection: Option<Vec<String>>,
        data: S,
    }

    #[derive(Deserialize)]
    pub(crate) struct Header {
        pub(crate) eggs_format: u32,
        pub(crate) schema_version: u32,
    }

    pub(crate) fn check_header<T: Egg>(header: &Header) -> Result<(), EggError> {
        if header.eggs_format != FORMAT_VERSION {
            return Err(EggError::IncompatibleSchema {
                expected: format!("eggs snapshot format {FORMAT_VERSION}"),
                found: format!("format {}", header.eggs_format),
            });
        }
        if header.schema_version != T::schema_version() {
            return Err(EggError::IncompatibleSchema {
                expected: format!("schema version {}", T::schema_version()),
                found: format!("schema version {}", header.schema_version),
            });
        }
        Ok(())
    }

    impl<T: Egg> Serialize for Snapshot<T>
    where
        T::Snapshot: Serialize,
    {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            EnvelopeRef {
                eggs_format: FORMAT_VERSION,
                schema_version: T::schema_version(),
                selection: self
                    .selection
                    .as_ref()
                    .map(|paths| paths.iter().map(ToString::to_string).collect()),
                data: &self.data,
            }
            .serialize(serializer)
        }
    }

    impl<'de, T: Egg> Deserialize<'de> for Snapshot<T>
    where
        T::Snapshot: Deserialize<'de>,
    {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            use serde::de::Error;
            let envelope = Envelope::<T::Snapshot>::deserialize(deserializer)?;
            check_header::<T>(&Header {
                eggs_format: envelope.eggs_format,
                schema_version: envelope.schema_version,
            })
            .map_err(D::Error::custom)?;
            let selection = envelope
                .selection
                .map(|paths| paths.iter().map(|p| EggPath::parse(p)).collect::<Result<Vec<_>, _>>())
                .transpose()
                .map_err(D::Error::custom)?;
            Ok(Snapshot {
                id: super::SnapshotId::next(),
                data: envelope.data,
                selection,
                metadata: None,
            })
        }
    }
}

#[cfg(feature = "json")]
impl<T: Egg> Snapshot<T> {
    /// Serializes to JSON in a versioned envelope (`eggs_format`, `schema_version`, `data`).
    ///
    /// Metadata and the process-local ID are not serialized. Debug output is never used as a
    /// persistence format.
    ///
    /// # Errors
    ///
    /// [`EggError::Serialization`] on failure.
    pub fn to_json(&self) -> EggResult<String>
    where
        T::Snapshot: serde::Serialize,
    {
        serde_json::to_string(self).map_err(|e| EggError::Serialization(e.to_string()))
    }

    /// Deserializes JSON produced by [`Snapshot::to_json`].
    ///
    /// The format and schema versions are checked before the data is decoded. The decoded
    /// snapshot receives a new [`SnapshotId`].
    ///
    /// # Security
    ///
    /// JSON is parsed with `serde_json`, which is memory-safe, but a snapshot is still
    /// *data that overwrites application state* when restored. Do not restore snapshots from
    /// untrusted sources without validating them, and apply your own size limits to the input.
    ///
    /// # Errors
    ///
    /// [`EggError::IncompatibleSchema`] for a different format or schema version;
    /// [`EggError::Deserialization`] for malformed input.
    pub fn from_json<'de>(json: &'de str) -> EggResult<Self>
    where
        T::Snapshot: serde::Deserialize<'de>,
    {
        let header: serde_impl::Header =
            serde_json::from_str(json).map_err(|e| EggError::Deserialization(e.to_string()))?;
        serde_impl::check_header::<T>(&header)?;
        serde_json::from_str(json).map_err(|e| EggError::Deserialization(e.to_string()))
    }
}
