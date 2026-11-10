use navree_validation::Validate;

#[derive(Validate)]
struct UnknownOption {
    #[validate(pattern("^[a-z]+$", client))]
    handle: String,
}

#[derive(Validate)]
struct InlineFlags {
    #[validate(pattern("(?i)^[a-z]+$", browser))]
    handle: String,
}

#[derive(Validate)]
struct NamedGroup {
    #[validate(pattern("^(?P<word>[a-z]+)$", browser))]
    handle: String,
}

#[derive(Validate)]
struct RustAnchors {
    #[validate(pattern("\\A[a-z]+\\z", browser))]
    handle: String,
}

#[derive(Validate)]
struct UnicodeWord {
    #[validate(pattern("^\\w+$", browser))]
    handle: String,
}

#[derive(Validate)]
struct UnicodeDigit {
    #[validate(pattern("^\\d{4}$", browser))]
    code: String,
}

#[derive(Validate)]
struct Unanchored {
    #[validate(pattern("[0-9]", browser))]
    code: String,
}

#[derive(Validate)]
struct TopLevelChoice {
    #[validate(pattern("^a|b$", browser))]
    code: String,
}

#[derive(Validate)]
struct LooseDash {
    #[validate(pattern("^[a-z_-]+$", browser))]
    slug: String,
}

#[derive(Validate)]
struct AnyCharacter {
    #[validate(pattern("^.+$", browser))]
    slug: String,
}

#[derive(Validate)]
struct ServerOnlyIsFree {
    #[validate(pattern("(?i)^\\w+[a-z_-]$"))]
    handle: String,
}

fn main() {
    let _ = ServerOnlyIsFree { handle: String::new() }.validate();
}
