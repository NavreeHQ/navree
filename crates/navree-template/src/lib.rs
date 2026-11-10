//! # navree-template
//!
//! Navree's template engine: the Jinja syntax with autoescaping by mode, a
//! value model that gives templates data and nothing else, and caps on
//! everything a render can use.
//!
//! Items hidden from this documentation are not public API: Navree's own
//! macros and crates use them, and they can change in any release.
//!
//! ```
//! use navree_template::Environment;
//! use serde_json::json;
//!
//! let mut env = Environment::new();
//! env.add_template("layout.html", "<main>{% block body %}{% endblock %}</main>").unwrap();
//! env.add_template(
//!     "hello.html",
//!     r#"{% extends "layout.html" %}{% block body %}Hello {{ name }}!{% endblock %}"#,
//! )
//! .unwrap();
//!
//! let html = env
//!     .get_template("hello.html")
//!     .unwrap()
//!     .render(json!({ "name": "<Anu>" }))
//!     .unwrap();
//! assert_eq!(html, "<main>Hello &lt;Anu&gt;!</main>");
//! ```
//!
//! ## Syntax
//!
//! - `{{ expr }}` prints, `{% stmt %}` runs a statement, `{# .. #}` is a
//!   comment; a `-` just inside a delimiter removes the whitespace on that
//!   side, a `+` keeps it. [`Environment::set_trim_blocks`],
//!   [`set_lstrip_blocks`](Environment::set_lstrip_blocks) and
//!   [`set_keep_trailing_newline`](Environment::set_keep_trailing_newline)
//!   set the whitespace rules.
//! - Statements: `if`/`elif`/`else`, `for`/`else` with `loop`, `break` and
//!   `continue` (and a loop filter, `for x in xs if x.ok`), `set` (also as a
//!   block), `with`, `block` with `super()`, `extends`, `include` (`ignore missing`),
//!   `import ... as`, `from ... import`, `macro`, `call` (with arguments),
//!   `filter`, `autoescape`, `do`, `raw`.
//! - Expressions: literals (strings, numbers, `true`, `false`, `none`,
//!   lists, tuples, dicts), names, `a.b`, `a[b]`, `a[b:c:d]`, calls with
//!   positional and keyword arguments (`*args`, `**kwargs`), `|` filters,
//!   `is` tests, `and`, `or`, `not`, comparisons (chained), `in`, `not in`,
//!   `+`, `-`, `*`, `/`, `//`, `%`, `**`, `~`, and `a if b else c`.
//! - Filters: `abs`, `capitalize`, `count`, `d`, `default`, `e`, `escape`,
//!   `first`, `int`, `items`, `join`, `last`, `length`, `list`, `lower`,
//!   `min`, `replace`, `round`, `safe`, `split`, `string`, `tojson`, `trim`,
//!   `upper`, `urlencode`.
//! - Tests: `boolean`, `defined`, `divisibleby`, `endingwith`, `eq`,
//!   `equalto`, `escaped`, `even`, `false`, `filter`, `float`, `ge`, `gt`,
//!   `greaterthan`, `in`, `int`, `integer`, `iterable`, `le`, `lessthan`,
//!   `lower`, `lt`, `mapping`, `ne`, `none`, `number`, `odd`, `safe`,
//!   `sameas`, `sequence`, `startingwith`, `string`, `test`, `true`,
//!   `undefined`, `upper`.
//! - Functions: `range`, `dict`. Add your own with
//!   [`Environment::add_function`], `add_filter`, `add_test` and
//!   `add_global`.
//!
//! ## Escaping
//!
//! A template's name decides its mode, in any case and also before a trailing
//! `.j2`, `.jinja` or `.jinja2`: `.html`, `.htm`, `.xhtml`, `.xml` and `.svg`
//! escape `&`, `<`, `>`, `"`, `'` and `/` in every printed value; `.json`,
//! `.json5`, `.js`, `.mjs`, `.cjs`, `.yaml` and `.yml` print values as JSON;
//! anything else prints values as they are ([`default_auto_escape`]).
//! [`strict_auto_escape`] is the rule for names of unknown origin, with HTML
//! for anything not known to be text or JSON, and
//! [`Environment::set_auto_escape_callback`] sets a rule.
//!
//! The mode belongs to the text where it is written, and can only get stricter:
//!
//! - A template rendered inside another (`extends`, `include`, a block) prints
//!   in its own mode and at least as strictly as the text it lands in. JSON
//!   landing in HTML is fit for both: `<`, `>`, `&` and `'` in its strings are
//!   `\u` escapes.
//! - `{% autoescape %}` reaches only the text inside it, and takes `true`,
//!   `false`, `"html"`, `"json"` or `"none"`, checked when it compiles.
//! - What macros, call blocks, `super()` and `{% set %}` blocks return is safe
//!   for the mode it was escaped in; printed in another, it is escaped again.
//! - Text marked with `|safe` prints as it is in every mode.
//! - `|tojson` is safe wherever it is printed: its quotes stay quotes inside a
//!   `<script>` and print as `&quot;` anywhere else in HTML, and wherever the
//!   engine can't be sure a script is one: in captured text (a macro, a set
//!   block), and after `<noscript>`, `<math>`, `<![CDATA[` or an `<svg>` holding
//!   more than plain shapes.
//!
//! ## The sandbox
//!
//! A template sees the values it was given: the context as it serialized,
//! the environment's globals, and its own variables. Attributes are keys of
//! maps. There are no methods on values, no reflection, and no access to
//! files other than templates by name, through the environment's own table
//! or loader.
//!
//! ## Limits
//!
//! Every render is capped by [`Limits`]: how deep macro calls and includes
//! nest, how much stack it uses, how many loop iterations and operations it
//! runs, how large its output and the strings and sequences it builds get,
//! and how much it allocates in total. Nesting in the
//! template's source is capped when it compiles, and values nest at most
//! 200 deep. A template that exceeds a limit fails with
//! [`ErrorKind::LimitExceeded`] naming the template and the line.
//!
//! Each allocation is charged before it happens, and each scan and copy by its
//! size, so the caps hold whatever a template or its values are. The context,
//! compiled templates and what registered functions do are not counted.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod builtins;
mod environment;
mod error;
mod escape;
pub mod functions;
mod output;
mod stack;
pub(crate) mod syntax;
pub mod value;
mod vm;

pub use environment::{
    AutoEscape, Environment, Limits, Template, TemplateSource, UndefinedBehavior,
    default_auto_escape, strict_auto_escape,
};
pub use error::{Error, ErrorKind};
pub use value::{Kwargs, Value, ValueKind};
