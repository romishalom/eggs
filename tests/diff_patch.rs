use eggs::{ChangeKind, Diff, Egg, EggError, EggExt, Patch};

#[derive(Egg, Clone, Debug, PartialEq)]
struct S {
    a: u32,
    b: String,
    c: Vec<u8>,
}

fn s(a: u32, b: &str) -> S {
    S { a, b: b.into(), c: vec![] }
}

#[test]
fn diff_reverse_preserves_path_and_swaps_values() {
    let d = s(1, "x").diff(&s(2, "y"));
    let r = d.reverse();
    assert_eq!(d.change_count(), r.change_count());
    for (orig, rev) in d.changes().iter().zip(r.changes().iter().rev()) {
        assert_eq!(orig.path, rev.path);
        assert_eq!(orig.before(), rev.after());
        assert_eq!(orig.after(), rev.before());
    }
    assert_eq!(d.reverse().reverse(), d);
    assert_eq!(d.invert(), d.reverse());
    // Reversing describes the same changes as diffing the other way. (Order differs: a
    // reversed diff is replayed back to front.)
    let other = s(2, "y").diff(&s(1, "x"));
    assert_eq!(r.change_count(), other.change_count());
    for c in other.changes() {
        assert_eq!(r.field_diff(&c.path), Some(c));
    }
}

#[test]
fn reverse_swaps_added_and_removed() {
    let mut longer = s(1, "x");
    longer.c.push(9);
    let d = s(1, "x").diff(&longer);
    assert_eq!(d.changes()[0].kind, ChangeKind::Added);
    assert_eq!(d.reverse().changes()[0].kind, ChangeKind::Removed);
}

#[test]
fn filter_and_exclude_fields() {
    let d = s(1, "x").diff(&s(2, "y"));
    assert_eq!(d.filter_fields(["a"]).unwrap().change_count(), 1);
    assert!(d.filter_fields(["a"]).unwrap().contains_change("a"));
    assert!(!d.exclude_fields(["a"]).unwrap().contains_change("a"));
    assert!(matches!(d.filter_fields(["a..b"]), Err(EggError::InvalidFieldPath { .. })));
    let mut m = s(1, "x");
    m.c = vec![1, 2];
    let nested = s(1, "x").diff(&m);
    assert_eq!(nested.filter_fields(["c"]).unwrap().change_count(), 2, "prefix match covers children");
}

#[test]
fn merge_unions_disjoint_and_rejects_conflicts() {
    let base = s(1, "x");
    let da = base.diff(&s(2, "x"));
    let db = base.diff(&s(1, "y"));
    let merged = da.merge(&db).unwrap();
    assert_eq!(merged.change_count(), 2);
    assert_eq!(da.merge(&da).unwrap(), da, "identical changes are not duplicated");
    let conflicting = base.diff(&s(3, "x"));
    assert!(matches!(da.merge(&conflicting), Err(EggError::PatchConflict { .. })));
}

#[test]
fn compose_chains_and_cancels() {
    let (v1, v2, v3) = (s(1, "x"), s(2, "x"), s(3, "x"));
    let composed = v1.diff(&v2).compose(&v2.diff(&v3)).unwrap();
    assert_eq!(composed, v1.diff(&v3));
    // Returning to the start cancels out.
    assert!(v1.diff(&v2).compose(&v2.diff(&v1)).unwrap().is_empty());
    // Broken continuity is reported.
    assert!(matches!(
        v1.diff(&v2).compose(&v1.diff(&v3)),
        Err(EggError::PatchConflict { .. })
    ));
    // Disjoint paths concatenate.
    let both = v1.diff(&v2).compose(&v2.diff(&s(2, "y"))).unwrap();
    assert_eq!(both.change_count(), 2);
    // Sequence-index changes cannot be verified.
    let mut longer = v1.clone();
    longer.c.push(1);
    assert!(matches!(v1.diff(&longer).compose(&Diff::new()), Err(EggError::UnsupportedOperation { .. })));
}

