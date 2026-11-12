#![cfg(feature = "transaction")]

use eggs::{transaction, Egg, EggError, History};

#[derive(Egg, Clone, Debug, PartialEq)]
struct State {
    count: u32,
    enabled: bool,
}

#[test]
fn commit_keeps_edits() {
    let mut s = State { count: 0, enabled: false };
    let mut tx = transaction(&mut s).unwrap();
    tx.edit(|s| {
        s.count += 1;
        s.enabled = true;
    })
    .unwrap();
    assert_eq!(tx.value().count, 1);
    tx.commit().unwrap();
    assert!(tx.is_committed() && !tx.is_aborted());
    drop(tx);
    assert_eq!(s, State { count: 1, enabled: true });
}

#[test]
fn abort_restores() {
    let mut s = State { count: 5, enabled: false };
    let mut tx = transaction(&mut s).unwrap();
    tx.edit(|s| s.count = 9).unwrap();
    tx.abort().unwrap();
    assert!(tx.is_aborted());
    drop(tx);
    assert_eq!(s.count, 5);
}

#[test]
fn drop_aborts_uncommitted_transactions() {
    let mut s = State { count: 5, enabled: false };
    {
        let mut tx = transaction(&mut s).unwrap();
        tx.edit(|s| s.count = 9).unwrap();
    }
    assert_eq!(s.count, 5);
}

#[test]
fn drop_aborts_when_the_edit_panics() {
    let mut s = State { count: 5, enabled: false };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut tx = transaction(&mut s).unwrap();
        tx.edit(|s| {
            s.count = 9;
            panic!("boom");
        })
        .unwrap();
    }));
    assert!(result.is_err());
    assert_eq!(s.count, 5);
}

#[test]
fn finished_transactions_reject_further_operations() {
    let mut s = State { count: 0, enabled: false };
    let mut tx = transaction(&mut s).unwrap();
    tx.commit().unwrap();
    for r in [tx.commit(), tx.abort(), tx.edit(|_| {}), tx.rollback_transaction()] {
        assert!(matches!(r, Err(EggError::TransactionState(_))));
    }
    assert!(tx.checkpoint().is_err());
}

#[test]
fn checkpoints_and_rollback_transaction() {
    let mut s = State { count: 0, enabled: false };
    let mut tx = transaction(&mut s).unwrap();
    tx.edit(|s| s.count = 1).unwrap();
    let one = tx.checkpoint().unwrap();
    tx.edit(|s| s.count = 2).unwrap();
    let two = tx.checkpoint().unwrap();
    tx.edit(|s| s.count = 3).unwrap();
    tx.rollback_to(one).unwrap();
    assert_eq!(tx.value().count, 1);
    assert!(tx.rollback_to(two).is_err(), "later checkpoints are discarded");
    tx.rollback_transaction().unwrap();
    assert_eq!(tx.value().count, 0);
    tx.edit(|s| s.count = 7).unwrap(); // still open
    tx.commit().unwrap();
    drop(tx);
    assert_eq!(s.count, 7);
}

#[test]
fn commit_can_record_history() {
    let mut s = State { count: 0, enabled: false };
    let mut h = History::new(&s);
    let mut tx = transaction(&mut s).unwrap();
    tx.edit(|s| s.count = 4).unwrap();
    tx.commit_and_record(&mut h).unwrap();
    drop(tx);
    assert!(h.can_undo());
    h.undo(&mut s).unwrap();
    assert_eq!(s.count, 0);
}

#[test]
fn restore_errors_are_not_swallowed() {
    #[derive(Clone)]
    struct Fragile(u32);
    impl Egg for Fragile {
        type Snapshot = u32;
        fn snapshot(&self) -> u32 {
            self.0
        }
        fn restore_in_place(&mut self, _: &u32) -> eggs::EggResult<()> {
            Err(EggError::restore_failed("nope"))
        }
        fn diff_into(&self, _: &Self, _: &eggs::EggPath, _: &mut Vec<eggs::Change>) {}
    }
    let mut f = Fragile(1);
    let mut tx = transaction(&mut f).unwrap();
    assert!(matches!(tx.abort(), Err(EggError::RestoreFailed { .. })));
    drop(tx); // does not retry or panic after the error was reported
}
