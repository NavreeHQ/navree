//! [`Environment`]: templates, filters, tests, globals and settings.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;

use crate::builtins;
use crate::error::{Error, ErrorKind};
use crate::escape::Mode;
use crate::functions::{self, Callable, Function, FunctionResult};
use crate::syntax::{Ast, Whitespace, parse};
use crate::value::Value;
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
