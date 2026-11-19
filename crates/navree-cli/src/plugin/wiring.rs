//! Wiring and unwiring module lines, `.env.example` entries and the shell hint.

use std::collections::BTreeSet;

use crate::edit::{Edit, insert_code_above_marker, strip_comments};
use crate::plugin::files::AppFiles;

/// Formats `.module(PATH)`.
#[allow(dead_code)]
pub(crate) fn module_line(module: &str) -> String {
    format!(".module({module})")
}

/// Outcome of planning how a plugin's module is wired into the app.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Wiring {
    /// Updated contents of `src/app.rs` with the line inserted above `// navree:modules`.
    Added(String),
    /// The exact module line is already on a non-comment line of `src/app.rs`.
    AlreadyInApp,
    /// The module type is already wired in this file under `src/`.
    WiredIn(String),
    /// Print the line instead (`--no-wire`, missing `src/app.rs` or missing marker).
    Print,
}

/// Plans wiring `line` (`type_name` from `crate_mention`) into `files`.
#[allow(dead_code)]
pub(crate) fn wire(
    files: &AppFiles,
    line: &str,
    type_name: &str,
    crate_mention: &str,
    no_wire: bool,
) -> Wiring {
    if let Some(app_rs) = files.app_rs.as_deref()
        && has_exact_module_line(app_rs, line)
    {
        return Wiring::AlreadyInApp;
    }

    if let Some(file) = wired_in_files(files, type_name, crate_mention) {
        return Wiring::WiredIn(file);
    }

    if no_wire {
        return Wiring::Print;
    }

    let Some(app_rs) = files.app_rs.as_deref() else {
        return Wiring::Print;
    };

    match insert_code_above_marker(app_rs, "navree:modules", line) {
        Some(Edit::Changed(updated)) => Wiring::Added(updated),
        Some(Edit::Unchanged) => Wiring::AlreadyInApp,
        None => Wiring::Print,
    }
}

/// The first file in `src`, in path order, whose non-comment code holds `type_name`
/// as a whole word and also names `crate_mention`.
#[allow(dead_code)]
pub(crate) fn wired_in(
    src: &[(String, String)],
    type_name: &str,
    crate_mention: &str,
) -> Option<String> {
    let mut indices: Vec<usize> = (0..src.len()).collect();
    indices.sort_by(|&a, &b| src[a].0.cmp(&src[b].0));
    for idx in indices {
        let (path, contents) = &src[idx];
        if file_wires_module(contents, type_name, crate_mention) {
            return Some(path.clone());
        }
    }
    None
}

/// Removes `line` from `app_rs` when a single non-comment line matches it (ignoring
/// whitespace and trailing commas); `None` when absent or edited into another shape.
#[allow(dead_code)]
pub(crate) fn unwire(app_rs: &str, line: &str) -> Option<String> {
    let target = squeeze_line(line);
    if target.is_empty() {
        return None;
    }

    let nl = if app_rs.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let trailing = app_rs.ends_with('\n') || app_rs.is_empty();
    let raw_lines: Vec<&str> = app_rs.lines().collect();
    let stripped = strip_comments(app_rs);
    let code_lines: Vec<&str> = stripped.lines().collect();

    let mut kept = Vec::with_capacity(raw_lines.len());
    let mut removed = false;
    for (idx, &raw) in raw_lines.iter().enumerate() {
        let code = code_lines.get(idx).copied().unwrap_or("");
        let is_exact =
            !code.trim().is_empty() && squeeze_line(raw) == target && squeeze_line(code) == target;
        if is_exact {
            removed = true;
        } else {
            kept.push(raw);
        }
    }

    if !removed {
        return None;
    }

    let mut out = kept.join(nl);
    if trailing && !out.is_empty() {
        out.push_str(nl);
    }
    Some(out)
}

