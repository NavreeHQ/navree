//! [`Environment`]: templates, filters, tests, globals and settings.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;

use crate::builtins;
use crate::error::{Error, ErrorKind};
use crate::escape::Mode;
use crate::functions::{self, Callable, Function, FunctionResult};
use crate::syntax::{Ast, Whitespace, parse};
use crate::value::{Repr, Value};
use crate::vm;

/// How printed values are escaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AutoEscape {
    /// Values print as they are.
    None,
    /// `&`, `<`, `>`, `"`, `'` and `/` print as HTML entities.
    Html,
    /// Values print as JSON.
    Json,
}

/// What using an undefined value does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum UndefinedBehavior {
    /// Printing gives nothing, iterating gives nothing, testing gives
    /// false; looking up an attribute of an undefined value is an error.
    #[default]
    Lenient,
    /// Like `Lenient`, and an attribute of an undefined value is undefined.
    Chainable,
    /// Printing, iterating, comparing or passing an undefined value where
    /// a string is needed is an error; testing it (`{% if x %}`) is false.
    SemiStrict,
    /// Like `SemiStrict`, and testing an undefined value is an error too.
    Strict,
}

/// Caps on rendering, so that no template (trusted or not) can exhaust the
/// stack, memory or time. Exceeding one is an [`ErrorKind::LimitExceeded`]
/// error naming the limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// How deep macro calls, call blocks, blocks, includes and imports may
    /// nest. Default 100.
    pub max_depth: usize,
    /// How deep includes, imports and `extends` chains may nest. Default 32.
    pub max_include_depth: usize,
    /// Loop iterations of one render, all loops together. Default
    /// 10,000,000.
    pub max_loop_iterations: u64,
    /// Work of one render: every statement and expression evaluated, every
    /// item a filter or operator goes through, and every string it scans or
    /// copies, a 64-byte stretch counting as one. Default 100,000,000.
    pub max_operations: u64,
    /// Bytes of the output, and of any string a template builds (captured
    /// output, joined or repeated strings). Default 64 MiB.
    pub max_output_bytes: usize,
    /// Bytes a render may allocate over its life for its output and for the
    /// strings, sequences and maps it builds. Each is charged before it is
    /// allocated, freed or not, so what a render holds stays below this
    /// however it holds it. The context, what registered functions allocate,
    /// compiled templates and a little bookkeeping per scope are not counted.
    /// Default 1 GiB.
    pub max_allocated_bytes: u64,
    /// Stack the compiles and renders on one thread may use together, measured
    /// from where the outermost one started, so a render inside a render and a
    /// compile during a render share it. It also stops while 256 KiB of the
    /// thread's stack are free, wherever the outermost one started, on
    /// platforms where the end of the stack is known (Linux, macOS, Windows);
    /// a thread with less than that left can't render. How deep this
    /// reaches depends on the build: optimized code takes less per level.
    /// Default 768 KiB.
    pub max_stack_bytes: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_depth: 100,
            max_include_depth: 32,
            max_loop_iterations: 10_000_000,
            max_operations: 100_000_000,
            max_output_bytes: 64 << 20,
            max_allocated_bytes: 1 << 30,
            max_stack_bytes: 768 << 10,
        }
    }
}

/// A template's source as a [loader](Environment::set_loader) returns it,
/// with an optional check of whether it is still current.
pub struct TemplateSource {
    source: String,
    up_to_date: Option<Box<dyn Fn() -> bool + Send + Sync>>,
}

impl TemplateSource {
    /// A source that never changes.
    pub fn new(source: impl Into<String>) -> TemplateSource {
        TemplateSource {
            source: source.into(),
            up_to_date: None,
        }
    }

    /// Adds a check run on every lookup: when it returns `false`, the
    /// template is loaded again.
    pub fn with_uptodate_check(
        mut self,
        check: impl Fn() -> bool + Send + Sync + 'static,
    ) -> TemplateSource {
        self.up_to_date = Some(Box::new(check));
        self
    }
}

