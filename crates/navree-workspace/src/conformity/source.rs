//! A light reading of an app's Rust source for the workspace conformity
//! checks: which navree-orm models it defines (`#[derive(Model)]` structs:
//! their tables, and whether `#[model(workspace)]` scopes them to the
//! current workspace), and where code reaches a model's table directly
//! (`Deal::query()`, `Deal::find(..)`, `Deal::create(..)`, ..) instead of
//! through `WorkspaceRepo`, or leaves a self-scoping model's workspace
//! scope (`.without_workspace_scope()`).
//!
//! It reads text, not a syntax tree: comments and string contents are
//! blanked, paths are resolved through `use` declarations (trees, `as`,
//! `self`, `super`, `crate` and globs), `type` aliases and the `Self` of
//! `impl` blocks, and modules come from file locations and inline
//! `mod name { .. }` blocks. Test code is left out: `tests/`, `benches/`
//! and `examples/` directories, `#[cfg(test)] mod` blocks and
//! `#[cfg(test)]` structs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The marker comment that allows one direct access, on its line or the
/// line above: `// navree:allow(workspace-repo)`.
pub(crate) const ALLOW_MARKER: &str = "navree:allow(workspace-repo)";

/// The associated functions of `navree_orm::Model` that read or write the
/// model's table. Without `#[model(workspace)]` none of them is scoped to
/// a workspace.
pub(crate) const MODEL_ENTRY_POINTS: [&str; 20] = [
    "query",
    "where_",
    "on_connection",
    "on_write_connection",
    "all",
    "find",
    "find_or_fail",
    "find_many",
    "create",
    "create_from",
    "force_create",
    "insert",
    "insert_or_ignore",
    "fill_and_insert",
    "upsert",
    "destroy",
    "first_or_create",
    "first_or_new",
    "create_or_first",
    "update_or_create",
];

/// Query builder methods that leave out the workspace scope of a
/// `#[model(workspace)]` model (every workspace's rows); so does
/// `without_global_scope::<WorkspaceScope>()`.
const SCOPE_OPT_OUTS: [&str; 2] = ["without_workspace_scope", "without_global_scopes"];

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

/// A navree-orm model the source defines: a struct that derives `Model`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModelDef {
    /// The struct's name (`Deal`): how code names it.
    pub(crate) name: String,
    /// Its table: `#[model(table = "..")]` (or `collection = ".."`), else
    /// the derive's default, the snake_case plural of the name.
    pub(crate) table: String,
    /// The workspace column of a model declared `#[model(workspace)]`
    /// (`workspace_id`) or `#[model(workspace = "..")]`: it scopes its own
    /// queries to the current workspace.
    pub(crate) workspace: Option<String>,
    /// The absolute path of the struct: crate, modules, name.
    pub(crate) path: Vec<String>,
    /// The file and line of the struct.
    pub(crate) location: String,
}

impl ModelDef {
    /// Whether `#[model(workspace)]` scopes its queries to the current
    /// workspace.
    pub(crate) fn scopes_itself(&self) -> bool {
        self.workspace.is_some()
    }
}

/// How code reaches a workspace model's table past its workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccessKind {
    /// A [`MODEL_ENTRY_POINTS`] call on a model that does not scope itself.
    Unscoped,
    /// A self-scoping model's query that leaves its workspace scope.
    LeavesScope,
}

