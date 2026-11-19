//! Writing suggested page policy sources commented out in `config/ui.policy.toml`.

use std::collections::{BTreeMap, BTreeSet};

use crate::plugin::files::AppFiles;
use crate::plugin::manifest::PolicyAsk;

const POLICY_DIRECTIVES: &[&str] = &["img_src", "media_src", "connect_src", "font_src"];
const DOCS_HEADER: &str =
    "# Sources every page may load beyond Navree UI's own policy: /docs/ui/page-policy/";

/// Plans the new `config/ui.policy.toml` (or `None` when unchanged) and the output lines
/// for `ask` of `package` in namespace `ns`.
#[allow(dead_code)]
pub(crate) fn plan(
    files: &AppFiles,
    process_env: &[(String, String)],
    package: &str,
    ns: &str,
    ask: &PolicyAsk,
    set_in_code: Option<&str>,
) -> (Option<String>, Vec<String>) {
    if ask.is_empty() {
        return (None, Vec::new());
    }

    if let Some(code_file) = set_in_code {
        let mut lines = Vec::new();
        for (key, source) in &ask.sources {
            let directive = directive_name(key);
            lines.push(format!(
                "  next     to allow {directive} {source} for {ns}, add it to ui.policy.allow.{ns}.{key} in {code_file}"
            ));
        }
        if ask.wasm {
            lines.push(format!(
                "  next     to allow WebAssembly for {ns}, set ui.policy.wasm = true in {code_file}"
            ));
        }
        return (None, lines);
    }

    let state = scan_policy_sources(files, process_env, ns);
    let existing_file = files
        .config
        .get("config/ui.policy.toml")
        .map(String::as_str);
    let commented = inspect_commented_policy(existing_file, ns);

    let mut lines = Vec::new();
    let mut to_write_by_key: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut write_wasm = false;

    for (key, source) in &ask.sources {
        let directive = directive_name(key);
        let already_allowed = state
            .by_key
            .get(key.as_str())
            .is_some_and(|(_, list)| list.iter().any(|item| item == source));
        let active_key_setter = state.by_key.get(key.as_str()).map(|(src, _)| src.as_str());
        let other_ns_setter = state
            .ns_setters
            .iter()
            .rev()
            .find(|src| src.as_str() != "config/ui.policy.toml");

        if already_allowed {
            lines.push(format!(
                "  kept     ui.policy.allow.{ns} already allows {directive} {source}"
            ));
        } else if active_key_setter.is_some() || other_ns_setter.is_some() {
            let where_set = active_key_setter
                .or_else(|| other_ns_setter.map(String::as_str))
                .unwrap_or("config/ui.policy.toml");
            lines.push(format!(
                "  next     to allow {directive} {source} for {ns}, add it to ui.policy.allow.{ns}.{key} in {where_set}"
            ));
        } else if commented
            .sources
            .get(key.as_str())
            .is_some_and(|set| set.contains(source.as_str()))
        {
            lines.push(format!(
                "  kept     {directive} {source} for {ns} is in config/ui.policy.toml, commented out"
            ));
        } else {
            let list = to_write_by_key.entry(key.as_str()).or_default();
            if !list.contains(&source.as_str()) {
                list.push(source.as_str());
            }
            lines.push(format!(
                "  policy   {directive} {source} for {ns}: written commented out in config/ui.policy.toml; uncomment it to allow it on every page"
            ));
        }
    }

    if ask.wasm {
        if let Some((where_set, allowed)) = &state.wasm {
            if *allowed {
                lines.push("  kept     ui.policy.wasm already allows WebAssembly".to_owned());
            } else {
                lines.push(format!(
                    "  next     to allow WebAssembly for {ns}, set ui.policy.wasm = true in {where_set}"
                ));
            }
        } else if commented.wasm {
            lines.push(format!(
                "  kept     WebAssembly for {ns} is in config/ui.policy.toml, commented out"
            ));
        } else {
            write_wasm = true;
            lines.push(format!(
                "  policy   WebAssembly for {ns}: written commented out in config/ui.policy.toml (ui.policy.wasm); it applies to every script on every page"
            ));
        }
    }

    if to_write_by_key.is_empty() && !write_wasm {
        return (None, lines);
    }

    let updated = build_ui_policy_toml(
        existing_file,
        package,
        ns,
        &to_write_by_key,
        write_wasm,
        commented.has_ns_header,
    );
    (Some(updated), lines)
}