type Loader = dyn Fn(&str) -> Result<Option<TemplateSource>, Error> + Send + Sync;
type AutoEscapeCallback = dyn Fn(&str) -> AutoEscape + Send + Sync;

/// A compiled template.
pub(crate) struct Compiled {
    pub(crate) name: Arc<str>,
    pub(crate) ast: Arc<Ast>,
    /// How its values print, from its name.
    pub(crate) mode: Mode,
}

struct Loaded {
    compiled: Arc<Compiled>,
    up_to_date: Option<Box<dyn Fn() -> bool + Send + Sync>>,
}

/// Templates and everything they render with: filters, tests, global
/// functions and values, the whitespace rules, the undefined behavior, the
/// autoescaping rule and the [`Limits`].
///
/// ```
/// use navree_template::Environment;
/// use serde_json::json;
///
/// let mut env = Environment::new();
/// env.add_template("hello.html", "<p>Hello {{ name }}!</p>").unwrap();
/// let html = env.get_template("hello.html").unwrap().render(json!({ "name": "<Anu>" })).unwrap();
/// assert_eq!(html, "<p>Hello &lt;Anu&gt;!</p>");
/// ```
pub struct Environment {
    templates: BTreeMap<String, Arc<Compiled>>,
    loader: Option<Arc<Loader>>,
    loaded: Mutex<BTreeMap<String, Loaded>>,
    pub(crate) filters: BTreeMap<Cow<'static, str>, Arc<Callable>>,
    pub(crate) tests: BTreeMap<Cow<'static, str>, Arc<Callable>>,
    pub(crate) globals: BTreeMap<Cow<'static, str>, Value>,
    whitespace: Whitespace,
    pub(crate) undefined: UndefinedBehavior,
    auto_escape: Arc<AutoEscapeCallback>,
    pub(crate) limits: Limits,
}

impl Default for Environment {
    fn default() -> Environment {
        Environment::new()
    }
}

