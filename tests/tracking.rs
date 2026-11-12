#![cfg(feature = "tracking")]

use std::cell::RefCell;
use std::rc::Rc;

use eggs::tracking::track;
use eggs::Egg;

#[derive(Egg, Clone, Debug, PartialEq)]
struct Settings {
    volume: u8,
    muted: bool,
    #[egg(ignore_diff)]
    opened_at: u64,
}

fn settings() -> Settings {
    Settings { volume: 3, muted: false, opened_at: 0 }
}

#[test]
fn detects_changes_by_comparison() {
    let mut t = track(settings());
    assert!(!t.has_changed());
    t.volume = 9;
    assert!(t.has_changed());
    assert!(t.has_changed_field("volume"));
    assert!(!t.has_changed_field("muted"));
    assert!(!t.has_changed_field("a..b"));
    let diff = t.take_changes();
    assert_eq!(diff.change_count(), 1);
    assert!(!t.has_changed());
    assert_eq!(t.untrack().volume, 9);
}

#[test]
fn mark_changed_covers_what_diffs_cannot_see() {
    let mut t = track(settings());
    t.opened_at = 5; // ignore_diff
    assert!(!t.has_changed());
    t.mark_changed("opened_at").unwrap();
    assert!(t.has_changed() && t.has_changed_field("opened_at"));
    assert_eq!(t.marked_paths().count(), 1);
    t.reset_tracking();
    assert!(!t.has_changed());
    assert!(t.mark_changed("x..y").is_err());
}

#[test]
fn tracked_fields_restrict_reporting() {
    let mut t = track(settings());
    t.track_field("volume").unwrap();
    t.track_field("volume").unwrap();
    assert_eq!(t.tracked_fields().len(), 1);
    t.muted = true;
    assert!(!t.has_changed());
    t.volume = 1;
    assert_eq!(t.changes().change_count(), 1);
    assert!(t.untrack_field("volume"));
    assert!(!t.untrack_field("volume"));
    assert_eq!(t.changes().change_count(), 2);
}

#[test]
fn watchers_are_called_synchronously_only_when_there_are_changes() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut t = track(settings());
    let sink = Rc::clone(&seen);
    let id = t.watch(move |d| sink.borrow_mut().push(d.change_count()));
    t.flush_changes(); // nothing changed
    assert!(seen.borrow().is_empty());
    t.volume = 4;
    t.muted = true;
    t.flush_changes();
    assert_eq!(*seen.borrow(), [2]);
    assert!(t.unwatch(id));
    assert!(!t.unwatch(id));
    t.volume = 5;
    t.flush_changes();
    assert_eq!(seen.borrow().len(), 1);
}
