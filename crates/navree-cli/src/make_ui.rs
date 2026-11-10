//! `navree make:page`, `make:form` and `make:table`: Navree UI pages, forms and
//! data tables in a module.
//!
//! Each writes one file in the module (`pages/<name>.rs`, `forms/<name>.rs`,
//! `tables/<name>.rs`) and declares it. A page is also registered with the
//! module, like a controller, and comes with a test that boots the app and
//! gets the page. With `dry_run` nothing is written; with `force` the files
//! that exist are replaced.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Serialize;

use crate::edit::{Edit, declare_module, insert_above_marker};
use crate::make::{MakeKind, MakeReport, WIRE_INDENT, generic_call};
use crate::names::Name;
use crate::scaffold::{FileSet, join};
use crate::templates::{TemplateError, render};

/// Options of `make:page`, `make:form` and `make:table`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UiOptions {
    /// `make:page --public`: anyone may open the page, not only signed-in
    /// users.
    pub public: bool,
    /// `--dry-run`: say what would be written, and write nothing.
    pub dry_run: bool,
    /// `--force`: replace the files that exist.
    pub force: bool,
}

/// What depends on the files already in the module, which decides what is
/// generated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiLayout {
    /// The kind's directory (`pages/`) has no `mod.rs` yet: write one.
    pub new_dir: bool,
    /// A page's `tests/` has no `mod.rs` yet: write one.
    pub new_tests: bool,
    /// A table's model, as a path from the table's file
    /// (`super::super::entity::Deal`), when the module has one with the
    /// columns the table reads (`id`, `name`, `created_at`).
    pub model: Option<String>,
}

impl UiLayout {
    /// A module that has none of the kind's files yet: a new directory with
    /// its `mod.rs`, and no model for a table.
    pub fn fresh() -> Self {
        Self {
            new_dir: true,
            new_tests: false,
            model: None,
        }
    }
}

/// What the UI templates are rendered with.
#[derive(Debug, Clone, Serialize)]
struct UiContext {
    /// `sales`: the module's directory.
    module: String,
    /// `sales`: the module, in words.
    module_words: String,
    /// `SalesModule`.
    module_struct: String,
    /// `sales_report`: the name as given.
    name: String,
    /// `SalesReport`: the type, singular for a form and a table.
    pascal: String,
    /// `SalesReportPage`, `SalesReportForm` or `SalesReportRow`.
    type_name: String,
    /// `sales report`.
    words: String,
    /// `Sales report`.
    title: String,
    /// `sales-report`: the route of a page and a form; a table's is plural
    /// (`sales-reports`).
    route: String,
    /// A page anyone may open.
    public: bool,
    /// `SalesReport`: the table's model.
    entity_pascal: String,
    /// `sales_reports`: the table.
    table: String,
    /// `sales reports`.
    plural_words: String,
    /// `Sales reports`.
    plural_title: String,
    /// The table's model, when the module has one.
    model_path: Option<String>,
}

/// `Page` after `Dashboard`, unless the name already ends with it.
fn typed(pascal: String, suffix: &str) -> String {
    if pascal.len() > suffix.len() && pascal.ends_with(suffix) {
        pascal
    } else {
        format!("{pascal}{suffix}")
    }
}

impl UiContext {
    fn new(
        kind: MakeKind,
        module: &Name,
        name: &Name,
        options: &UiOptions,
        layout: &UiLayout,
    ) -> Self {
        // A page is named for its route; a form and a table are of one record.
        let base = match kind {
            MakeKind::Page => name.clone(),
            _ => name.singular(),
        };
        let plural = name.plural();
        let pascal = base.pascal();
        let type_name = match kind {
            MakeKind::Page => typed(pascal.clone(), "Page"),
            MakeKind::Form => typed(pascal.clone(), "Form"),
            _ => typed(pascal.clone(), "Row"),
        };
        let route = match kind {
            MakeKind::Table => plural.kebab(),
            _ => name.kebab(),
        };
        Self {
            module: module.snake().to_owned(),
            module_words: module.snake().replace('_', " "),
            module_struct: format!("{}Module", module.pascal()),
            name: name.snake().to_owned(),
            pascal,
            type_name,
            words: base.snake().replace('_', " "),
            title: base.title(),
            route,
            public: options.public,
            entity_pascal: name.singular().pascal(),
            table: plural.snake().to_owned(),
            plural_words: plural.snake().replace('_', " "),
            plural_title: plural.title(),
            model_path: layout.model.clone(),
        }
    }
}