/// Appends new environment variable names under `# plugin:add PACKAGE` in `.env.example`,
/// returning the new file (or `None` when nothing was added) and per-name `(name, added)`.
#[allow(dead_code)]
pub(crate) fn add_env(
    example: Option<&str>,
    package: &str,
    names: &[String],
) -> (Option<String>, Vec<(String, bool)>) {
    if names.is_empty() {
        return (None, Vec::new());
    }

    let raw = example.unwrap_or("");
    let nl = if raw.contains("\r\n") { "\r\n" } else { "\n" };

    let mut present: BTreeSet<String> = BTreeSet::new();
    for line in raw.lines() {
        let trimmed = line.trim_start();
        let candidate = trimmed.strip_prefix("# ").unwrap_or(trimmed);
        if let Some((key, _)) = candidate.split_once('=')
            && !key.is_empty()
            && !key.contains(char::is_whitespace)
        {
            present.insert(key.to_owned());
        }
    }

    let mut statuses = Vec::with_capacity(names.len());
    let mut to_add = Vec::new();
    for name in names {
        if present.contains(name.as_str()) {
            statuses.push((name.clone(), false));
        } else {
            present.insert(name.clone());
            statuses.push((name.clone(), true));
            to_add.push(name.as_str());
        }
    }

    if to_add.is_empty() {
        return (None, statuses);
    }

    let trimmed_end = raw.trim_end_matches(['\r', '\n']);
    let mut out = String::new();
    if !trimmed_end.is_empty() {
        out.push_str(trimmed_end);
        out.push_str(nl);
        out.push_str(nl);
    }
    out.push_str(&format!("# plugin:add {package}{nl}"));
    for name in to_add {
        out.push_str(&format!("{name}={nl}"));
    }

    (Some(out), statuses)
}

/// Which sidebar hint `plugin:add` should print for a plugin that turns on `ui`.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShellHint {
    Nothing,
    Contributed,
    ForRequest,
}

/// Inspects `src/ui/shell.rs` for `.contributed()` and `for_request(`.
#[allow(dead_code)]
pub(crate) fn shell_hint(shell_rs: Option<&str>) -> ShellHint {
    let Some(shell) = shell_rs else {
        return ShellHint::Nothing;
    };
    let code = strip_comments(shell);
    if code.contains(".contributed()") {
        ShellHint::Nothing
    } else if code.contains("for_request(") {
        ShellHint::ForRequest
    } else {
        ShellHint::Contributed
    }
}

fn wired_in_files(files: &AppFiles, type_name: &str, crate_mention: &str) -> Option<String> {
    let has_app_in_src = files.src.iter().any(|(path, _)| path == "src/app.rs");
    if has_app_in_src || files.app_rs.is_none() {
        return wired_in(&files.src, type_name, crate_mention);
    }

    let mut combined: Vec<(String, String)> = Vec::with_capacity(files.src.len() + 1);
    combined.extend(files.src.iter().cloned());
    if let Some(app_rs) = &files.app_rs {
        combined.push(("src/app.rs".to_owned(), app_rs.clone()));
    }
    wired_in(&combined, type_name, crate_mention)
}

fn has_exact_module_line(src: &str, line: &str) -> bool {
    let target = squeeze_line(line);
    if target.is_empty() {
        return false;
    }
    let stripped = strip_comments(src);
    stripped
        .lines()
        .any(|code_line| squeeze_line(code_line) == target)
}

fn squeeze_line(text: &str) -> String {
    let compact: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    compact
        .replace(",>", ">")
        .replace(",)", ")")
        .replace(",]", "]")
        .trim_end_matches(',')
        .to_owned()
}

fn file_wires_module(contents: &str, type_name: &str, crate_mention: &str) -> bool {
    let crate_prefix = crate_mention.trim_end_matches("::");
    if type_name.is_empty() || crate_prefix.is_empty() {
        return false;
    }
    let code = strip_comments(contents);
    if !mentions_crate(&code, crate_mention) {
        return false;
    }
    let imported_bare = use_imports_type(&code, crate_prefix, type_name);
    let imported_short_mod = crate_prefix
        .rsplit_once("::")
        .and_then(|(_, tail)| has_use_of_prefix(&code, crate_prefix).then_some(tail));

    let bytes = code.as_bytes();
    let mut from = 0;
    while let Some(rel) = code[from..].find(type_name) {
        let start = from + rel;
        let end = start + type_name.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            match qualifier_before(&code, start) {
                Some(qual) => {
                    if qual == crate_prefix
                        || qual.starts_with(&format!("{crate_prefix}::"))
                        || imported_short_mod.is_some_and(|short| {
                            qual == short || qual.starts_with(&format!("{short}::"))
                        })
                    {
                        return true;
                    }
                }
                None => {
                    if imported_bare {
                        return true;
                    }
                }
            }
        }
        from = start + 1;
    }
    false
}

