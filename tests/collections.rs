use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use eggs::{ChangeKind, Egg, EggExt, Patch};

#[derive(Egg, Clone, Debug, PartialEq)]
struct Item {
    name: String,
    qty: u32,
}

#[derive(Egg, Clone, Debug, PartialEq)]
struct Bag {
    items: Vec<Item>,
    scores: BTreeMap<String, i32>,
    lookup: HashMap<u8, Item>,
    tags: BTreeSet<String>,
    seen: HashSet<u32>,
    grid: [u8; 3],
    maybe: Option<Box<Item>>,
    res: Result<u8, String>,
}

fn bag() -> Bag {
    Bag {
        items: vec![Item { name: "a".into(), qty: 1 }, Item { name: "b".into(), qty: 2 }],
        scores: BTreeMap::from([("ann".to_string(), 1), ("bob".to_string(), 2)]),
        lookup: HashMap::from([(1, Item { name: "x".into(), qty: 1 })]),
        tags: ["red".to_string(), "blue".to_string()].into(),
        seen: [1, 2, 3].into(),
        grid: [1, 2, 3],
        maybe: None,
        res: Ok(1),
    }
}

#[test]
fn equal_bags_have_empty_diffs() {
    assert!(bag().diff(&bag()).is_empty());
}

#[test]
fn sequence_diffs_identify_index_changes() {
    let a = bag();
    let mut b = a.clone();
    b.items[1].qty = 20;
    b.items.push(Item { name: "c".into(), qty: 3 });
    let diff = a.diff(&b);
    assert!(diff.contains_change("items[1].qty"));
    let added = diff.field_diff("items[2]").unwrap();
    assert_eq!(added.kind, ChangeKind::Added);
    assert!(added.before().is_none() && added.after().is_some());

    let mut c = a.clone();
    c.items.clear();
    let removed: Vec<String> = a
        .diff(&c)
        .changes()
        .iter()
        .map(|ch| format!("{:?}@{}", ch.kind, ch.path))
        .collect();
    // Removals go from the highest index down so they can be applied in order.
    assert_eq!(removed, ["Removed@items[1]", "Removed@items[0]"]);
}

#[test]
fn map_and_set_diffs() {
    let a = bag();
    let mut b = a.clone();
    b.scores.insert("cy".into(), 3);
    b.scores.remove("ann");
    *b.scores.get_mut("bob").unwrap() = 20;
    b.tags.remove("red");
    b.tags.insert("green".into());
    b.seen.insert(9);
    b.lookup.get_mut(&1).unwrap().qty = 5;
    let diff = a.diff(&b);
    assert_eq!(diff.field_diff("scores{\"ann\"}").unwrap().kind, ChangeKind::Removed);
    assert_eq!(diff.field_diff("scores{\"cy\"}").unwrap().kind, ChangeKind::Added);
    assert_eq!(diff.field_diff("scores{\"bob\"}").unwrap().kind, ChangeKind::Modified);
    assert_eq!(diff.field_diff("tags{\"red\"}").unwrap().kind, ChangeKind::Removed);
    assert_eq!(diff.field_diff("tags{\"green\"}").unwrap().kind, ChangeKind::Added);
    assert_eq!(diff.field_diff("seen{9}").unwrap().kind, ChangeKind::Added);
    assert!(diff.contains_change("lookup{1}.qty"));
}

#[test]
fn hash_collection_diffs_are_deterministic() {
    let a = bag();
    let mut b = a.clone();
    for i in 10..40 {
        b.seen.insert(i);
        b.lookup.insert(i as u8, Item { name: format!("n{i}"), qty: 0 });
    }
    let first: Vec<String> = a.diff(&b).changed_fields().map(ToString::to_string).collect();
    for _ in 0..5 {
        let again: Vec<String> = a.clone().diff(&b.clone()).changed_fields().map(ToString::to_string).collect();
        assert_eq!(first, again);
    }
    let mut sorted = first.clone();
    sorted.sort();
    // Within one collection, entries come out sorted by their key rendering.
    let seen: Vec<&String> = first.iter().filter(|p| p.starts_with("seen")).collect();
    let mut seen_sorted = seen.clone();
    seen_sorted.sort();
    assert_eq!(seen, seen_sorted);
}

