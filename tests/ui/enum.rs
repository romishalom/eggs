use eggs::Egg;

#[derive(Egg)]
enum Mode {
    A,
    B(u8),
}

fn main() {}