fn directive_name(key: &str) -> &str {
    match key {
        "img_src" => "img-src",
        "media_src" => "media-src",
        "connect_src" => "connect-src",
        "font_src" => "font-src",
        other => other,
    }
}

fn build_ui_policy_toml(
    existing: Option<&str>,
    package: &str,
    ns: &str,
    to_write_by_key: &BTreeMap<&str, Vec<&str>>,
    write_wasm: bool,
    has_ns_header: bool,
) -> String {
    let raw = existing.unwrap_or("");
    let nl = if raw.contains("\r\n") { "\r\n" } else { "\n" };

    let mut out_lines: Vec<String> = if raw.trim().is_empty() {
        vec![DOCS_HEADER.to_owned()]
    } else {
        raw.lines().map(str::to_owned).collect()
    };

    if write_wasm {
        let wasm_comment = format!(
            "# plugin:add {package} asks for WebAssembly on every page. Uncomment to allow it."
        );
        let wasm_line = "# wasm = true".to_owned();
        let at = out_lines
            .iter()
            .position(|line| is_table_header(line))
            .unwrap_or(out_lines.len());
        out_lines.splice(at..at, [wasm_comment, wasm_line]);
    }

    if !to_write_by_key.is_empty() {
        let mut directive_lines = Vec::new();
        for &key in POLICY_DIRECTIVES {
            if let Some(sources) = to_write_by_key.get(key) {
                let formatted_sources: Vec<String> = sources
                    .iter()
                    .map(|src| {
                        let escaped = src.replace('\\', "\\\\").replace('"', "\\\"");
                        format!("\"{escaped}\"")
                    })
                    .collect();
                directive_lines.push(format!("# {key} = [{}]", formatted_sources.join(", ")));
            }
        }

        if has_ns_header {
            let target_header = format!("allow.{ns}");
            if let Some(header_idx) = out_lines
                .iter()
                .position(|line| table_header_name(line).as_deref() == Some(target_header.as_str()))
            {
                let insert_at = out_lines[header_idx + 1..]
                    .iter()
                    .position(|line| is_table_header(line))
                    .map_or(out_lines.len(), |rel| header_idx + 1 + rel);
                let mut pos = insert_at;
                while pos > header_idx + 1 && out_lines[pos - 1].trim().is_empty() {
                    pos -= 1;
                }
                out_lines.splice(pos..pos, directive_lines);
            }
        } else {
            let has_non_comment = out_lines.iter().any(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty() && !trimmed.starts_with('#')
            });
            if has_non_comment && out_lines.last().is_some_and(|last| !last.trim().is_empty()) {
                out_lines.push(String::new());
            }
            out_lines.push(format!("[allow.{ns}]"));
            out_lines.push(format!(
                "# plugin:add {package} asks for these. Uncomment a line to allow it on every page."
            ));
            out_lines.extend(directive_lines);
        }
    }

    let mut rendered = out_lines.join(nl);
    rendered.push_str(nl);
    rendered
}

#[derive(Default)]
struct CommentedPolicy {
    wasm: bool,
    has_ns_header: bool,
    sources: BTreeMap<String, BTreeSet<String>>,
}

