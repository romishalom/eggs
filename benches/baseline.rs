//! Baseline timings (no external harness): snapshot, restore, diff, history record.
use eggs::{Egg, EggExt, History};
use std::hint::black_box;
use std::time::Instant;

#[derive(Egg, Clone)]
struct Small {
    a: u32,
    b: String,
}

#[derive(Egg, Clone)]
struct Large {
    items: Vec<Small>,
}

fn time(label: &str, iters: u32, mut f: impl FnMut()) {
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    println!("{label:<28} {:>10.2?} / iter", start.elapsed() / iters);
}

fn main() {
    let small = Small { a: 1, b: "hello".into() };
    let large = Large { items: (0..10_000).map(|i| Small { a: i, b: format!("item{i}") }).collect() };
    let mut changed = large.clone();
    changed.items[5000].a = 0;

    time("snapshot small", 100_000, || { black_box(small.capture()); });
    time("snapshot large (10k)", 100, || { black_box(large.capture()); });
    let snap = large.capture();
    let mut target = large.clone();
    time("restore large", 100, || snap.restore_into(&mut target).unwrap());
    time("diff large (equal)", 100, || { black_box(large.diff(&large)); });
    time("diff large (1 change)", 100, || { black_box(large.diff(&changed)); });
    let mut history = History::with_limit(&small, 128).unwrap();
    time("history record small", 100_000, || history.record(&small));
}