/// A direct access to a model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectAccess {
    /// `src/deals/service.rs:42`.
    pub(crate) location: String,
    /// The model's name.
    pub(crate) model: String,
    /// The model's table.
    pub(crate) table: String,
    /// What the code wrote: `deal::Deal::find`,
    /// `Project::query().without_workspace_scope()`.
    pub(crate) expression: String,
    /// Which bypass it is.
    pub(crate) kind: AccessKind,
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

    /// Every model definition, in file order.
    pub(crate) fn models(&self) -> Vec<ModelDef> {
        self.files
            .iter()
            .flat_map(|file| file.models.iter().cloned())
            .collect()
    }

    /// Every direct access to one of `models` not allowed by the marker
    /// comment: any entry point of a model that does not scope itself, and
    /// a query that leaves the workspace scope of one that does.
    pub(crate) fn direct_accesses(&self, models: &[ModelDef]) -> Vec<DirectAccess> {
        let by_path: BTreeMap<&[String], &ModelDef> =
            models.iter().map(|m| (m.path.as_slice(), m)).collect();
        let mut found = Vec::new();
        for file in &self.files {
            for candidate in &file.paths {
                let resolved = file.resolve(&candidate.module, &candidate.segments, &self.crates);
                let Some(model) = resolved
                    .iter()
                    .find_map(|path| by_path.get(path.as_slice()))
                else {
                    continue;
                };
                let (line, expression, kind) = if model.scopes_itself() {
                    let Some(opt_out) = &candidate.opt_out else {
                        continue;
                    };
                    (
                        opt_out.line,
                        format!("{}{}", candidate.call, opt_out.call),
                        AccessKind::LeavesScope,
                    )
                } else {
                    (
                        candidate.line,
                        candidate.expression.clone(),
                        AccessKind::Unscoped,
                    )
                };
                if file.allowed(candidate.line) || file.allowed(line) {
                    continue;
                }
                found.push(DirectAccess {
                    location: format!("{}:{line}", file.display),
                    model: model.name.clone(),
                    table: model.table.clone(),
                    expression,
                    kind,
                });
            }
        }
        found
    }
}

/// A model entry point call: `path::entry_point(..)`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    line: usize,
    /// The inline module path (inside the file) where it appears.
    module: Vec<String>,
    /// The model path as written (`deal::Deal`, `Deal`), with `Self`
    /// replaced by the type of the enclosing `impl` block.
    segments: Vec<String>,
    /// The path as written, for the report: `deal::Deal::find`.
    expression: String,
    /// The call, for the report: `Deal::query()`, `Deal::find(..)`.
    call: String,
    /// Where the call's method chain leaves the workspace scope, if it
    /// does.
    opt_out: Option<OptOut>,
}