/// The files of a UI item `name` of module `module`, relative to the module's
/// directory: `pages/<name>.rs` (and its test, `tests/<name>_page.rs`),
/// `forms/<name>.rs` or `tables/<name>.rs`, each with the `mod.rs` of its
/// directory when `layout` says the directory is new.
///
/// # Panics
///
/// When `kind` is not one of [`MakeKind::UI`].
pub fn ui_files(
    kind: MakeKind,
    module: &Name,
    name: &Name,
    options: &UiOptions,
    layout: &UiLayout,
) -> Result<FileSet, TemplateError> {
    assert!(MakeKind::UI.contains(&kind), "{kind:?} is not a UI kind");
    let ctx = UiContext::new(kind, module, name, options, layout);
    let dir = kind.dir();
    let stem = name.snake();
    let mut files = FileSet::new();
    files.add(
        format!("{dir}/{stem}.rs"),
        render(&format!("make/{}.rs", kind.noun()), &ctx)?,
    );
    if layout.new_dir {
        files.add(
            format!("{dir}/mod.rs"),
            format!("{}\npub mod {stem};\n", kind.dir_doc(&ctx.module_words)),
        );
    }
    if kind == MakeKind::Page {
        files.add(
            format!("tests/{stem}_page.rs"),
            render("make/page_test.rs", &ctx)?,
        );
        if layout.new_tests {
            files.add(
                "tests/mod.rs",
                format!(
                    "{}\nmod {stem}_page;\n",
                    MakeKind::Test.dir_doc(&ctx.module_words)
                ),
            );
        }
    }
    Ok(files)
}

/// What a UI generator did, or would do.
#[derive(Debug, Default)]
pub struct UiReport {
    /// The files created, the files edited and what is left to do by hand.
    pub made: MakeReport,
    /// Files that existed and were replaced (`--force`).
    pub overwritten: Vec<PathBuf>,
    /// Nothing was written (`--dry-run`): the lists above are what would be.
    pub dry_run: bool,
    /// The new and replaced files with their contents, under their path from
    /// the app's root.
    pub files: FileSet,
}

/// The edits of existing files, applied in memory: two edits of one file build
/// on each other, and a dry run can say what they would change.
#[derive(Default)]
struct Edits {
    /// The new contents of each file edited.
    files: BTreeMap<PathBuf, String>,
    /// The files, in the order they were first edited.
    order: Vec<PathBuf>,
    /// Edits that could not be applied: steps left to do by hand.
    hints: Vec<String>,
}

impl Edits {
    /// Applies `f` to the file at `path` (as the edits so far left it); adds
    /// `hint` when the file is missing or `f` cannot apply.
    fn apply(
        &mut self,
        path: &Path,
        f: impl FnOnce(&str) -> Option<Edit>,
        hint: impl FnOnce() -> String,
    ) -> anyhow::Result<()> {
        let src = match self.files.get(path) {
            Some(src) => src.clone(),
            None => match std::fs::read_to_string(path) {
                Ok(src) => src,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.hints.push(hint());
                    return Ok(());
                }
                Err(error) => {
                    return Err(error).with_context(|| format!("cannot read {}", path.display()));
                }
            },
        };
        match f(&src) {
            Some(Edit::Changed(contents)) => {
                if !self.files.contains_key(path) {
                    self.order.push(path.to_path_buf());
                }
                self.files.insert(path.to_path_buf(), contents);
            }
            Some(Edit::Unchanged) => {}
            None => self.hints.push(hint()),
        }
        Ok(())
    }

    /// Declares `decl` (`pub mod pages;`) in the file at `path`.
    fn declare(&mut self, path: &Path, decl: &str) -> anyhow::Result<()> {
        self.apply(
            path,
            |src| Some(declare_module(src, decl)),
            || format!("declare `{decl}` in {}", path.display()),
        )
    }

    /// Writes the edited files.
    fn write(&self) -> anyhow::Result<()> {
        for (path, contents) in &self.files {
            std::fs::write(path, contents)
                .with_context(|| format!("cannot write {}", path.display()))?;
        }
        Ok(())
    }
}

