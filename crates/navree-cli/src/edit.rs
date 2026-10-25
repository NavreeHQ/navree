//! Small, conservative edits of existing Rust files: declaring a module and
//! adding a line above a `// navree:<name>` marker. Each edit either applies
//! cleanly or reports that it could not, so the caller prints a hint instead
//! of guessing.

/// The outcome of an edit that could be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// The new contents.
    Changed(String),
    /// The line was already there.
    Unchanged,
}

/// The line ending `src` uses (`\r\n` if it has any, else `\n`).
fn newline(src: &str) -> &'static str {
    if src.contains("\r\n") { "\r\n" } else { "\n" }
}

/// Lines (without their endings) and whether the text ended with a newline.
fn split(src: &str) -> (Vec<&str>, bool) {
    (src.lines().collect(), src.ends_with('\n') || src.is_empty())
}

fn join(lines: &[String], nl: &str, trailing: bool) -> String {
    let mut out = lines.join(nl);
    if trailing && !out.is_empty() {
        out.push_str(nl);
    }
    out
}

/// `s` without whitespace and trailing commas, to compare code laid out
/// differently (rustfmt adds a trailing comma when it breaks a list).
fn squeeze(s: &str) -> String {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    compact
        .replace(",>", ">")
        .replace(",)", ")")
        .replace(",]", "]")
}

/// Adds `block` (one or more lines, indented relative to each other) above
/// the line holding the `// <marker>` comment. It takes the marker's
/// indentation, or that of the method chain the marker ends (rustfmt
/// dedents such a comment). `None` when there is no marker; `Unchanged`
/// when the code is already in the file, whatever its layout.
pub fn insert_above_marker(src: &str, marker: &str, block: &str) -> Option<Edit> {
    let (lines, trailing) = split(src);
    let comment = format!("// {marker}");
    let at = lines.iter().position(|l| l.trim() == comment)?;
    if squeeze(src).contains(&squeeze(block)) {
        return Some(Edit::Unchanged);
    }
    let chain = lines[..at]
        .iter()
        .rev()
        .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
        .filter(|l| l.trim_start().starts_with('.'));
    let indent = indent_of(chain.copied().unwrap_or(lines[at]));
    let block_lines: Vec<&str> = block.trim_matches('\n').lines().collect();
    let base = block_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_of(l).len())
        .min()
        .unwrap_or(0);
    let new = block_lines
        .iter()
        .map(|l| format!("{indent}{}", l.get(base..).unwrap_or("").trim_end()));
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    out.splice(at..at, new);
    Some(Edit::Changed(join(&out, newline(src), trailing)))
}

/// Whether `line` declares a module at the top level (`mod x;`, `pub mod x;`,
/// `pub(crate) mod x;`).
fn is_mod_declaration(line: &str) -> bool {
    let line = line.trim_end();
    let rest = line
        .strip_prefix("pub(crate) ")
        .or_else(|| line.strip_prefix("pub "))
        .unwrap_or(line);
    rest.starts_with("mod ") && rest.ends_with(';')
}

/// The module a declaration line declares (`jobs` for `pub mod jobs;`).
fn declared_name(line: &str) -> Option<&str> {
    let line = line.trim();
    let start = line.find("mod ")? + 4;
    line.get(start..)?.strip_suffix(';').map(str::trim)
}

/// Adds the declaration `decl` (e.g. `pub mod jobs;`, or a `mod tests;`
/// preceded by its `#[cfg(test)]` line) to the last group of top-level
/// module declarations without attributes: in name order, as rustfmt sorts
/// them (or after the group when it is not sorted, or when `decl` has an
/// attribute), else at the end of the file. `Unchanged` when the `mod` line
/// is already there.
pub fn declare_module(src: &str, decl: &str) -> Edit {
    let (lines, _) = split(src);
    let decl_lines: Vec<&str> = decl.lines().map(str::trim).collect();
    let mod_line = decl_lines.last().copied().unwrap_or_default();
    if lines.iter().any(|l| l.trim() == mod_line) {
        return Edit::Unchanged;
    }
    let nl = newline(src);
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    let plain = |i: usize| {
        is_mod_declaration(lines[i]) && (i == 0 || !lines[i - 1].trim_start().starts_with("#["))
    };
    let last = (0..lines.len()).rev().find(|&i| plain(i));
    let at = last.map(|last| {
        let first = (0..=last)
            .rev()
            .take_while(|&i| plain(i))
            .last()
            .unwrap_or(last);
        let names: Vec<&str> = (first..=last)
            .filter_map(|i| declared_name(lines[i]))
            .collect();
        let sorted = names.windows(2).all(|w| w[0] <= w[1]);
        match declared_name(mod_line) {
            Some(new) if sorted && decl_lines.len() == 1 => (first..=last)
                .find(|&i| declared_name(lines[i]).is_some_and(|name| name > new))
                .unwrap_or(last + 1),
            _ => last + 1,
        }
    });
    let new = decl_lines.iter().map(|l| (*l).to_owned());
    match at {
        Some(i) => {
            out.splice(i..i, new);
        }
        None => {
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.extend(new);
        }
    }
    Edit::Changed(join(&out, nl, true))
}

