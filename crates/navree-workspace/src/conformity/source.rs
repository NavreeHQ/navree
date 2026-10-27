//! A light reading of an app's Rust source for the workspace conformity
//! checks: which SeaORM entities it defines (and their tables), and where
//! code reaches an entity directly (`Entity::find`, `Repo::<Entity>`)
//! instead of through `WorkspaceRepo`.
//!
//! It reads text, not a syntax tree: comments and string contents are
//! blanked, paths are resolved through `use` declarations (trees, `as`,
//! `self`, `super`, `crate` and globs) and `type` aliases, and modules come
//! from file locations and inline `mod name { .. }` blocks. Test code is
//! left out: `tests/`, `benches/` and `examples/` directories and
//! `#[cfg(test)] mod` blocks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The marker comment that allows one direct access, on its line or the
/// line above: `// navree:allow(workspace-repo)`.
pub(crate) const ALLOW_MARKER: &str = "navree:allow(workspace-repo)";

/// Associated functions on an entity that read or write its table without
/// a workspace scope.
const ENTITY_ENTRY_POINTS: [&str; 9] = [
    "find",
    "find_by_id",
    "insert",
    "insert_many",
    "update",
    "update_many",
    "delete",
    "delete_many",
    "delete_by_id",
];

/// Directories never read: build output, dependencies, front ends, test
/// and example code.
const SKIPPED_DIRS: [&str; 7] = [
    "target",
    "node_modules",
    "tests",
    "benches",
    "examples",
    "web",
    "migrations",
];

/// An entity the source defines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EntityDef {
    /// Its table (`#[sea_orm(table_name = "..")]`).
    pub(crate) table: String,
    /// The absolute path of its `Entity` type: crate, modules, `Entity`.
    pub(crate) path: Vec<String>,
    /// The file and line of the `table_name` attribute.
    pub(crate) location: String,
}

impl EntityDef {
    /// `deal::Entity`: how code usually names it.
    pub(crate) fn short_name(&self) -> String {
        match self.path.len() {
            0..=2 => "Entity".to_owned(),
            n => format!("{}::Entity", self.path[n - 2]),
        }
    }
}

/// A direct access to an entity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectAccess {
    /// `src/deals/service.rs:42`.
    pub(crate) location: String,
    /// The entity's table.
    pub(crate) table: String,
    /// What the code wrote: `entity::Entity::find`, `Repo::<Deal>`.
    pub(crate) expression: String,
}

/// The Rust source of the crates under a root.
#[derive(Debug, Default)]
pub(crate) struct Project {
    files: Vec<SourceFile>,
    crates: BTreeSet<String>,
}

impl Project {
    /// Reads every crate (a directory with a `Cargo.toml` that has a
    /// `[package]`) under `root`, `root` included.
    pub(crate) fn load(root: &Path) -> Project {
        let mut crates = Vec::new();
        find_crates(root, &mut crates);
        let mut project = Project::default();
        for (dir, name) in &crates {
            project.crates.insert(name.clone());
            let src = dir.join("src");
            let mut files = Vec::new();
            rust_files(&src, &mut files);
            files.sort();
            for path in files {
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let module = module_of(&src, &path);
                let display = display_path(root, &path);
                project
                    .files
                    .push(SourceFile::parse(display, name.clone(), module, &text));
            }
        }
        project
    }

    /// A project from in-memory files: `(display path, crate, module path,
    /// text)`.
    #[cfg(test)]
    pub(crate) fn from_files(files: &[(&str, &str, &[&str], &str)]) -> Project {
        let mut project = Project::default();
        for (display, krate, module, text) in files {
            project.crates.insert((*krate).to_owned());
            project.files.push(SourceFile::parse(
                (*display).to_owned(),
                (*krate).to_owned(),
                module.iter().map(|s| (*s).to_owned()).collect(),
                text,
            ));
        }
        project
    }

    /// How many `.rs` files were read.
    pub(crate) fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Every entity definition, in file order.
    pub(crate) fn entities(&self) -> Vec<EntityDef> {
        self.files
            .iter()
            .flat_map(|file| file.entities.iter().cloned())
            .collect()
    }

