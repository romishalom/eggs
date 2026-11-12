//! Error type shared by every Eggs operation.

use std::fmt;

use crate::path::PathSegment;

/// Result alias used throughout the crate.
pub type EggResult<T> = Result<T, EggError>;

/// Everything that can go wrong in Eggs.
///
/// Eggs returns errors instead of silently performing partial operations.
/// Variants that concern a location carry the rendered [`EggPath`](crate::EggPath).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EggError {
    /// A path (or field label) does not exist or cannot be addressed.
    InvalidFieldPath {
        /// The offending path, rendered with [`EggPath`](crate::EggPath)'s syntax.
        path: String,
        /// Why it was rejected.
        reason: String,
    },
    /// A snapshot, patch or serialized document does not match the target schema.
    IncompatibleSchema {
        /// What the operation expected.
        expected: String,
        /// What it found instead.
        found: String,
    },
    /// The type or operation does not support the request.
    UnsupportedOperation {
        /// The operation that was attempted.
        operation: String,
        /// Why it is unsupported.
        reason: String,
    },
    /// Restoring a snapshot into a target failed.
    RestoreFailed {
        /// Where the failure happened, if known.
        path: Option<String>,
        /// What went wrong.
        reason: String,
    },
    /// A patch does not match the current state of the target.
    PatchConflict {
        /// Where the conflict was detected.
        path: String,
        /// What conflicted.
        reason: String,
    },
    /// Serializing a snapshot failed.
    Serialization(String),
    /// Deserializing a snapshot failed.
    Deserialization(String),
    /// Snapshot data is internally inconsistent.
    CorruptSnapshot(String),
    /// A transaction was used in a state that does not allow the operation.
    TransactionState(String),
    /// A snapshot store failed.
    Storage(String),
    /// An argument or internal invariant failed validation.
    Validation(String),
}

impl EggError {
    /// Builds an [`EggError::InvalidFieldPath`].
    pub fn invalid_path(path: impl fmt::Display, reason: impl Into<String>) -> Self {
        EggError::InvalidFieldPath { path: path.to_string(), reason: reason.into() }
    }

    /// Builds an [`EggError::IncompatibleSchema`].
    pub fn incompatible(expected: impl Into<String>, found: impl Into<String>) -> Self {
        EggError::IncompatibleSchema { expected: expected.into(), found: found.into() }
    }

    /// Builds an [`EggError::UnsupportedOperation`].
    pub fn unsupported(operation: impl Into<String>, reason: impl Into<String>) -> Self {
        EggError::UnsupportedOperation { operation: operation.into(), reason: reason.into() }
    }

    /// Builds an [`EggError::RestoreFailed`] without a path.
    pub fn restore_failed(reason: impl Into<String>) -> Self {
        EggError::RestoreFailed { path: None, reason: reason.into() }
    }

    /// Builds an [`EggError::PatchConflict`].
    pub fn conflict(path: impl fmt::Display, reason: impl Into<String>) -> Self {
        EggError::PatchConflict { path: path.to_string(), reason: reason.into() }
    }

    /// Builds an [`EggError::Validation`].
    pub fn validation(reason: impl Into<String>) -> Self {
        EggError::Validation(reason.into())
    }

    /// Prepends `segment` to the path carried by this error, if it carries one.
    ///
    /// Nested [`Egg`](crate::Egg) implementations use this so that errors raised deep inside
    /// a value report the full path from the value they were called on.
    #[must_use]
    pub fn within(self, segment: &PathSegment) -> Self {
        let join = |rest: &str| crate::path::join_display(&segment.to_string(), rest);
        match self {
            EggError::InvalidFieldPath { path, reason } => {
                EggError::InvalidFieldPath { path: join(&path), reason }
            }
            EggError::PatchConflict { path, reason } => {
                EggError::PatchConflict { path: join(&path), reason }
            }
            EggError::RestoreFailed { path, reason } => EggError::RestoreFailed {
                path: Some(join(path.as_deref().unwrap_or(""))),
                reason,
            },
            other => other,
        }
    }
}

impl fmt::Display for EggError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EggError::InvalidFieldPath { path, reason } => {
                write!(f, "invalid field path `{path}`: {reason}")
            }
            EggError::IncompatibleSchema { expected, found } => {
                write!(f, "incompatible schema: expected {expected}, found {found}")
            }
            EggError::UnsupportedOperation { operation, reason } => {
                write!(f, "unsupported operation `{operation}`: {reason}")
            }
            EggError::RestoreFailed { path: Some(path), reason } => {
                write!(f, "restore failed at `{path}`: {reason}")
            }
            EggError::RestoreFailed { path: None, reason } => write!(f, "restore failed: {reason}"),
            EggError::PatchConflict { path, reason } => {
                write!(f, "patch conflict at `{path}`: {reason}")
            }
            EggError::Serialization(m) => write!(f, "serialization failed: {m}"),
            EggError::Deserialization(m) => write!(f, "deserialization failed: {m}"),
            EggError::CorruptSnapshot(m) => write!(f, "corrupt snapshot: {m}"),
            EggError::TransactionState(m) => write!(f, "invalid transaction state: {m}"),
            EggError::Storage(m) => write!(f, "storage error: {m}"),
            EggError::Validation(m) => write!(f, "validation failed: {m}"),
        }
    }
}

impl std::error::Error for EggError {}