/// Whether `src` declares `pub struct <name>`.
fn declares_struct(src: &str, name: &str) -> bool {
    src.lines().any(|line| {
        let Some(rest) = line.trim_start().strip_prefix("pub struct ") else {
            return false;
        };
        rest.strip_prefix(name)
            .is_some_and(|after| !after.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
    })
}

/// Whether `src` has a field starting `field` (`name: String`).
fn has_field(src: &str, field: &str) -> bool {
    src.lines().any(|line| {
        let line = line.trim();
        line.strip_prefix("pub ").unwrap_or(line).starts_with(field)
    })
}

/// Whether `src` is the source of model `pascal` with the three columns a new
/// table shows: an `id`, a `name` and `created_at`, as `make:model` writes them.
fn is_standard_model(src: &str, pascal: &str) -> bool {
    declares_struct(src, pascal)
        && ["id: Uuid", "name: String", "created_at: DateTime<Utc>"]
            .iter()
            .all(|field| has_field(src, field))
}

/// The path, from `tables/<name>.rs`, of the module's model `entity`, if it
/// has one a table can read: `models/<entity>.rs` (`make:model`), the module's
/// `entity.rs` (`make:module`) or `entities/<entity>.rs` (`make:entity`).
fn find_model(module_dir: &Path, entity: &Name) -> Option<String> {
    let (snake, pascal) = (entity.snake(), entity.pascal());
    [
        (
            format!("models/{snake}.rs"),
            format!("super::super::models::{snake}::{pascal}"),
        ),
        (
            "entity.rs".to_owned(),
            format!("super::super::entity::{pascal}"),
        ),
        (
            format!("entities/{snake}.rs"),
            format!("super::super::entities::{snake}::{pascal}"),
        ),
    ]
    .into_iter()
    .find_map(|(file, path)| {
        let src = std::fs::read_to_string(join(module_dir, &file)).ok()?;
        is_standard_model(&src, &pascal).then_some(path)
    })
}

/// Whether the app has the Navree UI kit's `Desk` extractor in `src/ui/shell.rs`.
fn uses_desk(root: &Path) -> bool {
    std::fs::read_to_string(join(root, "src/ui/shell.rs"))
        .is_ok_and(|src| declares_struct(&src, "Desk"))
}

/// The module `make:page`, `make:form` and `make:table` use without
/// `--module`: the one module the app has under `modules_dir`.
pub fn default_module(root: &Path, modules_dir: &str) -> anyhow::Result<Name> {
    let dir = join(root, modules_dir);
    let mut found: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.join("mod.rs").is_file() {
                continue;
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && Name::parse("module", name).is_ok()
            {
                found.push(name.to_owned());
            }
        }
    }
    found.sort();
    match found.as_slice() {
        [only] => Ok(Name::parse("module", only)?),
        [] => anyhow::bail!(
            "no module found in {modules_dir}; create one with `navree make:module <name>`, \
             or name the module with --module"
        ),
        many => anyhow::bail!(
            "which module? {modules_dir} has {}; name one with --module <name>",
            many.join(", ")
        ),
    }
}

/// `path` relative to `root`, with `/` separators.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// The steps a `kind` item leaves to the user.
fn next_steps(kind: MakeKind, ctx: &UiContext, stem: &str, module_dir: &str) -> Vec<String> {
    match kind {
        MakeKind::Page => vec![
            "the page takes `AppShell`: the app needs `navree::ui::UiModule` and a registered \
             `Shell` (`navree new <app> --ui navree` sets both up; see \
             https://navree.com/docs/ui/app-shell/)"
                .to_owned(),
            format!(
                "link to it from the shell: `NavItem::new(\"{}\", \"/{}\")`",
                ctx.title, ctx.route
            ),
        ],
        MakeKind::Form => vec![format!(
            "call `forms::{stem}::page` from a `GET /{route}` route and `forms::{stem}::store` \
             from a `POST /{route}` route, with the `Submit` and `Rejected` extractors (see \
             https://navree.com/docs/ui/forms/)",
            route = ctx.route
        )],
        _ => match &ctx.model_path {
            Some(_) => vec![format!(
                "call `tables::{stem}::index` from a `GET /{}` route (see \
                 https://navree.com/docs/ui/data-table/)",
                ctx.route
            )],
            None => vec![format!(
                "no model with an id, a name and a created_at was found in {module_dir}: map \
                 your own model to `{}` and add the `index` handler sketched in tables/{stem}.rs",
                ctx.type_name
            )],
        },
    }
}