fn inspect_commented_policy(existing: Option<&str>, ns: &str) -> CommentedPolicy {
    let mut info = CommentedPolicy::default();
    let Some(text) = existing else {
        return info;
    };

    let target = format!("allow.{ns}");
    let mut current_table: Option<String> = None;

    for line in text.lines() {
        if let Some(header) = table_header_name(line) {
            if header == target {
                info.has_ns_header = true;
            }
            current_table = Some(header);
            continue;
        }

        let trimmed = line.trim();
        let Some(after_hash) = trimmed.strip_prefix('#') else {
            continue;
        };
        let candidate = after_hash.trim_start();
        let Ok(parsed) = candidate.parse::<toml::Table>() else {
            continue;
        };

        if current_table.is_none() && parsed.get("wasm").and_then(toml::Value::as_bool).is_some() {
            info.wasm = true;
        }

        if current_table.as_deref() == Some(target.as_str()) {
            for &key in POLICY_DIRECTIVES {
                if let Some(arr) = parsed.get(key).and_then(toml::Value::as_array) {
                    let entry = info.sources.entry(key.to_owned()).or_default();
                    for item in arr {
                        if let Some(src) = item.as_str() {
                            entry.insert(src.to_owned());
                        }
                    }
                }
            }
        }
    }

    info
}

fn is_table_header(line: &str) -> bool {
    table_header_name(line).is_some()
}

fn table_header_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('[') || trimmed.starts_with("[[") {
        return None;
    }
    let without_comment = match toml_line_comment_start(trimmed) {
        Some(idx) => trimmed[..idx].trim_end(),
        None => trimmed.trim_end(),
    };
    let inner = without_comment.strip_prefix('[')?.strip_suffix(']')?;
    let normalized: String = inner.chars().filter(|ch| !ch.is_whitespace()).collect();
    (!normalized.is_empty()).then_some(normalized)
}

fn toml_line_comment_start(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        match quote {
            Some(b'"') => {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    quote = None;
                }
            }
            Some(b'\'') => {
                if bytes[i] == b'\'' {
                    quote = None;
                }
            }
            _ => match bytes[i] {
                b'"' | b'\'' => quote = Some(bytes[i]),
                b'#' => return Some(i),
                _ => {}
            },
        }
        i += 1;
    }
    None
}

#[derive(Default)]
struct PolicyScan {
    wasm: Option<(String, bool)>,
    ns_setters: Vec<String>,
    by_key: BTreeMap<String, (String, Vec<String>)>,
}

fn scan_policy_sources(files: &AppFiles, process_env: &[(String, String)], ns: &str) -> PolicyScan {
    let mut state = PolicyScan::default();

    let mut app_files = Vec::new();
    let mut section_files = Vec::new();
    let mut overlay_files = Vec::new();

    for (path, text) in &files.config {
        match classify_ui_config_path(path) {
            Some(UiConfigKind::AppToml) => app_files.push((path.as_str(), text.as_str())),
            Some(UiConfigKind::Section { key, json }) => {
                section_files.push((key, path.as_str(), text.as_str(), json));
            }
            Some(UiConfigKind::OverlayToml) => overlay_files.push((path.as_str(), text.as_str())),
            None => {}
        }
    }

    section_files.sort_by(|a, b| a.0.cmp(b.0).then_with(|| a.1.cmp(b.1)));

    for (path, text) in app_files {
        if let Some(root) = parse_toml_value(text) {
            apply_policy_tree(&mut state, &root, path, ns, false);
        }
    }

    for (key, path, text, json) in section_files {
        let parsed = if json {
            serde_json::from_str::<serde_json::Value>(text).ok()
        } else {
            parse_toml_value(text)
        };
        if let Some(node) = parsed {
            let wrapped = wrap_json_at_key(key, node);
            let is_default_policy_file = path == "config/ui.policy.toml";
            apply_policy_tree(&mut state, &wrapped, path, ns, is_default_policy_file);
        }
    }

    for (path, text) in overlay_files {
        if let Some(root) = parse_toml_value(text) {
            apply_policy_tree(&mut state, &root, path, ns, false);
        }
    }

    for text in files.dot_env.values() {
        for (var_name, var_value) in parse_dotenv_entries(text) {
            apply_policy_env(&mut state, &var_name, &var_value, ns);
        }
    }

    for (var_name, var_value) in process_env {
        apply_policy_env(&mut state, var_name, var_value, ns);
    }

    state
}

enum UiConfigKind<'a> {
    AppToml,
    Section { key: &'a str, json: bool },
    OverlayToml,
}

