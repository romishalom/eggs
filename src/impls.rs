//! [`Egg`] implementations for standard types.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Debug;
use std::mem::size_of;

use crate::diff::{Change, ChangeKind};
use crate::error::{EggError, EggResult};
use crate::path::{EggPath, PathSegment};
use crate::traits::{apply_whole, downcast_value, key_string, render, Egg};
use crate::value::{SnapshotData, Value};

/// Payload of `Added`/`Removed` changes on map entries: the key plus the entry's value snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct MapEntry<K, S> {
    /// The map key.
    pub key: K,
    /// The snapshot of the entry's value.
    pub snapshot: S,
}

crate::impl_egg_for_value!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64, bool, char, (),
    &'static str
);

impl Egg for String {
    type Snapshot = String;

    fn snapshot(&self) -> Self::Snapshot {
        self.clone()
    }

    fn restore_in_place(&mut self, snapshot: &Self::Snapshot) -> EggResult<()> {
        self.clone_from(snapshot);
        Ok(())
    }

    fn from_snapshot(snapshot: &Self::Snapshot) -> EggResult<Self> {
        Ok(snapshot.clone())
    }

    fn diff_into(&self, other: &Self, path: &EggPath, out: &mut Vec<Change>) {
        if self != other {
            out.push(Change::modified(path.clone(), Value::new(self.clone()), Value::new(other.clone())));
        }
    }

    fn snapshot_heap_size(snapshot: &Self::Snapshot) -> usize {
        snapshot.len()
    }
}

// ---------------------------------------------------------------------------------------
// Box, Option, Result
// ---------------------------------------------------------------------------------------

impl<T: Egg> Egg for Box<T> {
    type Snapshot = T::Snapshot;

    fn snapshot(&self) -> Self::Snapshot {
        (**self).snapshot()
    }

    fn restore_in_place(&mut self, snapshot: &Self::Snapshot) -> EggResult<()> {
        (**self).restore_in_place(snapshot)
    }

    fn from_snapshot(snapshot: &Self::Snapshot) -> EggResult<Self> {
        T::from_snapshot(snapshot).map(Box::new)
    }

    fn diff_into(&self, other: &Self, path: &EggPath, out: &mut Vec<Change>) {
        (**self).diff_into(other, path, out);
    }

    fn fields() -> &'static [&'static str] {
        T::fields()
    }

    fn schema_version() -> u32 {
        T::schema_version()
    }

    fn restore_path(&mut self, path: &[PathSegment], snapshot: &Self::Snapshot) -> EggResult<()> {
        (**self).restore_path(path, snapshot)
    }

    fn apply_change(&mut self, path: &[PathSegment], change: &Change) -> EggResult<()> {
        (**self).apply_change(path, change)
    }

    fn snapshot_is_empty(snapshot: &Self::Snapshot) -> bool {
        T::snapshot_is_empty(snapshot)
    }

    fn snapshot_heap_size(snapshot: &Self::Snapshot) -> usize {
        T::snapshot_heap_size(snapshot)
    }
}

impl<T: Egg> Egg for Option<T> {
    type Snapshot = Option<T::Snapshot>;

    fn snapshot(&self) -> Self::Snapshot {
        self.as_ref().map(T::snapshot)
    }

    fn restore_in_place(&mut self, snapshot: &Self::Snapshot) -> EggResult<()> {
        match snapshot {
            None => {
                *self = None;
                Ok(())
            }
            Some(s) => match self {
                Some(t) => t.restore_in_place(s),
                None => {
                    *self = Some(T::from_snapshot(s)?);
                    Ok(())
                }
            },
        }
    }

    fn from_snapshot(snapshot: &Self::Snapshot) -> EggResult<Self> {
        snapshot.as_ref().map(T::from_snapshot).transpose()
    }

    fn diff_into(&self, other: &Self, path: &EggPath, out: &mut Vec<Change>) {
        match (self, other) {
            (Some(a), Some(b)) => a.diff_into(b, path, out),
            (None, None) => {}
            _ => out.push(Change::variant_changed(
                path.clone(),
                Value::new(self.snapshot()),
                Value::new(other.snapshot()),
            )),
        }
    }

