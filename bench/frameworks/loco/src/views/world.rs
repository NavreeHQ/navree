use serde::Serialize;

use crate::models::world;

#[derive(Debug, Serialize)]
pub struct WorldResponse {
    pub id: i32,
    #[serde(rename = "randomNumber")]
    pub random_number: i32,
}

impl From<world::Model> for WorldResponse {
    fn from(row: world::Model) -> Self {
        Self {
            id: row.id,
            random_number: row.randomnumber,
        }
    }
}