impl fmt::Debug for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Environment")
            .field("templates", &self.templates.keys().collect::<Vec<_>>())
            .field("undefined", &self.undefined)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl Environment {
    /// An environment with the built-in filters, tests and functions,
    /// lenient undefined values, escaping by name as
    /// [`default_auto_escape`] says, and the default [`Limits`].
    pub fn new() -> Environment {
        let mut env = Environment {
            templates: BTreeMap::new(),
            loader: None,
            loaded: Mutex::new(BTreeMap::new()),
            filters: BTreeMap::new(),
            tests: BTreeMap::new(),
            globals: BTreeMap::new(),
            whitespace: Whitespace::default(),
            undefined: UndefinedBehavior::default(),
            auto_escape: Arc::new(default_auto_escape),
            limits: Limits::default(),
        };
        builtins::register(&mut env);
        env
    }

    /// Removes the first newline after a statement or comment tag.
    pub fn set_trim_blocks(&mut self, yes: bool) {
        self.whitespace.trim_blocks = yes;
    }

    /// Removes the spaces and tabs before a statement or comment tag that
    /// starts its line.
    pub fn set_lstrip_blocks(&mut self, yes: bool) {
        self.whitespace.lstrip_blocks = yes;
    }

    /// Keeps the newline that ends a template (removed by default).
    pub fn set_keep_trailing_newline(&mut self, yes: bool) {
        self.whitespace.keep_trailing_newline = yes;
    }

    /// Sets what using an undefined value does.
    pub fn set_undefined_behavior(&mut self, behavior: UndefinedBehavior) {
        self.undefined = behavior;
    }

    /// The undefined behavior.
    pub fn undefined_behavior(&self) -> UndefinedBehavior {
        self.undefined
    }

    /// Sets how a template's name picks its mode, for the templates there are
    /// and those to come. The default is [`default_auto_escape`]. Nothing
    /// rendered inside another template prints less strictly than where it
    /// lands, whatever this returns for its name.
    pub fn set_auto_escape_callback(
        &mut self,
        callback: impl Fn(&str) -> AutoEscape + Send + Sync + 'static,
    ) {
        self.auto_escape = Arc::new(callback);
        let templates = std::mem::take(&mut self.templates);
        self.templates = templates
            .into_iter()
            .map(|(name, compiled)| {
                let compiled = Arc::new(Compiled {
                    name: compiled.name.clone(),
                    ast: compiled.ast.clone(),
                    mode: Mode::from((self.auto_escape)(&name)),
                });
                (name, compiled)
            })
            .collect();
        self.loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// Sets the rendering [`Limits`].
    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
    }

    /// The rendering [`Limits`].
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Compiles `source` as template `name`, replacing a template of the
    /// same name. A syntax error is returned, naming the template.
    pub fn add_template(
        &mut self,
        name: impl AsRef<str>,
        source: impl AsRef<str>,
    ) -> Result<(), Error> {
        let name = name.as_ref();
        let compiled = self.compile(name, source.as_ref())?;
        self.templates.insert(name.to_owned(), Arc::new(compiled));
        Ok(())
    }

    /// Removes template `name`.
    pub fn remove_template(&mut self, name: &str) {
        self.templates.remove(name);
        self.loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name);
    }

    /// Sets a loader for templates not added with
    /// [`add_template`](Environment::add_template): it returns a template's
    /// source, or `None` when there is no such template. Loaded templates
    /// are kept and loaded again when their up-to-date check fails.
    pub fn set_loader(
        &mut self,
        loader: impl Fn(&str) -> Result<Option<TemplateSource>, Error> + Send + Sync + 'static,
    ) {
        self.loader = Some(Arc::new(loader));
        self.loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    pub(crate) fn compile(&self, name: &str, source: &str) -> Result<Compiled, Error> {
        let ast = parse(source, name, self.whitespace, self.limits.max_stack_bytes)?;
        Ok(Compiled {
            name: Arc::from(name),
            ast: Arc::new(ast),
            mode: Mode::from((self.auto_escape)(name)),
        })
    }

    pub(crate) fn lookup_template(&self, name: &str) -> Result<Arc<Compiled>, Error> {
        if let Some(compiled) = self.templates.get(name) {
            return Ok(compiled.clone());
        }
        let not_found = || {
            Error::new(
                ErrorKind::TemplateNotFound,
                format!("template {name:?} does not exist"),
            )
        };
        let Some(loader) = &self.loader else {
            return Err(not_found());
        };
        {
            let loaded = self.loaded.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(entry) = loaded.get(name)
                && entry.up_to_date.as_ref().is_none_or(|check| check())
            {
                return Ok(entry.compiled.clone());
            }
        }
        let Some(source) = loader(name)? else {
            self.loaded
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(name);
            return Err(not_found());
        };
        let compiled = Arc::new(self.compile(name, &source.source)?);
        self.loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                name.to_owned(),
                Loaded {
                    compiled: compiled.clone(),
                    up_to_date: source.up_to_date,
                },
            );
        Ok(compiled)
    }

    /// Template `name`, ready to render. An error when there is none
    /// ([`ErrorKind::TemplateNotFound`]) or it fails to compile.
    pub fn get_template(&self, name: &str) -> Result<Template<'_>, Error> {
        Ok(Template {
            env: self,
            compiled: self.lookup_template(name)?,
        })
    }

    /// Renders `source` as template `name` (which picks the escaping and
    /// names errors) with `ctx`. It can extend, include and import the
    /// environment's templates.
    pub fn render_str(
        &self,
        name: &str,
        source: &str,
        ctx: impl Serialize,
    ) -> Result<String, Error> {
        let compiled = Arc::new(self.compile(name, source)?);
        Template {
            env: self,
            compiled,
        }
        .render(ctx)
    }

    /// Adds (or replaces) filter `name`: `{{ value|name(args) }}`.
    pub fn add_filter<F, Rv, Args>(&mut self, name: impl Into<Cow<'static, str>>, f: F)
    where
        F: Function<Rv, Args>,
        Rv: FunctionResult,
        Args: for<'a> functions::FunctionArgs<'a>,
    {
        self.filters.insert(name.into(), functions::boxed(f));
    }

    /// Adds (or replaces) test `name`: `{% if value is name(args) %}`. The
    /// result counts as true or false like a condition.
    pub fn add_test<F, Rv, Args>(&mut self, name: impl Into<Cow<'static, str>>, f: F)
    where
        F: Function<Rv, Args>,
        Rv: FunctionResult,
        Args: for<'a> functions::FunctionArgs<'a>,
    {
        self.tests.insert(name.into(), functions::boxed(f));
    }

    /// Adds (or replaces) global function `name`: `{{ name(args) }}`.
    pub fn add_function<F, Rv, Args>(&mut self, name: impl Into<Cow<'static, str>>, f: F)
    where
        F: Function<Rv, Args>,
        Rv: FunctionResult,
        Args: for<'a> functions::FunctionArgs<'a>,
    {
        let name = name.into();
        let value = Value::from_function(&name, functions::boxed(f));
        self.globals.insert(name, value);
    }

    /// Adds (or replaces) global variable `name`, seen by every template
    /// unless its context has a variable of the same name.
    pub fn add_global(&mut self, name: impl Into<Cow<'static, str>>, value: impl Into<Value>) {
        self.globals.insert(name.into(), value.into());
    }

    /// Removes filter `name`.
    pub fn remove_filter(&mut self, name: &str) {
        self.filters.remove(name);
    }

    /// Removes global variable or function `name`.
    pub fn remove_global(&mut self, name: &str) {
        self.globals.remove(name);
    }
}

/// What an [`Environment`] holds at one moment, to tell afterwards what a
/// change to it touched: every template, filter, test and global by name, with
/// the identity of what each holds, and the loader, the escaping rule, the
/// undefined behavior, the limits and the whitespace settings. It keeps a
/// handle on each of them, so an identity is not taken for another's when a
/// freed one's address is used again. Not public API; Navree's own crates use
/// it.
pub struct Fingerprint {
    /// Each template's syntax tree: a template added again, with the same text
    /// or not, has a new one.
    templates: BTreeMap<String, Arc<Ast>>,
    filters: BTreeMap<String, Arc<Callable>>,
    tests: BTreeMap<String, Arc<Callable>>,
    /// Functions and variables, by name, as the very values the environment
    /// holds (compared by identity, see `Value::is_identical`): a function is
    /// its handle, a string its shared text with its safe mark.
    globals: BTreeMap<String, Value>,
    loader: Option<Arc<Loader>>,
    auto_escape: Arc<AutoEscapeCallback>,
    undefined: UndefinedBehavior,
    limits: Limits,
    whitespace: Whitespace,
}

/// The fingerprint of `env`. Not public API; Navree's own crates use it.
pub fn fingerprint(env: &Environment) -> Fingerprint {
    Fingerprint {
        templates: env
            .templates
            .iter()
            .map(|(name, compiled)| (name.clone(), compiled.ast.clone()))
            .collect(),
        filters: env
            .filters
            .iter()
            .map(|(name, call)| (name.to_string(), call.clone()))
            .collect(),
        tests: env
            .tests
            .iter()
            .map(|(name, call)| (name.to_string(), call.clone()))
            .collect(),
        globals: env
            .globals
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect(),
        loader: env.loader.clone(),
        auto_escape: env.auto_escape.clone(),
        undefined: env.undefined,
        limits: env.limits,
        whitespace: env.whitespace,
    }
}

impl Fingerprint {
    /// Every difference between this fingerprint and `after`, as the words
    /// that finish the sentence "the hook ...": templates first, then filters,
    /// tests and globals, each in name order (``adds the template `a.html` ``,
    /// ``replaces the filter `money` ``, ``removes the function `now` ``), then
    /// the settings (``changes the template loader``, ``changes how templates
    /// escape``, ``changes what an undefined value does``, ``changes the
    /// rendering limits``, ``changes the whitespace settings``). Not public
    /// API; Navree's own crates use it.
    pub fn changes(&self, after: &Fingerprint) -> Vec<String> {
        self.differences(after, None)
    }

    /// The differences [`changes`](Fingerprint::changes) lists that a plugin
    /// with `namespace` may not make: a filter, test, function or global added,
    /// replaced or removed whose name starts with `namespace` and an underscore
    /// is left out, and an addition of any other name says so (``adds the
    /// filter `shout`, which does not start with `announcements_` ``). Templates
    /// and settings are never left out. Not public API; Navree's own crates use
    /// it.
    pub fn changes_outside(&self, after: &Fingerprint, namespace: &str) -> Vec<String> {
        self.differences(after, Some(namespace))
    }

    fn differences(&self, after: &Fingerprint, namespace: Option<&str>) -> Vec<String> {
        let mut changes = Vec::new();
        let names: BTreeSet<&String> = self
            .templates
            .keys()
            .chain(after.templates.keys())
            .collect();
        for name in names {
            match (self.templates.get(name), after.templates.get(name)) {
                (None, Some(_)) => changes.push(format!("adds the template `{name}`")),
                (Some(_), None) => changes.push(format!("removes the template `{name}`")),
                (Some(before), Some(now)) if !Arc::ptr_eq(before, now) => {
                    changes.push(format!("replaces the template `{name}`"));
                }
                _ => {}
            }
        }
        for (kind, before, now) in [
            ("filter", &self.filters, &after.filters),
            ("test", &self.tests, &after.tests),
        ] {
            let names: BTreeSet<&String> = before.keys().chain(now.keys()).collect();
            for name in names {
                let (was, is) = (before.get(name), now.get(name));
                let same = matches!((was, is), (Some(a), Some(b)) if Arc::ptr_eq(a, b));
                record(
                    &mut changes,
                    name,
                    was.map(|_| kind),
                    is.map(|_| kind),
                    same,
                    namespace,
                );
            }
        }
        let names: BTreeSet<&String> = self.globals.keys().chain(after.globals.keys()).collect();
        for name in names {
            let (was, is) = (self.globals.get(name), after.globals.get(name));
            // By identity, not by equality: equal text put back marked safe,
            // or built again, is not what the app set.
            let same = matches!((was, is), (Some(a), Some(b)) if a.is_identical(b));
            record(
                &mut changes,
                name,
                was.map(global_kind),
                is.map(global_kind),
                same,
                namespace,
            );
        }
        let same_loader = match (&self.loader, &after.loader) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        };
        if !same_loader {
            changes.push("changes the template loader".to_owned());
        }
        if !Arc::ptr_eq(&self.auto_escape, &after.auto_escape) {
            changes.push("changes how templates escape".to_owned());
        }
        if self.undefined != after.undefined {
            changes.push("changes what an undefined value does".to_owned());
        }
        if self.limits != after.limits {
            changes.push("changes the rendering limits".to_owned());
        }
        if self.whitespace != after.whitespace {
            changes.push("changes the whitespace settings".to_owned());
        }
        changes
    }
}

/// What a global is called in a change: a function or a variable.
fn global_kind(value: &Value) -> &'static str {
    if matches!(value.0, Repr::Function(_)) {
        "function"
    } else {
        "global"
    }
}