    fn fields() -> &'static [&'static str] {
        T::fields()
    }

    fn restore_path(&mut self, path: &[PathSegment], snapshot: &Self::Snapshot) -> EggResult<()> {
        if path.is_empty() {
            return self.restore_in_place(snapshot);
        }
        match (self.as_mut(), snapshot) {
            (Some(t), Some(s)) => t.restore_path(path, s),
            _ => Err(EggError::invalid_path(render(path), "value is `None` in the target or the snapshot")),
        }
    }

    fn apply_change(&mut self, path: &[PathSegment], change: &Change) -> EggResult<()> {
        if path.is_empty() {
            return apply_whole(self, change);
        }
        match self {
            Some(t) => t.apply_change(path, change),
            None => Err(EggError::conflict(&change.path, "value is `None`, expected `Some`")),
        }
    }

    fn snapshot_is_empty(snapshot: &Self::Snapshot) -> bool {
        snapshot.is_none()
    }

    fn snapshot_heap_size(snapshot: &Self::Snapshot) -> usize {
        snapshot.as_ref().map_or(0, T::snapshot_heap_size)
    }
}

impl<T: Egg, E: Egg> Egg for Result<T, E> {
    type Snapshot = Result<T::Snapshot, E::Snapshot>;

    fn snapshot(&self) -> Self::Snapshot {
        match self {
            Ok(t) => Ok(t.snapshot()),
            Err(e) => Err(e.snapshot()),
        }
    }

    fn restore_in_place(&mut self, snapshot: &Self::Snapshot) -> EggResult<()> {
        match (&mut *self, snapshot) {
            (Ok(t), Ok(s)) => t.restore_in_place(s),
            (Err(e), Err(s)) => e.restore_in_place(s),
            _ => {
                *self = Self::from_snapshot(snapshot)?;
                Ok(())
            }
        }
    }

    fn from_snapshot(snapshot: &Self::Snapshot) -> EggResult<Self> {
        Ok(match snapshot {
            Ok(s) => Ok(T::from_snapshot(s)?),
            Err(s) => Err(E::from_snapshot(s)?),
        })
    }

    fn diff_into(&self, other: &Self, path: &EggPath, out: &mut Vec<Change>) {
        match (self, other) {
            (Ok(a), Ok(b)) => a.diff_into(b, path, out),
            (Err(a), Err(b)) => a.diff_into(b, path, out),
            _ => out.push(Change::variant_changed(
                path.clone(),
                Value::new(self.snapshot()),
                Value::new(other.snapshot()),
            )),
        }
    }

    fn restore_path(&mut self, path: &[PathSegment], snapshot: &Self::Snapshot) -> EggResult<()> {
        if path.is_empty() {
            return self.restore_in_place(snapshot);
        }
        match (self, snapshot) {
            (Ok(t), Ok(s)) => t.restore_path(path, s),
            (Err(e), Err(s)) => e.restore_path(path, s),
            _ => Err(EggError::invalid_path(render(path), "target and snapshot hold different variants")),
        }
    }

    fn apply_change(&mut self, path: &[PathSegment], change: &Change) -> EggResult<()> {
        if path.is_empty() {
            return apply_whole(self, change);
        }
        match self {
            Ok(t) => t.apply_change(path, change),
            Err(e) => e.apply_change(path, change),
        }
    }

    fn snapshot_heap_size(snapshot: &Self::Snapshot) -> usize {
        match snapshot {
            Ok(s) => T::snapshot_heap_size(s),
            Err(s) => E::snapshot_heap_size(s),
        }
    }
}

// ---------------------------------------------------------------------------------------
// Sequences
// ---------------------------------------------------------------------------------------

impl<T: Egg> Egg for Vec<T> {
    type Snapshot = Vec<T::Snapshot>;

    fn snapshot(&self) -> Self::Snapshot {
        self.iter().map(T::snapshot).collect()
    }

    fn restore_in_place(&mut self, snapshot: &Self::Snapshot) -> EggResult<()> {
        let common = self.len().min(snapshot.len());
        for (i, (t, s)) in self.iter_mut().zip(snapshot).enumerate() {
            t.restore_in_place(s).map_err(|e| e.within(&PathSegment::Index(i)))?;
        }
        self.truncate(snapshot.len());
        for (i, s) in snapshot.iter().enumerate().skip(common) {
            self.push(T::from_snapshot(s).map_err(|e| e.within(&PathSegment::Index(i)))?);
        }
        Ok(())
    }

