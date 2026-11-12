use eggs::Egg;

#[derive(Egg)]
struct S {
    #[egg(default)]
    a: u8,
}

fn main() {}
