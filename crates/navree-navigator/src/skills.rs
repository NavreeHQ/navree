//! The agent skills built into Navigator, and `navree skills:install`.
//!
//! The skills of the repository's `skills/` directory (every `navree` and
//! `navree-*` folder) are embedded at build time, so the installed skills
//! always match the CLI's version of Navree.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Where `skills:install` puts the skills, relative to the project:
/// Claude Code's folder and the shared folder other agents read.
pub const SKILL_DIRS: [&str; 2] = [".claude/skills", ".agents/skills"];

/// One embedded skill.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Skill {
    /// The folder name, which is the skill's `name`.
    pub name: String,
    /// The `description` of its `SKILL.md` front-matter.
    pub description: String,
}

/// Every embedded file of every skill: path (`navree-database/SKILL.md`)
/// and contents.
pub fn files() -> impl Iterator<Item = (&'static str, &'static [u8])> + Clone {
    crate::embedded::SKILL_FILES.iter().copied()
}

/// The embedded skills, by name.
pub fn skills() -> Vec<Skill> {
    let mut skills: BTreeMap<String, Skill> = BTreeMap::new();
    for (path, contents) in files() {
        let Some((folder, "SKILL.md")) = path.split_once('/') else {
            continue;
        };
        let text = String::from_utf8_lossy(contents);
        let front = front_matter(&text).unwrap_or_default();
        skills.insert(
            folder.to_owned(),
            Skill {
                name: front
                    .get("name")
                    .cloned()
                    .unwrap_or_else(|| folder.to_owned()),
                description: front.get("description").cloned().unwrap_or_default(),
            },
        );
    }
    skills.into_values().collect()
}

/// The `key: value` pairs of a `SKILL.md` front-matter (`---` fenced).
pub fn front_matter(text: &str) -> Option<BTreeMap<String, String>> {
    let text = text.replace("\r\n", "\n");
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    let mut pairs = BTreeMap::new();
    for line in rest[..end].lines() {
        if let Some((key, value)) = line.split_once(':')
            && !key.starts_with(' ')
        {
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .map_or_else(|| value.to_owned(), |v| v.replace("\\\"", "\""));
            pairs.insert(key.trim().to_owned(), value);
        }
    }
    Some(pairs)
}

/// What `skills:install` did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct InstallReport {
    /// The skill folders written (in each target).
    pub installed: Vec<String>,
    /// `navree*` folders removed because this version has no such skill.
    pub removed: Vec<String>,
    /// The folders the skills were written into.
    pub targets: Vec<PathBuf>,
}

/// Whether a skill folder belongs to Navree (`navree`, `navree-*`).
fn is_navree_skill(name: &str) -> bool {
    name == "navree" || name.starts_with("navree-")
}

/// Installs every embedded skill into `project/.claude/skills/` and
/// `project/.agents/skills/`: the `navree` and `navree-*` folders there are
/// replaced, every other folder is left alone.
pub fn install(project: &Path) -> io::Result<InstallReport> {
    install_files(project, files())
}

