use eggs::Egg;

#[derive(Egg)]
struct S {
    a: u8,
    #[egg(rename = "a")]
    b: u8,
}

fn main() {}