fn qualifier_before(code: &str, start: usize) -> Option<String> {
    let before = code[..start].trim_end();
    let mut cur = before.strip_suffix("::")?.trim_end();
    let mut segments = Vec::new();
    loop {
        let end = cur.len();
        let seg_start = cur
            .as_bytes()
            .iter()
            .rposition(|&b| !is_ident_byte(b))
            .map_or(0, |pos| pos + 1);
        if seg_start == end {
            break;
        }
        segments.push(&cur[seg_start..end]);
        let head = cur[..seg_start].trim_end();
        match head.strip_suffix("::") {
            Some(next) => cur = next.trim_end(),
            None => break,
        }
    }
    if segments.is_empty() {
        return None;
    }
    segments.reverse();
    Some(segments.join("::"))
}

fn use_imports_type(code: &str, crate_prefix: &str, type_name: &str) -> bool {
    let needle = format!("{crate_prefix}::");
    let bytes = code.as_bytes();
    let mut from = 0;
    while let Some(rel) = code[from..].find(&needle) {
        let start = from + rel;
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        if before_ok {
            let mut rest = code[start + needle.len()..].trim_start();
            loop {
                let ident_len = rest
                    .as_bytes()
                    .iter()
                    .take_while(|&&b| is_ident_byte(b))
                    .count();
                if ident_len > 0 {
                    let after_ident = rest[ident_len..].trim_start();
                    if let Some(next) = after_ident.strip_prefix("::") {
                        rest = next.trim_start();
                        continue;
                    }
                }
                break;
            }
            if rest.starts_with('*') {
                return true;
            }
            if rest.starts_with('{')
                && let Some(group) = brace_group(rest)
                && (group.contains('*') || has_whole_word(group, type_name))
            {
                return true;
            }
        }
        from = start + 1;
    }
    false
}

fn has_use_of_prefix(code: &str, crate_prefix: &str) -> bool {
    let bytes = code.as_bytes();
    let mut from = 0;
    while let Some(rel) = code[from..].find(crate_prefix) {
        let start = from + rel;
        let end = start + crate_prefix.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after = code[end..].trim_start();
        if before_ok && (after.starts_with(';') || after.starts_with("as ")) {
            return true;
        }
        from = start + 1;
    }
    false
}

fn brace_group(text: &str) -> Option<&str> {
    let mut depth = 0usize;
    for (idx, ch) in text.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&text[1..idx]);
                }
            }
            _ => {}
        }
    }
    None
}

