use navree_openapi::ToSchema;
use serde::Serialize;

#[derive(Serialize, ToSchema)]
enum State {
    #[serde(rename(serialize = "on"))]
    Active,
}

fn main() {}
