use eggs::Egg;

#[derive(Egg)]
struct S {
    #[egg(strategy = "cow")]
    a: u8,
}

fn main() {}
