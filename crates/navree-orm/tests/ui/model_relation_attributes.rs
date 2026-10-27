use navree_orm::Model;

#[derive(Model, Clone, Default, serde::Serialize)]
#[model(touches = "company")]
struct Touching {
    id: i64,
}

#[derive(Model, Clone, Default, serde::Serialize)]
#[model(touches = ["company owner"])]
struct BadName {
    id: i64,
}

#[derive(Model, Clone, Default, serde::Serialize)]
#[model(with = ["company..owner"])]
struct BadPath {
    id: i64,
}

#[derive(Model, Clone, Default, serde::Serialize)]
#[model(with_count = ["company.contacts"])]
struct NestedCount {
    id: i64,
}

#[derive(Model, Clone, Default, serde::Serialize)]
#[model(morph_name = " ")]
struct EmptyMorphName {
    id: i64,
}

#[derive(Model, Clone, Default, serde::Serialize)]
#[model(pivot(foreign_key = "deal_id"))]
struct HalfPivot {
    deal_id: i64,
    tag_id: i64,
}

#[derive(Model, Clone, Default, serde::Serialize)]
#[model(pivot(foreign_key = "deal_id", related_key = "label_id"))]
struct MissingColumn {
    deal_id: i64,
    tag_id: i64,
}

#[derive(Model, Clone, Default, serde::Serialize)]
#[model(pivot(left = "deal_id", related_key = "tag_id"))]
struct UnknownOption {
    deal_id: i64,
    tag_id: i64,
}

fn main() {}