    fn from_snapshot(snapshot: &Self::Snapshot) -> EggResult<Self> {
        snapshot.iter().map(T::from_snapshot).collect()
    }

    fn diff_into(&self, other: &Self, path: &EggPath, out: &mut Vec<Change>) {
        let common = self.len().min(other.len());
        for i in 0..common {
            self[i].diff_into(&other[i], &path.child_index(i), out);
        }
        for i in (common..self.len()).rev() {
            out.push(Change::removed(path.child_index(i), Value::new(self[i].snapshot())));
        }
        for i in common..other.len() {
            out.push(Change::added(path.child_index(i), Value::new(other[i].snapshot())));
        }
    }

    fn fields() -> &'static [&'static str] {
        T::fields()
    }

    fn restore_path(&mut self, path: &[PathSegment], snapshot: &Self::Snapshot) -> EggResult<()> {
        let Some((first, rest)) = path.split_first() else {
            return self.restore_in_place(snapshot);
        };
        let PathSegment::Index(i) = first else {
            return Err(EggError::invalid_path(render(path), "sequence paths use `[index]` segments"));
        };
        match (self.get_mut(*i), snapshot.get(*i)) {
            (Some(t), Some(s)) => t.restore_path(rest, s).map_err(|e| e.within(first)),
            _ => Err(EggError::invalid_path(render(path), "index is out of bounds in the target or the snapshot")),
        }
    }

    fn apply_change(&mut self, path: &[PathSegment], change: &Change) -> EggResult<()> {
        let Some((first, rest)) = path.split_first() else {
            return apply_whole(self, change);
        };
        let PathSegment::Index(i) = first else {
            return Err(EggError::invalid_path(&change.path, "sequence paths use `[index]` segments"));
        };
        let i = *i;
        let len = self.len();
        if rest.is_empty() && change.kind == ChangeKind::Added {
            if i != len {
                return Err(EggError::conflict(
                    &change.path,
                    format!("cannot append at index {i}: the sequence has {len} elements"),
                ));
            }
            let snapshot = downcast_value::<T::Snapshot>(change, change.after())?;
            self.push(T::from_snapshot(snapshot)?);
            return Ok(());
        }
        let element = self.get_mut(i).ok_or_else(|| {
            EggError::conflict(&change.path, format!("index {i} is out of bounds (length {len})"))
        })?;
        if rest.is_empty() && change.kind == ChangeKind::Removed {
            let before = downcast_value::<T::Snapshot>(change, change.before())?;
            if element.snapshot() != *before {
                return Err(EggError::conflict(
                    &change.path,
                    "element differs from the value the patch expects to remove",
                ));
            }
            self.remove(i);
            return Ok(());
        }
        element.apply_change(rest, change)
    }

    fn snapshot_is_empty(snapshot: &Self::Snapshot) -> bool {
        snapshot.is_empty()
    }

    fn snapshot_heap_size(snapshot: &Self::Snapshot) -> usize {
        snapshot.len() * size_of::<T::Snapshot>()
            + snapshot.iter().map(T::snapshot_heap_size).sum::<usize>()
    }
}

impl<T: Egg, const N: usize> Egg for [T; N] {
    type Snapshot = [T::Snapshot; N];

    fn snapshot(&self) -> Self::Snapshot {
        std::array::from_fn(|i| self[i].snapshot())
    }

    fn restore_in_place(&mut self, snapshot: &Self::Snapshot) -> EggResult<()> {
        for (i, (t, s)) in self.iter_mut().zip(snapshot).enumerate() {
            t.restore_in_place(s).map_err(|e| e.within(&PathSegment::Index(i)))?;
        }
        Ok(())
    }

    fn from_snapshot(snapshot: &Self::Snapshot) -> EggResult<Self> {
        let items: Vec<T> = snapshot.iter().map(T::from_snapshot).collect::<EggResult<_>>()?;
        items.try_into().map_err(|_| EggError::CorruptSnapshot("array length mismatch".into()))
    }

