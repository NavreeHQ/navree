//! Request and response bodies of `/api/v1/projects`.

use chrono::{DateTime, Utc};
use navree::openapi::ToSchema;
use navree::orm::list::{Field, ListResource, ListSpec};
use navree::validation::Validate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::entity::Project;

/// Whether a project is in use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    /// In use (the default).
    #[default]
    Active,
    /// Kept for reference.
    Archived,
}

impl ProjectStatus {
    /// The value stored in `projects.status`.
    pub fn as_str(self) -> &'static str {
        match self {
            ProjectStatus::Active => "active",
            ProjectStatus::Archived => "archived",
        }
    }
}

/// The body of `POST /api/v1/projects`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateProject {
    /// 1 to 120 characters.
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    /// At most 2000 characters.
    #[validate(length(max = 2000))]
    pub description: Option<String>,
    /// `active` (the default) or `archived`.
    #[validate(skip)]
    #[serde(default)]
    pub status: ProjectStatus,
}

/// The body of `PATCH /api/v1/projects/{id}`: only the fields present
/// change.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
pub struct UpdateProject {
    /// 1 to 120 characters.
    #[validate(length(min = 1, max = 120))]
    pub name: Option<String>,
    /// At most 2000 characters.
    #[validate(length(max = 2000))]
    pub description: Option<String>,
    /// `active` or `archived`.
    #[validate(skip)]
    pub status: Option<ProjectStatus>,
}

impl UpdateProject {
    /// The fields the body changes (for field-level access checks).
    pub fn changed_fields(&self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        if self.name.is_some() {
            fields.push("name");
        }
        if self.description.is_some() {
            fields.push("description");
        }
        if self.status.is_some() {
            fields.push("status");
        }
        fields
    }
}

/// The list query of `GET /api/v1/projects`, in the list-query grammar:
///
/// - `filter[status]`: `active` or `archived`; several with
///   `filter[status][in]=active,archived`.
/// - `search`: part of the name, ignoring case.
/// - `sort`: `name`, `created_at` or `updated_at`, with `-` in front for
///   descending. The default is `-created_at`.
/// - `page` and `per_page` (default 15, at most 100).
///
/// An unknown filter or sort, a value of the wrong kind or a `per_page` over
/// 100 is a 400 that names the parameter.
pub struct ProjectList;

impl ListResource for ProjectList {
    const SPEC: ListSpec = ListSpec::new(&[Field::one_of("status", &["active", "archived"])])
        .sortable(&["name", "created_at", "updated_at"])
        .default_sort(&["-created_at"])
        .searchable(&["name"]);
}

/// A project as the API returns it.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = Project)]
pub struct ProjectResource {
    /// Always `"Project"` (the access subject, for the front end's rules).
    #[serde(rename = "__type")]
    #[schema(example = "Project")]
    pub resource_type: &'static str,
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&Project> for ProjectResource {
    fn from(project: &Project) -> Self {
        Self {
            resource_type: "Project",
            id: project.id,
            workspace_id: project.workspace_id,
            name: project.name.clone(),
            description: project.description.clone(),
            status: project.status.clone(),
            created_by: project.created_by,
            created_at: project.created_at,
            updated_at: project.updated_at,
        }
    }
}

/// A page of projects, for the OpenAPI document.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectPage {
    /// The projects.
    pub data: Vec<ProjectResource>,
    /// Where the page is.
    #[schema(inline)]
    pub meta: PageMetaSchema,
}

/// The pagination metadata (`navree::db::PageMeta`), for the OpenAPI
/// document.
#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub struct PageMetaSchema {
    /// The page number.
    pub current_page: u64,
    /// The page size.
    pub per_page: u64,
    /// The number of matching projects.
    pub total: u64,
    /// The last page number (at least 1).
    pub last_page: u64,
}

#[cfg(test)]
mod tests {
    use navree::orm::list::Kind;

    use super::*;

    #[test]
    fn the_status_filter_takes_exactly_the_statuses() {
        let field = ProjectList::SPEC.field("status").expect("a status filter");
        let Kind::OneOf(words) = field.kind else {
            panic!("a list of words");
        };
        assert_eq!(
            words,
            [
                ProjectStatus::Active.as_str(),
                ProjectStatus::Archived.as_str()
            ]
        );
    }
}
