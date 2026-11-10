use navree_openapi::ToSchema;

#[derive(ToSchema)]
struct List {
    #[schema(inline)]
    next: Option<Box<List>>,
}

fn main() {}
