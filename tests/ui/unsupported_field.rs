use eggs::Egg;

struct NotAnEgg;

#[derive(Egg)]
struct S {
    a: NotAnEgg,
}

fn main() {}