/// Adds the change of the filter, test, function or global `name` to
/// `changes`, if it is one. `was` and `is` are its kind before and after (none
/// when it was not there, or is not), `same` whether what it holds is what it
/// held. With a `namespace`, a name that starts with it and an underscore is
/// not a change, and an addition of another name says it is outside.
fn record(
    changes: &mut Vec<String>,
    name: &str,
    was: Option<&str>,
    is: Option<&str>,
    same: bool,
    namespace: Option<&str>,
) {
    let own = namespace.is_some_and(|namespace| {
        name.strip_prefix(namespace)
            .is_some_and(|rest| rest.starts_with('_'))
    });
    match (was, is) {
        (None, Some(kind)) if !own => match namespace {
            Some(namespace) => changes.push(format!(
                "adds the {kind} `{name}`, which does not start with `{namespace}_`"
            )),
            None => changes.push(format!("adds the {kind} `{name}`")),
        },
        (Some(kind), None) if !own => changes.push(format!("removes the {kind} `{name}`")),
        (Some(kind), Some(_)) if !same && !own => {
            changes.push(format!("replaces the {kind} `{name}`"));
        }
        _ => {}
    }
}

/// A template name's extension in lower case, past a trailing `.j2`, `.jinja`
/// or `.jinja2`, and past the dots and spaces Windows ignores at the end of a
/// file name.
fn extension(name: &str) -> String {
    let name = name.trim_end_matches(['.', ' ']).to_ascii_lowercase();
    let mut name = name.as_str();
    for suffix in [".j2", ".jinja2", ".jinja"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped;
            break;
        }
    }
    name.rsplit('.').next().unwrap_or(name).to_owned()
}