fn classify_ui_config_path(path: &str) -> Option<UiConfigKind<'_>> {
    if let Some(rest) = path.strip_prefix("config/env/") {
        if rest.contains('/') {
            return None;
        }
        let (_, ext) = rest.rsplit_once('.')?;
        if ext.eq_ignore_ascii_case("toml") {
            return Some(UiConfigKind::OverlayToml);
        }
        return None;
    }

    let rest = path.strip_prefix("config/")?;
    if rest.contains('/') {
        return None;
    }
    let (stem, ext) = rest.rsplit_once('.')?;
    let is_toml = ext.eq_ignore_ascii_case("toml");
    let is_json = ext.eq_ignore_ascii_case("json");
    if !is_toml && !is_json {
        return None;
    }
    if stem == "app" {
        return is_toml.then_some(UiConfigKind::AppToml);
    }
    if stem == "ui" || stem.starts_with("ui.") {
        return Some(UiConfigKind::Section {
            key: stem,
            json: is_json,
        });
    }
    None
}

fn parse_toml_value(text: &str) -> Option<serde_json::Value> {
    let table: toml::Table = text.parse().ok()?;
    serde_json::to_value(table).ok()
}

fn wrap_json_at_key(key: &str, mut value: serde_json::Value) -> serde_json::Value {
    for segment in key.split('.').rev() {
        let mut map = serde_json::Map::new();
        map.insert(segment.to_owned(), value);
        value = serde_json::Value::Object(map);
    }
    value
}

fn apply_policy_tree(
    state: &mut PolicyScan,
    root: &serde_json::Value,
    source_name: &str,
    ns: &str,
    ignore_empty_ns_table: bool,
) {
    let Some(policy) = root
        .get("ui")
        .and_then(|ui| ui.get("policy"))
        .and_then(serde_json::Value::as_object)
    else {
        return;
    };

    if let Some(wasm_val) = policy.get("wasm")
        && let Some(flag) = json_to_bool(wasm_val)
    {
        state.wasm = Some((source_name.to_owned(), flag));
    }

    let Some(allow_ns) = policy
        .get("allow")
        .and_then(serde_json::Value::as_object)
        .and_then(|allow| allow.get(ns))
    else {
        return;
    };

    match allow_ns.as_object() {
        Some(ns_obj) => {
            if ns_obj.is_empty() && ignore_empty_ns_table {
                return;
            }
            record_ns_setter(&mut state.ns_setters, source_name);
            for (key, val) in ns_obj {
                let list = val
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                state
                    .by_key
                    .insert(key.clone(), (source_name.to_owned(), list));
            }
        }
        None => {
            record_ns_setter(&mut state.ns_setters, source_name);
        }
    }
}

fn apply_policy_env(state: &mut PolicyScan, name: &str, value: &str, ns: &str) {
    let Some(segments) = env_segments(name) else {
        return;
    };
    match segments.as_slice() {
        [ui, policy, wasm] if ui == "ui" && policy == "policy" && wasm == "wasm" => {
            let allowed = parse_bool_str(value).unwrap_or(false);
            state.wasm = Some((name.to_owned(), allowed));
        }
        [ui, policy, allow, env_ns, key]
            if ui == "ui" && policy == "policy" && allow == "allow" && env_ns == ns =>
        {
            record_ns_setter(&mut state.ns_setters, name);
            state
                .by_key
                .insert(key.clone(), (name.to_owned(), parse_env_policy_list(value)));
        }
        [ui, policy, allow, env_ns]
            if ui == "ui" && policy == "policy" && allow == "allow" && env_ns == ns =>
        {
            record_ns_setter(&mut state.ns_setters, name);
            if let Ok(serde_json::Value::Object(ns_obj)) = serde_json::from_str(value.trim()) {
                for (key, val) in ns_obj {
                    let list = val
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(serde_json::Value::as_str)
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default();
                    state.by_key.insert(key, (name.to_owned(), list));
                }
            }
        }
        _ => {}
    }
}