    /// Every direct access to one of `entities` (absolute `Entity` paths)
    /// not allowed by the marker comment.
    pub(crate) fn direct_accesses(&self, entities: &[EntityDef]) -> Vec<DirectAccess> {
        let by_path: BTreeMap<&[String], &EntityDef> =
            entities.iter().map(|e| (e.path.as_slice(), e)).collect();
        let mut found = Vec::new();
        for file in &self.files {
            for candidate in &file.paths {
                if file.allowed(candidate.line) {
                    continue;
                }
                let resolved = file.resolve(&candidate.module, &candidate.segments, &self.crates);
                if let Some(entity) = resolved
                    .iter()
                    .find_map(|path| by_path.get(path.as_slice()))
                {
                    found.push(DirectAccess {
                        location: format!("{}:{}", file.display, candidate.line),
                        table: entity.table.clone(),
                        expression: candidate.expression.clone(),
                    });
                }
            }
        }
        found
    }
}

/// A path the code uses in a way that bypasses `WorkspaceRepo`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    line: usize,
    /// The inline module path (inside the file) where it appears.
    module: Vec<String>,
    /// The type path as written (`entity::Entity`, `Deal`).
    segments: Vec<String>,
    /// The expression as written, for the report.
    expression: String,
}

/// One parsed `.rs` file.
#[derive(Debug)]
struct SourceFile {
    display: String,
    krate: String,
    /// The module path of the file inside its crate.
    module: Vec<String>,
    /// Comment text per line (1-based index - 1).
    comments: Vec<String>,
    /// `use` names per inline module path: local name -> written path
    /// (with `self`/`super`/`crate` still in it).
    uses: BTreeMap<Vec<String>, BTreeMap<String, Vec<String>>>,
    /// Glob imports per inline module path.
    globs: BTreeMap<Vec<String>, Vec<Vec<String>>>,
    entities: Vec<EntityDef>,
    paths: Vec<Candidate>,
    /// Line ranges of `#[cfg(test)]` modules, left out.
    test_lines: Vec<(usize, usize)>,
}

impl SourceFile {
    fn parse(display: String, krate: String, module: Vec<String>, text: &str) -> SourceFile {
        let stripped = strip(text);
        let tokens = tokenize(&stripped.bare);
        let mut file = SourceFile {
            display,
            krate,
            module,
            comments: stripped.comments,
            uses: BTreeMap::new(),
            globs: BTreeMap::new(),
            entities: Vec::new(),
            paths: Vec::new(),
            test_lines: Vec::new(),
        };
        let line_modules = file.walk(&tokens);
        file.find_entities(&stripped.code, &line_modules);
        file
    }

    /// Whether the marker comment allows line `line` (1-based).
    fn allowed(&self, line: usize) -> bool {
        let has = |l: usize| {
            l >= 1
                && self
                    .comments
                    .get(l - 1)
                    .is_some_and(|c| c.contains(ALLOW_MARKER))
        };
        has(line) || has(line.saturating_sub(1))
    }

