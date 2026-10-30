//! Mistakes in the rules themselves, pointed at the rule.

use navree_validation::Validate;

#[derive(Validate)]
struct UnknownRule {
    #[validate(frobnicate)]
    name: String,
}

#[derive(Validate)]
struct FamiliarName {
    #[validate(dive)]
    address: String,
}

#[derive(Validate)]
struct SkipAndRules {
    #[validate(skip, email)]
    email: String,
}

#[derive(Validate)]
struct Twice {
    #[validate(email, length(max = 5), email)]
    email: String,
}

#[derive(Validate)]
struct NoBound {
    #[validate(length())]
    name: String,
}

#[derive(Validate)]
struct NeverPasses {
    #[validate(range(min = 10, max = 1))]
    count: u32,
}

#[derive(Validate)]
struct Chars {
    #[validate(length(chars, max = 10))]
    name: String,
}

#[derive(Validate)]
struct BadPattern {
    #[validate(pattern("[a-z"))]
    code: String,
}

#[derive(Validate)]
struct Arguments {
    #[validate(email(strict))]
    email: String,
}

#[derive(Validate)]
struct EmptyEach {
    #[validate(each())]
    tags: Vec<String>,
}

#[derive(Validate)]
#[validate(strict)]
struct ContainerOption {
    #[validate(skip)]
    name: String,
}

fn main() {}