/// `.without_workspace_scope()` (or another of [`SCOPE_OPT_OUTS`]) in a
/// method chain.
#[derive(Clone, Debug, PartialEq, Eq)]
struct OptOut {
    line: usize,
    /// `.without_workspace_scope()`.
    call: String,
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
    models: Vec<ModelDef>,
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
            models: Vec::new(),
            paths: Vec::new(),
            test_lines: Vec::new(),
        };
        let line_modules = file.walk(&tokens);
        let code: Vec<char> = stripped.code.chars().collect();
        file.find_models(&tokens, &code, &line_modules);
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

    /// Walks the tokens: tracks inline modules and `impl` blocks, skips
    /// `#[cfg(test)]` modules, collects `use` declarations, `type` aliases
    /// and model entry point calls. Returns the inline module path at the
    /// start of each line.
    fn walk(&mut self, tokens: &[Token]) -> Vec<Vec<String>> {
        let mut line_modules: Vec<Vec<String>> = Vec::new();
        // (inline module name, brace depth inside it)
        let mut stack: Vec<(String, usize)> = Vec::new();
        // (the `Self` type of an `impl` block, brace depth inside it)
        let mut selves: Vec<(Vec<String>, usize)> = Vec::new();
        // (the index of an `impl` block's `{`, its `Self` type)
        let mut pending_impl: Option<(usize, Vec<String>)> = None;
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
                        let end = skip_group(tokens, i + 2);
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
                "impl" if starts_item(tokens, i) => {
                    pending_impl = impl_header(tokens, i);
                    i += 1;
                    continue;
                }
                "{" => {
                    depth += 1;
                    cfg_test = false;
                    if let Some((open, self_type)) = pending_impl.take() {
                        if open == i {
                            selves.push((self_type, depth));
                        } else {
                            pending_impl = Some((open, self_type));
                        }
                    }
                }
                "}" => {
                    if stack.last().is_some_and(|(_, d)| *d == depth) {
                        stack.pop();
                    }
                    if selves.last().is_some_and(|(_, d)| *d == depth) {
                        selves.pop();
                    }
                    depth = depth.saturating_sub(1);
                }
                ";" => cfg_test = false,
                text if is_ident(text) && !is_path_continuation(tokens, i) => {
                    let (path, used) = read_path(&tokens[i..]);
                    let after = i + used;
                    // `path::entry_point(`: the entry point is the last
                    // segment, the model the ones before it.
                    if path.len() >= 2
                        && MODEL_ENTRY_POINTS.contains(&path[path.len() - 1].as_str())
                        && tokens.get(after).is_some_and(|t| t.text == "(")
                    {
                        let mut model = path[..path.len() - 1].to_vec();
                        if model[0] == "Self"
                            && let Some((self_type, _)) = selves.last()
                        {
                            model.splice(0..1, self_type.iter().cloned());
                        }
                        let no_arguments = tokens.get(after + 1).is_some_and(|t| t.text == ")");
                        let expression = path.join("::");
                        self.paths.push(Candidate {
                            line: token.line,
                            module: current(&stack),
                            segments: model,
                            call: format!("{expression}({})", if no_arguments { "" } else { ".." }),
                            expression,
                            opt_out: chain_opt_out(tokens, after),
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

    /// Finds the structs that derive `Model`, with their `#[model(..)]`
    /// container attributes.
    fn find_models(&mut self, tokens: &[Token], code: &[char], line_modules: &[Vec<String>]) {
        let text_at = |k: usize| tokens.get(k).map(|t| t.text.as_str());
        let mut i = 0;
        while i < tokens.len() {
            if !(text_at(i) == Some("#") && text_at(i + 1) == Some("[")) {
                i += 1;
                continue;
            }
            // A run of outer attributes, then the item.
            let mut attributes: Vec<&[Token]> = Vec::new();
            let mut j = i;
            while text_at(j) == Some("#") && text_at(j + 1) == Some("[") {
                let end = skip_group(tokens, j + 1);
                attributes.push(&tokens[(j + 2).min(end)..end.saturating_sub(1).max(j + 2)]);
                j = end;
            }
            if text_at(j) == Some("pub") {
                j += 1;
                if text_at(j) == Some("(") {
                    j = skip_group(tokens, j);
                }
            }
            if text_at(j) == Some("struct")
                && let Some(name) = tokens.get(j + 1).filter(|t| is_ident(&t.text))
                && let Some((table, workspace)) = model_attributes(&attributes, code)
            {
                let in_tests = self
                    .test_lines
                    .iter()
                    .any(|&(start, end)| (start..=end).contains(&name.line));
                if !in_tests {
                    let inline = line_modules
                        .get(name.line - 1)
                        .or_else(|| line_modules.last())
                        .cloned()
                        .unwrap_or_default();
                    let mut path = self.absolute_module(&inline);
                    path.push(name.text.clone());
                    self.models.push(ModelDef {
                        table: table.unwrap_or_else(|| default_table(&name.text)),
                        name: name.text.clone(),
                        workspace,
                        path,
                        location: format!("{}:{}", self.display, name.line),
                    });
                }
            }
            i = j.max(i + 1);
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
            name if crates.contains(name) => out.push(segments.to_vec()),
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

/// `(table, workspace column)` of a struct whose outer attributes are
/// `attributes` (each the tokens inside `#[..]`), when it derives `Model`
/// and is not `#[cfg(test)]`: the table when `#[model(table = "..")]` (or
/// `collection`) names it, the column when `#[model(workspace)]` or
/// `#[model(workspace = "..")]` scopes it.
fn model_attributes(
    attributes: &[&[Token]],
    code: &[char],
) -> Option<(Option<String>, Option<String>)> {
    let named = |attribute: &[Token], name: &str| {
        attribute.first().is_some_and(|t| t.text == name)
            && attribute.get(1).is_some_and(|t| t.text == "(")
    };
    let derives_model = attributes.iter().any(|attribute| {
        named(attribute, "derive")
            && attribute
                .windows(2)
                .any(|pair| pair[0].text == "Model" && matches!(pair[1].text.as_str(), "," | ")"))
    });
    let cfg_test = attributes.iter().any(|attribute| {
        attribute.len() == 4
            && named(attribute, "cfg")
            && attribute[2].text == "test"
            && attribute[3].text == ")"
    });
    if !derives_model || cfg_test {
        return None;
    }
    let mut table = None;
    let mut workspace = None;
    for attribute in attributes.iter().filter(|a| named(a, "model")) {
        let inner = &attribute[2..attribute.len().saturating_sub(1).max(2)];
        for item in split_top_level(inner) {
            let Some(key) = item.first() else {
                continue;
            };
            let value = item
                .get(1)
                .filter(|t| t.text == "=")
                .and_then(|_| string_literal(&item[2..], code));
            match key.text.as_str() {
                "table" | "collection" => table = value.or(table),
                "workspace" => workspace = Some(value.unwrap_or_else(|| "workspace_id".to_owned())),
                _ => {}
            }
        }
    }
    Some((table, workspace))
}

/// The items of a comma-separated list, split at the top level.
fn split_top_level(tokens: &[Token]) -> Vec<&[Token]> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (k, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            "," if depth == 0 => {
                items.push(&tokens[start..k]);
                start = k + 1;
            }
            _ => {}
        }
    }
    items.push(&tokens[start..]);
    items
}

/// The contents of the string literal the tokens start with (`"deals"`,
/// `r#"deals"#`), read from the comment-free source `code` since the
/// tokens come from source with string contents blanked.
fn string_literal(tokens: &[Token], code: &[char]) -> Option<String> {
    let mut k = 0;
    if tokens.first().is_some_and(|t| t.text == "r") {
        k += 1;
        while tokens.get(k).is_some_and(|t| t.text == "#") {
            k += 1;
        }
    }
    let open = tokens.get(k).filter(|t| t.text == "\"")?;
    let close = tokens.get(k + 1).filter(|t| t.text == "\"")?;
    code.get(open.pos + 1..close.pos)
        .map(|chars| chars.iter().collect())
}

/// Words the same in the plural (navree-orm-macros' default table names).
const UNCOUNTABLE: &[&str] = &[
    "audio",
    "data",
    "deer",
    "equipment",
    "feedback",
    "fish",
    "information",
    "metadata",
    "money",
    "news",
    "rice",
    "series",
    "sheep",
    "species",
    "staff",
    "traffic",
];

/// Plurals that follow no rule (navree-orm-macros' default table names).
const IRREGULAR: &[(&str, &str)] = &[
    ("analysis", "analyses"),
    ("child", "children"),
    ("criterion", "criteria"),
    ("foot", "feet"),
    ("goose", "geese"),
    ("half", "halves"),
    ("index", "indices"),
    ("knife", "knives"),
    ("leaf", "leaves"),
    ("life", "lives"),
    ("man", "men"),
    ("mouse", "mice"),
    ("ox", "oxen"),
    ("person", "people"),
    ("quiz", "quizzes"),
    ("tooth", "teeth"),
    ("wife", "wives"),
    ("woman", "women"),
];

/// The table `#[derive(Model)]` gives a struct named `name` without
/// `#[model(table = "..")]`: the snake_case plural of the name (`Deal` →
/// `deals`, `DealStage` → `deal_stages`, `Person` → `people`), as
/// navree-orm-macros names it.
fn default_table(name: &str) -> String {
    let snake = heck::ToSnakeCase::to_snake_case(name);
    match snake.rsplit_once('_') {
        Some((head, last)) => format!("{head}_{}", plural(last)),
        None => plural(&snake),
    }
}

/// The plural of an English noun (lower case).
fn plural(word: &str) -> String {
    if UNCOUNTABLE.contains(&word) {
        return word.to_owned();
    }
    if let Some((_, plural)) = IRREGULAR.iter().find(|(singular, _)| *singular == word) {
        return (*plural).to_owned();
    }
    let consonant_before = |suffix_len: usize| {
        word.len() > suffix_len
            && word[..word.len() - suffix_len]
                .chars()
                .last()
                .is_some_and(|c| !"aeiou".contains(c))
    };
    if word.ends_with('y') && consonant_before(1) {
        return format!("{}ies", &word[..word.len() - 1]);
    }
    if word.ends_with("is") && word.len() > 3 {
        return format!("{}es", &word[..word.len() - 2]);
    }
    if ["s", "x", "z", "ch", "sh"]
        .iter()
        .any(|suffix| word.ends_with(suffix))
    {
        return format!("{word}es");
    }
    format!("{word}s")
}

/// Whether the token at `i` starts an item (an `impl` block rather than
/// `impl Trait` in a type): it follows the start of the file, `;`, `{`,
/// `}`, an attribute's `]` or `unsafe`.
fn starts_item(tokens: &[Token], i: usize) -> bool {
    i.checked_sub(1)
        .map(|p| tokens[p].text.as_str())
        .is_none_or(|prev| matches!(prev, ";" | "{" | "}" | "]" | "unsafe"))
}

/// The index of the `{` of the `impl` block starting at `i`, and its
/// `Self` type as written (`Deal` in `impl Deal` and in `impl Trait for
/// Deal`).
fn impl_header(tokens: &[Token], i: usize) -> Option<(usize, Vec<String>)> {
    let mut k = i + 1;
    if tokens.get(k).is_some_and(|t| t.text == "<") {
        k = skip_angles(tokens, k);
    }
    let (mut path, used) = read_path(tokens.get(k..)?);
    k += used;
    if tokens.get(k).is_some_and(|t| t.text == "<") {
        k = skip_angles(tokens, k);
    }
    if tokens.get(k).is_some_and(|t| t.text == "for") {
        let (for_path, used) = read_path(tokens.get(k + 1..)?);
        path = for_path;
        k += 1 + used;
    }
    if path.is_empty() {
        return None;
    }
    let open = find(tokens, k, "{");
    (open < tokens.len()).then_some((open, path))
}

/// The index after the `<..>` whose `<` is at `open` (a `>` of `->` does
/// not close it).
fn skip_angles(tokens: &[Token], open: usize) -> usize {
    let mut depth = 0usize;
    for (k, token) in tokens.iter().enumerate().skip(open) {
        match token.text.as_str() {
            "<" => depth += 1,
            ">" if k == 0 || tokens[k - 1].text != "-" => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return k + 1;
                }
            }
            "{" | ";" => return k,
            _ => {}
        }
    }
    tokens.len()
}

/// Follows the method chain after the call whose `(` is at `open`
/// (`Deal::query()` `.where_(..)` `.without_workspace_scope()` `.get(..)`
/// `.await?`) and returns where it leaves the workspace scope.
fn chain_opt_out(tokens: &[Token], open: usize) -> Option<OptOut> {
    let text_at = |k: usize| tokens.get(k).map(|t| t.text.as_str());
    let mut k = skip_group(tokens, open);
    loop {
        match text_at(k) {
            Some("?") => k += 1,
            Some(".") => {
                let method = tokens.get(k + 1).filter(|t| is_ident(&t.text))?;
                k += 2;
                let mut generic = None;
                if text_at(k) == Some("::") && text_at(k + 1) == Some("<") {
                    let close = skip_angles(tokens, k + 1);
                    generic = read_path(&tokens[k + 2..close]).0.last().cloned();
                    k = close;
                }
                let leaves = SCOPE_OPT_OUTS.contains(&method.text.as_str())
                    || (method.text == "without_global_scope"
                        && generic.as_deref() == Some("WorkspaceScope"));
                if leaves {
                    let turbofish = generic.map_or_else(String::new, |g| format!("::<{g}>"));
                    return Some(OptOut {
                        line: method.line,
                        call: format!(".{}{turbofish}()", method.text),
                    });
                }
                if text_at(k) == Some("(") {
                    k = skip_group(tokens, k);
                }
            }
            _ => return None,
        }
    }
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
    /// The index of its first character in the source.
    pos: usize,
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
                pos: start,
            });
        } else if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
        } else if c == ':' && chars.get(i + 1) == Some(&':') {
            tokens.push(Token {
                text: "::".to_owned(),
                line,
                pos: i,
            });
            i += 2;
        } else {
            tokens.push(Token {
                text: c.to_string(),
                line,
                pos: i,
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

/// The index after the group whose `(`, `[` or `{` is at `open`.
fn skip_group(tokens: &[Token], open: usize) -> usize {
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(open) {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => {
                depth = depth.saturating_sub(1);
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
            tokenize("use super::{deal::{self, Deal as D}, dto::*, service::DealService};");
        let (names, globs) = parse_use(&tokens[1..tokens.len() - 1]);
        assert_eq!(names["deal"], strs(&["super", "deal"]));
        assert_eq!(names["D"], strs(&["super", "deal", "Deal"]));
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

    const DEAL_MODEL: &str = r#"
use navree::orm::Model;
use uuid::Uuid;

/// A deal.
#[derive(Clone, Debug, Default, Model, serde::Serialize)]
#[model(table = "deals", timestamps)]
pub struct Deal {
    pub id: Uuid,
    #[model(cast = "json", table = "not_this")]
    pub meta: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct DealResource {}

#[derive(DeriveEntityModel, ModelFactory)]
pub struct Model {}
"#;

    const COMPANY_MODEL: &str = r#"
#[derive(navree_orm::Model, Clone)]
pub struct Company {
    pub id: Uuid,
}
"#;

    const PROJECT_MODEL: &str = r#"
#[model(workspace = "team_id", per_page = 25)]
#[derive(Clone, Debug, Default, navree::orm::Model)]
pub(crate) struct Project {}
"#;

    fn project(service: &str) -> Project {
        Project::from_files(&[
            ("src/deals/deal.rs", "shop", &["deals", "deal"], DEAL_MODEL),
            (
                "src/companies/company.rs",
                "shop",
                &["companies", "company"],
                COMPANY_MODEL,
            ),
            (
                "src/projects/project.rs",
                "shop",
                &["projects", "project"],
                PROJECT_MODEL,
            ),
            (
                "src/deals/service.rs",
                "shop",
                &["deals", "service"],
                service,
            ),
        ])
    }

    fn deals(project: &Project) -> Vec<ModelDef> {
        project
            .models()
            .into_iter()
            .filter(|m| m.table == "deals")
            .collect()
    }

    fn summary(found: &[DirectAccess]) -> Vec<(&str, &str)> {
        found
            .iter()
            .map(|f| (f.location.as_str(), f.expression.as_str()))
            .collect()
    }

    #[test]
    fn models_are_found_with_their_tables_and_paths() {
        let project = project("");
        let models = project.models();
        assert_eq!(models.len(), 3, "{models:?}");
        assert_eq!(models[0].name, "Deal");
        assert_eq!(models[0].table, "deals");
        assert_eq!(models[0].path, strs(&["shop", "deals", "deal", "Deal"]));
        assert_eq!(models[0].location, "src/deals/deal.rs:8");
        assert_eq!(models[0].workspace, None);
        assert!(!models[0].scopes_itself());
        assert_eq!(models[1].name, "Company");
        assert_eq!(models[1].table, "companies");
        assert_eq!(
            models[1].path,
            strs(&["shop", "companies", "company", "Company"])
        );
        assert_eq!(models[1].location, "src/companies/company.rs:3");
        assert_eq!(models[2].name, "Project");
        assert_eq!(models[2].table, "projects");
        assert_eq!(models[2].workspace.as_deref(), Some("team_id"));
        assert!(models[2].scopes_itself());
        assert_eq!(models[2].location, "src/projects/project.rs:4");
    }

    #[test]
    fn default_tables_follow_the_derive() {
        for (model, table) in [
            ("Deal", "deals"),
            ("DealStage", "deal_stages"),
            ("Company", "companies"),
            ("Activity", "activities"),
            ("Person", "people"),
            ("Status", "statuses"),
            ("Box", "boxes"),
            ("Branch", "branches"),
            ("Day", "days"),
            ("AuditLog", "audit_logs"),
            ("Metadata", "metadata"),
            ("Analysis", "analyses"),
            ("ApiKey", "api_keys"),
            ("APIToken", "api_tokens"),
        ] {
            assert_eq!(default_table(model), table, "{model}");
        }
        let project = Project::from_files(&[(
            "src/lib.rs",
            "shop",
            &[],
            "#[derive(Model)]\n#[model(workspace)]\npub struct DealNote {}\n#[derive(Model)]\n#[model(collection = \"events\", driver = \"mongodb\")]\npub struct Event {}\n#[derive(Model)]\n#[model(table = r#\"raw_items\"#)]\npub struct Item {}\n",
        )]);
        let models = project.models();
        let tables: Vec<_> = models.iter().map(|m| m.table.as_str()).collect();
        assert_eq!(tables, ["deal_notes", "events", "raw_items"]);
        assert_eq!(models[0].workspace.as_deref(), Some("workspace_id"));
        assert_eq!(models[1].workspace, None);
    }

    #[test]
    fn inline_model_modules_are_part_of_the_path() {
        let project = Project::from_files(&[(
            "src/lib.rs",
            "shop",
            &[],
            "pub mod deal {\n    #[derive(Model)]\n    #[model(table = \"deals\")]\n    pub struct Deal {}\n}\n#[cfg(test)]\nmod tests {\n    #[derive(Model)]\n    struct Fixture {}\n}\n#[cfg(test)]\n#[derive(Model)]\nstruct TestOnly {}\n",
        )]);
        let models = project.models();
        assert_eq!(models.len(), 1, "{models:?}");
        assert_eq!(models[0].path, strs(&["shop", "deal", "Deal"]));
        assert_eq!(models[0].location, "src/lib.rs:4");
    }

    #[test]
    fn direct_accesses_are_found_through_use_declarations() {
        let service = r#"
use navree::orm::prelude::*;
use navree::workspace::WorkspaceRepo;
use super::deal::{self, Deal as D};
use crate::companies::company as co;

pub async fn index(uow: Uow, ws: Uuid) {
    let a = deal::Deal::query().get(&uow).await;
    let b = D::find(&uow, id).await;
    let c = D::create(&uow, deal).await;
    let d = WorkspaceRepo::<D>::query(ws).get(&uow).await;
    let e = co::Company::all(&uow).await;
    // navree:allow(workspace-repo)
    let f = D::destroy(&uow, [id]).await;
    let g = D::all(&uow).await; // navree:allow(workspace-repo)
    let h = "D::find(&uow, id)";
    let i = x.find(y);
    let j = D::table_name();
    let k = D::where_(D::TITLE.eq("x")).first(&uow).await;
}
"#;
        let project = project(service);
        let found = project.direct_accesses(&deals(&project));
        assert_eq!(
            summary(&found),
            [
                ("src/deals/service.rs:8", "deal::Deal::query"),
                ("src/deals/service.rs:9", "D::find"),
                ("src/deals/service.rs:10", "D::create"),
                ("src/deals/service.rs:19", "D::where_"),
            ]
        );
        assert!(
            found
                .iter()
                .all(|f| f.table == "deals" && f.model == "Deal" && f.kind == AccessKind::Unscoped)
        );
    }

    #[test]
    fn self_scoping_models_are_reported_only_when_they_leave_the_scope() {
        let service = r#"
use crate::projects::project::Project;

pub async fn report(uow: Uow, other: Uuid) -> Result<()> {
    let mine = Project::query().get(&uow).await?;
    let theirs = Project::query().for_workspace(other).get(&uow).await?;
    let every = Project::query()
        .where_(Project::NAME.eq("x"))
        .without_workspace_scope()
        .count(&uow)
        .await?;
    let all = Project::all(&uow).await?;
    let raw = Project::on_write_connection().without_global_scope::<WorkspaceScope>().get(&uow).await?;
    let soft = Project::query().without_global_scope::<SoftDeleteScope>().get(&uow).await?;
    // A report across workspaces. navree:allow(workspace-repo)
    let total = Project::query()
        .without_global_scopes()
        .count(&uow)
        .await?;
    let later = Project::find(&uow, id).await?.map(|p| p.id);
    Ok(())
}
"#;
        let project = project(service);
        let projects: Vec<_> = project
            .models()
            .into_iter()
            .filter(ModelDef::scopes_itself)
            .collect();
        let found = project.direct_accesses(&projects);
        assert_eq!(
            summary(&found),
            [
                (
                    "src/deals/service.rs:9",
                    "Project::query().without_workspace_scope()"
                ),
                (
                    "src/deals/service.rs:13",
                    "Project::on_write_connection().without_global_scope::<WorkspaceScope>()"
                ),
            ]
        );
        assert!(found.iter().all(|f| f.kind == AccessKind::LeavesScope));
    }

    #[test]
    fn self_in_impl_blocks_is_the_implementing_type() {
        let model = format!(
            "{DEAL_MODEL}\nimpl Deal {{\n    pub fn open() -> Builder<Self> {{\n        Self::query()\n    }}\n    pub fn later() -> impl Future<Output = ()> {{\n        async {{ Self::all(&db); }}\n    }}\n}}\n#[async_trait]\nimpl Summary for Deal {{\n    fn count() {{ Self::find_many(&db, ids); }}\n}}\nimpl Other {{\n    fn f() {{ Self::query(); }}\n}}\n"
        );
        let project =
            Project::from_files(&[("src/deals/deal.rs", "shop", &["deals", "deal"], &model)]);
        let found = project.direct_accesses(&deals(&project));
        let expressions: Vec<_> = found.iter().map(|f| f.expression.as_str()).collect();
        assert_eq!(
            expressions,
            ["Self::query", "Self::all", "Self::find_many"],
            "{found:?}"
        );
    }

    #[test]
    fn a_module_of_the_same_name_elsewhere_is_not_confused() {
        // `deal` here would be a module of companies.
        let other = r#"
use super::deal;

fn list() { deal::Deal::query(); }
"#;
        let project = Project::from_files(&[
            ("src/deals/deal.rs", "shop", &["deals", "deal"], DEAL_MODEL),
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
use crate::deals::deal::*;
type Deals = shop::deals::deal::Deal;

fn a() { Deal::find_many(&db, ids); }
fn b() { Deals::upsert(&db, rows, ["id"]); }
fn c() { crate::deals::deal::Deal::first_or_create(&db, attrs); }
fn d() { super::deal::Deal::insert(&db, rows); }
fn e() { ::shop::deals::deal::Deal::update_or_create(&db, a, b); }
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
use super::deal::Deal;

#[cfg(test)]
mod tests {
    use super::Deal;
    fn setup() { Deal::query(); }
}

fn real() {}
"#;
        let project = project(service);
        assert!(project.direct_accesses(&deals(&project)).is_empty());
    }

    #[test]
    fn projects_load_every_crate_under_the_root() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
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
        std::fs::write(deals.join("src/deal.rs"), DEAL_MODEL).unwrap();
        std::fs::create_dir_all(deals.join("tests")).unwrap();
        std::fs::write(deals.join("tests/it.rs"), COMPANY_MODEL).unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("target/debug/junk.rs"), COMPANY_MODEL).unwrap();

        let project = Project::load(root);
        assert_eq!(project.file_count(), 3);
        let models = project.models();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].path, strs(&["crm_deals", "deal", "Deal"]));
        assert_eq!(models[0].location, "crates/crm-deals/src/deal.rs:8");
    }
}