    /// Walks the tokens: tracks inline modules, skips `#[cfg(test)]`
    /// modules, collects `use` declarations, `type` aliases and candidate
    /// paths. Returns the inline module path at the start of each line.
    fn walk(&mut self, tokens: &[Token]) -> Vec<Vec<String>> {
        let mut line_modules: Vec<Vec<String>> = Vec::new();
        // (inline module name, brace depth inside it)
        let mut stack: Vec<(String, usize)> = Vec::new();
        let mut depth = 0usize;
        let mut cfg_test = false;
        let mut i = 0;
        let current = |stack: &[(String, usize)]| -> Vec<String> {
            stack.iter().map(|(name, _)| name.clone()).collect()
        };
        while i < tokens.len() {
            let token = &tokens[i];
            while line_modules.len() < token.line {
                line_modules.push(current(&stack));
            }
            match token.text.as_str() {
                "#" if is_cfg_test(tokens, i) => {
                    cfg_test = true;
                    i += 1;
                    continue;
                }
                "mod"
                    if i + 2 < tokens.len()
                        && is_ident(&tokens[i + 1].text)
                        && tokens[i + 2].text == "{" =>
                {
                    if cfg_test {
                        // Skip the whole test module.
                        let end = skip_block(tokens, i + 2);
                        let last = tokens
                            .get(end.saturating_sub(1))
                            .map_or(token.line, |t| t.line);
                        self.test_lines.push((token.line, last));
                        i = end;
                        cfg_test = false;
                        continue;
                    }
                    depth += 1;
                    stack.push((tokens[i + 1].text.clone(), depth));
                    i += 3;
                    continue;
                }
                "use" => {
                    let end = find(tokens, i, ";");
                    let module = current(&stack);
                    let (names, globs) = parse_use(&tokens[i + 1..end]);
                    self.uses.entry(module.clone()).or_default().extend(names);
                    self.globs.entry(module).or_default().extend(globs);
                    cfg_test = false;
                    i = end + 1;
                    continue;
                }
                "type"
                    if i + 3 < tokens.len()
                        && is_ident(&tokens[i + 1].text)
                        && tokens[i + 2].text == "=" =>
                {
                    let end = find(tokens, i, ";");
                    let (path, used) = read_path(&tokens[i + 3..end]);
                    if !path.is_empty() && used == end - (i + 3) {
                        self.uses
                            .entry(current(&stack))
                            .or_default()
                            .insert(tokens[i + 1].text.clone(), path);
                    }
                    i = end + 1;
                    continue;
                }
                "{" => {
                    depth += 1;
                    cfg_test = false;
                }
                "}" => {
                    if stack.last().is_some_and(|(_, d)| *d == depth) {
                        stack.pop();
                    }
                    depth = depth.saturating_sub(1);
                }
                ";" => cfg_test = false,
                text if is_ident(text) && !is_path_continuation(tokens, i) => {
                    let (path, used) = read_path(&tokens[i..]);
                    let after = i + used;
                    let text_at = |k: usize| tokens.get(k).map(|t| t.text.as_str());
                    // `Repo::<path>`: navree-db's repository on an entity.
                    if path.last().is_some_and(|s| s == "Repo")
                        && text_at(after) == Some("::")
                        && text_at(after + 1) == Some("<")
                    {
                        let (entity, entity_used) = read_path(&tokens[after + 2..]);
                        if !entity.is_empty() && text_at(after + 2 + entity_used) == Some(">") {
                            self.paths.push(Candidate {
                                line: token.line,
                                module: current(&stack),
                                expression: format!("Repo::<{}>", entity.join("::")),
                                segments: entity,
                            });
                        }
                        i = after + 2 + entity_used;
                        continue;
                    }
                    // `path::entry_point(`: the entry point is the last
                    // segment, the entity the ones before it.
                    if path.len() >= 2
                        && ENTITY_ENTRY_POINTS.contains(&path[path.len() - 1].as_str())
                        && text_at(after) == Some("(")
                    {
                        let entity = path[..path.len() - 1].to_vec();
                        self.paths.push(Candidate {
                            line: token.line,
                            module: current(&stack),
                            expression: path.join("::"),
                            segments: entity,
                        });
                    }
                    i += used.max(1);
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
        line_modules.push(current(&stack));
        line_modules
    }

    /// Finds `#[sea_orm(table_name = "..")]` attributes.
    fn find_entities(&mut self, code: &str, line_modules: &[Vec<String>]) {
        let lines: Vec<&str> = code.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            let in_attribute =
                line.contains("sea_orm(") || (index > 0 && lines[index - 1].contains("#[sea_orm("));
            let in_tests = self
                .test_lines
                .iter()
                .any(|&(start, end)| (start..=end).contains(&(index + 1)));
            if !in_attribute || in_tests {
                continue;
            }
            let Some(table) = table_name(line) else {
                continue;
            };
            let inline = line_modules
                .get(index)
                .or_else(|| line_modules.last())
                .cloned()
                .unwrap_or_default();
            let mut path = vec![self.krate.clone()];
            path.extend(self.module.iter().cloned());
            path.extend(inline);
            path.push("Entity".to_owned());
            self.entities.push(EntityDef {
                table,
                path,
                location: format!("{}:{}", self.display, index + 1),
            });
        }
    }

    /// The absolute module path of inline module `inline` of this file.
    fn absolute_module(&self, inline: &[String]) -> Vec<String> {
        let mut path = vec![self.krate.clone()];
        path.extend(self.module.iter().cloned());
        path.extend(inline.iter().cloned());
        path
    }

    /// The absolute paths `segments`, written in inline module `inline`,
    /// may stand for (several when a glob import could provide the name).
    fn resolve(
        &self,
        inline: &[String],
        segments: &[String],
        crates: &BTreeSet<String>,
    ) -> Vec<Vec<String>> {
        let mut resolved = Vec::new();
        self.resolve_into(inline, segments, crates, &mut resolved, 0);
        resolved
    }

    fn resolve_into(
        &self,
        inline: &[String],
        segments: &[String],
        crates: &BTreeSet<String>,
        out: &mut Vec<Vec<String>>,
        hops: usize,
    ) {
        let Some(first) = segments.first() else {
            return;
        };
        if hops > 8 {
            return;
        }
        let here = self.absolute_module(inline);
        let rest = &segments[1..];
        // `use` names and aliases of this module.
        if let Some(target) = self.uses.get(inline).and_then(|names| names.get(first)) {
            let mut written = target.clone();
            written.extend(rest.iter().cloned());
            // A use path is relative to the module it is declared in.
            self.resolve_into(inline, &written, crates, out, hops + 1);
            return;
        }
        match first.as_str() {
            "crate" => out.push(join(std::slice::from_ref(&self.krate), rest)),
            "self" => out.push(join(&here, rest)),
            "super" => {
                let mut base = here.clone();
                let mut rest = segments;
                while rest.first().is_some_and(|s| s == "super") {
                    base.pop();
                    rest = &rest[1..];
                }
                out.push(join(&base, rest));
            }
            name if crates.contains(name) && name != "Entity" => out.push(segments.to_vec()),
            _ => {
                // An item of this module, or one a glob import brings in.
                out.push(join(&here, segments));
                for glob in self.globs.get(inline).into_iter().flatten() {
                    let mut written = glob.clone();
                    written.extend(segments.iter().cloned());
                    self.resolve_into(inline, &written, crates, out, hops + 1);
                }
            }
        }
    }
}

fn join(base: &[String], rest: &[String]) -> Vec<String> {
    let mut path = base.to_vec();
    path.extend(rest.iter().cloned());
    path
}

/// `table_name = "deals"` on a line: `deals`.
fn table_name(line: &str) -> Option<String> {
    let start = line.find("table_name")?;
    let rest = line[start + "table_name".len()..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

/// Source text with comments removed (`code`), with comments and string
/// contents removed (`bare`), and each line's comment text.
#[derive(Debug, Default, PartialEq, Eq)]
struct Stripped {
    code: String,
    bare: String,
    comments: Vec<String>,
}

/// Blanks comments (and, in `bare`, string and char literal contents) with
/// spaces, keeping every line break, so lines and columns stay where they
/// were.
fn strip(text: &str) -> Stripped {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Stripped::default();
    let mut comment = String::new();
    let mut i = 0;
    let push = |out: &mut Stripped, code: char, bare: char| {
        out.code.push(code);
        out.bare.push(bare);
    };
    let newline = |out: &mut Stripped, comment: &mut String| {
        out.code.push('\n');
        out.bare.push('\n');
        out.comments.push(std::mem::take(comment));
    };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let prev_ident = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        if c == '\n' {
            newline(&mut out, &mut comment);
            i += 1;
        } else if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                comment.push(chars[i]);
                push(&mut out, ' ', ' ');
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0usize;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    push(&mut out, ' ', ' ');
                    push(&mut out, ' ', ' ');
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    push(&mut out, ' ', ' ');
                    push(&mut out, ' ', ' ');
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else if chars[i] == '\n' {
                    newline(&mut out, &mut comment);
                    i += 1;
                } else {
                    comment.push(chars[i]);
                    push(&mut out, ' ', ' ');
                    i += 1;
                }
            }
        } else if !prev_ident
            && (c == 'r' || (c == 'b' && next == Some('r')))
            && raw_string_start(&chars, i).is_some()
        {
            let (hashes, open) = raw_string_start(&chars, i).unwrap_or_default();
            for &ch in &chars[i..=open] {
                push(&mut out, ch, ch);
            }
            i = open + 1;
            while i < chars.len() {
                let closes =
                    chars[i] == '"' && (0..hashes).all(|k| chars.get(i + 1 + k) == Some(&'#'));
                if closes {
                    for &ch in &chars[i..=i + hashes] {
                        push(&mut out, ch, ch);
                    }
                    i += hashes + 1;
                    break;
                }
                if chars[i] == '\n' {
                    newline(&mut out, &mut comment);
                } else {
                    push(&mut out, chars[i], ' ');
                }
                i += 1;
            }
        } else if c == '"' {
            push(&mut out, '"', '"');
            i += 1;
            while i < chars.len() {
                match chars[i] {
                    '\\' => {
                        push(&mut out, '\\', ' ');
                        if let Some(&escaped) = chars.get(i + 1) {
                            if escaped == '\n' {
                                newline(&mut out, &mut comment);
                            } else {
                                push(&mut out, escaped, ' ');
                            }
                        }
                        i += 2;
                    }
                    '"' => {
                        push(&mut out, '"', '"');
                        i += 1;
                        break;
                    }
                    '\n' => {
                        newline(&mut out, &mut comment);
                        i += 1;
                    }
                    other => {
                        push(&mut out, other, ' ');
                        i += 1;
                    }
                }
            }
        } else if c == '\'' {
            // A char literal ('a', '\n', '\u{..}') or a lifetime ('a).
            let end = if next == Some('\\') {
                chars[i + 2..]
                    .iter()
                    .position(|&ch| ch == '\'' || ch == '\n')
                    .map(|p| i + 2 + p)
                    .filter(|&p| chars[p] == '\'')
            } else if chars.get(i + 2) == Some(&'\'') && next != Some('\n') {
                Some(i + 2)
            } else {
                None
            };
            match end {
                Some(end) => {
                    push(&mut out, '\'', '\'');
                    for &ch in &chars[i + 1..end] {
                        push(&mut out, ch, ' ');
                    }
                    push(&mut out, '\'', '\'');
                    i = end + 1;
                }
                None => {
                    push(&mut out, c, c);
                    i += 1;
                }
            }
        } else {
            push(&mut out, c, c);
            i += 1;
        }
    }
    out.comments.push(comment);
    out
}

/// `(hashes, index of the opening quote)` when a raw string starts at `i`
/// (`r"`, `r#"`, `br##"`).
fn raw_string_start(chars: &[char], i: usize) -> Option<(usize, usize)> {
    let mut j = i;
    if chars.get(j) == Some(&'b') {
        j += 1;
    }
    if chars.get(j) != Some(&'r') {
        return None;
    }
    j += 1;
    let mut hashes = 0;
    while chars.get(j) == Some(&'#') {
        hashes += 1;
        j += 1;
    }
    (chars.get(j) == Some(&'"')).then_some((hashes, j))
}

/// A token of blanked source: an identifier, `::` or one punctuation
/// character.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Token {
    text: String,
    line: usize,
}

