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
struct ServerOnlyFlagsAreFine {
    #[validate(pattern("(?i)^[a-z]+$"))]
    handle: String,
}

fn main() {
    let _ = ServerOnlyFlagsAreFine { handle: String::new() }.validate();
}