/// The leading whitespace of `line`.
fn indent_of(line: &str) -> String {
    line.chars().take_while(|c| c.is_whitespace()).collect()
}

/// Adds `.migration(<stem>::Migration)` to the `MigrationSet` chain: after
/// the last `.migration(` line (splitting a chain rustfmt joined into one
/// line), or after `MigrationSet::new(..)` when the set is empty.
/// `Unchanged` when it is already there; `None` when the file builds no set.
pub fn add_migration(src: &str, stem: &str) -> Option<Edit> {
    const CALL: &str = ".migration(";
    let call = format!("{CALL}{stem}::Migration)");
    let (lines, trailing) = split(src);
    if lines.iter().any(|l| l.contains(&call)) {
        return Some(Edit::Unchanged);
    }
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    match lines.iter().rposition(|l| l.contains(CALL)) {
        Some(at) if lines[at].trim_start().starts_with(CALL) => {
            out.insert(at + 1, format!("{}{call}", indent_of(lines[at])));
        }
        Some(at) => {
            // `MigrationSet::new("x").migration(a::Migration)` on one line:
            // one element per line, as rustfmt lays out longer chains.
            let line = lines[at];
            let split_at = line.find(CALL)?;
            let indent = format!("{}    ", indent_of(line));
            let (chain, end) = match line.strip_suffix(';') {
                Some(chain) => (chain, ";"),
                None => (line, ""),
            };
            let mut replacement = vec![chain[..split_at].to_owned()];
            replacement.extend(
                chain[split_at..]
                    .split(CALL)
                    .filter(|part| !part.is_empty())
                    .map(|part| format!("{indent}{CALL}{part}")),
            );
            replacement.push(format!("{indent}{call}{end}"));
            out.splice(at..=at, replacement);
        }
        None => {
            let at = lines
                .iter()
                .rposition(|l| l.contains("MigrationSet::new("))?;
            out.insert(at + 1, format!("{}    {call}", indent_of(lines[at])));
        }
    }
    Some(Edit::Changed(join(&out, newline(src), trailing)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_above_the_marker_with_its_indentation() {
        let src = "fn wire(&self, m: &mut ModuleBuilder) {\n    m.a();\n    // navree:wire\n}\n";
        let out = insert_above_marker(src, "navree:wire", "m.b();").unwrap();
        assert_eq!(
            out,
            Edit::Changed("fn wire(&self, m: &mut ModuleBuilder) {\n    m.a();\n    m.b();\n    // navree:wire\n}\n".to_owned())
        );
    }

    #[test]
    fn marker_insertion_is_idempotent_and_reports_a_missing_marker() {
        let src = "    m.b();\n    // navree:wire\n";
        assert_eq!(
            insert_above_marker(src, "navree:wire", "m.b();"),
            Some(Edit::Unchanged)
        );
        assert_eq!(
            insert_above_marker("fn x() {}\n", "navree:wire", "m.b();"),
            None
        );
    }

    #[test]
    fn chain_lines_take_the_indentation_of_the_chain() {
        // rustfmt dedents a comment that ends a method chain.
        let src = "    Navree::app()\n        .module(A)\n    // navree:modules\n}\n";
        assert_eq!(
            insert_above_marker(src, "navree:modules", ".module(B)"),
            Some(Edit::Changed(
                "    Navree::app()\n        .module(A)\n        .module(B)\n    // navree:modules\n}\n"
                    .to_owned()
            ))
        );
    }

    #[test]
    fn inserts_a_block_keeping_its_relative_indentation() {
        let src = "    fn wire() {\n        // navree:wire\n    }\n";
        let block = "listen::<\n    a::A,\n    b::B,\n>(m);";
        let expected = "    fn wire() {\n        listen::<\n            a::A,\n            b::B,\n        >(m);\n        // navree:wire\n    }\n";
        assert_eq!(
            insert_above_marker(src, "navree:wire", block),
            Some(Edit::Changed(expected.to_owned()))
        );
        // Already there, even laid out differently.
        assert_eq!(
            insert_above_marker(expected, "navree:wire", "listen::<a::A, b::B>(m);"),
            Some(Edit::Unchanged)
        );
    }

    #[test]
    fn declarations_stay_sorted_like_rustfmt_orders_them() {
        assert_eq!(
            declare_module("//! Modules.\n\npub mod users;\n", "pub mod posts;"),
            Edit::Changed("//! Modules.\n\npub mod posts;\npub mod users;\n".to_owned())
        );
        let src = "mod controller;\npub mod dto;\nmod service;\n#[cfg(test)]\nmod tests;\n";
        assert_eq!(
            declare_module(src, "pub mod jobs;"),
            Edit::Changed(
                "mod controller;\npub mod dto;\npub mod jobs;\nmod service;\n#[cfg(test)]\nmod tests;\n"
                    .to_owned()
            )
        );
    }

    #[test]
    fn keeps_crlf_line_endings() {
        let src = "a\r\n// navree:wire\r\n";
        assert_eq!(
            insert_above_marker(src, "navree:wire", "b"),
            Some(Edit::Changed("a\r\nb\r\n// navree:wire\r\n".to_owned()))
        );
    }

    #[test]
    fn declares_a_module_after_the_last_declaration() {
        let src =
            "//! Docs.\n\nmod controller;\npub mod dto;\n#[cfg(test)]\nmod tests;\n\nuse x::y;\n";
        assert_eq!(
            declare_module(src, "pub mod jobs;"),
            Edit::Changed(
                "//! Docs.\n\nmod controller;\npub mod dto;\npub mod jobs;\n#[cfg(test)]\nmod tests;\n\nuse x::y;\n"
                    .to_owned()
            )
        );
        assert_eq!(declare_module(src, "pub mod dto;"), Edit::Unchanged);
    }

    #[test]
    fn declares_a_module_in_a_file_without_declarations() {
        assert_eq!(
            declare_module("//! Jobs.\n", "pub mod send;"),
            Edit::Changed("//! Jobs.\n\npub mod send;\n".to_owned())
        );
        assert_eq!(
            declare_module("", "pub mod send;"),
            Edit::Changed("pub mod send;\n".to_owned())
        );
    }

    #[test]
    fn declares_a_module_with_its_attribute() {
        assert_eq!(
            declare_module("mod a;\n", "#[cfg(test)]\nmod tests;"),
            Edit::Changed("mod a;\n#[cfg(test)]\nmod tests;\n".to_owned())
        );
        assert_eq!(
            declare_module(
                "mod a;\n#[cfg(test)]\nmod tests;\n",
                "#[cfg(test)]\nmod tests;"
            ),
            Edit::Unchanged
        );
    }

    #[test]
    fn adds_a_migration_to_a_multi_line_set() {
        let src = "pub fn migrations() -> MigrationSet {\n    MigrationSet::new(\"users\")\n        .after([\"auth\"])\n        .migration(a::Migration)\n}\n";
        assert_eq!(
            add_migration(src, "b"),
            Some(Edit::Changed(
                "pub fn migrations() -> MigrationSet {\n    MigrationSet::new(\"users\")\n        .after([\"auth\"])\n        .migration(a::Migration)\n        .migration(b::Migration)\n}\n"
                    .to_owned()
            ))
        );
        assert_eq!(add_migration(src, "a"), Some(Edit::Unchanged));
    }

    #[test]
    fn adds_a_migration_to_a_set_rustfmt_put_on_one_line() {
        let src = "pub fn migrations() -> MigrationSet {\n    MigrationSet::new(\"posts\").migration(a::Migration)\n}\n";
        assert_eq!(
            add_migration(src, "b"),
            Some(Edit::Changed(
                "pub fn migrations() -> MigrationSet {\n    MigrationSet::new(\"posts\")\n        .migration(a::Migration)\n        .migration(b::Migration)\n}\n"
                    .to_owned()
            ))
        );
    }

    #[test]
    fn adds_a_first_migration_to_an_empty_set() {
        let src = "pub fn migrations() -> MigrationSet {\n    MigrationSet::new(\"posts\")\n}\n";
        assert_eq!(
            add_migration(src, "b"),
            Some(Edit::Changed(
                "pub fn migrations() -> MigrationSet {\n    MigrationSet::new(\"posts\")\n        .migration(b::Migration)\n}\n"
                    .to_owned()
            ))
        );
        assert_eq!(add_migration("fn x() {}\n", "b"), None);
    }
}
