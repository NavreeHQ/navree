//! Scanning role sources and writing suggested grants into `config/workspace.toml`.

use std::collections::{BTreeMap, BTreeSet};

use toml_edit::{Array, Document, DocumentMut, Item, Table, Value};

use crate::plugin::files::AppFiles;
use crate::plugin::manifest::Granted;

/// One source that sets `workspace.roles.ROLE.permissions`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RoleSetter {
    source: String,
    permissions: Vec<(String, Option<String>)>,
}

/// Every place the app's configuration sets a workspace role's permissions.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub(crate) struct RoleSources {
    roles: BTreeSet<String>,
    demo_guest_role: Option<String>,
    by_role: BTreeMap<String, Vec<RoleSetter>>,
}

#[allow(dead_code)]
impl RoleSources {
    /// The roles under `workspace.roles` in any source.
    pub(crate) fn roles(&self) -> BTreeSet<String> {
        self.roles.clone()
    }

    /// `workspace.demo.guest_role`, the last source that sets it.
    pub(crate) fn demo_guest_role(&self) -> Option<String> {
        self.demo_guest_role.clone()
    }

    /// Every permission configured in any workspace role list, with its source and optional mark.
    pub(crate) fn granted(&self) -> Vec<Granted> {
        let mut out = Vec::new();
        for setters in self.by_role.values() {
            for setter in setters {
                for (permission, marked_for) in &setter.permissions {
                    out.push(Granted {
                        permission: permission.clone(),
                        source: setter.source.clone(),
                        marked_for: marked_for.clone(),
                    });
                }
            }
        }
        out
    }

    /// The sources that set `workspace.roles.ROLE.permissions`, in load order.
    pub(crate) fn setters(&self, role: &str) -> Vec<String> {
        self.by_role
            .get(role)
            .map(|list| list.iter().map(|setter| setter.source.clone()).collect())
            .unwrap_or_default()
    }