fn all_changed() -> (Bag, Bag) {
    let a = bag();
    let mut b = a.clone();
    b.items[0].qty = 10;
    b.items.push(Item { name: "c".into(), qty: 3 });
    b.scores.insert("cy".into(), 3);
    b.scores.remove("ann");
    b.lookup.insert(2, Item { name: "y".into(), qty: 2 });
    b.lookup.remove(&1);
    b.tags.insert("green".into());
    b.tags.remove("red");
    b.seen.remove(&1);
    b.grid[2] = 30;
    b.maybe = Some(Box::new(Item { name: "m".into(), qty: 1 }));
    b.res = Err("boom".into());
    (a, b)
}

#[test]
fn patch_applies_and_reverses_every_collection_kind() {
    let (a, b) = all_changed();
    let patch = Patch::between(&a, &b).unwrap();
    let mut state = a.clone();
    patch.apply(&mut state).unwrap();
    assert_eq!(state, b);
    patch.reverse().apply(&mut state).unwrap();
    assert_eq!(state, a);
}

#[test]
fn patch_removing_several_elements_and_shrinking() {
    let a = bag();
    let mut b = a.clone();
    b.items.clear();
    let patch = Patch::between(&a, &b).unwrap();
    let mut s = a.clone();
    patch.apply(&mut s).unwrap();
    assert!(s.items.is_empty());
    patch.reverse().apply(&mut s).unwrap();
    assert_eq!(s, a);
}

#[test]
fn restore_recreates_and_removes_elements() {
    let (a, b) = all_changed();
    let snap = a.capture();
    let mut target = b.clone();
    snap.try_restore(&mut target).unwrap();
    assert_eq!(target, a);
    // And the other direction: growing from fewer elements.
    let snap_b = b.capture();
    snap_b.try_restore(&mut target).unwrap();
    assert_eq!(target, b);
}

#[test]
fn restore_nested_path_in_collections() {
    let a = bag();
    let snap = a.capture();
    let mut b = a.clone();
    b.items[1].qty = 99;
    b.items[0].qty = 98;
    snap.restore_field(&mut b, "items[1].qty").unwrap();
    assert_eq!((b.items[0].qty, b.items[1].qty), (98, 2));
    b.scores.insert("ann".into(), 50);
    snap.restore_field(&mut b, "scores{\"ann\"}").unwrap();
    assert_eq!(b.scores["ann"], 1);
    assert!(snap.restore_field(&mut b, "items[7]").is_err());
}

#[test]
fn tuples_options_results_and_box() {
    let a = (1u8, "x".to_string(), Some(3u8));
    let b = (1u8, "y".to_string(), None);
    let diff = a.diff(&b);
    assert!(diff.contains_change("1"));
    assert_eq!(diff.field_diff("2").unwrap().kind, ChangeKind::VariantChanged);
    let patch = Patch::between(&a, &b).unwrap();
    let mut s = a.clone();
    patch.apply(&mut s).unwrap();
    assert_eq!(s, b);
    patch.reverse().apply(&mut s).unwrap();
    assert_eq!(s, a);
}

#[test]
fn snapshot_size_estimate_grows_with_data() {
    let small = bag();
    let mut big = bag();
    big.items.extend((0..500).map(|i| Item { name: "n".repeat(20), qty: i }));
    assert!(big.capture().estimate_size() > small.capture().estimate_size() + 500 * 20);
}

#[test]
fn user_value_types_via_macro() {
    #[derive(Clone, Debug, PartialEq)]
    struct Rgb(u8, u8, u8);
    eggs::impl_egg_for_value!(Rgb);
    let a = vec![Rgb(1, 2, 3)];
    let b = vec![Rgb(1, 2, 4)];
    assert_eq!(a.diff(&b).field_diff("[0]").unwrap().kind, ChangeKind::Modified);
    let mut s = a.clone();
    Patch::between(&a, &b).unwrap().apply(&mut s).unwrap();
    assert_eq!(s, b);
}
