use eggs::{Egg, EggError, EggExt, EggPath, PathSegment};

#[derive(Egg, Clone, Debug, PartialEq)]
struct Player {
    name: String,
    health: u32,
    position: (f32, f32),
    #[egg(skip)]
    cache: Vec<u8>,
}

fn player() -> Player {
    Player { name: "Ann".into(), health: 100, position: (1.0, 2.0), cache: vec![] }
}

#[test]
fn snapshot_and_restore_roundtrip() {
    let mut p = player();
    let snap = p.capture();
    p.health = 1;
    p.name.push('!');
    p.position.0 = 9.0;
    snap.try_restore(&mut p).unwrap();
    assert_eq!(p, player());
}

#[test]
fn skipped_fields_remain_unchanged_on_restore() {
    let mut p = player();
    let snap = p.capture();
    p.cache.push(7);
    p.health = 5;
    snap.try_restore(&mut p).unwrap();
    assert_eq!(p.health, 100);
    assert_eq!(p.cache, vec![7]);
}

#[test]
fn skipped_fields_are_not_diffed() {
    let a = player();
    let mut b = a.clone();
    b.cache.push(1);
    assert!(a.diff(&b).is_empty());
}

#[test]
fn equal_states_produce_empty_diffs() {
    assert!(player().diff(&player()).is_empty());
}

#[test]
fn changed_fields_are_reported_in_declaration_order() {
    let a = player();
    let mut b = a.clone();
    b.position.1 = 5.0;
    b.name = "Bob".into();
    let diff = a.diff(&b);
    let paths: Vec<String> = diff.changed_fields().map(ToString::to_string).collect();
    assert_eq!(paths, ["name", "position.1"]);
    assert_eq!(diff.change_count(), 2);
    assert!(diff.contains_change("name"));
    assert!(!diff.contains_change("health"));
}

#[test]
fn fields_are_labelled_in_declaration_order() {
    assert_eq!(Player::fields(), ["name", "health", "position"]);
}

#[derive(Egg, Clone, Debug, PartialEq)]
struct Pair(u8, String);

#[derive(Egg, Clone, Debug, PartialEq)]
struct Unit;

#[test]
fn tuple_and_unit_structs() {
    let mut p = Pair(1, "x".into());
    let snap = p.capture();
    p.0 = 2;
    assert_eq!(Pair(1, "x".into()).diff(&p).changed_fields().next().unwrap().to_string(), "0");
    snap.try_restore(&mut p).unwrap();
    assert_eq!(p, Pair(1, "x".into()));
    assert_eq!(Pair::fields(), ["0", "1"]);

    let u = Unit;
    assert!(u.diff(&Unit).is_empty());
    assert!(u.capture().is_empty());
    assert_eq!(Unit::from_snapshot(&u.snapshot()).unwrap(), Unit);
}

#[derive(Egg, Clone, Debug, PartialEq)]
struct Wrapper<T, U = u8> {
    inner: T,
    extra: Vec<U>,
}

#[test]
fn generic_structs() {
    let mut w = Wrapper { inner: String::from("a"), extra: vec![1u8, 2] };
    let snap = w.capture();
    w.inner.push('b');
    w.extra.push(3);
    let diff = snap.diff(&w).unwrap();
    assert!(diff.contains_change("inner"));
    assert!(diff.contains_change("extra[2]"));
    snap.try_restore(&mut w).unwrap();
    assert_eq!(w, Wrapper { inner: "a".into(), extra: vec![1, 2] });
}

#[derive(Egg, Clone, Debug, PartialEq)]
struct Borrowing<'a> {
    label: &'static str,
    #[egg(skip)]
    _marker: std::marker::PhantomData<&'a ()>,
}

#[test]
fn lifetimes_are_preserved() {
    let b = Borrowing { label: "x", _marker: std::marker::PhantomData };
    assert_eq!(b.capture().value().clone(), b.snapshot());
}

