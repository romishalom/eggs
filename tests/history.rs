use eggs::{Egg, EggError, History, EggConfig};

#[derive(Egg, Clone, Debug, PartialEq)]
struct Doc {
    n: u32,
    #[egg(skip)]
    scratch: u32,
}

fn doc(n: u32) -> Doc {
    Doc { n, scratch: 0 }
}

fn recorded(upto: u32) -> (Doc, History<Doc>) {
    let mut d = doc(0);
    let mut h = History::new(&d);
    for i in 1..=upto {
        d.n = i;
        h.record(&d);
    }
    (d, h)
}

#[test]
fn undo_redo_navigation() {
    let (mut d, mut h) = recorded(3);
    assert_eq!(h.len(), 4);
    assert!(!h.is_empty());
    assert!(h.can_undo() && !h.can_redo());
    assert!(h.undo(&mut d).unwrap());
    assert_eq!(d.n, 2);
    assert!(h.can_redo());
    assert!(h.redo(&mut d).unwrap());
    assert_eq!(d.n, 3);
    assert!(!h.redo(&mut d).unwrap(), "nothing to redo reports false");
    for _ in 0..3 {
        assert!(h.undo(&mut d).unwrap());
    }
    assert!(!h.undo(&mut d).unwrap());
    assert_eq!(d.n, 0);
    assert_eq!(h.position(), 0);
}

#[test]
fn accessors() {
    let (mut d, mut h) = recorded(3);
    h.undo(&mut d).unwrap();
    assert_eq!(h.current().value(), &d.snapshot());
    assert_eq!(h.previous().unwrap().value(), &doc(1).snapshot());
    assert_eq!(h.next().unwrap().value(), &doc(3).snapshot());
    assert_eq!(h.first().value(), &doc(0).snapshot());
    assert_eq!(h.last().value(), &doc(3).snapshot());
}

#[test]
fn recording_after_undo_truncates_the_redo_branch() {
    let (mut d, mut h) = recorded(3);
    h.undo(&mut d).unwrap();
    h.undo(&mut d).unwrap();
    d.n = 99;
    h.record(&d);
    assert!(!h.can_redo());
    assert_eq!(h.len(), 3);
    h.undo(&mut d).unwrap();
    assert_eq!(d.n, 1);
}

#[test]
fn bounded_eviction_drops_oldest() {
    let mut d = doc(0);
    let mut h = History::with_limit(&d, 3).unwrap();
    for i in 1..=10 {
        d.n = i;
        h.record(&d);
    }
    assert_eq!(h.len(), 3);
    assert_eq!(h.first().value(), &doc(8).snapshot());
    assert_eq!(h.limit(), 3);
    let mut undone = 0;
    while h.undo(&mut d).unwrap() {
        undone += 1;
    }
    assert_eq!((undone, d.n), (2, 8));
}

#[test]
fn default_limit_is_128_and_zero_is_rejected() {
    let d = doc(0);
    assert_eq!(History::new(&d).limit(), 128);
    assert!(matches!(History::with_limit(&d, 0), Err(EggError::Validation(_))));
    let mut h = History::new(&d);
    assert!(h.set_limit(0).is_err());
    assert_eq!(h.limit(), 128);
    assert!(History::with_config(&d, &EggConfig::new().with_history_limit(0)).is_err());
    assert_eq!(History::with_config(&d, &EggConfig::new().with_history_limit(7).with_metadata(true)).unwrap().limit(), 7);
}

#[test]
fn shrinking_the_limit_keeps_the_current_entry() {
    let (mut d, mut h) = recorded(5);
    h.undo(&mut d).unwrap();
    h.undo(&mut d).unwrap(); // cursor at 3
    h.set_limit(2).unwrap();
    assert_eq!(h.len(), 2);
    assert_eq!(h.current().value(), &d.snapshot());
}