fn parse_env_policy_list(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.starts_with('[')
        && let Ok(parsed) = serde_json::from_str::<Vec<String>>(trimmed)
    {
        return parsed
            .into_iter()
            .map(|item| item.trim().to_owned())
            .filter(|item| !item.is_empty())
            .collect();
    }
    trimmed
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn record_ns_setter(setters: &mut Vec<String>, source: &str) {
    if setters.last().map(String::as_str) != Some(source) {
        setters.push(source.to_owned());
    }
}

fn json_to_bool(value: &serde_json::Value) -> Option<bool> {
    match value {
        serde_json::Value::Bool(flag) => Some(*flag),
        serde_json::Value::Number(num) => num
            .as_i64()
            .map(|n| n != 0)
            .or_else(|| num.as_f64().map(|f| f != 0.0)),
        serde_json::Value::String(text) => parse_bool_str(text),
        _ => None,
    }
}

fn parse_bool_str(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" => Some(false),
        _ => None,
    }
}

fn env_segments(name: &str) -> Option<Vec<String>> {
    let head = name.get(..4)?;
    if !head.eq_ignore_ascii_case("APP_") {
        return None;
    }
    let rest = &name[4..];
    let segments: Vec<String> = rest.split("__").map(str::to_ascii_lowercase).collect();
    if segments.is_empty() || segments.iter().any(String::is_empty) {
        return None;
    }
    Some(segments)
}

fn parse_dotenv_entries(src: &str) -> Vec<(String, String)> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let lines: Vec<&str> = src.lines().collect();
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut next = 0;

    while next < lines.len() {
        let line = lines[next].trim_start();
        next += 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let stripped = match line.strip_prefix("export") {
            Some(rest) if rest.starts_with([' ', '\t']) => rest.trim_start(),
            _ => line,
        };
        let Some((key, rest)) = stripped.split_once('=') else {
            continue;
        };
        let key = key.trim_end();
        if key.is_empty() {
            continue;
        }
        let trimmed = rest.trim_start();
        let value = if let Some(body) = trimmed.strip_prefix('"') {
            let Some(raw) = read_quoted_dotenv(body, '"', &lines, &mut next) else {
                continue;
            };
            expand_dotenv(&raw, true, &entries)
        } else if let Some(body) = trimmed.strip_prefix('\'') {
            let Some(raw) = read_quoted_dotenv(body, '\'', &lines, &mut next) else {
                continue;
            };
            raw
        } else {
            let unquoted = strip_dotenv_comment(rest).trim();
            expand_dotenv(unquoted, false, &entries)
        };
        entries.push((key.to_owned(), value));
    }
    entries
}

fn read_quoted_dotenv(
    first: &str,
    quote: char,
    lines: &[&str],
    next: &mut usize,
) -> Option<String> {
    let mut body = String::new();
    let mut text = first;
    loop {
        if let Some(end) = find_closing_quote(text, quote) {
            body.push_str(&text[..end]);
            return Some(body);
        }
        body.push_str(text);
        body.push('\n');
        let line = *lines.get(*next)?;
        text = line;
        *next += 1;
    }
}

fn find_closing_quote(text: &str, quote: char) -> Option<usize> {
    let mut escaped = false;
    for (idx, ch) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if quote == '"' && ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return Some(idx);
        }
    }
    None
}

fn strip_dotenv_comment(value: &str) -> &str {
    let bytes = value.as_bytes();
    match (1..bytes.len()).find(|&idx| bytes[idx] == b'#' && bytes[idx - 1].is_ascii_whitespace()) {
        Some(idx) => &value[..idx],
        None => value,
    }
}

