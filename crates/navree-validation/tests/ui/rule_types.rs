//! Rules on fields of the wrong type: the message names the rule and what
//! it needs.

use navree_validation::Validate;

#[derive(Validate)]
struct Mismatched {
    #[validate(email)]
    age: u32,
    #[validate(range(min = 1))]
    name: String,
    #[validate(length(max = 3))]
    count: u32,
    #[validate(required)]
    title: String,
    #[validate(each(length(max = 3)))]
    tag: String,
    #[validate(nested)]
    city: String,
}

fn main() {}