#[test]
fn checkpoints() {
    let (mut d, mut h) = recorded(4); // cursor at n = 4
    let end = h.checkpoint("end");
    h.undo(&mut d).unwrap();
    h.undo(&mut d).unwrap();
    let two = h.checkpoint("two"); // cursor at n = 2
    assert_ne!(end, two);

    // Forward to `end`, then back to `two`.
    assert!(h.redo_to(end, &mut d).unwrap());
    assert_eq!(d.n, 4);
    assert!(h.undo_to(two, &mut d).unwrap());
    assert_eq!(d.n, 2);
    assert!(!h.undo_to(two, &mut d).unwrap(), "already there");
    assert!(!h.redo_to(two, &mut d).unwrap(), "already there");

    // Direction is checked and nothing moves on a wrong-direction call.
    assert!(matches!(h.undo_to(end, &mut d), Err(EggError::Validation(_))), "end is ahead; use redo_to");
    assert_eq!(d.n, 2);
    h.redo_to(end, &mut d).unwrap();
    assert!(matches!(h.redo_to(two, &mut d), Err(EggError::Validation(_))), "two is behind; use undo_to");
    assert_eq!(d.n, 4);

    // Named restore works in either direction.
    assert!(h.restore_checkpoint("two", &mut d).unwrap());
    assert_eq!(d.n, 2);
    assert!(h.restore_checkpoint("end", &mut d).unwrap());
    assert_eq!(d.n, 4);

    assert_eq!(h.checkpoints().count(), 2);
    assert!(h.restore_checkpoint("missing", &mut d).is_err());
    assert!(h.remove_checkpoint("two"));
    assert!(!h.remove_checkpoint("two"));
    // A CheckpointId is a handle to the history *entry*; removing a name does not invalidate it.
    assert!(h.undo_to(two, &mut d).unwrap());
    assert_eq!(d.n, 2);
}

#[test]
fn checkpoint_names_replace_and_eviction_invalidates() {
    let mut d = doc(0);
    let mut h = History::with_limit(&d, 3).unwrap();
    let first = h.checkpoint("c");
    d.n = 1;
    h.record(&d);
    let second = h.checkpoint("c"); // replaces
    assert_eq!(h.checkpoints().count(), 1);
    assert_eq!(h.checkpoints().next().unwrap(), ("c", second));
    assert_ne!(first, second);
    for i in 2..8 {
        d.n = i;
        h.record(&d);
    }
    assert_eq!(h.checkpoints().count(), 0, "evicted entries drop their checkpoints");
    assert!(h.undo_to(second, &mut d).is_err());
}

#[test]
fn clear_and_truncate() {
    let (mut d, mut h) = recorded(4);
    h.checkpoint("x");
    h.undo(&mut d).unwrap();
    h.truncate(1).unwrap();
    assert_eq!(h.len(), 2);
    assert_eq!(h.position(), 1);
    assert!(h.truncate(5).is_err());
    h.clear();
    assert_eq!((h.len(), h.position(), h.checkpoints().count()), (1, 0, 0));
    assert!(!h.can_undo() && !h.can_redo());
    assert_eq!(h.current().value(), &doc(1).snapshot());
}

#[test]
fn skipped_fields_are_not_rewound() {
    let (mut d, mut h) = recorded(2);
    d.scratch = 42;
    h.undo(&mut d).unwrap();
    assert_eq!(d.scratch, 42);
}

#[test]
fn model_variants_and_metadata() {
    let (mut d, mut h) = recorded(2);
    let model = doc(0);
    assert!(h.undo_with_model(&mut d, &model).unwrap());
    assert!(h.redo_with_model(&mut d, &model).unwrap());
    assert_eq!(d.n, 2);
    let h = History::with_config(&d, &EggConfig::new().with_metadata(true)).unwrap();
    assert!(h.current().created_at().is_some());
}

#[test]
fn failed_restore_does_not_move_the_cursor() {
    // A type whose restore can fail.
    #[derive(Clone, Debug, PartialEq)]
    struct Picky(u32);
    impl Egg for Picky {
        type Snapshot = u32;
        fn snapshot(&self) -> u32 {
            self.0
        }
        fn restore_in_place(&mut self, s: &u32) -> eggs::EggResult<()> {
            if *s == 0 {
                Err(EggError::restore_failed("zero is not allowed"))
            } else {
                self.0 = *s;
                Ok(())
            }
        }
        fn diff_into(&self, _: &Self, _: &eggs::EggPath, _: &mut Vec<eggs::Change>) {}
    }
    let mut p = Picky(0);
    let mut h = History::new(&p);
    p.0 = 5;
    h.record(&p);
    let err = h.undo(&mut p).unwrap_err();
    assert!(matches!(err, EggError::RestoreFailed { .. }));
    assert_eq!(h.position(), 1);
    assert_eq!(p, Picky(5), "atomic restore rolled back");
}
