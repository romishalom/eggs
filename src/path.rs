//! Structured paths that address a location inside a value.

use std::fmt;
use std::str::FromStr;

use crate::error::{EggError, EggResult};

/// One step of an [`EggPath`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PathSegment {
    /// A struct field (or tuple position such as `0`), by label.
    Field(String),
    /// A sequence or array index.
    Index(usize),
    /// A map key or set element, identified by its `Debug` rendering.
    Key(String),
}

const FIELD_SPECIAL: &str = "\\.[]{}";
const KEY_SPECIAL: &str = "\\{}";

fn escape(s: &str, special: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    for c in s.chars() {
        if special.contains(c) {
            f.write_str("\\")?;
        }
        write!(f, "{c}")?;
    }
    Ok(())
}

impl fmt::Display for PathSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathSegment::Field(name) => escape(name, FIELD_SPECIAL, f),
            PathSegment::Index(i) => write!(f, "[{i}]"),
            PathSegment::Key(key) => {
                f.write_str("{")?;
                escape(key, KEY_SPECIAL, f)?;
                f.write_str("}")
            }
        }
    }
}

/// Joins an already-rendered prefix segment and an already-rendered remainder.
pub(crate) fn join_display(prefix: &str, rest: &str) -> String {
    if rest.is_empty() {
        prefix.to_string()
    } else if rest.starts_with('[') || rest.starts_with('{') {
        format!("{prefix}{rest}")
    } else {
        format!("{prefix}.{rest}")
    }
}

/// A structured path such as `profile.address.city`, `items[3]` or `scores{"ann"}`.
///
/// # Syntax
///
/// * fields are separated by `.`;
/// * sequence indices are written `[3]`;
/// * map keys and set elements are written `{key}`, where `key` is the `Debug`
///   rendering of the key (so a string key `"ann"` is `{"ann"}`);
/// * the characters `\ . [ ] { }` inside a field label are escaped with `\`, and
///   `\ { }` inside a key.
///
/// The root path is the empty string. Rendering and [`EggPath::parse`] round-trip.
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EggPath(Vec<PathSegment>);

impl EggPath {
    /// The empty path that addresses the whole value.
    #[must_use]
    pub fn root() -> Self {
        EggPath(Vec::new())
    }

    /// Builds a path from segments.
    #[must_use]
    pub fn from_segments(segments: Vec<PathSegment>) -> Self {
        EggPath(segments)
    }

    /// The segments, outermost first.
    #[must_use]
    pub fn segments(&self) -> &[PathSegment] {
        &self.0
    }

    /// Whether this is the root path.
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of segments.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Alias of [`EggPath::is_root`].
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns this path extended by `segment`.
    #[must_use]
    pub fn child(&self, segment: PathSegment) -> EggPath {
        let mut segments = Vec::with_capacity(self.0.len() + 1);
        segments.extend(self.0.iter().cloned());
        segments.push(segment);
        EggPath(segments)
    }

    /// Returns this path extended by a field label.
    #[must_use]
    pub fn child_field(&self, name: &str) -> EggPath {
        self.child(PathSegment::Field(name.to_string()))
    }

    /// Returns this path extended by an index.
    #[must_use]
    pub fn child_index(&self, index: usize) -> EggPath {
        self.child(PathSegment::Index(index))
    }

    /// Returns this path extended by a map-key / set-element segment.
    #[must_use]
    pub fn child_key(&self, key: impl Into<String>) -> EggPath {
        self.child(PathSegment::Key(key.into()))
    }

    /// The path without its last segment, or `None` for the root.
    #[must_use]
    pub fn parent(&self) -> Option<EggPath> {
        self.0.split_last().map(|(_, rest)| EggPath(rest.to_vec()))
    }

    /// Whether `prefix` equals this path or is an ancestor of it.
    #[must_use]
    pub fn starts_with(&self, prefix: &EggPath) -> bool {
        self.0.starts_with(&prefix.0)
    }

    /// The remainder of this path after `prefix`, if `prefix` is a prefix.
    #[must_use]
    pub fn strip_prefix(&self, prefix: &EggPath) -> Option<EggPath> {
        self.starts_with(prefix).then(|| EggPath(self.0[prefix.0.len()..].to_vec()))
    }

