use navree_macros::flag;

/// A flag with fields.
#[flag]
pub struct WithFields {
    on: bool,
}

#[flag]
pub enum NotAStruct {
    On,
}

#[flag]
pub struct Generic<T>(std::marker::PhantomData<T>);

#[flag(label = "board")]
pub struct UnknownKey;

#[flag(name = "New Board")]
pub struct BadName;

#[flag(default = "yes")]
pub struct BadDefault;

#[flag(expose, expose)]
pub struct Twice;

fn main() {}
