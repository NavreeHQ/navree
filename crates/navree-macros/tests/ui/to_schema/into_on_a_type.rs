use navree_openapi::ToSchema;
use serde::Serialize;

#[derive(Clone, Serialize, ToSchema)]
#[serde(into = "String")]
struct Label {
    text: String,
}

impl From<Label> for String {
    fn from(label: Label) -> String {
        label.text
    }
}

fn main() {}