    fn diff_into(&self, other: &Self, path: &EggPath, out: &mut Vec<Change>) {
        for i in 0..N {
            self[i].diff_into(&other[i], &path.child_index(i), out);
        }
    }

    fn fields() -> &'static [&'static str] {
        T::fields()
    }

    fn restore_path(&mut self, path: &[PathSegment], snapshot: &Self::Snapshot) -> EggResult<()> {
        let Some((first, rest)) = path.split_first() else {
            return self.restore_in_place(snapshot);
        };
        match first {
            PathSegment::Index(i) if *i < N => {
                self[*i].restore_path(rest, &snapshot[*i]).map_err(|e| e.within(first))
            }
            _ => Err(EggError::invalid_path(render(path), "no such array index")),
        }
    }

    fn apply_change(&mut self, path: &[PathSegment], change: &Change) -> EggResult<()> {
        let Some((first, rest)) = path.split_first() else {
            return apply_whole(self, change);
        };
        match first {
            PathSegment::Index(i) if *i < N => self[*i].apply_change(rest, change),
            _ => Err(EggError::invalid_path(&change.path, "no such array index")),
        }
    }

    fn snapshot_heap_size(snapshot: &Self::Snapshot) -> usize {
        snapshot.iter().map(T::snapshot_heap_size).sum()
    }
}

// ---------------------------------------------------------------------------------------
// Tuples
// ---------------------------------------------------------------------------------------

macro_rules! impl_egg_tuple {
    ($( ($($T:ident $idx:tt),+) )+) => {$(
        impl<$($T: Egg),+> Egg for ($($T,)+) {
            type Snapshot = ($($T::Snapshot,)+);

            fn snapshot(&self) -> Self::Snapshot {
                ($(self.$idx.snapshot(),)+)
            }

            fn restore_in_place(&mut self, snapshot: &Self::Snapshot) -> EggResult<()> {
                $(
                    self.$idx
                        .restore_in_place(&snapshot.$idx)
                        .map_err(|e| e.within(&PathSegment::Field(stringify!($idx).to_string())))?;
                )+
                Ok(())
            }

            fn from_snapshot(snapshot: &Self::Snapshot) -> EggResult<Self> {
                Ok(($($T::from_snapshot(&snapshot.$idx)?,)+))
            }

            fn diff_into(&self, other: &Self, path: &EggPath, out: &mut Vec<Change>) {
                $( self.$idx.diff_into(&other.$idx, &path.child_field(stringify!($idx)), out); )+
            }

            fn fields() -> &'static [&'static str] {
                &[$(stringify!($idx)),+]
            }

            fn restore_path(&mut self, path: &[PathSegment], snapshot: &Self::Snapshot) -> EggResult<()> {
                let Some((first, rest)) = path.split_first() else {
                    return self.restore_in_place(snapshot);
                };
                if let PathSegment::Field(name) = first {
                    $(
                        if name == stringify!($idx) {
                            return self.$idx
                                .restore_path(rest, &snapshot.$idx)
                                .map_err(|e| e.within(first));
                        }
                    )+
                }
                Err(EggError::invalid_path(render(path), "no such tuple position"))
            }

            fn apply_change(&mut self, path: &[PathSegment], change: &Change) -> EggResult<()> {
                let Some((first, rest)) = path.split_first() else {
                    return apply_whole(self, change);
                };
                if let PathSegment::Field(name) = first {
                    $(
                        if name == stringify!($idx) {
                            return self.$idx.apply_change(rest, change);
                        }
                    )+
                }
                Err(EggError::invalid_path(&change.path, "no such tuple position"))
            }

            fn snapshot_heap_size(snapshot: &Self::Snapshot) -> usize {
                0 $( + $T::snapshot_heap_size(&snapshot.$idx) )+
            }
        }
    )+};
}

impl_egg_tuple! {
    (A 0)
    (A 0, B 1)
    (A 0, B 1, C 2)
    (A 0, B 1, C 2, D 3)
    (A 0, B 1, C 2, D 3, E 4)
    (A 0, B 1, C 2, D 3, E 4, F 5)
}

// ---------------------------------------------------------------------------------------
// Maps
// ---------------------------------------------------------------------------------------

