//! `navree make:module` and `navree make:<kind>`: code generators for feature
//! modules.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Serialize;

use crate::clock::Timestamp;
use crate::edit::{Edit, add_migration, declare_module, insert_above_marker};
use crate::names::Name;
use crate::scaffold::{FileSet, join};
use crate::templates::{TemplateError, render};

/// `words` with an English indefinite article, by spelling (best effort):
/// `an` before a vowel, except a `u` sounding like "you" (`a user`, `a unit`,
/// but `an update`).
fn with_article(words: &str, a: &str, an: &str) -> String {
    let vowel = |c: Option<u8>| matches!(c, Some(b'a' | b'e' | b'i' | b'o' | b'u'));
    let bytes = words.as_bytes();
    let first = bytes.first().copied();
    let you = first == Some(b'u') && !vowel(bytes.get(1).copied()) && vowel(bytes.get(2).copied());
    let article = if vowel(first) && !you { an } else { a };
    format!("{article} {words}")
}

/// What the module templates are rendered with.
#[derive(Debug, Clone, Serialize)]
struct ModuleContext {
    /// `blog_posts`: the directory, the Rust module and the migration set.
    module: String,
    /// `BlogPosts`.
    module_pascal: String,
    /// `Blog posts`.
    module_title: String,
    /// `blog posts`.
    module_words: String,
    /// `blog-posts`: the route prefix.
    route: String,
    /// `blog_post`.
    entity: String,
    /// `blog_post` (`r#match` when the singular is a keyword).
    entity_var: String,
    /// `BlogPost`: the type names and the access subject.
    entity_pascal: String,
    /// `blog post`.
    entity_words: String,
    /// `a blog post`.
    a_entity: String,
    /// `A blog post`.
    a_entity_title: String,
    /// `blog_posts`: the table and the permission prefix.
    table: String,
    /// `BlogPosts`: the `DeriveIden` enum of the table.
    table_iden: String,
    /// `m20261025_143005_create_blog_posts_table`.
    migration: String,
    /// Full CRUD with access checks, or a plain module.
    crud: bool,
}

impl ModuleContext {
    fn new(module: &Name, crud: bool, now: Timestamp) -> Self {
        let entity = module.singular();
        let table = module.plural();
        let entity_words = entity.snake().replace('_', " ");
        let entity_var = match Name::parse("variable", entity.snake()) {
            Ok(_) => entity.snake().to_owned(),
            Err(_) => format!("r#{}", entity.snake()),
        };
        Self {
            module: module.snake().to_owned(),
            module_pascal: module.pascal(),
            module_title: module.title(),
            module_words: module.snake().replace('_', " "),
            route: module.kebab(),
            entity: entity.snake().to_owned(),
            entity_var,
            entity_pascal: entity.pascal(),
            a_entity: with_article(&entity_words, "a", "an"),
            a_entity_title: with_article(&entity_words, "A", "An"),
            entity_words,
            table: table.snake().to_owned(),
            table_iden: table.pascal(),
            migration: format!("{}_create_{}_table", now.migration_prefix(), table.snake()),
            crud,
        }
    }
}

/// The files of feature module `name`, relative to the modules directory
/// (`posts/mod.rs`, `posts/controller.rs`, ...). With `crud`, the module
/// serves the five JSON resource routes guarded by abilities on its entity;
/// without, one authenticated `index` route.
pub fn module_files(name: &Name, crud: bool, now: Timestamp) -> Result<FileSet, TemplateError> {
    let ctx = ModuleContext::new(name, crud, now);
    let dir = &ctx.module;
    let mut files = FileSet::new();
    for (template, path) in [
        ("module/mod.rs", format!("{dir}/mod.rs")),
        ("module/controller.rs", format!("{dir}/controller.rs")),
        ("module/service.rs", format!("{dir}/service.rs")),
        ("module/entity.rs", format!("{dir}/entity.rs")),
        ("module/dto.rs", format!("{dir}/dto.rs")),
        (
            "module/migrations/mod.rs",
            format!("{dir}/migrations/mod.rs"),
        ),
        (
            "module/migrations/create_table.rs",
            format!("{dir}/migrations/{}.rs", ctx.migration),
        ),
        ("module/tests/mod.rs", format!("{dir}/tests/mod.rs")),
    ] {
        files.add(path, render(template, &ctx)?);
    }
    Ok(files)
}