#[derive(Egg, Clone, Debug, PartialEq)]
struct Inner {
    city: String,
    zip: u32,
}

#[derive(Egg, Clone, Debug, PartialEq)]
struct Outer {
    profile: Inner,
    maybe: Option<Inner>,
}

#[test]
fn nested_values_have_structured_paths() {
    let a = Outer { profile: Inner { city: "A".into(), zip: 1 }, maybe: None };
    let mut b = a.clone();
    b.profile.city = "B".into();
    b.maybe = Some(Inner { city: "C".into(), zip: 2 });
    let diff = a.diff(&b);
    assert!(diff.contains_change("profile.city"));
    assert!(!diff.contains_change("profile.zip"));
    assert_eq!(diff.field_diff("maybe").unwrap().kind, eggs::ChangeKind::VariantChanged);
}

#[derive(Egg, Clone, Debug, PartialEq)]
struct Attrs {
    #[egg(rename = "hp")]
    health: u32,
    #[egg(ignore_diff)]
    timestamp: u64,
    #[egg(ignore_restore)]
    seen: u32,
    #[egg(skip, default)]
    cache: Vec<u8>,
}

#[test]
fn attribute_semantics() {
    assert_eq!(Attrs::fields(), ["hp", "timestamp", "seen"]);
    let a = Attrs { health: 1, timestamp: 1, seen: 1, cache: vec![9] };
    let mut b = a.clone();
    b.timestamp = 2;
    assert!(a.diff(&b).is_empty(), "ignore_diff fields are not diffed");
    b.seen = 2;
    assert!(a.diff(&b).contains_change("seen"), "ignore_restore fields are diffed");

    // ignore_restore: captured, but restore leaves it alone; ignore_diff: restored.
    let snap = a.capture();
    b.timestamp = 50;
    b.health = 5;
    snap.try_restore(&mut b).unwrap();
    assert_eq!((b.health, b.timestamp, b.seen), (1, 1, 2));

    // skip + default makes from_snapshot possible.
    let rebuilt = Attrs::from_snapshot(&a.snapshot()).unwrap();
    assert_eq!(rebuilt, Attrs { cache: vec![], ..a });
}

#[derive(Egg, Clone, Debug, PartialEq)]
struct NoDefault {
    a: u32,
    #[egg(skip)]
    hidden: u32,
}

#[test]
fn from_snapshot_without_default_is_a_clear_error() {
    let err = NoDefault::from_snapshot(&NoDefault { a: 1, hidden: 2 }.snapshot()).unwrap_err();
    assert!(matches!(err, EggError::UnsupportedOperation { .. }));
    assert!(err.to_string().contains("hidden"));
}

fn same_len(a: &String, b: &String) -> bool {
    a.len() == b.len()
}

#[derive(Egg, Clone, Debug, PartialEq)]
#[egg(version = 3)]
struct Custom {
    #[egg(compare_with = "same_len", strategy = "clone")]
    name: String,
}

#[test]
fn compare_with_and_version() {
    assert_eq!(Custom::schema_version(), 3);
    let a = Custom { name: "ab".into() };
    assert!(a.diff(&Custom { name: "cd".into() }).is_empty());
    assert!(a.diff(&Custom { name: "abc".into() }).contains_change("name"));
}

mod adapter {
    use eggs::{Change, EggPath, EggResult};
    pub type Snapshot = u32;
    pub fn snapshot(v: &u32) -> Snapshot {
        *v / 10 * 10 // quantise
    }
    pub fn restore(v: &mut u32, s: &Snapshot) -> EggResult<()> {
        *v = *s;
        Ok(())
    }
    pub fn from_snapshot(s: &Snapshot) -> EggResult<u32> {
        Ok(*s)
    }
    pub fn diff(a: &u32, b: &u32, _path: &EggPath, _out: &mut Vec<Change>) {
        let _ = (a, b);
    }
}