/// The default rule for a template's mode (see
/// [`set_auto_escape_callback`](Environment::set_auto_escape_callback)), in any
/// case, also before a trailing `.j2`, `.jinja` or `.jinja2`: `.html`, `.htm`,
/// `.xhtml`, `.xml` and `.svg` escape HTML; `.json`, `.json5`, `.js`, `.mjs`,
/// `.cjs`, `.yaml` and `.yml` print JSON; anything else prints values as they
/// are.
pub fn default_auto_escape(name: &str) -> AutoEscape {
    match extension(name).as_str() {
        "html" | "htm" | "xhtml" | "xml" | "svg" => AutoEscape::Html,
        "json" | "json5" | "js" | "mjs" | "cjs" | "yaml" | "yml" => AutoEscape::Json,
        _ => AutoEscape::None,
    }
}

/// The rule for templates of unknown origin, the safe side of
/// [`default_auto_escape`]: `.txt` and `.md` print values as they are, what
/// that rule prints as JSON prints as JSON, and any other name, an unknown
/// extension or none at all, escapes HTML.
pub fn strict_auto_escape(name: &str) -> AutoEscape {
    match default_auto_escape(name) {
        AutoEscape::None if matches!(extension(name).as_str(), "txt" | "md") => AutoEscape::None,
        AutoEscape::Json => AutoEscape::Json,
        _ => AutoEscape::Html,
    }
}