/// Generates a UI item `name` of kind `kind` (a page, a form or a table) in
/// module `module` (under `root/modules_dir`), declares it, and registers a
/// page where the module has a `// navree:wire` marker. Nothing is written
/// when a file exists, unless `options.force`, and nothing at all with
/// `options.dry_run`.
pub fn make_ui_item(
    root: &Path,
    modules_dir: &str,
    kind: MakeKind,
    module: &Name,
    name: &Name,
    options: &UiOptions,
) -> anyhow::Result<UiReport> {
    anyhow::ensure!(MakeKind::UI.contains(&kind), "{kind:?} is not a UI kind");
    let module_dir = join(root, modules_dir).join(module.snake());
    let module_rs = module_dir.join("mod.rs");
    if !module_rs.is_file() {
        anyhow::bail!(
            "module `{m}` not found at {}; create it with `navree make:module {m}`",
            module_rs.display(),
            m = module.snake()
        );
    }
    let dir = kind.dir();
    let stem = name.snake();
    let dir_mod = module_dir.join(dir).join("mod.rs");
    let tests_mod = module_dir.join("tests").join("mod.rs");
    let layout = UiLayout {
        new_dir: !dir_mod.is_file(),
        new_tests: kind == MakeKind::Page && !tests_mod.is_file(),
        model: match kind {
            MakeKind::Table => find_model(&module_dir, &name.singular()),
            _ => None,
        },
    };
    let files = ui_files(kind, module, name, options, &layout)?;
    let ctx = UiContext::new(kind, module, name, options, &layout);

    let mut created = Vec::new();
    let mut overwritten = Vec::new();
    for file in files.iter() {
        let path = join(&module_dir, &file.path);
        if path.exists() {
            overwritten.push(path);
        } else {
            created.push(path);
        }
    }
    if !overwritten.is_empty() && !options.force {
        let list: Vec<String> = overwritten
            .iter()
            .map(|p| format!("  {}", p.display()))
            .collect();
        anyhow::bail!(
            "refusing to overwrite existing files (nothing was written; --force replaces them):\n{}",
            list.join("\n")
        );
    }

    // The edits of existing files: the declarations, then the registration.
    let mut edits = Edits::default();
    if layout.new_dir {
        edits.declare(&module_rs, &format!("pub mod {dir};"))?;
    } else {
        edits.declare(&dir_mod, &format!("pub mod {stem};"))?;
    }
    if kind == MakeKind::Page {
        if layout.new_tests {
            edits.declare(&module_rs, "#[cfg(test)]\nmod tests;")?;
        } else {
            edits.declare(&tests_mod, &format!("mod {stem}_page;"))?;
        }
        let line = generic_call(
            "navree::http::ModuleBuilderHttpExt::controller",
            &[format!("{dir}::{stem}::{}", ctx.type_name)],
            "m",
            WIRE_INDENT,
        );
        edits.apply(
            &module_rs,
            |src| insert_above_marker(src, "navree:wire", &line),
            || format!("register it in the module's `register` (or `wire`) method: `{line}`"),
        )?;
    }

    let mut hints = edits.hints.clone();
    hints.extend(next_steps(kind, &ctx, stem, &relative(root, &module_dir)));
    if uses_desk(root) {
        hints.push(
            "this app's pages take `Desk` (src/ui/shell.rs), which also draws the signed-in \
             person, the workspace and the workspace switcher: use it where the generated code \
             takes `AppShell`"
                .to_owned(),
        );
    }

    if !options.dry_run {
        for file in files.iter() {
            let path = join(&module_dir, &file.path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("cannot create {}", parent.display()))?;
            }
            std::fs::write(&path, &file.contents)
                .with_context(|| format!("cannot write {}", path.display()))?;
        }
        edits.write()?;
    }

    // The contents under their path from the root, for `--dry-run` to show.
    let mut shown = FileSet::new();
    for file in files.iter() {
        shown.add(
            relative(root, &join(&module_dir, &file.path)),
            file.contents.as_str(),
        );
    }
    Ok(UiReport {
        made: MakeReport {
            created,
            updated: edits.order,
            hints,
        },
        overwritten,
        dry_run: options.dry_run,
        files: shown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_type_is_named_for_its_kind_once() {
        assert_eq!(typed("Dashboard".to_owned(), "Page"), "DashboardPage");
        assert_eq!(typed("AboutPage".to_owned(), "Page"), "AboutPage");
        assert_eq!(typed("Page".to_owned(), "Page"), "PagePage");
        assert_eq!(typed("Homepage".to_owned(), "Page"), "HomepagePage");
    }

    #[test]
    fn a_model_is_found_by_its_struct_and_its_columns() {
        let model = "pub struct Deal {\n    pub id: Uuid,\n    pub name: String,\n    pub created_at: DateTime<Utc>,\n}\n";
        assert!(is_standard_model(model, "Deal"));
        assert!(!is_standard_model(model, "Dea"), "a prefix is not the name");
        assert!(!is_standard_model(model, "Deals"));
        let no_name = "pub struct Profile {\n    pub id: Uuid,\n    pub timezone: String,\n}\n";
        assert!(!is_standard_model(no_name, "Profile"));
    }
}