fn tokenize(bare: &str) -> Vec<Token> {
    let chars: Vec<char> = bare.chars().collect();
    let mut tokens = Vec::new();
    let mut line = 1;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            tokens.push(Token {
                text: chars[start..i].iter().collect(),
                line,
            });
        } else if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
        } else if c == ':' && chars.get(i + 1) == Some(&':') {
            tokens.push(Token {
                text: "::".to_owned(),
                line,
            });
            i += 2;
        } else {
            tokens.push(Token {
                text: c.to_string(),
                line,
            });
            i += 1;
        }
    }
    tokens
}

fn is_ident(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
}

/// Whether the identifier at `i` continues a path (`a::b` at `b`), or
/// follows `.` (a method or field).
fn is_path_continuation(tokens: &[Token], i: usize) -> bool {
    match i.checked_sub(1).map(|p| tokens[p].text.as_str()) {
        Some(".") => true,
        // `a::b` continues a path; a leading `::crate_name` starts one.
        Some("::") => i >= 2 && (is_ident(&tokens[i - 2].text) || tokens[i - 2].text == ">"),
        _ => false,
    }
}

/// Reads `a::b::C` from the start of `tokens`; returns the segments and how
/// many tokens they took. A leading `::` is dropped.
fn read_path(tokens: &[Token]) -> (Vec<String>, usize) {
    let mut segments = Vec::new();
    let mut i = 0;
    if tokens.first().is_some_and(|t| t.text == "::") {
        i = 1;
    }
    while let Some(token) = tokens.get(i) {
        if !is_ident(&token.text) {
            break;
        }
        segments.push(token.text.clone());
        i += 1;
        if tokens.get(i).is_some_and(|t| t.text == "::")
            && tokens.get(i + 1).is_some_and(|t| is_ident(&t.text))
        {
            i += 1;
        } else {
            break;
        }
    }
    (segments, i)
}

