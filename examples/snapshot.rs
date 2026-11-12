//! The basics: capture a snapshot, change things, see what changed, roll back.
//!
//! Run with `cargo run --example snapshot`.

use eggs::{Egg, EggExt};

#[derive(Egg, Clone, Debug)]
struct Player {
    name: String,
    health: u32,
    position: (f32, f32),
    /// Skipped: never captured, diffed or restored.
    #[egg(skip)]
    frame_cache: Vec<u8>,
}

fn main() -> eggs::EggResult<()> {
    let mut player = Player {
        name: "Ann".into(),
        health: 100,
        position: (0.0, 0.0),
        frame_cache: Vec::new(),
    };

    // 1. Capture the current state.
    let checkpoint = player.capture();

    // 2. Change things.
    player.health = 35;
    player.position.0 = 12.5;
    player.frame_cache.extend([1, 2, 3]);
    println!("after the fight: {player:?}");

    // 3. What changed since the snapshot?
    for change in checkpoint.diff(&player)?.changes() {
        println!("  changed: {} ({:?})", change.path, change.kind);
    }

    // 4. Roll back. The restore is atomic: it fully succeeds or changes nothing.
    checkpoint.try_restore(&mut player)?;
    println!("after rollback:  {player:?}");
    // `frame_cache` was marked #[egg(skip)], so rollback left it alone.

    Ok(())
}