#[derive(Egg, Clone, Debug, PartialEq)]
struct WithAdapter {
    #[egg(with = "adapter")]
    level: u32,
}

#[test]
fn with_adapter() {
    let mut w = WithAdapter { level: 37 };
    let snap = w.capture();
    w.level = 99;
    snap.try_restore(&mut w).unwrap();
    assert_eq!(w.level, 30);
}

#[test]
fn builder_include_exclude_and_errors() {
    let mut p = player();
    let snap = p.snapshot_with().include("health").with_metadata().capture().unwrap();
    assert!(snap.created_at().is_some());
    p.health = 1;
    p.name = "Zed".into();
    snap.try_restore(&mut p).unwrap();
    assert_eq!((p.health, p.name.as_str()), (100, "Zed"));

    let snap = player().snapshot_with().exclude("name").capture().unwrap();
    let mut q = player();
    q.name = "Q".into();
    q.health = 3;
    snap.try_restore(&mut q).unwrap();
    assert_eq!((q.health, q.name.as_str()), (100, "Q"));

    // Misspelled, unknown, nested-exclude and malformed names are errors, not silent no-ops.
    assert!(matches!(
        player().snapshot_with().include("helth").capture(),
        Err(EggError::InvalidFieldPath { .. })
    ));
    assert!(player().snapshot_with().exclude("position.0").capture().is_err());
    assert!(player().snapshot_with().include("a..b").capture().is_err());
    assert!(player().snapshot_with().exclude("name").exclude("health").exclude("position").capture().unwrap().is_empty());
}

#[test]
fn restore_field_validates_paths_and_is_atomic() {
    let base = player();
    let snap = base.capture();
    let mut p = base.clone();
    p.health = 1;
    p.position = (7.0, 8.0);
    snap.restore_field(&mut p, "position.1").unwrap();
    assert_eq!(p.position, (7.0, 2.0), "only position.1 is restored");
    assert!(snap.restore_field(&mut p, "nope").is_err());
    // Nothing changed by the failed call.
    assert_eq!(p.health, 1);
    // A bad path in the middle of restore_fields leaves the target untouched.
    assert!(snap.restore_fields(&mut p, ["health", "nope"]).is_err());
    assert_eq!(p.health, 1);
    snap.restore_fields(&mut p, ["health", "position"]).unwrap();
    assert_eq!(p.health, 100);
}

#[test]
fn snapshot_ids_are_unique_and_clone_keeps_or_renews_them() {
    let p = player();
    let a = p.capture();
    let b = p.capture();
    assert_ne!(a.id(), b.id());
    assert_eq!(a.clone().id(), a.id());
    assert_ne!(a.clone_snapshot().id(), a.id());
    assert!(a.estimate_size() >= std::mem::size_of::<u32>());
    assert_eq!(a.size(), a.estimate_size());
}

#[test]
fn can_restore_and_rollback_to() {
    let mut p = player();
    let snap = p.capture();
    assert!(snap.can_restore(&p));
    p.health = 2;
    p.rollback_to(&snap).unwrap();
    assert_eq!(p.health, 100);
    assert!(p.crack(&snap).unwrap().is_empty());
}

#[test]
fn paths_parse_and_render() {
    for s in ["", "a", "a.b", "a[3]", "a{\"k\"}.b[0]", "[1][2]", "weird\\.name.x"] {
        let p = EggPath::parse(s).unwrap();
        assert_eq!(p.to_string(), s, "round trip of {s:?}");
    }
    assert_eq!(
        EggPath::parse("a[3].b").unwrap().segments(),
        [PathSegment::Field("a".into()), PathSegment::Index(3), PathSegment::Field("b".into())]
    );
    for bad in [".a", "a..b", "a[", "a[x]", "a{", "a]", "a.", "a\\"] {
        assert!(EggPath::parse(bad).is_err(), "{bad:?} should not parse");
    }
}