fn find_key<'a, K: Debug>(mut keys: impl Iterator<Item = &'a K>, wanted: &str) -> Option<&'a K> {
    keys.find(|k| key_string(*k) == wanted)
}

macro_rules! impl_egg_map {
    ($map:ident, $($bound:tt)+) => {
        impl<K, V> Egg for $map<K, V>
        where
            K: SnapshotData + $($bound)+,
            V: Egg,
        {
            type Snapshot = $map<K, V::Snapshot>;

            fn snapshot(&self) -> Self::Snapshot {
                self.iter().map(|(k, v)| (k.clone(), v.snapshot())).collect()
            }

            fn restore_in_place(&mut self, snapshot: &Self::Snapshot) -> EggResult<()> {
                self.retain(|k, _| snapshot.contains_key(k));
                for (k, s) in snapshot {
                    match self.get_mut(k) {
                        Some(v) => v
                            .restore_in_place(s)
                            .map_err(|e| e.within(&PathSegment::Key(key_string(k))))?,
                        None => {
                            let value = V::from_snapshot(s)
                                .map_err(|e| e.within(&PathSegment::Key(key_string(k))))?;
                            self.insert(k.clone(), value);
                        }
                    }
                }
                Ok(())
            }

            fn from_snapshot(snapshot: &Self::Snapshot) -> EggResult<Self> {
                snapshot
                    .iter()
                    .map(|(k, s)| Ok((k.clone(), V::from_snapshot(s)?)))
                    .collect()
            }

            fn diff_into(&self, other: &Self, path: &EggPath, out: &mut Vec<Change>) {
                let mut rows: BTreeMap<String, (&K, Option<&V>, Option<&V>)> = BTreeMap::new();
                for (k, v) in self.iter() {
                    rows.insert(key_string(k), (k, Some(v), None));
                }
                for (k, v) in other.iter() {
                    rows.entry(key_string(k))
                        .and_modify(|row| row.2 = Some(v))
                        .or_insert((k, None, Some(v)));
                }
                for (key_text, (key, before, after)) in rows {
                    let child = path.child_key(key_text);
                    match (before, after) {
                        (Some(a), Some(b)) => a.diff_into(b, &child, out),
                        (Some(a), None) => out.push(Change::removed(
                            child,
                            Value::new(MapEntry { key: key.clone(), snapshot: a.snapshot() }),
                        )),
                        (None, Some(b)) => out.push(Change::added(
                            child,
                            Value::new(MapEntry { key: key.clone(), snapshot: b.snapshot() }),
                        )),
                        (None, None) => {}
                    }
                }
            }

            fn fields() -> &'static [&'static str] {
                V::fields()
            }

            fn restore_path(&mut self, path: &[PathSegment], snapshot: &Self::Snapshot) -> EggResult<()> {
                let Some((first, rest)) = path.split_first() else {
                    return self.restore_in_place(snapshot);
                };
                let PathSegment::Key(text) = first else {
                    return Err(EggError::invalid_path(render(path), "map paths use `{key}` segments"));
                };
                let key = find_key(self.keys(), text).cloned();
                match key {
                    Some(key) => match (self.get_mut(&key), snapshot.get(&key)) {
                        (Some(v), Some(s)) => v.restore_path(rest, s).map_err(|e| e.within(first)),
                        _ => Err(EggError::invalid_path(render(path), "key is missing from the snapshot")),
                    },
                    None => Err(EggError::invalid_path(render(path), "key is missing from the target")),
                }
            }

            fn apply_change(&mut self, path: &[PathSegment], change: &Change) -> EggResult<()> {
                let Some((first, rest)) = path.split_first() else {
                    return apply_whole(self, change);
                };
                let PathSegment::Key(text) = first else {
                    return Err(EggError::invalid_path(&change.path, "map paths use `{key}` segments"));
                };
                if rest.is_empty() && change.kind == ChangeKind::Added {
                    let entry = downcast_value::<MapEntry<K, V::Snapshot>>(change, change.after())?;
                    if self.contains_key(&entry.key) {
                        return Err(EggError::conflict(&change.path, "key already exists"));
                    }
                    let value = V::from_snapshot(&entry.snapshot)?;
                    self.insert(entry.key.clone(), value);
                    return Ok(());
                }
                let key = find_key(self.keys(), text)
                    .cloned()
                    .ok_or_else(|| EggError::conflict(&change.path, "key does not exist"))?;
                let value = self
                    .get_mut(&key)
                    .ok_or_else(|| EggError::conflict(&change.path, "key does not exist"))?;
                if rest.is_empty() && change.kind == ChangeKind::Removed {
                    let entry = downcast_value::<MapEntry<K, V::Snapshot>>(change, change.before())?;
                    if value.snapshot() != entry.snapshot {
                        return Err(EggError::conflict(
                            &change.path,
                            "entry differs from the value the patch expects to remove",
                        ));
                    }
                    self.remove(&key);
                    return Ok(());
                }
                value.apply_change(rest, change)
            }

            fn snapshot_is_empty(snapshot: &Self::Snapshot) -> bool {
                snapshot.is_empty()
            }

            fn snapshot_heap_size(snapshot: &Self::Snapshot) -> usize {
                snapshot.len() * (size_of::<K>() + size_of::<V::Snapshot>())
                    + snapshot.values().map(V::snapshot_heap_size).sum::<usize>()
            }
        }
    };
}

