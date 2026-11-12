#![cfg(feature = "json")]

use std::collections::BTreeMap;

use eggs::{Egg, EggError, EggExt, EggPath, Snapshot};

#[derive(Egg, Clone, Debug, PartialEq)]
#[egg(version = 2)]
struct Player {
    name: String,
    stats: BTreeMap<String, u32>,
    items: Vec<(u8, String)>,
    #[egg(skip)]
    cache: u8,
}

#[derive(Egg, Clone, Debug, PartialEq)]
struct OtherVersion {
    name: String,
}

fn player() -> Player {
    Player {
        name: "Ann".into(),
        stats: BTreeMap::from([("hp".into(), 10)]),
        items: vec![(1, "sword".into())],
        cache: 0,
    }
}

#[test]
fn json_roundtrip() {
    let mut p = player();
    let json = p.capture().to_json().unwrap();
    assert!(json.contains("\"eggs_format\":1") && json.contains("\"schema_version\":2"));
    p.name = "Zed".into();
    p.items.clear();
    p.stats.clear();
    let snap = Snapshot::<Player>::from_json(&json).unwrap();
    snap.try_restore(&mut p).unwrap();
    assert_eq!(p, player());
}

#[test]
fn selection_survives_roundtrip() {
    let p = player();
    let snap = p.snapshot_with().include("name").capture().unwrap();
    let back = Snapshot::<Player>::from_json(&snap.to_json().unwrap()).unwrap();
    assert_eq!(back.selection().unwrap(), [EggPath::parse("name").unwrap()]);
    assert_ne!(back.id(), snap.id(), "ids are process-local and not persisted");
}

#[test]
fn version_and_corruption_checks() {
    let json = player().capture().to_json().unwrap();
    assert!(matches!(
        Snapshot::<OtherVersion>::from_json(&json),
        Err(EggError::IncompatibleSchema { .. })
    ));
    let future = json.replace("\"eggs_format\":1", "\"eggs_format\":99");
    assert!(matches!(Snapshot::<Player>::from_json(&future), Err(EggError::IncompatibleSchema { .. })));
    assert!(matches!(Snapshot::<Player>::from_json("{"), Err(EggError::Deserialization(_))));
    assert!(matches!(
        Snapshot::<Player>::from_json(r#"{"eggs_format":1,"schema_version":2,"data":{"f0":1}}"#),
        Err(EggError::Deserialization(_))
    ));
}

#[test]
fn path_serializes_as_a_string() {
    let p = EggPath::parse("a[1].b").unwrap();
    assert_eq!(serde_json::to_string(&p).unwrap(), "\"a[1].b\"");
    assert_eq!(serde_json::from_str::<EggPath>("\"a[1].b\"").unwrap(), p);
}