    fn active_permissions(&self, role: &str) -> BTreeSet<&str> {
        self.by_role
            .get(role)
            .and_then(|list| list.last())
            .map(|setter| {
                setter
                    .permissions
                    .iter()
                    .map(|(perm, _)| perm.as_str())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// One printed line produced while planning suggested grants.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GrantLine {
    Grant {
        role: String,
        permission: String,
    },
    Kept {
        role: String,
        permission: String,
    },
    Next {
        role: String,
        permission: String,
        source: String,
    },
}

#[allow(dead_code)]
impl GrantLine {
    /// Formats the line as `plugin:add` prints it.
    pub(crate) fn text(&self) -> String {
        match self {
            Self::Grant { role, permission } => {
                format!("  grant    {role}: {permission} (config/workspace.toml)")
            }
            Self::Kept { role, permission } => {
                format!("  kept     {role} already has {permission}")
            }
            Self::Next {
                role,
                permission,
                source,
            } => {
                format!(
                    "  next     add {permission} to workspace.roles.{role}.permissions in {source}"
                )
            }
        }
    }
}

/// Scans configuration files, `.env` files and `process_env` for workspace roles,
/// `workspace.demo.guest_role` and granted permissions.
#[allow(dead_code)]
pub(crate) fn scan(files: &AppFiles, process_env: &[(String, String)]) -> RoleSources {
    let mut sources = RoleSources::default();

    let mut app_files = Vec::new();
    let mut section_files = Vec::new();
    let mut overlay_files = Vec::new();

    for (path, text) in &files.config {
        match classify_config_path(path) {
            Some(ConfigKind::AppToml) => app_files.push((path.as_str(), text.as_str())),
            Some(ConfigKind::Section { key, json }) => {
                section_files.push((key, path.as_str(), text.as_str(), json));
            }
            Some(ConfigKind::OverlayToml) => overlay_files.push((path.as_str(), text.as_str())),
            None => {}
        }
    }

    section_files.sort_by(|a, b| a.0.cmp(b.0).then_with(|| a.1.cmp(b.1)));

    for (path, text) in app_files {
        if let Some(root) = parse_toml_node(text)
            && let Some(ws) = root.get("workspace")
        {
            apply_workspace_node(&mut sources, ws, path);
        }
    }

    for (key, path, text, json) in section_files {
        let parsed = if json {
            parse_json_node(text)
        } else {
            parse_toml_node(text)
        };
        if let Some(node) = parsed {
            let wrapped = wrap_at_key(key, node);
            if let Some(ws) = wrapped.get("workspace") {
                apply_workspace_node(&mut sources, ws, path);
            }
        }
    }

    for (path, text) in overlay_files {
        if let Some(root) = parse_toml_node(text)
            && let Some(ws) = root.get("workspace")
        {
            apply_workspace_node(&mut sources, ws, path);
        }
    }

    for text in files.dot_env.values() {
        for (var_name, var_value) in parse_dotenv_entries(text) {
            apply_env_entry(&mut sources, &var_name, &var_value);
        }
    }

    for (var_name, var_value) in process_env {
        apply_env_entry(&mut sources, var_name, var_value);
    }

    sources
}

/// Plans edits to `config/workspace.toml` (or `None` when unchanged) and the output lines
/// for `grants` of `package`.
#[allow(dead_code)]
pub(crate) fn plan(
    sources: &RoleSources,
    workspace_toml: Option<&str>,
    package: &str,
    grants: &BTreeMap<String, Vec<String>>,
    set_in_code: Option<&str>,
) -> Result<(Option<String>, Vec<GrantLine>), String> {
    if grants.is_empty() {
        return Ok((None, Vec::new()));
    }

    let mut lines = Vec::new();
    let mut additions: Vec<(&str, Vec<&str>)> = Vec::new();

    for (role, perms) in grants {
        if let Some(code_file) = set_in_code {
            for perm in perms {
                lines.push(GrantLine::Next {
                    role: role.clone(),
                    permission: perm.clone(),
                    source: code_file.to_owned(),
                });
            }
            continue;
        }

        let setters = sources.setters(role);
        let mut active = sources.active_permissions(role);
        let can_write_workspace_toml = setters
            .iter()
            .all(|source| source == "config/workspace.toml");

        if can_write_workspace_toml {
            let mut role_adds = Vec::new();
            for perm in perms {
                if active.contains(perm.as_str()) {
                    lines.push(GrantLine::Kept {
                        role: role.clone(),
                        permission: perm.clone(),
                    });
                } else {
                    active.insert(perm.as_str());
                    role_adds.push(perm.as_str());
                    lines.push(GrantLine::Grant {
                        role: role.clone(),
                        permission: perm.clone(),
                    });
                }
            }
            if !role_adds.is_empty() {
                additions.push((role.as_str(), role_adds));
            }
        } else {
            let winning_source = setters
                .last()
                .cloned()
                .unwrap_or_else(|| "config/workspace.toml".to_owned());
            for perm in perms {
                if active.contains(perm.as_str()) {
                    lines.push(GrantLine::Kept {
                        role: role.clone(),
                        permission: perm.clone(),
                    });
                } else {
                    lines.push(GrantLine::Next {
                        role: role.clone(),
                        permission: perm.clone(),
                        source: winning_source.clone(),
                    });
                }
            }
        }
    }

    if additions.is_empty() {
        return Ok((None, lines));
    }

    let edited = edit_workspace_toml(workspace_toml, package, &additions)?;
    Ok((Some(edited), lines))
}

/// The first `src/` file, in path order, that builds `ConfigSources` with custom files,
/// `.env` paths, environment snapshots or overrides in code.
#[allow(dead_code)]
pub(crate) fn set_in_code(src: &[(String, String)]) -> Option<String> {
    const CALLS: &[&str] = &[
        ".file(",
        ".section_file(",
        ".set(",
        ".dotenv(",
        ".env_vars(",
    ];
    let mut indices: Vec<usize> = (0..src.len()).collect();
    indices.sort_by(|&a, &b| src[a].0.cmp(&src[b].0));
    for idx in indices {
        let (path, contents) = &src[idx];
        let code = strip_rust_comments(contents);
        if has_whole_word(&code, "ConfigSources") && CALLS.iter().any(|call| code.contains(call)) {
            return Some(path.clone());
        }
    }
    None
}

fn edit_workspace_toml(
    workspace_toml: Option<&str>,
    package: &str,
    additions: &[(&str, Vec<&str>)],
) -> Result<String, String> {
    let raw = workspace_toml.unwrap_or("");
    let crlf = raw.contains("\r\n");
    let mut document: DocumentMut = raw
        .parse()
        .map_err(|err: toml_edit::TomlError| format!("config/workspace.toml: {err}"))?;

    // A file holding only comments keeps them in `document.trailing()`, which `toml_edit`
    // otherwise prints after newly added tables. Move them above the first added table.
    let mut leading_comments = if document.as_table().is_empty() {
        let trailing = document
            .trailing()
            .as_str()
            .unwrap_or_default()
            .replace("\r\n", "\n");
        if trailing.trim().is_empty() {
            None
        } else {
            document.set_trailing("");
            let trimmed = trailing.trim_end_matches('\n');
            Some(format!("{trimmed}\n"))
        }
    } else {
        None
    };

    let roles_item = document.entry("roles").or_insert_with(|| {
        let mut table = Table::new();
        table.set_implicit(true);
        Item::Table(table)
    });
    let roles_is_inline = roles_item.is_inline_table();
    let Some(roles_table) = roles_item.as_table_like_mut() else {
        return Err("config/workspace.toml: `roles` must be a table".to_owned());
    };

    for (role, new_perms) in additions {
        if roles_table.get(role).is_none() {
            if roles_is_inline {
                roles_table.insert(
                    role,
                    Item::Value(Value::InlineTable(toml_edit::InlineTable::new())),
                );
            } else {
                let mut role_table = Table::new();
                if let Some(prefix) = leading_comments.take() {
                    role_table.decor_mut().set_prefix(prefix);
                }
                roles_table.insert(role, Item::Table(role_table));
            }
        }

        let Some(role_table) = roles_table.get_mut(role).and_then(Item::as_table_like_mut) else {
            return Err(format!(
                "config/workspace.toml: `roles.{role}` must be a table"
            ));
        };

        match role_table.get_mut("permissions") {
            None => {
                let mut array = Array::new();
                append_marked_permissions(&mut array, package, new_perms);
                role_table.insert("permissions", Item::Value(Value::Array(array)));
            }
            Some(item) => {
                let Some(array) = item.as_value_mut().and_then(Value::as_array_mut) else {
                    return Err(format!(
                        "config/workspace.toml: `roles.{role}.permissions` must be a list of strings"
                    ));
                };
                append_marked_permissions(array, package, new_perms);
            }
        }
    }

    let rendered = document.to_string();
    if crlf {
        Ok(rendered.replace("\r\n", "\n").replace('\n', "\r\n"))
    } else {
        Ok(rendered)
    }
}

fn append_marked_permissions(array: &mut Array, package: &str, new_perms: &[&str]) {
    if new_perms.is_empty() {
        return;
    }

    let mark = format!(" # plugin:add {package}");
    let existing_len = array.len();

    if existing_len == 0 {
        for (index, &perm) in new_perms.iter().enumerate() {
            let mut val = Value::from(perm);
            let prefix = if index == 0 {
                "\n    ".to_owned()
            } else {
                format!("{mark}\n    ")
            };
            val.decor_mut().set_prefix(prefix);
            val.decor_mut().set_suffix("");
            array.push_formatted(val);
        }
        array.set_trailing_comma(true);
        array.set_trailing(format!("{mark}\n"));
        return;
    }

    let is_multiline = array.iter().any(|val| {
        val.decor()
            .prefix()
            .and_then(|raw| raw.as_str())
            .is_some_and(|prefix| prefix.contains('\n'))
    }) || array
        .trailing()
        .as_str()
        .is_some_and(|trailing| trailing.contains('\n'));

    if !is_multiline {
        for val in array.iter_mut() {
            val.decor_mut().set_prefix("\n    ");
            val.decor_mut().set_suffix("");
        }
        for (index, &perm) in new_perms.iter().enumerate() {
            let mut val = Value::from(perm);
            let prefix = if index == 0 {
                "\n    ".to_owned()
            } else {
                format!("{mark}\n    ")
            };
            val.decor_mut().set_prefix(prefix);
            val.decor_mut().set_suffix("");
            array.push_formatted(val);
        }
        array.set_trailing_comma(true);
        array.set_trailing(format!("{mark}\n"));
        return;
    }

    let elem_indent = array
        .get(existing_len - 1)
        .and_then(|last| last.decor().prefix())
        .and_then(|raw| raw.as_str())
        .and_then(|prefix| {
            prefix
                .rsplit_once('\n')
                .map(|(_, indent)| indent.to_owned())
        })
        .filter(|indent| indent.chars().all(|ch| ch == ' ' || ch == '\t'))
        .unwrap_or_else(|| "    ".to_owned());

    // Turn any multi-element line inside the existing list into one element per line.
    for val in array.iter_mut() {
        let has_newline = val
            .decor()
            .prefix()
            .and_then(|raw| raw.as_str())
            .is_some_and(|prefix| prefix.contains('\n'));
        if !has_newline {
            let current = val
                .decor()
                .prefix()
                .and_then(|raw| raw.as_str())
                .unwrap_or("")
                .trim_start_matches([' ', '\t'])
                .to_owned();
            val.decor_mut()
                .set_prefix(format!("\n{elem_indent}{current}"));
        }
    }

    let after_last = if array.trailing_comma() {
        array.trailing().as_str().unwrap_or("").to_owned()
    } else {
        let suffix = array
            .get(existing_len - 1)
            .and_then(|last| last.decor().suffix())
            .and_then(|raw| raw.as_str())
            .unwrap_or("")
            .to_owned();
        if let Some(last) = array.get_mut(existing_len - 1) {
            last.decor_mut().set_suffix("");
        }
        suffix
    };

    let (before_close_indent, close_indent) = match after_last.rsplit_once('\n') {
        Some((head, tail)) if tail.chars().all(|ch| ch == ' ' || ch == '\t') => {
            (format!("{head}\n"), tail.to_owned())
        }
        _ => (format!("{after_last}\n"), String::new()),
    };

    for (index, &perm) in new_perms.iter().enumerate() {
        let mut val = Value::from(perm);
        let prefix = if index == 0 {
            format!("{before_close_indent}{elem_indent}")
        } else {
            format!("{mark}\n{elem_indent}")
        };
        val.decor_mut().set_prefix(prefix);
        val.decor_mut().set_suffix("");
        array.push_formatted(val);
    }

    array.set_trailing_comma(true);
    array.set_trailing(format!("{mark}\n{close_indent}"));
}

enum ConfigKind<'a> {
    AppToml,
    Section { key: &'a str, json: bool },
    OverlayToml,
}

fn classify_config_path(path: &str) -> Option<ConfigKind<'_>> {
    if let Some(rest) = path.strip_prefix("config/env/") {
        if rest.contains('/') {
            return None;
        }
        let (_, ext) = rest.rsplit_once('.')?;
        if ext.eq_ignore_ascii_case("toml") {
            return Some(ConfigKind::OverlayToml);
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
        return is_toml.then_some(ConfigKind::AppToml);
    }
    if stem == "workspace" || stem.starts_with("workspace.") {
        return Some(ConfigKind::Section {
            key: stem,
            json: is_json,
        });
    }
    None
}

#[derive(Clone, Debug)]
enum ConfigNode {
    Table(BTreeMap<String, ConfigNode>),
    List(Vec<(String, Option<String>)>),
    Scalar(Option<String>),
}

impl ConfigNode {
    fn get(&self, key: &str) -> Option<&ConfigNode> {
        match self {
            Self::Table(map) => map.get(key),
            _ => None,
        }
    }
}

fn wrap_at_key(key: &str, mut node: ConfigNode) -> ConfigNode {
    for segment in key.split('.').rev() {
        let mut map = BTreeMap::new();
        map.insert(segment.to_owned(), node);
        node = ConfigNode::Table(map);
    }
    node
}

fn apply_workspace_node(sources: &mut RoleSources, ws: &ConfigNode, source_name: &str) {
    if let Some(ConfigNode::Table(demo)) = ws.get("demo")
        && let Some(guest) = demo.get("guest_role")
    {
        match guest {
            ConfigNode::Scalar(Some(role)) if !role.trim().is_empty() => {
                sources.demo_guest_role = Some(role.clone());
            }
            ConfigNode::Scalar(_) => {
                sources.demo_guest_role = None;
            }
            _ => {}
        }
    }

    if let Some(ConfigNode::Table(roles)) = ws.get("roles") {
        for (role, role_node) in roles {
            sources.roles.insert(role.clone());
            if let ConfigNode::Table(role_table) = role_node
                && let Some(perms_node) = role_table.get("permissions")
            {
                let permissions = match perms_node {
                    ConfigNode::List(items) => items.clone(),
                    ConfigNode::Scalar(Some(joined)) => split_comma_list(joined)
                        .into_iter()
                        .map(|perm| (perm, None))
                        .collect(),
                    _ => Vec::new(),
                };
                sources
                    .by_role
                    .entry(role.clone())
                    .or_default()
                    .push(RoleSetter {
                        source: source_name.to_owned(),
                        permissions,
                    });
            }
        }
    }
}

fn apply_env_entry(sources: &mut RoleSources, name: &str, value: &str) {
    let Some(segments) = env_segments(name) else {
        return;
    };
    if segments.first().map(String::as_str) != Some("workspace") {
        return;
    }
    match segments.as_slice() {
        [_, demo, guest] if demo == "demo" && guest == "guest_role" => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                sources.demo_guest_role = None;
            } else {
                sources.demo_guest_role = Some(trimmed.to_owned());
            }
        }
        [_, roles, role, perms] if roles == "roles" && perms == "permissions" => {
            sources.roles.insert(role.clone());
            let permissions = split_comma_list(value)
                .into_iter()
                .map(|perm| (perm, None))
                .collect();
            sources
                .by_role
                .entry(role.clone())
                .or_default()
                .push(RoleSetter {
                    source: name.to_owned(),
                    permissions,
                });
        }
        [_, roles, role, ..] if roles == "roles" => {
            sources.roles.insert(role.clone());
        }
        _ => {}
    }
}

fn split_comma_list(joined: &str) -> Vec<String> {
    joined
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
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

fn parse_toml_node(text: &str) -> Option<ConfigNode> {
    let doc = Document::parse(text).ok()?;
    Some(toml_item_to_node(doc.as_item(), text))
}

fn toml_item_to_node(item: &Item, raw: &str) -> ConfigNode {
    match item {
        Item::Table(table) => {
            let mut map = BTreeMap::new();
            for (key, child) in table.iter() {
                map.insert(key.to_owned(), toml_item_to_node(child, raw));
            }
            ConfigNode::Table(map)
        }
        Item::ArrayOfTables(tables) => {
            let mut list = Vec::new();
            for table in tables.iter() {
                let _ = table;
                list.clear();
            }
            ConfigNode::List(list)
        }
        Item::Value(val) => toml_value_to_node(val, raw),
        Item::None => ConfigNode::Scalar(None),
    }
}

fn toml_value_to_node(value: &Value, raw: &str) -> ConfigNode {
    match value {
        Value::InlineTable(table) => {
            let mut map = BTreeMap::new();
            for (key, child) in table.iter() {
                map.insert(key.to_owned(), toml_value_to_node(child, raw));
            }
            ConfigNode::Table(map)
        }
        Value::Array(arr) => {
            let mut items = Vec::new();
            for elem in arr.iter() {
                if let Some(text) = elem.as_str() {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        let mark = elem.span().and_then(|span| line_mark_at(raw, span.start));
                        items.push((trimmed.to_owned(), mark));
                    }
                }
            }
            ConfigNode::List(items)
        }
        Value::String(repr) => ConfigNode::Scalar(Some(repr.value().clone())),
        Value::Integer(repr) => ConfigNode::Scalar(Some(repr.value().to_string())),
        Value::Float(repr) => ConfigNode::Scalar(Some(repr.value().to_string())),
        Value::Boolean(repr) => ConfigNode::Scalar(Some(repr.value().to_string())),
        Value::Datetime(repr) => ConfigNode::Scalar(Some(repr.value().to_string())),
    }
}

fn line_mark_at(raw: &str, offset: usize) -> Option<String> {
    let bytes = raw.as_bytes();
    if offset > bytes.len() {
        return None;
    }
    let line_start = raw[..offset].rfind('\n').map_or(0, |pos| pos + 1);
    let line_end = raw[offset..]
        .find(['\r', '\n'])
        .map_or(raw.len(), |pos| offset + pos);
    let line = &raw[line_start..line_end];
    let comment_index = toml_line_comment_start(line)?;
    let comment = line[comment_index..].trim_end();
    let pkg = comment.strip_prefix("# plugin:add ")?;
    is_package_mark(pkg).then(|| pkg.to_owned())
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

fn is_package_mark(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-' || *b == b'_')
}

fn parse_json_node(text: &str) -> Option<ConfigNode> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    Some(json_value_to_node(&value))
}

fn json_value_to_node(value: &serde_json::Value) -> ConfigNode {
    match value {
        serde_json::Value::Object(map) => {
            let mut out = BTreeMap::new();
            for (key, child) in map {
                out.insert(key.clone(), json_value_to_node(child));
            }
            ConfigNode::Table(out)
        }
        serde_json::Value::Array(items) => {
            let list = items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| (item.to_owned(), None))
                .collect();
            ConfigNode::List(list)
        }
        serde_json::Value::String(text) => ConfigNode::Scalar(Some(text.clone())),
        serde_json::Value::Bool(flag) => ConfigNode::Scalar(Some(flag.to_string())),
        serde_json::Value::Number(num) => ConfigNode::Scalar(Some(num.to_string())),
        serde_json::Value::Null => ConfigNode::Scalar(None),
    }
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

fn strip_rust_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            let mut depth = 1usize;
            i += 2;
            while i < bytes.len() && depth > 0 {
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    if bytes[i] == b'\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
            }
            out.push(' ');
            continue;
        }
        if let Some(raw_end) = raw_string_end(bytes, i) {
            out.push_str(&src[i..raw_end]);
            i = raw_end;
            continue;
        }
        if bytes[i] == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            out.push_str(&src[start..i]);
            continue;
        }
        if let Some(char_end) = char_literal_end(src, i) {
            out.push_str(&src[i..char_end]);
            i = char_end;
            continue;
        }
        let ch = src[i..].chars().next().expect("non-empty slice");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn raw_string_end(bytes: &[u8], at: usize) -> Option<usize> {
    if at > 0 && is_ident_byte(bytes[at - 1]) {
        return None;
    }
    let prefix_len = if bytes[at..].starts_with(b"br") || bytes[at..].starts_with(b"cr") {
        2
    } else if bytes[at] == b'r' {
        1
    } else {
        return None;
    };
    let mut pos = at + prefix_len;
    let mut hashes = 0usize;
    while pos < bytes.len() && bytes[pos] == b'#' {
        hashes += 1;
        pos += 1;
    }
    if pos >= bytes.len() || bytes[pos] != b'"' {
        return None;
    }
    pos += 1;
    while pos < bytes.len() {
        if bytes[pos] == b'"'
            && pos + 1 + hashes <= bytes.len()
            && bytes[pos + 1..pos + 1 + hashes]
                .iter()
                .all(|&byte| byte == b'#')
        {
            return Some(pos + 1 + hashes);
        }
        pos += 1;
    }
    Some(bytes.len())
}