/// What a generator did.
#[derive(Debug, Default)]
pub struct MakeReport {
    /// Files created.
    pub created: Vec<PathBuf>,
    /// Existing files edited (module declarations, registrations).
    pub updated: Vec<PathBuf>,
    /// Steps left to do by hand (an edit could not be applied safely).
    pub hints: Vec<String>,
}

impl MakeReport {
    /// Applies `edit` to `path`, recording the change; `None` adds `hint`.
    fn apply(
        &mut self,
        path: &Path,
        edit: Option<Edit>,
        hint: impl FnOnce() -> String,
    ) -> anyhow::Result<()> {
        match edit {
            Some(Edit::Changed(contents)) => {
                std::fs::write(path, contents)
                    .with_context(|| format!("cannot write {}", path.display()))?;
                self.updated.push(path.to_path_buf());
            }
            Some(Edit::Unchanged) => {}
            None => self.hints.push(hint()),
        }
        Ok(())
    }

    /// Edits the file at `path` with `f` when it exists; adds `hint` otherwise
    /// (or when `f` cannot apply).
    fn edit_file(
        &mut self,
        path: &Path,
        f: impl FnOnce(&str) -> Option<Edit>,
        hint: impl FnOnce() -> String,
    ) -> anyhow::Result<()> {
        let edit = match std::fs::read_to_string(path) {
            Ok(src) => f(&src),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| format!("cannot read {}", path.display()));
            }
        };
        self.apply(path, edit, hint)
    }
}

/// `crate::modules` for `src/modules`; `None` when the directory is not
/// under `src/` or a segment is not an identifier.
fn rust_path(modules_dir: &str) -> Option<String> {
    let rest = modules_dir.trim_matches('/').strip_prefix("src/")?;
    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() || segments.iter().any(|s| Name::parse("module", s).is_err()) {
        return None;
    }
    Some(format!("crate::{}", segments.join("::")))
}

/// Creates feature module `name` under `root/modules_dir`, declares it in
/// the parent module and adds it to the app root (`src/app.rs`, above the
/// `// navree:modules` marker). Nothing is written when a file exists.
pub fn make_module(
    root: &Path,
    modules_dir: &str,
    name: &Name,
    crud: bool,
    now: Timestamp,
) -> anyhow::Result<MakeReport> {
    let dir = join(root, modules_dir);
    let mut report = MakeReport {
        created: module_files(name, crud, now)?.write(&dir)?,
        ..MakeReport::default()
    };
    let decl = format!("pub mod {};", name.snake());
    let parent = [dir.join("mod.rs"), dir.with_extension("rs")]
        .into_iter()
        .find(|p| p.is_file())
        .unwrap_or_else(|| dir.join("mod.rs"));
    report.edit_file(
        &parent,
        |src| Some(declare_module(src, &decl)),
        || format!("declare the module: add `{decl}` to {}", parent.display()),
    )?;
    let path = rust_path(modules_dir).unwrap_or_else(|| "crate::modules".to_owned());
    let path = if path == "crate::modules" {
        "modules".to_owned()
    } else {
        path
    };
    let line = format!(".module({path}::{}::{}Module)", name.snake(), name.pascal());
    let app = root.join("src").join("app.rs");
    report.edit_file(
        &app,
        |src| insert_above_marker(src, "navree:modules", &line),
        || format!("add the module to the app root (`Navree::app()`): `{line}`"),
    )?;
    Ok(report)
}

/// What `navree make:<kind>` generates inside a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MakeKind {
    /// `controllers/<name>.rs`: `<Name>Controller`, registered with the module.
    Controller,
    /// `services/<name>.rs`: `<Name>Service`, a provider of the module.
    Service,
    /// `entities/<name>.rs`: a SeaORM entity for the `<names>` table.
    Entity,
    /// `migrations/m<timestamp>_<name>.rs`, added to the module's set.
    Migration,
    /// `jobs/<name>.rs`: a queued job, registered with the module.
    Job,
    /// `events/<name>.rs`: an event.
    Event,
    /// `listeners/<name>.rs`: a listener, subscribed when `--event` is given.
    Listener,
    /// `mail/<name>.rs` with `<name>.html` and `<name>.txt` templates.
    Mail,
    /// `notifications/<name>.rs`: a database and broadcast notification.
    Notification,
    /// `policies/<name>.rs`: ability rules, added to `src/abilities.rs`.
    Policy,
    /// `commands/<name>.rs`: a CLI command, registered with the module.
    Command,
    /// `tests/<name>.rs`: a test booting the module.
    Test,
    /// `mcp/<name>.rs`: an MCP server with one tool, registered with the
    /// module (needs navree's `mcp` feature and `McpModule`).
    Mcp,
}