fn mentions_crate(code: &str, crate_mention: &str) -> bool {
    let bytes = code.as_bytes();
    let ends_with_sep = crate_mention.ends_with("::");
    let mut from = 0;
    while let Some(rel) = code[from..].find(crate_mention) {
        let start = from + rel;
        let end = start + crate_mention.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after_ok = ends_with_sep || end == bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn has_whole_word(text: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(rel) = text[from..].find(word) {
        let start = from + rel;
        let end = start + word.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_app_rs() -> String {
        crate::templates::render(
            "app/src/app.rs",
            serde_json::json!({
                "ui_navree": false,
                "title": "Kesar",
            }),
        )
        .expect("render minimal src/app.rs")
    }

    #[test]
    fn the_module_line_goes_above_the_marker_once() {
        let initial = minimal_app_rs();
        let line = module_line("navree_plugin_announcements::AnnouncementsModule::from_config()");
        let files = AppFiles {
            app_rs: Some(initial.clone()),
            src: vec![("src/app.rs".to_owned(), initial.clone())],
            ..AppFiles::default()
        };

        let Wiring::Added(after) = wire(
            &files,
            &line,
            "AnnouncementsModule",
            "navree_plugin_announcements::",
            false,
        ) else {
            panic!("expected Added");
        };
        assert!(
            after.contains(
                "        .module(modules::users::UsersModule)\n        .module(navree_plugin_announcements::AnnouncementsModule::from_config())\n    // navree:modules\n"
            ),
            "{after}"
        );

        let second_files = AppFiles {
            app_rs: Some(after.clone()),
            src: vec![("src/app.rs".to_owned(), after)],
            ..AppFiles::default()
        };
        assert_eq!(
            wire(
                &second_files,
                &line,
                "AnnouncementsModule",
                "navree_plugin_announcements::",
                false,
            ),
            Wiring::AlreadyInApp
        );

        // Marker deleted -> Print.
        let without_marker = initial.replace("    // navree:modules\n", "");
        let no_marker_files = AppFiles {
            app_rs: Some(without_marker.clone()),
            src: vec![("src/app.rs".to_owned(), without_marker)],
            ..AppFiles::default()
        };
        assert_eq!(
            wire(
                &no_marker_files,
                &line,
                "AnnouncementsModule",
                "navree_plugin_announcements::",
                false,
            ),
            Wiring::Print
        );

        // CRLF kept.
        let crlf_src = initial.replace('\n', "\r\n");
        let crlf_files = AppFiles {
            app_rs: Some(crlf_src.clone()),
            src: vec![("src/app.rs".to_owned(), crlf_src)],
            ..AppFiles::default()
        };
        let Wiring::Added(crlf_after) = wire(
            &crlf_files,
            &line,
            "AnnouncementsModule",
            "navree_plugin_announcements::",
            false,
        ) else {
            panic!("expected Added on CRLF file");
        };
        assert!(crlf_after.contains("\r\n"));
        assert!(!crlf_after.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn a_commented_out_module_line_is_wired_again() {
        let line = module_line("navree_plugin_announcements::AnnouncementsModule::from_config()");
        let app_rs = concat!(
            "pub fn app() {\n",
            "    Navree::app()\n",
            "        .module(modules::users::UsersModule)\n",
            "        // .module(navree_plugin_announcements::AnnouncementsModule::from_config())\n",
            "    // navree:modules\n",
            "}\n",
        );
        let src = vec![("src/app.rs".to_owned(), app_rs.to_owned())];
        assert_eq!(
            wired_in(&src, "AnnouncementsModule", "navree_plugin_announcements::"),
            None
        );

        let files = AppFiles {
            app_rs: Some(app_rs.to_owned()),
            src,
            ..AppFiles::default()
        };
        let Wiring::Added(updated) = wire(
            &files,
            &line,
            "AnnouncementsModule",
            "navree_plugin_announcements::",
            false,
        ) else {
            panic!("expected Added");
        };
        assert_eq!(
            updated,
            concat!(
                "pub fn app() {\n",
                "    Navree::app()\n",
                "        .module(modules::users::UsersModule)\n",
                "        // .module(navree_plugin_announcements::AnnouncementsModule::from_config())\n",
                "        .module(navree_plugin_announcements::AnnouncementsModule::from_config())\n",
                "    // navree:modules\n",
                "}\n",
            )
        );
        assert_eq!(
            wired_in(
                &[("src/app.rs".to_owned(), updated)],
                "AnnouncementsModule",
                "navree_plugin_announcements::"
            ),
            Some("src/app.rs".to_owned())
        );
    }

    #[test]
    fn a_module_type_named_anywhere_under_src_is_wired() {
        let line = module_line("navree::billing::BillingModule::from_config()");
        let platform_rs = "pub fn platform(b: AppBuilder) -> AppBuilder {\n    b.module(navree::billing::BillingModule::from_config())\n}\n";
        let app_rs = minimal_app_rs();
        let files = AppFiles {
            app_rs: Some(app_rs.clone()),
            src: vec![
                ("src/app.rs".to_owned(), app_rs),
                ("src/platform.rs".to_owned(), platform_rs.to_owned()),
            ],
            ..AppFiles::default()
        };

        assert_eq!(
            wire(&files, &line, "BillingModule", "navree::billing", false),
            Wiring::WiredIn("src/platform.rs".to_owned())
        );
    }

    #[test]
    fn two_plugins_that_share_a_module_type_are_wired_apart() {
        let khata_app_rs = concat!(
            "pub fn app() {\n",
            "    Navree::app()\n",
            "        .module(navree_plugin_khata::ClientModule::<reqwest::Client,KhataOptions>::new())\n",
            "    // navree:modules\n",
            "}\n",
        );
        let src = vec![("src/app.rs".to_owned(), khata_app_rs.to_owned())];

        assert_eq!(
            wired_in(&src, "ClientModule", "navree_plugin_khata::"),
            Some("src/app.rs".to_owned())
        );
        assert_eq!(
            wired_in(&src, "ClientModule", "navree_plugin_notices::"),
            None
        );

        let files = AppFiles {
            app_rs: Some(khata_app_rs.to_owned()),
            src,
            ..AppFiles::default()
        };
        let notices_line = module_line(
            "navree_plugin_notices::ClientModule::<reqwest::Client,NoticesOptions>::new()",
        );
        let Wiring::Added(both) = wire(
            &files,
            &notices_line,
            "ClientModule",
            "navree_plugin_notices::",
            false,
        ) else {
            panic!("expected notices ClientModule to be Added");
        };
        assert!(both.contains(&notices_line), "{both}");

        let unrelated = vec![(
            "src/app.rs".to_owned(),
            "use other_crate::ClientModule;\npub fn app() { Navree::app().module(ClientModule::new()); }\n"
                .to_owned(),
        )];
        assert_eq!(
            wired_in(&unrelated, "ClientModule", "navree_plugin_khata::"),
            None
        );
        assert_eq!(
            wired_in(&unrelated, "ClientModule", "navree_plugin_notices::"),
            None
        );

        // Importing an unrelated item from the plugin crate alongside an app module of the same
        // struct name does not count as wiring the plugin's module.
        let helper_only = vec![(
            "src/app.rs".to_owned(),
            concat!(
                "use navree_plugin_khata::Currency;\n",
                "pub fn app() { Navree::app().module(modules::khata::KhataModule); }\n",
            )
            .to_owned(),
        )];
        assert_eq!(
            wired_in(&helper_only, "KhataModule", "navree_plugin_khata::"),
            None
        );

        // Importing the module type itself from the plugin crate does count as wired.
        let imported_module = vec![(
            "src/app.rs".to_owned(),
            concat!(
                "use navree_plugin_khata::{Currency, KhataModule};\n",
                "pub fn app() { Navree::app().module(KhataModule::from_config()); }\n",
            )
            .to_owned(),
        )];
        assert_eq!(
            wired_in(&imported_module, "KhataModule", "navree_plugin_khata::"),
            Some("src/app.rs".to_owned())
        );
    }

    #[test]
    fn a_type_named_only_in_a_comment_or_inside_another_word_is_not_wired() {
        let src = vec![(
            "src/app.rs".to_owned(),
            concat!(
                "use navree::billing;\n",
                "// BillingModule\n",
                "/* BillingModule */\n",
                "struct MyBillingModule;\n",
                "struct BillingModules;\n",
            )
            .to_owned(),
        )];
        assert_eq!(wired_in(&src, "BillingModule", "navree::billing"), None);
    }

    #[test]
    fn unwiring_removes_only_the_exact_line() {
        let line = module_line("navree_plugin_announcements::AnnouncementsModule::from_config()");
        let exact = concat!(
            "pub fn app() {\n",
            "    Navree::app()\n",
            "        .module(modules::users::UsersModule)\n",
            "        .module(navree_plugin_announcements::AnnouncementsModule::from_config())\n",
            "    // navree:modules\n",
            "}\n",
        );
        assert_eq!(
            unwire(exact, &line).as_deref(),
            Some(concat!(
                "pub fn app() {\n",
                "    Navree::app()\n",
                "        .module(modules::users::UsersModule)\n",
                "    // navree:modules\n",
                "}\n",
            ))
        );

        let wrapped = concat!(
            "pub fn app() {\n",
            "    Navree::app()\n",
            "        .module(\n",
            "            navree_plugin_announcements::AnnouncementsModule::from_config(),\n",
            "        )\n",
            "}\n",
        );
        assert_eq!(unwire(wrapped, &line), None);

        let with_options = "pub fn app() {\n    Navree::app()\n        .module(navree_plugin_announcements::AnnouncementsModule::for_root(opts))\n}\n";
        assert_eq!(unwire(with_options, &line), None);

        let commented = "pub fn app() {\n    Navree::app()\n        // .module(navree_plugin_announcements::AnnouncementsModule::from_config())\n}\n";
        assert_eq!(unwire(commented, &line), None);

        let absent = "pub fn app() {\n    Navree::app()\n}\n";
        assert_eq!(unwire(absent, &line), None);
    }

    #[test]
    fn env_names_are_appended_once_under_the_packages_mark() {
        let names = vec![
            "APP_ANNOUNCEMENTS__SIGNING_KEY".to_owned(),
            "APP_ANNOUNCEMENTS__WEBHOOK_SECRET".to_owned(),
        ];

        // Existing file with active and commented entries.
        let initial = "APP_KEY=\n# APP_ANNOUNCEMENTS__SIGNING_KEY=demo\n";
        let (updated, statuses) = add_env(Some(initial), "navree-plugin-announcements", &names);
        assert_eq!(
            statuses,
            vec![
                ("APP_ANNOUNCEMENTS__SIGNING_KEY".to_owned(), false),
                ("APP_ANNOUNCEMENTS__WEBHOOK_SECRET".to_owned(), true),
            ]
        );
        assert_eq!(
            updated.as_deref(),
            Some(concat!(
                "APP_KEY=\n",
                "# APP_ANNOUNCEMENTS__SIGNING_KEY=demo\n",
                "\n",
                "# plugin:add navree-plugin-announcements\n",
                "APP_ANNOUNCEMENTS__WEBHOOK_SECRET=\n",
            ))
        );

        // Second run changes nothing.
        let (again, again_statuses) =
            add_env(updated.as_deref(), "navree-plugin-announcements", &names);
        assert_eq!(again, None);
        assert_eq!(
            again_statuses,
            vec![
                ("APP_ANNOUNCEMENTS__SIGNING_KEY".to_owned(), false),
                ("APP_ANNOUNCEMENTS__WEBHOOK_SECRET".to_owned(), false),
            ]
        );

        // Absent file is created without a leading blank line.
        let (created, _) = add_env(
            None,
            "navree-plugin-khata",
            &["APP_KHATA__API_KEY".to_owned()],
        );
        assert_eq!(
            created.as_deref(),
            Some("# plugin:add navree-plugin-khata\nAPP_KHATA__API_KEY=\n")
        );
    }

    #[test]
    fn the_sidebar_hint_reads_the_shell() {
        let kit_shell = crate::templates::render(
            "ui/src/ui/shell.rs.j2",
            serde_json::json!({ "title": "Kesar" }),
        )
        .expect("render UI kit shell.rs");
        assert_eq!(shell_hint(Some(&kit_shell)), ShellHint::Nothing);
        assert_eq!(shell_hint(None), ShellHint::Nothing);

        let legacy_shell = concat!(
            "pub fn shell() -> Shell { Shell::new(\"Kesar\") }\n",
            "fn draw(s: Shell, parts: &Parts) { let _ = s.for_request(parts); }\n",
        );
        assert_eq!(shell_hint(Some(legacy_shell)), ShellHint::ForRequest);

        let plain_shell = "pub fn shell() -> Shell { Shell::new(\"Kesar\") }\n";
        assert_eq!(shell_hint(Some(plain_shell)), ShellHint::Contributed);
    }
}
