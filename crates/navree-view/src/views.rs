//! [`Views`]: the template registry and renderer.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use navree_template::{Environment, ErrorKind, Template, UndefinedBehavior};
use serde::Serialize;

use crate::source::{Sources, is_template_name};
use crate::{ViewError, ViewOptions, filters};

/// The template registry: renders templates by name.
///
/// Inject it (`Arc<Views>`, exported by [`ViewModule`](crate::ViewModule))
/// or build one directly with [`Views::new`], [`Views::with_templates`] or
/// [`Views::from_templates`].
///
/// **Names** are `/`-separated paths relative to a template directory, such
/// as `mail/welcome.html`. A name without a template extension tries
/// `.html`, `.txt` and `.j2` in that order, then the bare name, so
/// `render("mail/welcome", ..)` finds `mail/welcome.html`. Template files
/// have one of the extensions `html`, `htm`, `xml`, `txt`, `md`, `j2` or
/// `jinja`; hidden files and directories are ignored, and a name cannot
/// leave its directory (`..`, absolute paths and backslashes find nothing).
/// `{% extends %}`, `{% include %}` and `{% import %}` take full names
/// (`"layouts/mail.html"`).
///
/// **Precedence**: the first directory holding a name wins, and any file
/// wins over an embedded template; among embedded templates the last one
/// given wins.
///
/// **Escaping**: `.html`, `.htm` and `.xml` templates (also with a trailing
/// `.j2`) escape HTML in every printed value; mark trusted HTML with
/// `|safe`. Other templates print values as they are.
///
/// **Undefined values**: printing, iterating or filtering an undefined
/// variable fails the render; testing one (`{% if title %}`) is false.
///
/// **Reloading**: without [`ViewOptions::reload`] every template is read and
/// compiled when `Views` is built, and rendering never touches the disk.
/// With it, templates are still compiled up front (so syntax errors surface
/// early) and each lookup checks the file's modification time and size,
/// picking up changed, new and deleted files; that is blocking file-system
/// I/O, meant for development.
pub struct Views {
    env: Environment,
    sources: Arc<Sources>,
    reload: bool,
}

impl Views {
    /// The templates in `options.dirs`. Reads (and, without reloading,
    /// compiles) every template file now: call it at startup or on a
    /// blocking thread. A directory that does not exist is skipped; an
    /// unreadable one, or a template with a syntax error, is an error.
    pub fn new(options: &ViewOptions) -> Result<Views, ViewError> {
        Self::with_templates(options, std::iter::empty::<(String, String)>())
    }

    /// The templates in `options.dirs` plus the embedded `templates`
    /// (name, source pairs; a later pair replaces an earlier one with the
    /// same name). A file overrides an embedded template of the same name.
    /// Reads the directories like [`Views::new`].
    pub fn with_templates<I, N, S>(options: &ViewOptions, templates: I) -> Result<Views, ViewError>
    where
        I: IntoIterator<Item = (N, S)>,
        N: Into<String>,
        S: Into<String>,
    {
        let embedded: BTreeMap<String, String> = templates
            .into_iter()
            .map(|(name, source)| (name.into(), source.into()))
            .collect();
        let sources = Arc::new(Sources::new(options.dirs.clone(), embedded));
        let mut env = environment();
        if options.reload {
            let loader = sources.clone();
            env.set_loader(move |name| loader.load(name));
            // Compile everything now anyway, so syntax errors fail early.
            for name in sources.names()? {
                env.get_template(&name)
                    .map_err(|source| ViewError::Template { name, source })?;
            }
        } else {
            for (name, source) in sources.read_all()? {
                env.add_template(name.clone(), source)
                    .map_err(|source| ViewError::Template { name, source })?;
            }
        }
        Ok(Views {
            env,
            sources,
            reload: options.reload,
        })
    }

    /// Only the given `templates` (name, source pairs), no directories and
    /// no file-system access: handy in tests.
    pub fn from_templates<I, N, S>(templates: I) -> Result<Views, ViewError>
    where
        I: IntoIterator<Item = (N, S)>,
        N: Into<String>,
        S: Into<String>,
    {
        let options = ViewOptions {
            dirs: Vec::new(),
            reload: false,
        };
        Self::with_templates(&options, templates)
    }