fn expand_dotenv(raw: &str, escapes: bool, entries: &[(String, String)]) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if escapes => match chars.peek() {
                Some('n') => {
                    chars.next();
                    out.push('\n');
                }
                Some('r') => {
                    chars.next();
                    out.push('\r');
                }
                Some('t') => {
                    chars.next();
                    out.push('\t');
                }
                Some(&esc @ ('"' | '\\' | '$')) => {
                    chars.next();
                    out.push(esc);
                }
                _ => out.push('\\'),
            },
            '$' if chars.peek() == Some(&'{') => {
                chars.next();
                let mut var_name = String::new();
                let mut closed = false;
                for inner in chars.by_ref() {
                    if inner == '}' {
                        closed = true;
                        break;
                    }
                    var_name.push(inner);
                }
                if closed {
                    let var_name = var_name.trim();
                    if let Some((_, val)) = entries.iter().rev().find(|(k, _)| k == var_name) {
                        out.push_str(val);
                    }
                }
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geo_ask(wasm: bool) -> PolicyAsk {
        PolicyAsk {
            sources: vec![(
                "img_src".to_owned(),
                "https://tile.openstreetmap.org".to_owned(),
            )],
            wasm,
        }
    }

    #[test]
    fn plugin_add_writes_suggested_sources_and_wasm_commented_out() {
        let files = AppFiles::default();
        let (written, lines) = plan(
            &files,
            &[],
            "navree-plugin-geo",
            "geo",
            &geo_ask(true),
            None,
        );
        assert_eq!(
            written.as_deref(),
            Some(concat!(
                "# Sources every page may load beyond Navree UI's own policy: /docs/ui/page-policy/\n",
                "# plugin:add navree-plugin-geo asks for WebAssembly on every page. Uncomment to allow it.\n",
                "# wasm = true\n",
                "[allow.geo]\n",
                "# plugin:add navree-plugin-geo asks for these. Uncomment a line to allow it on every page.\n",
                "# img_src = [\"https://tile.openstreetmap.org\"]\n",
            ))
        );
        assert_eq!(
            lines,
            vec![
                "  policy   img-src https://tile.openstreetmap.org for geo: written commented out in config/ui.policy.toml; uncomment it to allow it on every page".to_owned(),
                "  policy   WebAssembly for geo: written commented out in config/ui.policy.toml (ui.policy.wasm); it applies to every script on every page".to_owned(),
            ]
        );
    }

    #[test]
    fn plugin_add_never_replaces_an_allowance_set_elsewhere() {
        // Set in config/app.toml to another source -> next naming config/app.toml.
        let mut files = AppFiles::default();
        files.config.insert(
            "config/app.toml".to_owned(),
            "[ui.policy.allow.geo]\nimg_src = [\"https://tiles.ludhiana.example\"]\n".to_owned(),
        );
        let (written, lines) = plan(
            &files,
            &[],
            "navree-plugin-geo",
            "geo",
            &geo_ask(false),
            None,
        );
        assert_eq!(written, None);
        assert_eq!(
            lines,
            vec![
                "  next     to allow img-src https://tile.openstreetmap.org for geo, add it to ui.policy.allow.geo.img_src in config/app.toml".to_owned()
            ]
        );

        // Set in .env -> next naming the variable, never its value.
        let mut files = AppFiles::default();
        files.dot_env.insert(
            ".env".to_owned(),
            "APP_UI__POLICY__ALLOW__GEO__IMG_SRC=https://internal.amritsar.example\n".to_owned(),
        );
        let (written, lines) = plan(
            &files,
            &[],
            "navree-plugin-geo",
            "geo",
            &geo_ask(false),
            None,
        );
        assert_eq!(written, None);
        assert_eq!(
            lines,
            vec![
                "  next     to allow img-src https://tile.openstreetmap.org for geo, add it to ui.policy.allow.geo.img_src in APP_UI__POLICY__ALLOW__GEO__IMG_SRC".to_owned()
            ]
        );
        assert!(!lines[0].contains("amritsar"));

        // Already allowed -> kept line.
        let mut files = AppFiles::default();
        files.config.insert(
            "config/app.toml".to_owned(),
            "[ui.policy.allow.geo]\nimg_src = [\"https://tile.openstreetmap.org\"]\n".to_owned(),
        );
        let (written, lines) = plan(
            &files,
            &[],
            "navree-plugin-geo",
            "geo",
            &geo_ask(false),
            None,
        );
        assert_eq!(written, None);
        assert_eq!(
            lines,
            vec![
                "  kept     ui.policy.allow.geo already allows img-src https://tile.openstreetmap.org".to_owned()
            ]
        );
    }

    #[test]
    fn the_wasm_lines_go_above_the_first_table() {
        let mut files = AppFiles::default();
        files.config.insert(
            "config/ui.policy.toml".to_owned(),
            "[allow.maps]\nimg_src = [\"https://tiles.jalandhar.example\"]\n".to_owned(),
        );
        let wasm_only = PolicyAsk {
            sources: Vec::new(),
            wasm: true,
        };
        let (written, lines) = plan(
            &files,
            &[],
            "navree-plugin-khata",
            "khata",
            &wasm_only,
            None,
        );
        let written = written.expect("updated ui.policy.toml");
        assert_eq!(
            written,
            concat!(
                "# plugin:add navree-plugin-khata asks for WebAssembly on every page. Uncomment to allow it.\n",
                "# wasm = true\n",
                "[allow.maps]\n",
                "img_src = [\"https://tiles.jalandhar.example\"]\n",
            )
        );
        // Uncommenting `# wasm = true` produces valid top-level `wasm = true`, not `allow.maps.wasm`.
        let uncommented = written.replace("# wasm = true", "wasm = true");
        let parsed: toml::Table = uncommented.parse().expect("valid TOML");
        assert_eq!(
            parsed.get("wasm").and_then(toml::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            lines,
            vec![
                "  policy   WebAssembly for khata: written commented out in config/ui.policy.toml (ui.policy.wasm); it applies to every script on every page".to_owned()
            ]
        );
    }

    #[test]
    fn a_second_run_writes_no_second_block() {
        let mut files = AppFiles::default();
        let (first, _) = plan(
            &files,
            &[],
            "navree-plugin-geo",
            "geo",
            &geo_ask(true),
            None,
        );
        files
            .config
            .insert("config/ui.policy.toml".to_owned(), first.unwrap());

        let (second, lines) = plan(
            &files,
            &[],
            "navree-plugin-geo",
            "geo",
            &geo_ask(true),
            None,
        );
        assert_eq!(second, None);
        assert_eq!(
            lines,
            vec![
                "  kept     img-src https://tile.openstreetmap.org for geo is in config/ui.policy.toml, commented out".to_owned(),
                "  kept     WebAssembly for geo is in config/ui.policy.toml, commented out".to_owned(),
            ]
        );
    }

    #[test]
    fn page_sources_set_by_code_are_printed_not_written() {
        let files = AppFiles::default();
        let (written, lines) = plan(
            &files,
            &[],
            "navree-plugin-geo",
            "geo",
            &geo_ask(false),
            Some("src/main.rs"),
        );
        assert_eq!(written, None);
        assert_eq!(
            lines,
            vec![
                "  next     to allow img-src https://tile.openstreetmap.org for geo, add it to ui.policy.allow.geo.img_src in src/main.rs".to_owned()
            ]
        );
    }

    #[test]
    fn page_sources_in_any_section_file_of_the_ui_section_are_found() {
        let other_cases = [
            (
                "config/ui.json",
                r#"{"policy":{"allow":{"geo":{"img_src":["https://other.pune.example"]}}}}"#,
            ),
            (
                "config/ui.policy.json",
                r#"{"allow":{"geo":{"img_src":["https://other.pune.example"]}}}"#,
            ),
            (
                "config/ui.policy.allow.toml",
                "[geo]\nimg_src = [\"https://other.pune.example\"]\n",
            ),
            (
                "config/ui.policy.TOML",
                "[allow.geo]\nimg_src = [\"https://other.pune.example\"]\n",
            ),
        ];

        for (file_path, contents) in other_cases {
            let mut files = AppFiles::default();
            files
                .config
                .insert(file_path.to_owned(), contents.to_owned());
            let (written, lines) = plan(
                &files,
                &[],
                "navree-plugin-geo",
                "geo",
                &geo_ask(false),
                None,
            );
            assert_eq!(written, None, "{file_path}");
            assert_eq!(
                lines,
                vec![format!(
                    "  next     to allow img-src https://tile.openstreetmap.org for geo, add it to ui.policy.allow.geo.img_src in {file_path}"
                )],
                "{file_path}"
            );
        }

        let allowed_cases = [
            (
                "config/ui.json",
                r#"{"policy":{"allow":{"geo":{"img_src":["https://tile.openstreetmap.org"]}}}}"#,
            ),
            (
                "config/ui.policy.json",
                r#"{"allow":{"geo":{"img_src":["https://tile.openstreetmap.org"]}}}"#,
            ),
            (
                "config/ui.policy.allow.toml",
                "[geo]\nimg_src = [\"https://tile.openstreetmap.org\"]\n",
            ),
            (
                "config/ui.policy.TOML",
                "[allow.geo]\nimg_src = [\"https://tile.openstreetmap.org\"]\n",
            ),
        ];

        for (file_path, contents) in allowed_cases {
            let mut files = AppFiles::default();
            files
                .config
                .insert(file_path.to_owned(), contents.to_owned());
            let (written, lines) = plan(
                &files,
                &[],
                "navree-plugin-geo",
                "geo",
                &geo_ask(false),
                None,
            );
            assert_eq!(written, None, "{file_path}");
            assert_eq!(
                lines,
                vec![
                    "  kept     ui.policy.allow.geo already allows img-src https://tile.openstreetmap.org".to_owned()
                ],
                "{file_path}"
            );
        }
    }

    #[test]
    fn env_allowances_and_wasm_are_recognised_and_new_directives_append_under_active_tables() {
        // 1. `.env` JSON array and `APP_UI__POLICY__WASM=true` produce `kept` lines.
        let mut files = AppFiles::default();
        files.dot_env.insert(
            ".env".to_owned(),
            concat!(
                "APP_UI__POLICY__ALLOW__GEO__IMG_SRC='[\"https://tile.openstreetmap.org\"]'\n",
                "APP_UI__POLICY__WASM=true\n",
            )
            .to_owned(),
        );
        let (written, lines) = plan(
            &files,
            &[],
            "navree-plugin-geo",
            "geo",
            &geo_ask(true),
            None,
        );
        assert_eq!(written, None);
        assert_eq!(
            lines,
            vec![
                "  kept     ui.policy.allow.geo already allows img-src https://tile.openstreetmap.org".to_owned(),
                "  kept     ui.policy.wasm already allows WebAssembly".to_owned(),
            ]
        );

        // 2. Uncommented `img_src` in `config/ui.policy.toml` still appends a new commented `# connect_src`.
        let mut upgrade_files = AppFiles::default();
        upgrade_files.config.insert(
            "config/ui.policy.toml".to_owned(),
            concat!(
                "# Sources every page may load beyond Navree UI's own policy: /docs/ui/page-policy/\n",
                "[allow.geo]\n",
                "# plugin:add navree-plugin-geo asks for these. Uncomment a line to allow it on every page.\n",
                "img_src = [\"https://tile.openstreetmap.org\"]\n",
            )
            .to_owned(),
        );
        let upgrade_ask = PolicyAsk {
            sources: vec![
                (
                    "img_src".to_owned(),
                    "https://tile.openstreetmap.org".to_owned(),
                ),
                (
                    "connect_src".to_owned(),
                    "https://nominatim.openstreetmap.org".to_owned(),
                ),
            ],
            wasm: false,
        };
        let (updated, upgrade_lines) = plan(
            &upgrade_files,
            &[],
            "navree-plugin-geo",
            "geo",
            &upgrade_ask,
            None,
        );
        assert_eq!(
            updated.as_deref(),
            Some(concat!(
                "# Sources every page may load beyond Navree UI's own policy: /docs/ui/page-policy/\n",
                "[allow.geo]\n",
                "# plugin:add navree-plugin-geo asks for these. Uncomment a line to allow it on every page.\n",
                "img_src = [\"https://tile.openstreetmap.org\"]\n",
                "# connect_src = [\"https://nominatim.openstreetmap.org\"]\n",
            ))
        );
        assert_eq!(
            upgrade_lines,
            vec![
                "  kept     ui.policy.allow.geo already allows img-src https://tile.openstreetmap.org".to_owned(),
                "  policy   connect-src https://nominatim.openstreetmap.org for geo: written commented out in config/ui.policy.toml; uncomment it to allow it on every page".to_owned(),
            ]
        );
    }
}