/// [`install`] with another set of files (tests).
pub fn install_files<'a>(
    project: &Path,
    files: impl Iterator<Item = (&'a str, &'a [u8])> + Clone,
) -> io::Result<InstallReport> {
    let mut installed: Vec<String> = files
        .clone()
        .filter_map(|(path, _)| path.split_once('/').map(|(folder, _)| folder.to_owned()))
        .filter(|folder| is_navree_skill(folder))
        .collect();
    installed.sort();
    installed.dedup();
    let mut report = InstallReport {
        installed: installed.clone(),
        ..InstallReport::default()
    };
    for dir in SKILL_DIRS {
        let target = project.join(dir);
        std::fs::create_dir_all(&target)?;
        for entry in std::fs::read_dir(&target)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_navree_skill(&name) {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                // Remove the link, never what it points at.
                std::fs::remove_file(entry.path())
                    .or_else(|_| std::fs::remove_dir(entry.path()))?;
            } else if kind.is_dir() {
                std::fs::remove_dir_all(entry.path())?;
            } else {
                continue;
            }
            if !installed.contains(&name) && !report.removed.contains(&name) {
                report.removed.push(name);
            }
        }
        for (path, contents) in files.clone() {
            let Some((folder, _)) = path.split_once('/') else {
                continue;
            };
            if !is_navree_skill(folder)
                || path.split('/').any(|part| part == ".." || part.is_empty())
            {
                continue;
            }
            let file = target.join(path);
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(file, contents)?;
        }
        report.targets.push(target);
    }
    report.removed.sort();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILES: &[(&str, &[u8])] = &[
        (
            "navree/SKILL.md",
            b"---\nname: navree\ndescription: The index.\n---\n# Navree\n",
        ),
        (
            "navree-database/SKILL.md",
            b"---\nname: navree-database\ndescription: Entities.\n---\n",
        ),
        (
            "navree-database/references/migrations.md",
            b"# Migrations\n",
        ),
    ];

    fn read(path: PathBuf) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn every_embedded_skill_is_named_after_its_folder() {
        let skills = skills();
        assert!(
            skills.iter().any(|s| s.name == "navree"),
            "the umbrella skill"
        );
        assert!(skills.iter().any(|s| s.name == "navree-navigator"));
        // MOD-021: the skill for building Navree UI pages is installed with its references.
        assert!(skills.iter().any(|s| s.name == "navree-ui"), "the UI skill");
        assert!(
            files().any(|(path, _)| path == "navree-ui/references/pickers-and-dates.md"),
            "the UI skill's references are embedded with it"
        );
        for (path, contents) in files() {
            let Some((folder, "SKILL.md")) = path.split_once('/') else {
                continue;
            };
            let text = String::from_utf8(contents.to_vec()).expect("UTF-8");
            let front = front_matter(&text).unwrap_or_else(|| panic!("{path}: no front-matter"));
            assert_eq!(
                front.get("name").map(String::as_str),
                Some(folder),
                "{path}"
            );
            assert!(
                front.get("description").is_some_and(|d| d.len() > 40),
                "{path}: a description says when to use the skill"
            );
        }
    }

    #[test]
    fn installs_into_both_folders() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let report = install_files(dir.path(), FILES.iter().copied()).unwrap();
        assert_eq!(report.installed, ["navree", "navree-database"]);
        for target in SKILL_DIRS {
            let skills = dir.path().join(target);
            assert!(read(skills.join("navree/SKILL.md")).contains("The index."));
            assert_eq!(
                read(skills.join("navree-database/references/migrations.md")),
                "# Migrations\n"
            );
        }
        assert_eq!(report.targets.len(), 2);
    }

    #[test]
    fn installing_again_changes_nothing() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        install_files(dir.path(), FILES.iter().copied()).unwrap();
        let report = install_files(dir.path(), FILES.iter().copied()).unwrap();
        assert!(report.removed.is_empty());
        assert!(read(dir.path().join(".claude/skills/navree/SKILL.md")).contains("The index."));
    }

    #[test]
    fn other_skills_are_left_alone_and_stale_navree_skills_go() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let skills = dir.path().join(".claude/skills");
        std::fs::create_dir_all(skills.join("crm-pipeline-stages")).unwrap();
        std::fs::write(skills.join("crm-pipeline-stages/SKILL.md"), "mine").unwrap();
        std::fs::create_dir_all(skills.join("navree-old")).unwrap();
        std::fs::write(skills.join("navree-old/SKILL.md"), "old").unwrap();
        std::fs::create_dir_all(skills.join("navree")).unwrap();
        std::fs::write(skills.join("navree/STALE.md"), "stale").unwrap();
        std::fs::write(skills.join("navree-notes.txt"), "a file, not a skill").unwrap();

        let report = install_files(dir.path(), FILES.iter().copied()).unwrap();
        assert_eq!(report.removed, ["navree-old"]);
        assert_eq!(read(skills.join("crm-pipeline-stages/SKILL.md")), "mine");
        assert!(!skills.join("navree-old").exists());
        assert!(
            !skills.join("navree/STALE.md").exists(),
            "navree is replaced whole"
        );
        assert!(skills.join("navree-notes.txt").exists());
    }
}
