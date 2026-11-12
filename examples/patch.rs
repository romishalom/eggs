//! Diff two states, turn the diff into a patch, apply and reverse it.
use eggs::{Egg, Patch};
use std::collections::BTreeMap;

#[derive(Egg, Clone, Debug, PartialEq)]
struct Config {
    volume: u8,
    tags: Vec<String>,
    limits: BTreeMap<String, u32>,
}

fn main() {
    let old = Config {
        volume: 3,
        tags: vec!["a".into()],
        limits: BTreeMap::from([("cpu".to_string(), 1)]),
    };
    let mut new = old.clone();
    new.volume = 8;
    new.tags.push("b".into());
    new.limits.insert("mem".into(), 512);

    let diff = old.diff(&new);
    for change in diff.changes() {
        println!("{:?} at {}", change.kind, change.path);
    }

    let patch = Patch::from_diff(&diff).unwrap();
    let mut state = old.clone();
    patch.apply(&mut state).unwrap();
    assert_eq!(state, new);
    patch.reverse().apply(&mut state).unwrap();
    assert_eq!(state, old);
    println!("applied and reversed {} operations", patch.len());
}