    /// Parses the syntax described on [`EggPath`].
    ///
    /// # Errors
    ///
    /// Returns [`EggError::InvalidFieldPath`] for malformed input.
    pub fn parse(input: &str) -> EggResult<EggPath> {
        let bad = |reason: &str| EggError::InvalidFieldPath {
            path: input.to_string(),
            reason: reason.to_string(),
        };
        let chars: Vec<char> = input.chars().collect();
        let mut i = 0;
        let mut segments = Vec::new();

        while i < chars.len() {
            match chars[i] {
                '.' => {
                    if segments.is_empty() {
                        return Err(bad("path cannot start with `.`"));
                    }
                    i += 1;
                    let (name, next) = parse_field(&chars, i).map_err(|r| bad(r))?;
                    segments.push(PathSegment::Field(name));
                    i = next;
                }
                '[' => {
                    let close = chars[i..]
                        .iter()
                        .position(|&c| c == ']')
                        .ok_or_else(|| bad("unterminated `[`"))?;
                    let digits: String = chars[i + 1..i + close].iter().collect();
                    let index = digits
                        .parse::<usize>()
                        .map_err(|_| bad("index must be a non-negative integer"))?;
                    segments.push(PathSegment::Index(index));
                    i += close + 1;
                }
                '{' => {
                    let mut key = String::new();
                    let mut j = i + 1;
                    loop {
                        match chars.get(j) {
                            None => return Err(bad("unterminated `{`")),
                            Some('\\') => {
                                let c = chars.get(j + 1).ok_or_else(|| bad("dangling `\\`"))?;
                                key.push(*c);
                                j += 2;
                            }
                            Some('}') => break,
                            Some('{') => return Err(bad("unescaped `{` inside key")),
                            Some(c) => {
                                key.push(*c);
                                j += 1;
                            }
                        }
                    }
                    segments.push(PathSegment::Key(key));
                    i = j + 1;
                }
                _ if segments.is_empty() => {
                    let (name, next) = parse_field(&chars, i).map_err(|r| bad(r))?;
                    segments.push(PathSegment::Field(name));
                    i = next;
                }
                _ => return Err(bad("expected `.`, `[` or `{` between segments")),
            }
        }
        Ok(EggPath(segments))
    }
}

fn parse_field(chars: &[char], mut i: usize) -> Result<(String, usize), &'static str> {
    let mut name = String::new();
    while let Some(&c) = chars.get(i) {
        match c {
            '\\' => {
                let next = chars.get(i + 1).ok_or("dangling `\\`")?;
                name.push(*next);
                i += 2;
            }
            '.' | '[' | '{' => break,
            ']' | '}' => return Err("unescaped `]` or `}` in field label"),
            _ => {
                name.push(c);
                i += 1;
            }
        }
    }
    if name.is_empty() {
        Err("empty field label")
    } else {
        Ok((name, i))
    }
}

impl fmt::Display for EggPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, segment) in self.0.iter().enumerate() {
            if i > 0 && matches!(segment, PathSegment::Field(_)) {
                f.write_str(".")?;
            }
            write!(f, "{segment}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for EggPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EggPath({:?})", self.to_string())
    }
}

impl FromStr for EggPath {
    type Err = EggError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        EggPath::parse(s)
    }
}

/// Conversion into an [`EggPath`] that accepts structured paths and strings.
///
/// Implemented for [`EggPath`], `&EggPath`, `str`, `String` and references to them,
/// so most path-taking methods accept `"profile.address.city"` directly.
pub trait ToEggPath {
    /// Converts to a structured path.
    ///
    /// # Errors
    ///
    /// Returns [`EggError::InvalidFieldPath`] if a string path is malformed.
    fn to_egg_path(&self) -> EggResult<EggPath>;
}

impl ToEggPath for EggPath {
    fn to_egg_path(&self) -> EggResult<EggPath> {
        Ok(self.clone())
    }
}

impl ToEggPath for str {
    fn to_egg_path(&self) -> EggResult<EggPath> {
        EggPath::parse(self)
    }
}

impl ToEggPath for String {
    fn to_egg_path(&self) -> EggResult<EggPath> {
        EggPath::parse(self)
    }
}

impl<T: ToEggPath + ?Sized> ToEggPath for &T {
    fn to_egg_path(&self) -> EggResult<EggPath> {
        (**self).to_egg_path()
    }
}

#[cfg(feature = "serde")]
mod serde_impl {
    use super::EggPath;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    impl Serialize for EggPath {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            s.collect_str(self)
        }
    }

    impl<'de> Deserialize<'de> for EggPath {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let s = String::deserialize(d)?;
            EggPath::parse(&s).map_err(serde::de::Error::custom)
        }
    }
}