impl MakeKind {
    /// Every kind, in `navree help` order.
    pub const ALL: [MakeKind; 13] = [
        MakeKind::Controller,
        MakeKind::Service,
        MakeKind::Entity,
        MakeKind::Migration,
        MakeKind::Job,
        MakeKind::Event,
        MakeKind::Listener,
        MakeKind::Mail,
        MakeKind::Notification,
        MakeKind::Policy,
        MakeKind::Command,
        MakeKind::Test,
        MakeKind::Mcp,
    ];

    /// `controller`, `service`, ...
    pub fn noun(self) -> &'static str {
        match self {
            MakeKind::Controller => "controller",
            MakeKind::Service => "service",
            MakeKind::Entity => "entity",
            MakeKind::Migration => "migration",
            MakeKind::Job => "job",
            MakeKind::Event => "event",
            MakeKind::Listener => "listener",
            MakeKind::Mail => "mail",
            MakeKind::Notification => "notification",
            MakeKind::Policy => "policy",
            MakeKind::Command => "command",
            MakeKind::Test => "test",
            MakeKind::Mcp => "mcp",
        }
    }

    /// The directory of the module holding items of this kind.
    pub fn dir(self) -> &'static str {
        match self {
            MakeKind::Controller => "controllers",
            MakeKind::Service => "services",
            MakeKind::Entity => "entities",
            MakeKind::Migration => "migrations",
            MakeKind::Job => "jobs",
            MakeKind::Event => "events",
            MakeKind::Listener => "listeners",
            MakeKind::Mail => "mail",
            MakeKind::Notification => "notifications",
            MakeKind::Policy => "policies",
            MakeKind::Command => "commands",
            MakeKind::Test => "tests",
            MakeKind::Mcp => "mcp",
        }
    }

    /// The first line of a new `<dir>/mod.rs`.
    fn dir_doc(self, module_words: &str) -> String {
        let what = match self {
            MakeKind::Controller => "HTTP controllers",
            MakeKind::Service => "Services",
            MakeKind::Entity => "Database entities",
            MakeKind::Migration => "Migrations",
            MakeKind::Job => "Queued jobs",
            MakeKind::Event => "Events",
            MakeKind::Listener => "Event listeners",
            MakeKind::Mail => "Mail",
            MakeKind::Notification => "Notifications",
            MakeKind::Policy => "Ability policies",
            MakeKind::Command => "CLI commands",
            MakeKind::Test => "Tests",
            MakeKind::Mcp => "MCP servers",
        };
        format!("//! {what} of the {module_words} module.\n")
    }
}

/// Options of `navree make:<kind>`.
#[derive(Debug, Clone, Default)]
pub struct MakeOptions {
    /// `make:listener --event <event>`: the module's event the listener
    /// handles (in `events/<event>.rs`).
    pub event: Option<Name>,
}

/// What the item templates are rendered with.
#[derive(Debug, Clone, Serialize)]
struct ItemContext {
    module: String,
    module_words: String,
    module_kebab: String,
    module_struct: String,
    name: String,
    pascal: String,
    words: String,
    title: String,
    a_title: String,
    kebab: String,
    table: String,
    table_iden: String,
    create_table: bool,
    event: Option<String>,
    event_pascal: Option<String>,
}

impl ItemContext {
    fn new(kind: MakeKind, module: &Name, name: &Name, options: &MakeOptions) -> Self {
        let created = name
            .snake()
            .strip_prefix("create_")
            .and_then(|rest| rest.strip_suffix("_table"))
            .and_then(|table| Name::parse("table", table).ok());
        let create_table = kind == MakeKind::Migration && created.is_some();
        let table = created.unwrap_or_else(|| name.plural());
        let words = name.snake().replace('_', " ");
        Self {
            module: module.snake().to_owned(),
            module_words: module.snake().replace('_', " "),
            module_kebab: module.kebab(),
            module_struct: format!("{}Module", module.pascal()),
            name: name.snake().to_owned(),
            pascal: name.pascal(),
            a_title: with_article(&words, "A", "An"),
            words,
            title: name.title(),
            kebab: name.kebab(),
            table: table.snake().to_owned(),
            table_iden: table.pascal(),
            create_table,
            event: options.event.as_ref().map(|e| e.snake().to_owned()),
            event_pascal: options.event.as_ref().map(Name::pascal),
        }
    }
}

/// rustfmt's line width.
const MAX_WIDTH: usize = 100;

/// The indentation of statements in a module's `wire` method.
const WIRE_INDENT: usize = 8;

