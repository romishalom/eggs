use eggs::Egg;

#[derive(Egg)]
struct S {
    #[egg(frobnicate)]
    a: u8,
}

fn main() {}
