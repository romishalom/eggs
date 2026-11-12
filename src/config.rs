//! Configuration types.

use std::time::SystemTime;

/// Default number of entries a [`History`](crate::History) retains.
pub const DEFAULT_HISTORY_LIMIT: usize = 128;

/// How a snapshot stores captured state.
///
/// Only [`Strategy::Clone`] exists today: every capture deep-clones the participating
/// state. The enum is `#[non_exhaustive]` so shared/copy-on-write strategies can be added
/// after benchmarks justify them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Strategy {
    /// Deep-clone the participating state.
    #[default]
    Clone,
}

/// Library-wide defaults used by [`History::with_config`](crate::History::with_config).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EggConfig {
    history_limit: usize,
    strategy: Strategy,
    metadata: bool,
}

impl Default for EggConfig {
    fn default() -> Self {
        EggConfig { history_limit: DEFAULT_HISTORY_LIMIT, strategy: Strategy::Clone, metadata: false }
    }
}

impl EggConfig {
    /// The default configuration (128-entry history, clone strategy, no metadata).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the history retention limit. A limit of `0` is rejected when a history is built.
    #[must_use]
    pub fn with_history_limit(mut self, limit: usize) -> Self {
        self.history_limit = limit;
        self
    }

    /// Selects the snapshot strategy.
    #[must_use]
    pub fn with_strategy(mut self, strategy: Strategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Whether snapshots recorded by histories carry [`EggMetadata`].
    #[must_use]
    pub fn with_metadata(mut self, metadata: bool) -> Self {
        self.metadata = metadata;
        self
    }

    /// The history retention limit.
    #[must_use]
    pub fn history_limit(&self) -> usize {
        self.history_limit
    }

    /// The snapshot strategy.
    #[must_use]
    pub fn strategy(&self) -> Strategy {
        self.strategy
    }

    /// Whether snapshots carry metadata.
    #[must_use]
    pub fn metadata(&self) -> bool {
        self.metadata
    }
}

/// Options for one capture; built through [`SnapshotBuilder`](crate::SnapshotBuilder).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapshotOptions {
    pub(crate) include: Vec<String>,
    pub(crate) exclude: Vec<String>,
    pub(crate) metadata: bool,
    pub(crate) label: Option<String>,
    pub(crate) capacity: Option<usize>,
    pub(crate) strategy: Strategy,
}

impl SnapshotOptions {
    /// Default options: everything included, no metadata.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Paths selected for restore/diff (empty means "all fields").
    #[must_use]
    pub fn included(&self) -> &[String] {
        &self.include
    }

    /// Top-level fields excluded from restore/diff.
    #[must_use]
    pub fn excluded(&self) -> &[String] {
        &self.exclude
    }

    /// Whether metadata is attached.
    #[must_use]
    pub fn metadata(&self) -> bool {
        self.metadata
    }

    /// The capacity hint, if one was given. The clone strategy currently ignores it.
    #[must_use]
    pub fn capacity(&self) -> Option<usize> {
        self.capacity
    }

    /// The snapshot strategy.
    #[must_use]
    pub fn strategy(&self) -> Strategy {
        self.strategy
    }
}

/// Descriptive data attached to a snapshot when metadata is requested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EggMetadata {
    pub(crate) created_at: SystemTime,
    pub(crate) label: Option<String>,
    pub(crate) schema_version: u32,
}

impl EggMetadata {
    /// When the snapshot was captured (wall-clock time; not monotonic).
    #[must_use]
    pub fn created_at(&self) -> SystemTime {
        self.created_at
    }

    /// An optional human-readable label.
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// The captured type's [`Egg::schema_version`](crate::Egg::schema_version).
    #[must_use]
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }
}