/// `path::<generics>(args);` laid out as rustfmt would at `indent`: on one
/// line when it fits, else one generic argument per line.
fn generic_call(path: &str, generics: &[String], args: &str, indent: usize) -> String {
    let line = format!("{path}::<{}>({args});", generics.join(", "));
    if indent + line.len() <= MAX_WIDTH {
        return line;
    }
    let mut out = format!("{path}::<\n");
    for generic in generics {
        out.push_str(&format!("    {generic},\n"));
    }
    out.push_str(&format!(">({args});"));
    out
}

/// `path(args...);` laid out as rustfmt would at `indent`.
fn call(path: &str, args: &[String], indent: usize) -> String {
    let line = format!("{path}({});", args.join(", "));
    if indent + line.len() <= MAX_WIDTH {
        return line;
    }
    let mut out = format!("{path}(\n");
    for arg in args {
        out.push_str(&format!("    {arg},\n"));
    }
    out.push_str(");");
    out
}

/// The code registering a `kind` item in the module's `wire`, if it needs one.
fn wire_line(kind: MakeKind, stem: &str, ctx: &ItemContext, new_dir: bool) -> Option<String> {
    let pascal = &ctx.pascal;
    let m = "m".to_owned();
    match kind {
        MakeKind::Controller => Some(generic_call(
            "navree::http::ModuleBuilderHttpExt::controller",
            &[format!("controllers::{stem}::{pascal}Controller")],
            "m",
            WIRE_INDENT,
        )),
        MakeKind::Service => Some(generic_call(
            "m.provider",
            &[format!("services::{stem}::{pascal}Service")],
            "",
            WIRE_INDENT,
        )),
        MakeKind::Job => Some(generic_call(
            "navree::contracts::ModuleBuilderJobsExt::job",
            &[format!("jobs::{stem}::{pascal}")],
            "m",
            WIRE_INDENT,
        )),
        MakeKind::Listener => {
            let (event, event_pascal) = ctx.event.as_ref().zip(ctx.event_pascal.as_ref())?;
            Some(generic_call(
                "navree::events::ModuleBuilderEventsExt::listen",
                &[
                    format!("events::{event}::{event_pascal}"),
                    format!("listeners::{stem}::{pascal}"),
                ],
                "m",
                WIRE_INDENT,
            ))
        }
        MakeKind::Mail => Some(call(
            "navree::view::ModuleBuilderViewExt::templates",
            &[m, format!("mail::{stem}::TEMPLATES")],
            WIRE_INDENT,
        )),
        MakeKind::Command => Some(call(
            "m.command",
            &[format!("commands::{stem}::command()")],
            WIRE_INDENT,
        )),
        MakeKind::Mcp => Some(generic_call(
            "navree::mcp::ModuleBuilderMcpExt::mcp_server",
            &[format!("mcp::{stem}::{pascal}Mcp")],
            "m",
            WIRE_INDENT,
        )),
        MakeKind::Migration if new_dir => Some(call(
            "navree::db::ModuleBuilderDbExt::migrations",
            &[m, "migrations::migrations()".to_owned()],
            WIRE_INDENT,
        )),
        _ => None,
    }
}

