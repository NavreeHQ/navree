//! `navree harbor:ci github`: a GitHub Actions workflow that tests and
//! deploys every push.

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::build::relative;
use crate::config::Environment;
use crate::error::{HarborError, Result};
use crate::render;

/// The repository containing `dir`: the nearest directory with `.git`.
pub fn repository_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|d| d.join(".git").exists())
        .map(Path::to_path_buf)
}

/// The workflow's path and contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workflow {
    /// `.github/workflows/deploy.yml` (`deploy-<env>.yml` for another
    /// environment) at the repository root.
    pub path: PathBuf,
    /// The YAML.
    pub contents: String,
}

/// The workflow deploying the app in `dir` on every push to `branch`.
pub fn github_workflow(dir: &Path, env: &Environment, branch: &str) -> Result<Workflow> {
    let root = repository_root(dir).unwrap_or_else(|| dir.to_path_buf());
    let file = match env.name() {
        "production" => "deploy.yml".to_owned(),
        name => format!("deploy-{name}.yml"),
    };
    let contents = render::render(
        "ci/github.yml",
        json!({
            "env_name": env.name(),
            "env_flag": env.flag(),
            "branch": branch,
            "working_directory": relative(dir, &root),
        }),
    )?;
    Ok(Workflow {
        path: root.join(".github").join("workflows").join(file),
        contents,
    })
}

/// Writes the workflow; refuses to overwrite one.
pub fn write_workflow(workflow: &Workflow) -> Result<()> {
    if workflow.path.exists() {
        return Err(HarborError::msg(format!(
            "{} already exists",
            workflow.path.display()
        )));
    }
    if let Some(parent) = workflow.path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| HarborError::io(parent, e))?;
    }
    std::fs::write(&workflow.path, &workflow.contents)
        .map_err(|e| HarborError::io(&workflow.path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_production_workflow() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir(repo.path().join(".git")).unwrap();
        let workflow = github_workflow(repo.path(), &Environment::production(), "main").unwrap();
        assert_eq!(
            workflow.path,
            repo.path().join(".github/workflows/deploy.yml")
        );
        insta::assert_snapshot!(workflow.contents, @r"
        name: Deploy

        on:
          push:
            branches: [main]

        concurrency:
          group: deploy-production
          cancel-in-progress: false

        jobs:
          deploy:
            runs-on: ubuntu-24.04
            environment: production
            steps:
              - uses: actions/checkout@v4
              - uses: dtolnay/rust-toolchain@stable
              - uses: Swatinem/rust-cache@v2
              - run: cargo test --locked
              - run: cargo install --git https://github.com/NavreeHQ/navree navree-cli --locked
              - run: navree deploy
                env:
                  NAVREE_HARBOR_KEY: ${{ secrets.NAVREE_HARBOR_KEY }}
                  NAVREE_HARBOR_SSH_KEY: ${{ secrets.NAVREE_HARBOR_SSH_KEY }}
        ");
        write_workflow(&workflow).unwrap();
        assert!(write_workflow(&workflow).is_err());
    }

    #[test]
    fn a_staging_workflow_for_a_service_in_a_subdirectory() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir(repo.path().join(".git")).unwrap();
        let dir = repo.path().join("services").join("billing");
        std::fs::create_dir_all(&dir).unwrap();
        let workflow =
            github_workflow(&dir, &Environment::named("staging").unwrap(), "develop").unwrap();
        assert_eq!(
            workflow.path,
            repo.path().join(".github/workflows/deploy-staging.yml")
        );
        let text = &workflow.contents;
        assert!(text.starts_with("name: Deploy (staging)\n\non:"), "{text}");
        assert!(text.contains("branches: [develop]"), "{text}");
        assert!(text.contains("group: deploy-staging"), "{text}");
        assert!(
            text.contains("      - run: navree deploy --env staging\n        working-directory: services/billing\n        env:\n"),
            "{text}"
        );
    }
}
