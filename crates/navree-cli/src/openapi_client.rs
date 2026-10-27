//! `navree openapi:client`: the typed client's schema for the front end.
//! The app exports its OpenAPI document (`openapi:export`), then
//! `openapi-typescript` turns it into `schema.d.ts`, the `paths` type that
//! `createNavreeClient<paths>()` from `@navree/client` takes.

use std::path::{Path, PathBuf};

use crate::forward::{Invocation, forward};

/// The `openapi-typescript` release line the generator runs.
pub const OPENAPI_TYPESCRIPT: &str = "openapi-typescript@7";

/// The default output directory: the Next.js front end's API folder.
pub const DEFAULT_OUT: &str = "web/src/lib/api";

/// What `navree openapi:client` runs, in order, in the app's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientPlan {
    /// The OpenAPI document the schema is generated from.
    pub document: PathBuf,
    /// The generated `schema.d.ts`.
    pub schema: PathBuf,
    /// The export (absent with `--input`), then the generator.
    pub steps: Vec<Invocation>,
}

/// The command name of a Node tool (`npx.cmd` on Windows).
fn node_program(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_owned()
    }
}

/// `path` with `/` separators, as tools on every platform accept it.
fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// The steps for `out` (relative to the app), from the app's own document
/// or from `input`.
pub fn plan(out: &Path, input: Option<&Path>) -> ClientPlan {
    let schema = out.join("schema.d.ts");
    let mut steps = Vec::new();
    let document = match input {
        Some(input) => input.to_path_buf(),
        None => {
            let document = out.join("openapi.json");
            steps.push(forward("openapi:export", &[slash(&document)]));
            document
        }
    };
    steps.push(Invocation::new(
        node_program("npx"),
        [
            "--yes".to_owned(),
            OPENAPI_TYPESCRIPT.to_owned(),
            slash(&document),
            "--output".to_owned(),
            slash(&schema),
        ],
    ));
    ClientPlan {
        document,
        schema,
        steps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_the_document_then_generates_the_schema() {
        let plan = plan(Path::new("web/src/lib/api"), None);
        assert_eq!(plan.document, Path::new("web/src/lib/api").join("openapi.json"));
        assert_eq!(plan.schema, Path::new("web/src/lib/api").join("schema.d.ts"));
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].program, "cargo");
        assert_eq!(
            plan.steps[0].args,
            ["run", "--quiet", "--", "openapi:export", "web/src/lib/api/openapi.json"]
        );
        assert!(plan.steps[1].program.starts_with("npx"));
        assert_eq!(
            plan.steps[1].args,
            [
                "--yes",
                "openapi-typescript@7",
                "web/src/lib/api/openapi.json",
                "--output",
                "web/src/lib/api/schema.d.ts"
            ]
        );
    }

    #[test]
    fn generates_from_a_given_document_without_exporting() {
        let plan = plan(Path::new("web/app/lib/api"), Some(Path::new("openapi.json")));
        assert_eq!(plan.document, Path::new("openapi.json"));
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(
            plan.steps[0].args,
            [
                "--yes",
                "openapi-typescript@7",
                "openapi.json",
                "--output",
                "web/app/lib/api/schema.d.ts"
            ]
        );
    }

    #[test]
    fn runs_the_windows_shim_of_npx_on_windows() {
        let program = &plan(Path::new("out"), None).steps[1].program;
        if cfg!(windows) {
            assert_eq!(program, "npx.cmd");
        } else {
            assert_eq!(program, "npx");
        }
    }
}