/// Generates a `kind` item `name` in module `module` (under
/// `root/modules_dir`), declares it, and registers it where the module has a
/// `// navree:wire` marker (`src/abilities.rs`'s `// navree:policies` for
/// policies). Nothing is written when a file exists.
pub fn make_item(
    root: &Path,
    modules_dir: &str,
    kind: MakeKind,
    module: &Name,
    name: &Name,
    options: &MakeOptions,
    now: Timestamp,
) -> anyhow::Result<MakeReport> {
    let module_dir = join(root, modules_dir).join(module.snake());
    let module_rs = module_dir.join("mod.rs");
    if !module_rs.is_file() {
        anyhow::bail!(
            "module `{m}` not found at {}; create it with `navree make:module {m}`",
            module_rs.display(),
            m = module.snake()
        );
    }
    if let (MakeKind::Listener, Some(event)) = (kind, &options.event) {
        let path = module_dir
            .join("events")
            .join(format!("{}.rs", event.snake()));
        if !path.is_file() {
            anyhow::bail!(
                "event `{e}` not found at {}; create it with `navree make:event {} {e}`",
                path.display(),
                module.snake(),
                e = event.snake()
            );
        }
    }
    let ctx = ItemContext::new(kind, module, name, options);
    let dir = kind.dir();
    let stem = match kind {
        MakeKind::Migration => format!("{}_{}", now.migration_prefix(), name.snake()),
        _ => name.snake().to_owned(),
    };

    let mut files = FileSet::new();
    files.add(
        format!("{dir}/{stem}.rs"),
        render(&format!("make/{}.rs", kind.noun()), &ctx)?,
    );
    if kind == MakeKind::Mail {
        files.add(
            format!("{dir}/{stem}.html"),
            render("make/mail.html", &ctx)?,
        );
        files.add(format!("{dir}/{stem}.txt"), render("make/mail.txt", &ctx)?);
    }
    // The kind's `mod.rs`: created with the item, or updated after writing.
    let item_decl = match kind {
        MakeKind::Migration | MakeKind::Test => format!("mod {stem};"),
        _ => format!("pub mod {stem};"),
    };
    let dir_mod = module_dir.join(dir).join("mod.rs");
    let new_dir = !dir_mod.is_file();
    if new_dir {
        let contents = if kind == MakeKind::Migration {
            render(
                "module/migrations/mod.rs",
                ModuleContext {
                    migration: stem.clone(),
                    ..ModuleContext::new(module, false, now)
                },
            )?
        } else {
            format!("{}\n{item_decl}\n", kind.dir_doc(&ctx.module_words))
        };
        files.add(format!("{dir}/mod.rs"), contents);
    }

    let mut report = MakeReport {
        created: files.write(&module_dir)?,
        ..MakeReport::default()
    };

    if new_dir {
        let decl = match kind {
            MakeKind::Test => "#[cfg(test)]\nmod tests;".to_owned(),
            MakeKind::Migration => "mod migrations;".to_owned(),
            _ => format!("pub mod {dir};"),
        };
        report.edit_file(
            &module_rs,
            |src| Some(declare_module(src, &decl)),
            || format!("declare `{decl}` in {}", module_rs.display()),
        )?;
    } else {
        report.edit_file(
            &dir_mod,
            |src| Some(declare_module(src, &item_decl)),
            || format!("declare `{item_decl}` in {}", dir_mod.display()),
        )?;
        if kind == MakeKind::Migration {
            report.edit_file(
                &dir_mod,
                |src| add_migration(src, &stem),
                || {
                    format!(
                        "add `.migration({stem}::Migration)` to the migration set in {}",
                        dir_mod.display()
                    )
                },
            )?;
        }
    }

    if let Some(line) = wire_line(kind, &stem, &ctx, new_dir) {
        report.edit_file(
            &module_rs,
            |src| insert_above_marker(src, "navree:wire", &line),
            || format!("register it in the module's `register` (or `wire`) method: `{line}`"),
        )?;
    }

    match kind {
        MakeKind::Policy => {
            let modules = rust_path(modules_dir).unwrap_or_else(|| "crate::modules".to_owned());
            let call = format!(
                "{modules}::{}::policies::{stem}::define(ctx, rules);",
                module.snake()
            );
            let abilities = root.join("src").join("abilities.rs");
            report.edit_file(
                &abilities,
                |src| insert_above_marker(src, "navree:policies", &call),
                || format!("call the policy from the app's abilities function: `{call}`"),
            )?;
        }
        MakeKind::Entity => report.hints.push(format!(
            "create its table: navree make:migration {} create_{}_table",
            module.snake(),
            ctx.table
        )),
        MakeKind::Mcp => report.hints.extend([
            "turn on navree's MCP feature in Cargo.toml: navree = { ..., features = [\"mcp\"] }"
                .to_owned(),
            "add utoipa for the arguments' `ToSchema` derive: utoipa = { version = \"6\", features = [\"chrono\", \"uuid\"] }"
                .to_owned(),
            "serve /mcp: add `.module(McpModule::from_config())` to the app root, after HttpModule and AuthModule"
                .to_owned(),
        ]),
        MakeKind::Listener if options.event.is_none() => report.hints.push(format!(
            "subscribe it in the module's `wire`: navree::events::ModuleBuilderEventsExt::listen::<SomeEvent, listeners::{stem}::{}>(m);",
            ctx.pascal
        )),
        _ => {}
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indefinite_articles() {
        let cases = [
            ("post", "a post"),
            ("item", "an item"),
            ("order", "an order"),
            ("user", "a user"),
            ("unit", "a unit"),
            ("update", "an update"),
            ("upload", "an upload"),
            ("blog post", "a blog post"),
        ];
        for (words, expected) in cases {
            assert_eq!(with_article(words, "a", "an"), expected);
        }
        assert_eq!(with_article("item", "A", "An"), "An item");
    }
}
