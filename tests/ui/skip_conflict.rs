use eggs::Egg;

#[derive(Egg)]
struct S {
    #[egg(skip, ignore_diff)]
    a: u8,
}

fn main() {}