impl_egg_map!(BTreeMap, Ord);
impl_egg_map!(HashMap, Eq + ::std::hash::Hash);

// ---------------------------------------------------------------------------------------
// Sets
// ---------------------------------------------------------------------------------------

macro_rules! impl_egg_set {
    ($set:ident, $($bound:tt)+) => {
        impl<T> Egg for $set<T>
        where
            T: SnapshotData + $($bound)+,
        {
            type Snapshot = $set<T>;

            fn snapshot(&self) -> Self::Snapshot {
                self.clone()
            }

            fn restore_in_place(&mut self, snapshot: &Self::Snapshot) -> EggResult<()> {
                self.clone_from(snapshot);
                Ok(())
            }

            fn from_snapshot(snapshot: &Self::Snapshot) -> EggResult<Self> {
                Ok(snapshot.clone())
            }

            fn diff_into(&self, other: &Self, path: &EggPath, out: &mut Vec<Change>) {
                let mut rows: BTreeMap<String, (&T, bool, bool)> = BTreeMap::new();
                for x in self.iter() {
                    rows.insert(key_string(x), (x, true, false));
                }
                for x in other.iter() {
                    rows.entry(key_string(x)).and_modify(|row| row.2 = true).or_insert((x, false, true));
                }
                for (text, (item, in_before, in_after)) in rows {
                    match (in_before, in_after) {
                        (true, false) => {
                            out.push(Change::removed(path.child_key(text), Value::new(item.clone())));
                        }
                        (false, true) => {
                            out.push(Change::added(path.child_key(text), Value::new(item.clone())));
                        }
                        _ => {}
                    }
                }
            }

            fn apply_change(&mut self, path: &[PathSegment], change: &Change) -> EggResult<()> {
                match path {
                    [] => apply_whole(self, change),
                    [PathSegment::Key(text)] => match change.kind {
                        ChangeKind::Added => {
                            let item = downcast_value::<T>(change, change.after())?;
                            if self.contains(item) {
                                return Err(EggError::conflict(&change.path, "element already exists"));
                            }
                            self.insert(item.clone());
                            Ok(())
                        }
                        ChangeKind::Removed => {
                            let item = downcast_value::<T>(change, change.before())?;
                            if key_string(item) != *text || !self.remove(item) {
                                return Err(EggError::conflict(&change.path, "element does not exist"));
                            }
                            Ok(())
                        }
                        _ => Err(EggError::unsupported(
                            "apply_change",
                            format!("{:?} is not applicable to a set element", change.kind),
                        )),
                    },
                    _ => Err(EggError::invalid_path(&change.path, "set elements have no addressable children")),
                }
            }

            fn snapshot_is_empty(snapshot: &Self::Snapshot) -> bool {
                snapshot.is_empty()
            }

            fn snapshot_heap_size(snapshot: &Self::Snapshot) -> usize {
                snapshot.len() * size_of::<T>()
            }
        }
    };
}

impl_egg_set!(BTreeSet, Ord);
impl_egg_set!(HashSet, Eq + ::std::hash::Hash);
