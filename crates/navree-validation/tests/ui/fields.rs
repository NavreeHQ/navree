//! Fields without rules, comparisons with missing fields, unions.

use navree_validation::Validate;

#[derive(Validate)]
struct Unmarked {
    #[validate(length(min = 1))]
    name: String,
    nickname: String,
}

#[derive(Validate)]
struct UnknownSibling {
    #[validate(same_as(pasword))]
    password_confirmation: String,
    #[validate(skip)]
    password: String,
}

#[derive(Validate)]
struct BareAttribute {
    #[validate]
    name: String,
}

#[derive(Validate)]
union Bits {
    int: u32,
    float: f32,
}

fn main() {}
