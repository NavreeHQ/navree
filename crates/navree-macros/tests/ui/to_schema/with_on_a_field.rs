use navree_openapi::ToSchema;
use serde::Serialize;

fn as_text<S: serde::Serializer>(value: &u32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&value.to_string())
}

#[derive(Serialize, ToSchema)]
struct Item {
    #[serde(serialize_with = "as_text")]
    count: u32,
}

fn main() {}