fn char_literal_end(src: &str, at: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    if bytes.get(at) != Some(&b'\'') {
        return None;
    }
    let rest = &src[at + 1..];
    let mut chars = rest.char_indices();
    let (_, first) = chars.next()?;
    if first == '\'' || first == '\n' || first == '\r' {
        return None;
    }
    if first == '\\' {
        let (_, _) = chars.next()?;
        let (close_idx, close_ch) = chars.next()?;
        if close_ch == '\'' {
            return Some(at + 1 + close_idx + 1);
        }
        return None;
    }
    let (close_idx, close_ch) = chars.next()?;
    (close_ch == '\'').then_some(at + 1 + close_idx + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grants_map(entries: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
        entries
            .iter()
            .map(|(role, perms)| {
                (
                    (*role).to_owned(),
                    perms.iter().map(|perm| (*perm).to_owned()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn every_file_the_loader_reads_is_a_source_of_a_role_list() {
        let requested = grants_map(&[("member", &["announcements.read"])]);

        // 1. config/app.toml (also checks roles() and demo_guest_role()).
        let mut files = AppFiles::default();
        files.config.insert(
            "config/app.toml".to_owned(),
            "[workspace.demo]\nguest_role = \"guest\"\n\n[workspace.roles.member]\npermissions = [\"deals.read\"]\n\n[workspace.roles.support]\npermissions = [\"tickets.reply\"]\n".to_owned(),
        );
        let scanned = scan(&files, &[]);
        assert_eq!(scanned.setters("member"), vec!["config/app.toml"]);
        assert!(scanned.roles().contains("support"));
        assert!(scanned.roles().contains("member"));
        assert_eq!(scanned.demo_guest_role().as_deref(), Some("guest"));
        let (edited, lines) = plan(
            &scanned,
            None,
            "navree-plugin-announcements",
            &requested,
            None,
        )
        .unwrap();
        assert_eq!(edited, None);
        assert_eq!(
            lines[0].text(),
            "  next     add announcements.read to workspace.roles.member.permissions in config/app.toml"
        );

        // 2. config/workspace.json
        let mut files = AppFiles::default();
        files.config.insert(
            "config/workspace.json".to_owned(),
            r#"{"roles":{"member":{"permissions":["deals.read"]}}}"#.to_owned(),
        );
        let scanned = scan(&files, &[]);
        assert_eq!(scanned.setters("member"), vec!["config/workspace.json"]);
        let (edited, lines) = plan(
            &scanned,
            None,
            "navree-plugin-announcements",
            &requested,
            None,
        )
        .unwrap();
        assert_eq!(edited, None);
        assert_eq!(
            lines[0].text(),
            "  next     add announcements.read to workspace.roles.member.permissions in config/workspace.json"
        );

        // 3. config/workspace.roles.toml
        let mut files = AppFiles::default();
        files.config.insert(
            "config/workspace.roles.toml".to_owned(),
            "[member]\npermissions = [\"deals.read\"]\n".to_owned(),
        );
        let scanned = scan(&files, &[]);
        assert_eq!(
            scanned.setters("member"),
            vec!["config/workspace.roles.toml"]
        );
        let (edited, lines) = plan(
            &scanned,
            None,
            "navree-plugin-announcements",
            &requested,
            None,
        )
        .unwrap();
        assert_eq!(edited, None);
        assert_eq!(
            lines[0].text(),
            "  next     add announcements.read to workspace.roles.member.permissions in config/workspace.roles.toml"
        );

        // 4. config/env/production.toml
        let mut files = AppFiles::default();
        files.config.insert(
            "config/env/production.toml".to_owned(),
            "[workspace.roles.member]\npermissions = [\"deals.read\"]\n".to_owned(),
        );
        let scanned = scan(&files, &[]);
        assert_eq!(
            scanned.setters("member"),
            vec!["config/env/production.toml"]
        );
        let (edited, lines) = plan(
            &scanned,
            None,
            "navree-plugin-announcements",
            &requested,
            None,
        )
        .unwrap();
        assert_eq!(edited, None);
        assert_eq!(
            lines[0].text(),
            "  next     add announcements.read to workspace.roles.member.permissions in config/env/production.toml"
        );

        // 5. .env (names the variable, never its value).
        let mut files = AppFiles::default();
        files.dot_env.insert(
            ".env".to_owned(),
            "APP_WORKSPACE__ROLES__MEMBER__PERMISSIONS=khata.ludhiana_secret\n".to_owned(),
        );
        let scanned = scan(&files, &[]);
        assert_eq!(
            scanned.setters("member"),
            vec!["APP_WORKSPACE__ROLES__MEMBER__PERMISSIONS"]
        );
        let (edited, lines) = plan(
            &scanned,
            None,
            "navree-plugin-announcements",
            &requested,
            None,
        )
        .unwrap();
        assert_eq!(edited, None);
        assert_eq!(
            lines[0].text(),
            "  next     add announcements.read to workspace.roles.member.permissions in APP_WORKSPACE__ROLES__MEMBER__PERMISSIONS"
        );
        assert!(!lines[0].text().contains("ludhiana_secret"));

        // 6. Process environment (names the variable, never its value; overrides demo_guest_role).
        let mut files = AppFiles::default();
        files.config.insert(
            "config/app.toml".to_owned(),
            "[workspace.demo]\nguest_role = \"viewer\"\n".to_owned(),
        );
        let process_env = vec![
            (
                "APP_WORKSPACE__ROLES__MEMBER__PERMISSIONS".to_owned(),
                "amritsar.private_token".to_owned(),
            ),
            (
                "APP_WORKSPACE__DEMO__GUEST_ROLE".to_owned(),
                "visitor".to_owned(),
            ),
        ];
        let scanned = scan(&files, &process_env);
        assert_eq!(
            scanned.setters("member"),
            vec!["APP_WORKSPACE__ROLES__MEMBER__PERMISSIONS"]
        );
        assert_eq!(scanned.demo_guest_role().as_deref(), Some("visitor"));
        let (edited, lines) = plan(
            &scanned,
            None,
            "navree-plugin-announcements",
            &requested,
            None,
        )
        .unwrap();
        assert_eq!(edited, None);
        assert_eq!(
            lines[0].text(),
            "  next     add announcements.read to workspace.roles.member.permissions in APP_WORKSPACE__ROLES__MEMBER__PERMISSIONS"
        );
        assert!(!lines[0].text().contains("private_token"));
    }

    #[test]
    fn a_permission_is_added_on_a_line_of_its_own_with_its_mark() {
        let single = grants_map(&[("member", &["announcements.read"])]);

        // Absent file: created with one marked element.
        let empty_files = AppFiles::default();
        let scanned = scan(&empty_files, &[]);
        let (created, lines) =
            plan(&scanned, None, "navree-plugin-announcements", &single, None).unwrap();
        assert_eq!(
            created.as_deref(),
            Some(
                "[roles.member]\npermissions = [\n    \"announcements.read\", # plugin:add navree-plugin-announcements\n]\n"
            )
        );
        assert_eq!(
            lines[0].text(),
            "  grant    member: announcements.read (config/workspace.toml)"
        );

        // Existing one-line list becomes one element per line, keeping existing elements first.
        let initial = "[roles.member]\npermissions = [\"deals.read\"]\n";
        let mut files = AppFiles::default();
        files
            .config
            .insert("config/workspace.toml".to_owned(), initial.to_owned());
        let scanned = scan(&files, &[]);
        let (after_first, _) = plan(
            &scanned,
            Some(initial),
            "navree-plugin-announcements",
            &single,
            None,
        )
        .unwrap();
        let after_first = after_first.expect("edited workspace.toml");
        assert_eq!(
            after_first,
            "[roles.member]\npermissions = [\n    \"deals.read\",\n    \"announcements.read\", # plugin:add navree-plugin-announcements\n]\n"
        );

        // Adding a second permission appends below the first, keeping both marks.
        let mut files = AppFiles::default();
        files
            .config
            .insert("config/workspace.toml".to_owned(), after_first.clone());
        let scanned = scan(&files, &[]);
        let second = grants_map(&[("member", &["notices.publish"])]);
        let (after_second, _) = plan(
            &scanned,
            Some(&after_first),
            "navree-plugin-notices",
            &second,
            None,
        )
        .unwrap();
        assert_eq!(
            after_second.as_deref(),
            Some(
                "[roles.member]\npermissions = [\n    \"deals.read\",\n    \"announcements.read\", # plugin:add navree-plugin-announcements\n    \"notices.publish\", # plugin:add navree-plugin-notices\n]\n"
            )
        );

        // A comment above `[roles.member]` stays in place.
        let commented = "# Permissions for Ludhiana branch workspaces.\n[roles.member]\npermissions = [\"deals.read\"]\n";
        let mut files = AppFiles::default();
        files
            .config
            .insert("config/workspace.toml".to_owned(), commented.to_owned());
        let scanned = scan(&files, &[]);
        let (with_comment, _) = plan(
            &scanned,
            Some(commented),
            "navree-plugin-announcements",
            &single,
            None,
        )
        .unwrap();
        assert_eq!(
            with_comment.as_deref(),
            Some(
                "# Permissions for Ludhiana branch workspaces.\n[roles.member]\npermissions = [\n    \"deals.read\",\n    \"announcements.read\", # plugin:add navree-plugin-announcements\n]\n"
            )
        );
    }

    #[test]
    fn lines_marked_for_the_same_package_do_not_count_as_the_apps() {
        let mut files = AppFiles::default();
        files.config.insert(
            "config/workspace.toml".to_owned(),
            "[roles.member]\npermissions = [\n    \"deals.read\",\n    \"announcements.read\", # plugin:add navree-plugin-announcements\n]\n".to_owned(),
        );
        files.config.insert(
            "config/workspace.roles.json".to_owned(),
            r#"{"viewer":{"permissions":["khata.view"]}}"#.to_owned(),
        );
        let scanned = scan(&files, &[]);
        assert_eq!(
            scanned.granted(),
            vec![
                Granted {
                    permission: "deals.read".to_owned(),
                    source: "config/workspace.toml".to_owned(),
                    marked_for: None,
                },
                Granted {
                    permission: "announcements.read".to_owned(),
                    source: "config/workspace.toml".to_owned(),
                    marked_for: Some("navree-plugin-announcements".to_owned()),
                },
                Granted {
                    permission: "khata.view".to_owned(),
                    source: "config/workspace.roles.json".to_owned(),
                    marked_for: None,
                },
            ]
        );
    }

    #[test]
    fn a_role_list_set_by_code_is_not_written() {
        let default_src = vec![
            (
                "src/app.rs".to_owned(),
                "use navree::config::ConfigSources;\n// ConfigSources::new().set(\"a\", \"b\")\npub fn app(s: ConfigSources) {}\n".to_owned(),
            ),
            (
                "src/main.rs".to_owned(),
                "use navree::config::ConfigSources;\nfn main() { let _ = ConfigSources::defaults(); }\n".to_owned(),
            ),
        ];
        assert_eq!(set_in_code(&default_src), None);

        let custom_src = vec![
            (
                "src/main.rs".to_owned(),
                "use navree::config::ConfigSources;\nfn main() { let _ = ConfigSources::defaults().set(\"workspace.isolation\", \"row\"); }\n".to_owned(),
            ),
            (
                "src/other.rs".to_owned(),
                "use navree::config::ConfigSources;\nfn f() { let _ = ConfigSources::new().file(\"custom.toml\"); }\n".to_owned(),
            ),
        ];
        let code_source = set_in_code(&custom_src);
        assert_eq!(code_source.as_deref(), Some("src/main.rs"));

        let scanned = scan(&AppFiles::default(), &[]);
        let requested = grants_map(&[("member", &["announcements.read", "announcements.publish"])]);
        let (edited, lines) = plan(
            &scanned,
            None,
            "navree-plugin-announcements",
            &requested,
            code_source.as_deref(),
        )
        .unwrap();
        assert_eq!(edited, None);
        assert_eq!(
            lines,
            vec![
                GrantLine::Next {
                    role: "member".to_owned(),
                    permission: "announcements.read".to_owned(),
                    source: "src/main.rs".to_owned(),
                },
                GrantLine::Next {
                    role: "member".to_owned(),
                    permission: "announcements.publish".to_owned(),
                    source: "src/main.rs".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_permission_the_role_holds_is_kept() {
        let existing = "[roles.member]\npermissions = [\n    \"announcements.read\", # plugin:add navree-plugin-announcements\n]\n";
        let mut files = AppFiles::default();
        files
            .config
            .insert("config/workspace.toml".to_owned(), existing.to_owned());
        let scanned = scan(&files, &[]);
        let requested = grants_map(&[("member", &["announcements.read"])]);
        let (edited, lines) = plan(
            &scanned,
            Some(existing),
            "navree-plugin-announcements",
            &requested,
            None,
        )
        .unwrap();
        assert_eq!(edited, None);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].text(),
            "  kept     member already has announcements.read"
        );
    }

    #[test]
    fn inline_table_role_definitions_are_scanned_and_updated() {
        let mut files = AppFiles::default();
        files.config.insert(
            "config/app.toml".to_owned(),
            "[workspace]\nroles = { member = { permissions = [\"deals.read\", \"khata.audit\"] }, support = { permissions = [\"tickets.reply\"] } }\n"
                .to_owned(),
        );
        let scanned = scan(&files, &[]);
        assert!(scanned.roles().contains("member"));
        assert!(scanned.roles().contains("support"));
        assert_eq!(scanned.setters("member"), vec!["config/app.toml"]);
        assert!(
            scanned
                .granted()
                .iter()
                .any(|g| g.permission == "khata.audit" && g.source == "config/app.toml")
        );

        let requested = grants_map(&[("member", &["khata.read"])]);
        let (edited, lines) =
            plan(&scanned, None, "navree-plugin-khata", &requested, None).unwrap();
        assert_eq!(edited, None);
        assert_eq!(
            lines,
            vec![GrantLine::Next {
                role: "member".to_owned(),
                permission: "khata.read".to_owned(),
                source: "config/app.toml".to_owned(),
            }]
        );

        // Inline `roles` table inside `config/workspace.toml` can gain a new role without panicking.
        let inline_ws = "roles = { member = { permissions = [\"deals.read\"] } }\n";
        let mut ws_files = AppFiles::default();
        ws_files
            .config
            .insert("config/workspace.toml".to_owned(), inline_ws.to_owned());
        let ws_scanned = scan(&ws_files, &[]);
        let add_support = grants_map(&[("support", &["khata.read"])]);
        let (ws_edited, _) = plan(
            &ws_scanned,
            Some(inline_ws),
            "navree-plugin-khata",
            &add_support,
            None,
        )
        .unwrap();
        let ws_edited = ws_edited.expect("inline workspace.toml updated");
        assert!(ws_edited.contains("khata.read"));
    }
}