/// The index of the first `text` token at or after `from` (or the end).
fn find(tokens: &[Token], from: usize, text: &str) -> usize {
    tokens[from..]
        .iter()
        .position(|t| t.text == text)
        .map_or(tokens.len(), |p| from + p)
}

/// The index after the block whose `{` is at `open`.
fn skip_block(tokens: &[Token], open: usize) -> usize {
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(open) {
        match token.text.as_str() {
            "{" => depth += 1,
            "}" => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    tokens.len()
}

/// Whether `#[cfg(test)]` starts at `i`.
fn is_cfg_test(tokens: &[Token], i: usize) -> bool {
    let expected = ["#", "[", "cfg", "(", "test", ")", "]"];
    expected
        .iter()
        .enumerate()
        .all(|(k, text)| tokens.get(i + k).is_some_and(|t| t.text == *text))
}

/// The names and globs a `use` tree (the tokens between `use` and `;`)
/// brings into scope: local name -> written path.
fn parse_use(tokens: &[Token]) -> (BTreeMap<String, Vec<String>>, Vec<Vec<String>>) {
    let mut names = BTreeMap::new();
    let mut globs = Vec::new();
    let tokens: Vec<&Token> = tokens.iter().collect();
    let mut i = 0;
    parse_tree(&tokens, &mut i, &[], &mut names, &mut globs);
    (names, globs)
}

fn parse_tree(
    tokens: &[&Token],
    i: &mut usize,
    prefix: &[String],
    names: &mut BTreeMap<String, Vec<String>>,
    globs: &mut Vec<Vec<String>>,
) {
    let mut path = prefix.to_vec();
    if tokens.get(*i).is_some_and(|t| t.text == "::") {
        *i += 1;
    }
    loop {
        let Some(token) = tokens.get(*i) else {
            return;
        };
        match token.text.as_str() {
            "{" => {
                *i += 1;
                loop {
                    parse_tree(tokens, i, &path, names, globs);
                    match tokens.get(*i).map(|t| t.text.as_str()) {
                        Some(",") => {
                            *i += 1;
                            if tokens.get(*i).is_some_and(|t| t.text == "}") {
                                *i += 1;
                                return;
                            }
                        }
                        Some("}") => {
                            *i += 1;
                            return;
                        }
                        _ => return,
                    }
                }
            }
            "*" => {
                *i += 1;
                globs.push(path);
                return;
            }
            text if is_ident(text) => {
                path.push(text.to_owned());
                *i += 1;
                if tokens.get(*i).is_some_and(|t| t.text == "::") {
                    *i += 1;
                    continue;
                }
                // The end of a path: `self` names the parent, `as` renames.
                let mut local = path.last().cloned().unwrap_or_default();
                if local == "self" {
                    path.pop();
                    local = path.last().cloned().unwrap_or_default();
                }
                if tokens.get(*i).is_some_and(|t| t.text == "as") {
                    if let Some(alias) = tokens.get(*i + 1) {
                        local = alias.text.clone();
                    }
                    *i += 2;
                }
                if local != "_" && !local.is_empty() {
                    names.insert(local, path);
                }
                return;
            }
            _ => return,
        }
    }
}

/// Crates under `root`: directories with a `Cargo.toml` that has a
/// `[package]` name, as `(directory, crate name with `_` for `-`)`.
fn find_crates(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    if let Ok(manifest) = std::fs::read_to_string(dir.join("Cargo.toml"))
        && let Some(name) = package_name(&manifest)
    {
        out.push((dir.to_path_buf(), name.replace('-', "_")));
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !skipped_dir(p) && !p.ends_with("src"))
        .collect();
    dirs.sort();
    for sub in dirs {
        find_crates(&sub, out);
    }
}

/// The `name` of the `[package]` table of a manifest.
fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package
            && let Some(rest) = line.strip_prefix("name")
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            let value = value.trim().trim_matches('"');
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

fn skipped_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name.starts_with('.') || SKIPPED_DIRS.contains(&name))
}

