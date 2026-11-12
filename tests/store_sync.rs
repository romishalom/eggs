#![cfg(all(feature = "store", feature = "sync"))]

use eggs::store::{MemoryStore, SnapshotStore};
use eggs::sync::SharedSnapshot;
use eggs::{Egg, EggExt, Snapshot};

#[derive(Egg, Clone, Debug, PartialEq)]
struct S(u32);

#[test]
fn memory_store_roundtrip() {
    let mut store: MemoryStore<Snapshot<S>> = MemoryStore::new();
    assert!(store.is_empty());
    let ids: Vec<_> = (0..4).map(|i| store.insert(S(i).capture()).unwrap()).collect();
    assert_eq!(store.len(), 4);
    assert_eq!(store.list(), ids);
    let mut target = S(99);
    store.get(&ids[2]).unwrap().unwrap().try_restore(&mut target).unwrap();
    assert_eq!(target, S(2));
    assert!(store.contains(&ids[0]).unwrap());
    assert!(store.remove(&ids[0]).unwrap());
    assert!(!store.remove(&ids[0]).unwrap());
    assert!(store.get(&ids[0]).unwrap().is_none());
    assert_eq!(store.prune_oldest(1), 2);
    assert_eq!(store.len(), 1);
    assert!(store.estimate_size() > 0);
    store.clear();
    assert!(store.is_empty());
}

#[test]
fn shared_snapshots_cross_threads() {
    let shared = SharedSnapshot::capture(&S(7));
    let clone = shared.clone();
    assert!(SharedSnapshot::ptr_eq(&shared, &clone));
    let handle = std::thread::spawn(move || {
        let mut s = S(0);
        clone.try_restore(&mut s).unwrap();
        s
    });
    assert_eq!(handle.join().unwrap(), S(7));
}