/// A compiled template of an [`Environment`].
pub struct Template<'env> {
    env: &'env Environment,
    compiled: Arc<Compiled>,
}

impl fmt::Debug for Template<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Template")
            .field("name", &self.name())
            .finish_non_exhaustive()
    }
}

impl Template<'_> {
    /// The template's name.
    pub fn name(&self) -> &str {
        &self.compiled.name
    }

    /// Renders the template with `ctx`, any `Serialize` value whose fields
    /// become the template's variables.
    pub fn render(&self, ctx: impl Serialize) -> Result<String, Error> {
        let ctx = Value::from_serialize(&ctx)?;
        vm::render(self.env, &self.compiled, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An environment with two templates, a global and a function of its own.
    fn base() -> Environment {
        let mut env = Environment::new();
        env.add_template("mail/welcome.html", "<p>Welcome</p>")
            .unwrap();
        env.add_template("mail/bye.html", "<p>Bye</p>").unwrap();
        env.add_global("brand", "Agra");
        env.add_function("clock", || "IST");
        env
    }

    #[test]
    fn the_fingerprint_names_every_change() {
        let mut env = base();
        let before = fingerprint(&env);
        assert!(
            before.changes(&fingerprint(&env)).is_empty(),
            "nothing changed"
        );

        env.add_template("mail/new.html", "<p>New</p>").unwrap();
        env.add_template("mail/welcome.html", "<p>Hello</p>")
            .unwrap();
        env.remove_template("mail/bye.html");
        env.add_filter("shout", |text: &str| text.to_uppercase());
        env.add_filter("upper", |text: &str| text.to_lowercase());
        env.add_test("big", |a: i64| a > 10);
        env.add_global("brand", "Pune");
        env.remove_global("clock");
        env.add_function("greet", |name: &str| format!("Sat Sri Akal, {name}"));
        env.set_loader(|_| Ok(None));
        env.set_auto_escape_callback(|_| AutoEscape::None);
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        let mut limits = env.limits();
        limits.max_depth = 5;
        env.set_limits(limits);
        env.set_trim_blocks(true);
        assert_eq!(
            before.changes(&fingerprint(&env)),
            [
                "removes the template `mail/bye.html`",
                "adds the template `mail/new.html`",
                "replaces the template `mail/welcome.html`",
                "adds the filter `shout`",
                "replaces the filter `upper`",
                "adds the test `big`",
                "replaces the global `brand`",
                "removes the function `clock`",
                "adds the function `greet`",
                "changes the template loader",
                "changes how templates escape",
                "changes what an undefined value does",
                "changes the rendering limits",
                "changes the whitespace settings",
            ]
        );
    }

    #[test]
    fn a_removed_filter_added_again_is_a_replacement_not_the_same_filter() {
        let mut env = base();
        env.add_filter("shout", |text: &str| text.to_uppercase());
        let before = fingerprint(&env);
        env.remove_filter("shout");
        env.add_filter("shout", |text: &str| text.to_uppercase());
        assert_eq!(
            before.changes(&fingerprint(&env)),
            ["replaces the filter `shout`"]
        );
    }

    #[test]
    fn a_global_put_back_marked_safe_or_built_again_is_a_replacement() {
        let mut env = base();
        env.add_global("count", 7);
        env.add_global("ratio", 0.5);
        env.add_global("tags", vec!["a", "b"]);
        let before = fingerprint(&env);
        // Nothing touched, and a scalar set to the same scalar: no change.
        env.add_global("count", 7);
        env.add_global("ratio", 0.5);
        assert!(before.changes(&fingerprint(&env)).is_empty());

        // The same text put back marked safe equals the plain one, and is not it.
        env.add_global("brand", Value::from_safe_string("Agra".to_owned()));
        assert_eq!(
            before.changes(&fingerprint(&env)),
            ["replaces the global `brand`"]
        );
        assert_eq!(
            before.changes_outside(&fingerprint(&env), "announcements"),
            ["replaces the global `brand`"]
        );

        // Equal contents built again are another value: text, a list, a function.
        let mut env = base();
        env.add_global("tags", vec!["a", "b"]);
        let before = fingerprint(&env);
        env.add_global("tags", vec!["a", "b"]);
        env.add_function("clock", || "IST");
        assert_eq!(
            before.changes(&fingerprint(&env)),
            ["replaces the function `clock`", "replaces the global `tags`"]
        );

        // A safe string left alone is no change; the same text put back plain is a replacement.
        let mut env = base();
        env.add_global("note", Value::from_safe_string("<b>hi</b>".to_owned()));
        let before = fingerprint(&env);
        assert!(before.changes(&fingerprint(&env)).is_empty());
        env.add_global("note", "<b>hi</b>");
        assert_eq!(
            before.changes(&fingerprint(&env)),
            ["replaces the global `note`"]
        );
    }

    #[test]
    fn names_under_a_namespace_are_not_changes_outside_it() {
        let mut env = base();
        env.add_filter("announcements_badge", |text: &str| format!("[{text}]"));
        env.add_global("announcements_brand", "Ludhiana");
        let before = fingerprint(&env);
        // Its own names, replaced, removed and added: nothing outside its namespace.
        env.add_filter("announcements_badge", |text: &str| format!("({text})"));
        env.remove_global("announcements_brand");
        env.add_function("announcements_clock", || "IST");
        assert!(
            before
                .changes_outside(&fingerprint(&env), "announcements")
                .is_empty()
        );
        assert_eq!(before.changes(&fingerprint(&env)).len(), 3);
        // Another name, a name with the prefix but no underscore, a replaced built-in and a
        // template are outside it.
        env.add_filter("shout", |text: &str| text.to_uppercase());
        env.add_filter("upper", |text: &str| text.to_lowercase());
        env.add_test("announcementsbig", |a: i64| a > 10);
        env.add_template("x.html", "<p>x</p>").unwrap();
        assert_eq!(
            before.changes_outside(&fingerprint(&env), "announcements"),
            [
                "adds the template `x.html`",
                "adds the filter `shout`, which does not start with `announcements_`",
                "replaces the filter `upper`",
                "adds the test `announcementsbig`, which does not start with `announcements_`",
            ]
        );
    }
}