/// Every `.rs` file under `dir`, skipping test directories and nested
/// crates.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !skipped_dir(&path) && !path.join("Cargo.toml").is_file() {
                rust_files(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The module path of `file` inside the crate whose sources are `src`.
fn module_of(src: &Path, file: &Path) -> Vec<String> {
    let relative = file.strip_prefix(src).unwrap_or(file);
    let mut parts: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let Some(last) = parts.pop() else {
        return Vec::new();
    };
    let stem = last.strip_suffix(".rs").unwrap_or(&last).to_owned();
    let root_file = parts.is_empty() && (stem == "lib" || stem == "main");
    if stem != "mod" && !root_file {
        parts.push(stem);
    }
    parts
}

/// `path` relative to `root`, with `/` separators.
fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(path: &[&str]) -> Vec<String> {
        path.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn strip_blanks_comments_and_string_contents_keeping_lines() {
        let text = "let a = \"x // y\"; // navree:allow(workspace-repo)\n/* a\n b */ let c = 'q';\nlet r = r#\"raw \" here\"#; fn f<'a>(x: &'a str) {}\n";
        let stripped = strip(text);
        assert_eq!(stripped.code.lines().count(), text.lines().count());
        assert!(stripped.code.starts_with("let a = \"x // y\";"));
        assert!(!stripped.code.contains("navree:allow"));
        assert!(stripped.bare.starts_with("let a = \"      \";"));
        assert!(stripped.comments[0].contains(ALLOW_MARKER));
        assert!(stripped.comments[1].contains(" a"));
        // The block comment spans lines 2 and 3.
        let fourth = stripped.bare.lines().nth(3).unwrap();
        assert!(
            fourth.contains("r#\"") && !fourth.contains("raw"),
            "{fourth}"
        );
        assert!(fourth.contains("fn f<'a>(x: &'a str)"), "{fourth}");
        let third = stripped.bare.lines().nth(2).unwrap();
        assert!(third.ends_with("let c = ' ';"), "{third}");
    }

    #[test]
    fn module_paths_follow_file_locations() {
        let src = Path::new("app/src");
        assert!(module_of(src, Path::new("app/src/lib.rs")).is_empty());
        assert!(module_of(src, Path::new("app/src/main.rs")).is_empty());
        assert_eq!(
            module_of(src, Path::new("app/src/modules/deals/mod.rs")),
            strs(&["modules", "deals"])
        );
        assert_eq!(
            module_of(src, Path::new("app/src/modules/deals/entity.rs")),
            strs(&["modules", "deals", "entity"])
        );
    }

    #[test]
    fn use_trees_bring_names_and_globs_into_scope() {
        let tokens =
            tokenize("use super::{entity::{self, Entity as Deal}, dto::*, service::DealService};");
        let (names, globs) = parse_use(&tokens[1..tokens.len() - 1]);
        assert_eq!(names["entity"], strs(&["super", "entity"]));
        assert_eq!(names["Deal"], strs(&["super", "entity", "Entity"]));
        assert_eq!(
            names["DealService"],
            strs(&["super", "service", "DealService"])
        );
        assert_eq!(globs, vec![strs(&["super", "dto"])]);
    }

    #[test]
    fn package_names_come_from_the_package_table() {
        assert_eq!(
            package_name(
                "[workspace]\nmembers = []\n[package]\nname = \"crm-deals\"\nversion = \"0.1.0\"\n"
            ),
            Some("crm-deals".to_owned())
        );
        assert_eq!(package_name("[workspace]\nname = \"x\"\n"), None);
    }

    const DEALS_ENTITY: &str = r#"
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "deals")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
}
"#;

    const COMPANIES_ENTITY: &str = r#"
