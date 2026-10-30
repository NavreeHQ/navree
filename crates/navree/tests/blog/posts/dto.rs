//! Request bodies and their validation rules.

use navree::validation::Validate;
use serde::Deserialize;
use utoipa::ToSchema;

/// The body of `POST /api/v1/posts`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreatePost {
    /// 3 to 200 characters.
    #[validate(length(min = 3, max = 200))]
    pub title: String,
    /// Not empty.
    #[validate(length(min = 1, max = 10_000))]
    pub body: String,
}

/// The body of `PATCH /api/v1/posts/{id}`: only the fields present change.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdatePost {
    /// 3 to 200 characters.
    #[validate(length(min = 3, max = 200))]
    pub title: Option<String>,
    /// Not empty.
    #[validate(length(min = 1, max = 10_000))]
    pub body: Option<String>,
}

impl UpdatePost {
    /// The fields this update changes, for field-level ability checks.
    pub fn changed_fields(&self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        if self.title.is_some() {
            fields.push("title");
        }
        if self.body.is_some() {
            fields.push("body");
        }
        fields
    }
}
