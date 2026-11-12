//! `CHANGELOG.md`: one section per release, newest first, in the form
//! "Keep a Changelog" describes (Added, Changed, Deprecated, Removed, Fixed,
//! Security).

/// The repository the release links point at.
pub const REPOSITORY: &str = "https://github.com/NavreeHQ/navree";

/// Whether the changelog has a section for `version` (`## [1.2.3] ...` or
/// `## 1.2.3 ...`).
pub fn has_section(text: &str, version: &str) -> bool {
    text.lines().any(|line| {
        line.strip_prefix("## ").is_some_and(|heading| {
            let heading = heading.trim_start_matches('[');
            heading
                .strip_prefix(version)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with([']', ' ']))
        })
    })
}

/// The changelog with a section for `version` dated `date`, above the
/// newest one, holding `body` (which may be empty). When the file links its
/// headings to releases (`[1.2.2]: https://...`), the new heading gets its
/// link too. Unchanged when the section exists.
pub fn add_section(text: &str, version: &str, date: &str, body: &str) -> String {
    if has_section(text, version) {
        return text.to_owned();
    }
    let mut section = format!("## [{version}] - {date}\n\n");
    if !body.trim().is_empty() {
        section.push_str(body.trim_end());
        section.push_str("\n\n");
    }
    let link = format!("[{version}]: {REPOSITORY}/releases/tag/v{version}\n");
    let mut out = String::with_capacity(text.len() + section.len() + link.len());
    let mut placed_section = false;
    let mut placed_link = false;
    let has_links = text.lines().any(is_release_link);
    for line in text.split_inclusive('\n') {
        if !placed_section && line.starts_with("## ") {
            out.push_str(&section);
            placed_section = true;
        }
        if has_links && !placed_link && is_release_link(line.trim_end()) {
            out.push_str(&link);
            placed_link = true;
        }
        out.push_str(line);
    }
    if !placed_section {
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push_str(if out.ends_with('\n') { "\n" } else { "\n\n" });
        }
        out.push_str(&section);
    }
    out
}

/// The body of the section for `version`, without its heading and its
/// surrounding blank lines: from the heading to the next `## ` heading or
/// the release links at the end. `None` when there is no such section.
pub fn section_body(text: &str, version: &str) -> Option<String> {
    let mut body = String::new();
    let mut found = false;
    let mut inside = false;
    for line in text.split_inclusive('\n') {
        if let Some(heading) = line.strip_prefix("## ") {
            if inside {
                break;
            }
            let heading = heading.trim_start_matches('[');
            inside = heading
                .strip_prefix(version)
                .is_some_and(|rest| rest.trim_end().is_empty() || rest.starts_with([']', ' ']));
            found |= inside;
        } else if inside {
            if is_release_link(line.trim_end()) {
                break;
            }
            body.push_str(line);
        }
    }
    found.then(|| body.replace('\r', "").trim().to_owned() + "\n")
}

/// A line like `[1.2.2]: https://...`.
fn is_release_link(line: &str) -> bool {
    line.strip_prefix('[')
        .and_then(|rest| rest.split_once("]: http"))
        .is_some_and(|(version, _)| version.starts_with(|c: char| c.is_ascii_digit()))
}

/// A first draft of a section from the commit subjects since the last
/// release (conventional commits: `feat(ui): ...`, `fix: ...`): features
/// under Added, fixes under Fixed, performance work under Changed, and a
/// breaking change (`feat!:`) under Changed in bold. Other commits are left
/// out. The release pull request is where a person edits it.
pub fn draft(subjects: &[String]) -> String {
    let mut added = Vec::new();
    let mut changed = Vec::new();
    let mut fixed = Vec::new();
    for subject in subjects {
        let Some((kind, text)) = subject.split_once(": ") else {
            continue;
        };
        let breaking = kind.ends_with('!');
        let kind = kind.trim_end_matches('!');
        let (kind, scope) = match kind.split_once('(') {
            Some((kind, scope)) => (kind, Some(scope.trim_end_matches(')'))),
            None => (kind, None),
        };
        let mut entry = String::from("- ");
        if breaking {
            entry.push_str("**Breaking:** ");
        }
        if let Some(scope) = scope {
            entry.push_str(&format!("{scope}: "));
        }
        entry.push_str(text.trim());
        match (kind, breaking) {
            (_, true) => changed.push(entry),
            ("feat", _) => added.push(entry),
            ("fix", _) => fixed.push(entry),
            ("perf", _) => changed.push(entry),
            _ => {}
        }
    }
    let mut out = String::new();
    for (title, entries) in [("Added", added), ("Changed", changed), ("Fixed", fixed)] {
        if entries.is_empty() {
            continue;
        }
        out.push_str(&format!("### {title}\n\n"));
        for entry in entries {
            out.push_str(&entry);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}
