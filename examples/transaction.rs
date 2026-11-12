//! Group edits; abort restores the original state.
use eggs::{transaction, Egg};

#[derive(Egg, Debug)]
struct Counter {
    count: u32,
    enabled: bool,
}

fn main() {
    let mut state = Counter { count: 0, enabled: false };
    {
        let mut tx = transaction(&mut state).unwrap();
        tx.edit(|s| {
            s.count += 1;
            s.enabled = true;
        })
        .unwrap();
        tx.abort().unwrap();
    }
    println!("after abort: {state:?}");

    let mut tx = transaction(&mut state).unwrap();
    tx.edit(|s| s.count = 10).unwrap();
    tx.commit().unwrap();
    drop(tx);
    println!("after commit: {state:?}");
}
