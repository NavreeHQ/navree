//! `navree harbor:ci github`: a GitHub Actions workflow that tests and
//! deploys every push.

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::build::relative;
use crate::config::Environment;
use crate::error::{HarborError, Result};
use crate::render;

/// The repository `dir` belongs to: the nearest directory with `.git`,
/// unless that repository ignores `dir` (an app generated under a build
/// directory such as `target/` is not part of the repository around it, and
/// a workflow written there would land in someone else's checkout).
pub fn repository_root(dir: &Path) -> Option<PathBuf> {
    let root = dir.ancestors().find(|d| d.join(".git").exists())?;
    if root != dir && ignored_by(root, dir) {
        return None;
    }
    Some(root.to_path_buf())
}

/// Whether the repository at `root` ignores `path` (or a directory above
/// it). `false` when Git is not available or `root` is not a working
/// repository.
fn ignored_by(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "-q", "--"])
        .arg(relative)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
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
        let repo = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
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
              - run: cargo install navree-cli --locked
              - run: navree deploy
                env:
                  NAVREE_HARBOR_KEY: ${{ secrets.NAVREE_HARBOR_KEY }}
                  NAVREE_HARBOR_SSH_KEY: ${{ secrets.NAVREE_HARBOR_SSH_KEY }}
        ");
        write_workflow(&workflow).unwrap();
        assert!(write_workflow(&workflow).is_err());
    }

    fn git(dir: &Path, args: &[&str]) -> bool {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    #[test]
    fn an_app_ignored_by_the_repository_around_it_gets_its_own_workflow() {
        let repo = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        if !git(repo.path(), &["init", "-q"]) {
            println!("SKIPPED: git is not available");
            return;
        }
        std::fs::write(repo.path().join(".gitignore"), "target/\n").unwrap();
        let app = repo.path().join("target").join("audit").join("app");
        std::fs::create_dir_all(&app).unwrap();
        assert_eq!(repository_root(&app), None);
        let workflow = github_workflow(&app, &Environment::production(), "main").unwrap();
        assert_eq!(workflow.path, app.join(".github/workflows/deploy.yml"));
        // The same repository still owns a service that it tracks.
        let service = repo.path().join("services").join("billing");
        std::fs::create_dir_all(&service).unwrap();
        assert_eq!(repository_root(&service).as_deref(), Some(repo.path()));
        let workflow = github_workflow(&service, &Environment::production(), "main").unwrap();
        assert_eq!(
            workflow.path,
            repo.path().join(".github/workflows/deploy.yml")
        );
    }

    #[test]
    fn an_app_with_a_repository_of_its_own_uses_it() {
        let outer = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let app = outer.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        if !git(&app, &["init", "-q"]) {
            println!("SKIPPED: git is not available");
            return;
        }
        std::fs::write(outer.path().join(".gitignore"), "app/\n").unwrap();
        assert!(git(outer.path(), &["init", "-q"]));
        assert_eq!(repository_root(&app).as_deref(), Some(app.as_path()));
    }

    #[test]
    fn a_staging_workflow_for_a_service_in_a_subdirectory() {
        let repo = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
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
