use eggs::Egg;

mod adapter {
    pub type Snapshot = u8;
    pub fn snapshot(_: &u8) -> Snapshot { 0 }
    // `restore` has the wrong signature.
    pub fn restore(_: &mut u8) {}
    pub fn from_snapshot(_: &Snapshot) -> eggs::EggResult<u8> { Ok(0) }
    pub fn diff(_: &u8, _: &u8, _: &eggs::EggPath, _: &mut Vec<eggs::Change>) {}
}

#[derive(Egg)]
struct S {
    #[egg(with = "adapter")]
    a: u8,
}

fn main() {}