    /// Renders template `name` (see the [naming rules](Views)) with `ctx`,
    /// any `Serialize` value whose fields become the template's variables
    /// (a struct, a map or `serde_json::json!({ .. })`).
    ///
    /// Errors: [`ViewError::NotFound`] lists the names tried;
    /// [`ViewError::Template`] names the template that failed to load or
    /// render. Neither includes context values.
    pub fn render(&self, name: &str, ctx: impl Serialize) -> Result<String, ViewError> {
        let (resolved, template) = self.find(name)?;
        template.render(ctx).map_err(|source| ViewError::Template {
            name: resolved,
            source,
        })
    }

    /// Renders `source`, a template kept as text (in a database, in
    /// settings) rather than in a file, with `ctx`. `name` stands for the
    /// file it would be: its extension picks the escaping (`override.html`
    /// escapes HTML, `subject.txt` does not) and errors name it. The
    /// template sees the same environment as files: the filters, the strict
    /// undefined rule, and `{% extends %}` or `{% include %}` of the
    /// registered templates (`{% extends "layouts/mail.html" %}`).
    ///
    /// ```
    /// use navree_view::Views;
    /// use serde_json::json;
    ///
    /// let views = Views::from_templates([("layouts/mail.html", "<main>{% block body %}{% endblock %}</main>")]).unwrap();
    /// let html = views
    ///     .render_source(
    ///         "welcome.html",
    ///         r#"{% extends "layouts/mail.html" %}{% block body %}Hi {{ name }}{% endblock %}"#,
    ///         json!({ "name": "Ada" }),
    ///     )
    ///     .unwrap();
    /// assert_eq!(html, "<main>Hi Ada</main>");
    /// ```
    pub fn render_source(
        &self,
        name: &str,
        source: &str,
        ctx: impl Serialize,
    ) -> Result<String, ViewError> {
        self.env
            .render_str(name, source, ctx)
            .map_err(|source| ViewError::Template {
                name: name.to_owned(),
                source,
            })
    }

    /// Whether `name` resolves to a template, following the same rules as
    /// [`render`](Views::render) (for example to send a plain-text part only
    /// when `mail/welcome.txt` exists).
    pub fn exists(&self, name: &str) -> bool {
        !matches!(self.find(name), Err(ViewError::NotFound { .. }))
    }

    /// Changes the template environment: adds functions, filters or
    /// globals, or replaces the built-in `money` and `date` filters. Call it
    /// before rendering (modules use
    /// [`ModuleBuilderViewExt::view_environment`](crate::ModuleBuilderViewExt::view_environment),
    /// applied when `ViewModule` builds `Views`).
    ///
    /// ```
    /// use navree_view::Views;
    /// use serde_json::json;
    ///
    /// let mut views = Views::from_templates([("hi.txt", "{{ shout(name) }}")]).unwrap();
    /// views.configure(|env| env.add_function("shout", |name: &str| name.to_uppercase()));
    /// assert_eq!(views.render("hi", json!({ "name": "ada" })).unwrap(), "ADA");
    /// ```
    pub fn configure(&mut self, configure: impl FnOnce(&mut Environment)) {
        configure(&mut self.env);
    }

    /// The first candidate for `name` that exists, with its full name.
    fn find(&self, name: &str) -> Result<(String, Template<'_>), ViewError> {
        let candidates = candidates(name);
        for candidate in &candidates {
            match self.env.get_template(candidate) {
                Ok(template) => return Ok((candidate.clone(), template)),
                Err(error) if error.kind() == ErrorKind::TemplateNotFound => {}
                Err(source) => {
                    return Err(ViewError::Template {
                        name: candidate.clone(),
                        source,
                    });
                }
            }
        }
        Err(ViewError::NotFound {
            name: name.to_owned(),
            tried: candidates,
        })
    }
}

impl fmt::Debug for Views {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Views")
            .field("dirs", &self.sources.dirs())
            .field("reload", &self.reload)
            .finish_non_exhaustive()
    }
}

/// An environment with Navree's settings: printing an undefined value is an
/// error (a typo in a variable name must not send a blank email).
fn environment() -> Environment {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::SemiStrict);
    filters::register(&mut env);
    env
}

/// Extensions tried, in order, for a name without a template extension.
const RESOLUTION_ORDER: [&str; 3] = ["html", "txt", "j2"];

/// The template names `name` may refer to, in lookup order.
fn candidates(name: &str) -> Vec<String> {
    if is_template_name(name) {
        return vec![name.to_owned()];
    }
    RESOLUTION_ORDER
        .iter()
        .map(|extension| format!("{name}.{extension}"))
        .chain(std::iter::once(name.to_owned()))
        .collect()
}