#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(
    table_name = "companies"
)]
pub struct Model {}
"#;

    fn project(service: &str) -> Project {
        Project::from_files(&[
            (
                "src/deals/entity.rs",
                "shop",
                &["deals", "entity"],
                DEALS_ENTITY,
            ),
            (
                "src/companies/entity.rs",
                "shop",
                &["companies", "entity"],
                COMPANIES_ENTITY,
            ),
            (
                "src/deals/service.rs",
                "shop",
                &["deals", "service"],
                service,
            ),
        ])
    }

    fn deals(project: &Project) -> Vec<EntityDef> {
        project
            .entities()
            .into_iter()
            .filter(|e| e.table == "deals")
            .collect()
    }

    #[test]
    fn entities_are_found_with_their_tables_and_paths() {
        let project = project("");
        let entities = project.entities();
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].table, "deals");
        assert_eq!(
            entities[0].path,
            strs(&["shop", "deals", "entity", "Entity"])
        );
        assert_eq!(entities[0].location, "src/deals/entity.rs:5");
        assert_eq!(entities[0].short_name(), "entity::Entity");
        assert_eq!(entities[1].table, "companies");
        assert_eq!(entities[1].location, "src/companies/entity.rs:4");
    }

    #[test]
    fn inline_entity_modules_are_part_of_the_path() {
        let project = Project::from_files(&[(
            "src/lib.rs",
            "shop",
            &[],
            "pub mod deal {\n    #[sea_orm(table_name = \"deals\")]\n    pub struct Model {}\n}\n#[cfg(test)]\nmod tests {\n    #[sea_orm(table_name = \"fixtures\")]\n    struct Model {}\n}\n",
        )]);
        let entities = project.entities();
        assert_eq!(entities.len(), 1, "{entities:?}");
        assert_eq!(entities[0].path, strs(&["shop", "deal", "Entity"]));
        assert_eq!(entities[0].short_name(), "deal::Entity");
    }

    #[test]
    fn direct_accesses_are_found_through_use_declarations() {
        let service = r#"
use navree::db::Repo;
use navree::workspace::WorkspaceRepo;
use super::entity::{self, Entity as Deal};
use crate::companies::entity as company;

pub async fn index(ws: Uuid) {
    let a = entity::Entity::find().all(conn).await;
    let b = Deal::find_by_id(id);
    let c = Repo::<entity::Entity>::insert(&uow, am).await;
    let d = WorkspaceRepo::<entity::Entity>::select(ws);
    let e = company::Entity::find();
    // navree:allow(workspace-repo)
    let f = Deal::delete_many();
    let g = Deal::find(); // navree:allow(workspace-repo)
    let h = "Deal::find()";
    let i = x.find(y);
}
"#;
        let project = project(service);
        let found = project.direct_accesses(&deals(&project));
        let summary: Vec<_> = found
            .iter()
            .map(|f| (f.location.as_str(), f.expression.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("src/deals/service.rs:8", "entity::Entity::find"),
                ("src/deals/service.rs:9", "Deal::find_by_id"),
                ("src/deals/service.rs:10", "Repo::<entity::Entity>"),
            ]
        );
        assert!(found.iter().all(|f| f.table == "deals"));
    }

    #[test]
    fn a_module_of_the_same_name_elsewhere_is_not_confused() {
        // `entity` here is the companies module's entity.
        let other = r#"
use super::entity;

fn list() { entity::Entity::find(); }
"#;
        let project = Project::from_files(&[
            (
                "src/deals/entity.rs",
                "shop",
                &["deals", "entity"],
                DEALS_ENTITY,
            ),
            (
                "src/companies/entity.rs",
                "shop",
                &["companies", "entity"],
                COMPANIES_ENTITY,
            ),
            (
                "src/companies/service.rs",
                "shop",
                &["companies", "service"],
                other,
            ),
        ]);
        assert!(project.direct_accesses(&deals(&project)).is_empty());
    }

    #[test]
    fn globs_aliases_and_absolute_paths_resolve() {
        let service = r#"
use crate::deals::entity::*;
type Deals = shop::deals::entity::Entity;

fn a() { Entity::find(); }
fn b() { Deals::update_many(); }
fn c() { crate::deals::entity::Entity::delete_by_id(1); }
fn d() { super::entity::Entity::insert(am); }
fn e() { ::shop::deals::entity::Entity::find(); }
"#;
        let project = project(service);
        let found = project.direct_accesses(&deals(&project));
        let lines: Vec<_> = found.iter().map(|f| f.location.as_str()).collect();
        assert_eq!(
            lines,
            [
                "src/deals/service.rs:5",
                "src/deals/service.rs:6",
                "src/deals/service.rs:7",
                "src/deals/service.rs:8",
                "src/deals/service.rs:9"
            ]
        );
    }

    #[test]
    fn test_modules_are_left_out() {
        let service = r#"
use super::entity;

#[cfg(test)]
mod tests {
    use super::entity;
    fn setup() { entity::Entity::find(); }
}

fn real() {}
"#;
        let project = project(service);
        assert!(project.direct_accesses(&deals(&project)).is_empty());
    }

    #[test]
    fn the_defining_module_reaches_its_own_entity() {
        let entity = format!(
            "{DEALS_ENTITY}\nimpl Entity {{\n    pub fn open() -> Select<Entity> {{ Entity::find() }}\n}}\n"
        );
        let project =
            Project::from_files(&[("src/deals/entity.rs", "shop", &["deals", "entity"], &entity)]);
        let found = project.direct_accesses(&deals(&project));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].expression, "Entity::find");
    }

    #[test]
    fn projects_load_every_crate_under_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"crm-api\"\n").unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        let deals = root.join("crates/crm-deals");
        std::fs::create_dir_all(deals.join("src")).unwrap();
        std::fs::write(
            deals.join("Cargo.toml"),
            "[package]\nname = \"crm-deals\"\n",
        )
        .unwrap();
        std::fs::write(deals.join("src/lib.rs"), "pub mod deal;\n").unwrap();
        std::fs::write(deals.join("src/deal.rs"), DEALS_ENTITY).unwrap();
        std::fs::create_dir_all(deals.join("tests")).unwrap();
        std::fs::write(deals.join("tests/it.rs"), COMPANIES_ENTITY).unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("target/debug/junk.rs"), COMPANIES_ENTITY).unwrap();

        let project = Project::load(root);
        assert_eq!(project.file_count(), 3);
        let entities = project.entities();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].path, strs(&["crm_deals", "deal", "Entity"]));
        assert_eq!(entities[0].location, "crates/crm-deals/src/deal.rs:5");
    }
}