#[test]
fn validate_catches_malformed_diffs() {
    assert!(s(1, "x").diff(&s(2, "y")).validate().is_ok());
    let c = s(1, "x").diff(&s(2, "x")).changes()[0].clone();
    assert!(Diff::from_changes(vec![c.clone(), c.clone()]).validate().is_err(), "duplicate path");
    let noop = eggs::Change::modified("a".parse().unwrap(), eggs::Value::new(1u32), eggs::Value::new(1u32));
    assert!(Diff::from_changes(vec![noop]).validate().is_err());
    assert!(Patch::<S>::from_diff(&Diff::from_changes(vec![c.clone(), c])).is_err());
}

#[test]
fn patch_roundtrip_and_accessors() {
    let a = s(1, "x");
    let b = s(2, "y");
    let patch = Patch::between(&a, &b).unwrap();
    assert_eq!(patch.len(), 2);
    assert!(!patch.is_empty());
    assert_eq!(patch.operations().len(), 2);
    assert_eq!(patch.diff(), a.diff(&b));
    assert!(Patch::between(&a, &a).unwrap().is_empty());
    assert!(patch.is_applicable(&a));
    let mut st = a.clone();
    patch.apply(&mut st).unwrap();
    assert_eq!(st, b);
    patch.reverse().apply(&mut st).unwrap();
    assert_eq!(st, a);
}

#[test]
fn patch_detects_conflicts_and_is_atomic() {
    let patch = Patch::between(&s(1, "x"), &s(2, "y")).unwrap();
    // `a` matches but `b` does not: nothing may change.
    let mut target = s(1, "DIFFERENT");
    let err = patch.apply(&mut target).unwrap_err();
    assert!(matches!(err, EggError::PatchConflict { ref path, .. } if path == "b"), "{err}");
    assert_eq!(target, s(1, "DIFFERENT"), "atomic apply leaves the target untouched");
    assert!(!patch.is_applicable(&target));
    assert!(patch.validate(&target).is_err());

    // The non-staged variant documents partial application.
    let mut partial = s(1, "DIFFERENT");
    assert!(patch.apply_in_place(&mut partial).is_err());
    assert_eq!(partial.a, 2);
}

#[test]
fn patch_from_another_type_is_rejected_not_applied() {
    #[derive(Egg, Clone, PartialEq, Debug)]
    struct Other {
        a: String,
    }
    let patch = Patch::<Other>::between(&Other { a: "x".into() }, &Other { a: "y".into() }).unwrap();
    let foreign = Patch::<S>::from_diff(&patch.diff()).unwrap();
    let mut target = s(1, "x");
    let err = foreign.apply(&mut target).unwrap_err();
    assert!(matches!(err, EggError::IncompatibleSchema { .. }), "{err}");
    assert_eq!(target, s(1, "x"));
}

#[test]
fn patch_with_bad_paths_fails_cleanly() {
    let mut longer = s(1, "x");
    longer.c = vec![1, 2];
    let patch = Patch::between(&s(1, "x"), &longer).unwrap();
    // Appending at index 0 requires an empty `c`.
    let mut wrong = s(1, "x");
    wrong.c = vec![5, 6, 7];
    assert!(matches!(patch.apply(&mut wrong), Err(EggError::PatchConflict { .. })));
}

#[test]
fn rollback_changes_undoes_a_diff() {
    let a = s(1, "x");
    let mut b = s(2, "y");
    let diff = a.diff(&b);
    b.rollback_changes(&diff).unwrap();
    assert_eq!(b, a);
    // A second rollback no longer matches the state.
    assert!(b.rollback_changes(&diff).is_err());
}

#[test]
fn errors_display_and_implement_error() {
    let e = EggError::validation("x");
    assert_eq!(e.to_string(), "validation failed: x");
    let _: &dyn std::error::Error = &e;
    let nested = EggError::invalid_path("b", "r").within(&eggs::PathSegment::Field("a".into()));
    assert_eq!(nested.to_string(), "invalid field path `a.b`: r");
}
