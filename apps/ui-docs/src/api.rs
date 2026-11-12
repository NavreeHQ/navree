//! The props tables, read from the toolkit's source with `syn`: each type's doc
//! comment, its constructors, its builder methods and, for an enum, its
//! variants. Nothing here is written by hand, so the tables can't drift from
//! the code.

use std::collections::BTreeMap;

use quote::ToTokens;
use serde::Serialize;
use syn::{Attribute, Expr, FnArg, ImplItem, Item, Lit, Meta, Type, Visibility};

/// The source files whose public types the docs describe, relative to
/// `crates/navree-ui/src`.
pub const FILES: &[&str] = &[
    "components/mod.rs",
    "components/actions.rs",
    "components/inputs.rs",
    "components/input_otp.rs",
    "components/carousel.rs",
    "components/display.rs",
    "components/activity.rs",
    "components/timeline.rs",
    "components/fact.rs",
    "components/tone.rs",
    "components/overlays.rs",
    "components/layout.rs",
    "components/flag.rs",
    "components/color_picker.rs",
    "components/trace.rs",
    "components/questionnaire.rs",
    "icons/mod.rs",
    "flags/mod.rs",
    "components/tabs.rs",
    "components/toggle_group.rs",
    "components/dropdown.rs",
    "components/context_menu.rs",
    "components/menubar.rs",
    "components/navigation_menu.rs",
    "components/toast.rs",
    "flow/stepper.rs",
    "flow/wizard.rs",
    "flow/draft.rs",
    "flow/slider.rs",
    "flow/number.rs",
    "flow/tree.rs",
    "flow/toolbar.rs",
    "components/split.rs",
    "components/lazy.rs",
    "shell.rs",
    "form.rs",
    "dates/calendar.rs",
    "dates/picker.rs",
    "dates/time_picker.rs",
    "dates/datetime_picker.rs",
    "dates/locale.rs",
    "dates/range.rs",
    "dates/rules.rs",
    "dates/time.rs",
    "phone.rs",
    "table/view.rs",
    "table/region.rs",
    "select/choice.rs",
    "select/combobox.rs",
    "select/command.rs",
    "select/labels.rs",
    "select/multi.rs",
    "select/picker.rs",
    "select/query.rs",
    "select/single.rs",
    "sortable/mod.rs",
    "sortable/moves.rs",
    "sortable/ranked.rs",
    "cmd.rs",
    "op.rs",
    "theme/mod.rs",
    "theme/colour.rs",
    "theme/appearance.rs",
    "theme/menu.rs",
    "theme/builder.rs",
    "pages/state.rs",
    "pages/labels.rs",
    "pages/options.rs",
    "pages/render.rs",
    "pages/return_to.rs",
    "markdown/mod.rs",
    "upload/component.rs",
    "upload/labels.rs",
    "upload/limits.rs",
    "upload/server.rs",
    "charts/mod.rs",
    "charts/cartesian.rs",
    "charts/donut.rs",
    "charts/funnel.rs",
    "charts/sparkline.rs",
    "charts/radial/mod.rs",
    "charts/radial/ring.rs",
    "charts/radial/gauge.rs",
    "charts/radial/clock.rs",
    "charts/format.rs",
    "charts/scale.rs",
    "charts/report.rs",
    "charts/report_orm.rs",
    "charts/report_export.rs",
    "genui/chat.rs",
    "genui/frame.rs",
];

/// One constructor or method.
#[derive(Clone, Debug, Serialize)]
pub struct Method {
    pub name: String,
    /// `<N: Into<Node>>`, or empty.
    pub generics: String,
    /// The arguments after `self`, as written: `label: impl Into<Node>`.
    pub args: String,
    /// The first paragraph of its doc comment.
    pub doc: String,
}

/// One variant of an enum.
#[derive(Clone, Debug, Serialize)]
pub struct Variant {
    pub name: String,
    pub doc: String,
    /// `#[default]`.
    pub default: bool,
}

/// A type's table.
#[derive(Clone, Debug, Serialize)]
pub struct TypeDoc {
    pub name: String,
    /// `struct` or `enum`.
    pub kind: &'static str,
    /// Where it's defined, from the repository's root.
    pub file: String,
    /// The first paragraph of its doc comment.
    pub summary: String,
    pub constructors: Vec<Method>,
    pub methods: Vec<Method>,
    pub variants: Vec<Variant>,
    /// `id`, `attr` and `span`, which every component with a root element has.
    pub shared: Vec<Method>,
}

/// Every public type in [`FILES`], by name.
pub fn read(source: impl Fn(&str) -> String) -> BTreeMap<String, TypeDoc> {
    let mut types: BTreeMap<String, TypeDoc> = BTreeMap::new();
    let mut impls: Vec<(String, Vec<Method>, Vec<Method>)> = Vec::new();
    let mut rooted: Vec<String> = Vec::new();
    let mut shared: Vec<Method> = Vec::new();
    let mut macros: BTreeMap<String, String> = BTreeMap::new();
    let mut users: Vec<(String, String)> = Vec::new();
    for file in FILES {
        let text = source(file);
        let path = format!("crates/navree-ui/src/{file}");
        if *file == "components/mod.rs" {
            shared = root_methods(&text);
        }
        let parsed = syn::parse_file(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
        for item in parsed.items {
            match item {
                Item::Struct(item) if public(&item.vis) => {
                    types.insert(
                        item.ident.to_string(),
                        TypeDoc::new(item.ident.to_string(), "struct", &path, &item.attrs),
                    );
                }
                Item::Enum(item) if public(&item.vis) => {
                    let mut doc = TypeDoc::new(item.ident.to_string(), "enum", &path, &item.attrs);
                    doc.variants = item
                        .variants
                        .iter()
                        .map(|variant| Variant {
                            name: variant.ident.to_string(),
                            doc: summary(&variant.attrs),
                            default: variant.attrs.iter().any(|a| a.path().is_ident("default")),
                        })
                        .collect();
                    types.insert(item.ident.to_string(), doc);
                }
                Item::Impl(item) if item.trait_.is_none() => {
                    let Some(name) = type_name(&item.self_ty) else {
                        continue;
                    };
                    let (constructors, methods) = impl_methods(&item);
                    impls.push((name, constructors, methods));
                }
                // The builder methods several types share are written once, in a macro:
                // its definition is read, with the type put in, for each type that uses it.
                Item::Macro(item)
                    if item.mac.path.is_ident("macro_rules")
                        && item.ident.as_ref().is_some_and(|name| {
                            SHARED_MACROS.contains(&name.to_string().as_str())
                        }) =>
                {
                    if let Some(name) = &item.ident {
                        macros.insert(name.to_string(), item.mac.tokens.to_string());
                    }
                }
                Item::Macro(item)
                    if SHARED_MACROS
                        .iter()
                        .any(|name| item.mac.path.is_ident(name)) =>
                {
                    let kind = item
                        .mac
                        .path
                        .get_ident()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    users.push((kind, item.mac.tokens.to_string().trim().to_owned()));
                }
                Item::Macro(item) if item.mac.path.is_ident("root_methods") => {
                    rooted.extend(
                        item.mac
                            .tokens
                            .to_string()
                            .split(',')
                            .map(|name| name.trim().to_owned())
                            .filter(|name| !name.is_empty()),
                    );
                }
                _ => {}
            }
        }
    }
    for (kind, name) in users {
        let Some(definition) = macros.get(&kind) else {
            continue;
        };
        if let Some(item) = shared_impl(definition, &name) {
            let (constructors, methods) = impl_methods(&item);
            impls.push((name, constructors, methods));
        }
    }
    for (name, constructors, methods) in impls {
        if let Some(doc) = types.get_mut(&name) {
            doc.constructors.extend(constructors);
            doc.methods.extend(methods);
        }
    }
    for name in rooted {
        if let Some(doc) = types.get_mut(&name) {
            doc.shared.clone_from(&shared);
        }
    }
    types
}

/// The macros that write builder methods for several types.
const SHARED_MACROS: &[&str] = &[
    "picker_methods",
    "typed_methods",
    "common_methods",
    "radial_methods",
];

/// The public methods of an `impl` block: constructors (no `self`) and builders.
fn impl_methods(item: &syn::ItemImpl) -> (Vec<Method>, Vec<Method>) {
    let mut constructors = Vec::new();
    let mut methods = Vec::new();
    for item in &item.items {
        let ImplItem::Fn(function) = item else {
            continue;
        };
        if !public(&function.vis) || hidden(&function.attrs) {
            continue;
        }
        let method = Method {
            name: function.sig.ident.to_string(),
            generics: if function.sig.generics.params.is_empty() {
                String::new()
            } else {
                tidy(&function.sig.generics.to_token_stream().to_string())
            },
            args: function
                .sig
                .inputs
                .iter()
                .filter_map(|arg| match arg {
                    FnArg::Typed(arg) => Some(format!(
                        "{}: {}",
                        tidy(&arg.pat.to_token_stream().to_string()),
                        tidy(&arg.ty.to_token_stream().to_string())
                    )),
                    FnArg::Receiver(_) => None,
                })
                .collect::<Vec<_>>()
                .join(", "),
            doc: summary(&function.attrs),
        };
        if function.sig.receiver().is_some() {
            methods.push(method);
        } else {
            constructors.push(method);
        }
    }
    (constructors, methods)
}

/// The `impl` a `macro_rules!` writes for the type `name`: the macro's one arm, with
/// `$type` and `$crate` put in.
fn shared_impl(definition: &str, name: &str) -> Option<syn::ItemImpl> {
    let body = definition.split_once("=>")?.1;
    let open = body.find('{')?;
    let close = body.rfind('}')?;
    let inner = &body[open + 1..close];
    // Token text puts a space after `$` and around `::`. A macro that writes its
    // `impl` once for each type it is given (`$(impl $type { .. })+`) is read as
    // the one `impl` inside the repetition.
    let spaced = inner.replace("$ ", "$").replace(" :: ", "::");
    let text = match spaced.trim().strip_prefix("$(") {
        Some(repeated) => repeated
            .trim_end()
            .strip_suffix('+')?
            .trim_end()
            .strip_suffix(')')?
            .replace("$type", name)
            .replace("$crate::charts::", "")
            .replace("$crate::components::", "")
            .replace("$crate::icons::", "")
            .replace("$crate::dates::", "")
            .replace("$crate::node::", "")
            .replace("$crate::html::", "")
            .replace("$crate", "crate"),
        // The paths inside the crate are shown as the name a reader writes. This is the
        // spaced text, as the repeated form's is: the token text has a space in `$ type`.
        None => spaced
            .replace("$type", name)
            .replace("$crate::components::", "")
            .replace("$crate::select::", "")
            .replace("$crate::node::", "")
            .replace("$crate::html::", "")
            .replace("$crate", "crate"),
    };
    let file = syn::parse_file(&text).ok()?;
    file.items.into_iter().find_map(|item| match item {
        Item::Impl(item) => Some(item),
        _ => None,
    })
}

impl TypeDoc {
    fn new(name: String, kind: &'static str, file: &str, attrs: &[Attribute]) -> Self {
        Self {
            name,
            kind,
            file: file.to_owned(),
            summary: summary(attrs),
            constructors: Vec::new(),
            methods: Vec::new(),
            variants: Vec::new(),
            shared: Vec::new(),
        }
    }
}

fn public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

/// `#[doc(hidden)]`.
fn hidden(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("doc")
            && matches!(&attr.meta, Meta::List(list) if list.tokens.to_string().contains("hidden"))
    })
}

/// `Button` for `Button`, `DataTable` for `DataTable<'a, R, F>`.
fn type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(path) => path.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// The doc comment's lines.
fn doc_lines(attrs: &[Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            Meta::NameValue(pair) => match &pair.value {
                Expr::Lit(lit) => match &lit.lit {
                    Lit::Str(text) => Some(text.value()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// The first paragraph of the doc comment, on one line, with rustdoc's links
/// turned into plain code.
fn summary(attrs: &[Attribute]) -> String {
    paragraph(&doc_lines(attrs))
}

fn paragraph(lines: &[String]) -> String {
    let words: Vec<&str> = lines
        .iter()
        .map(|line| line.trim())
        .take_while(|line| !line.is_empty() && !line.starts_with("```"))
        .collect();
    unlink(&words.join(" "))
}

/// `[`Stack`]` → `` `Stack` ``; `[`href`](Self::href)` → `` `href` ``;
/// `[text](url)` → `text`. Brackets inside code (`#[col(skip)]`) stay.
pub fn unlink(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_code = false;
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        if c == '`' {
            in_code = !in_code;
        }
        if c == '['
            && !in_code
            && let Some(close) = (at + 1..chars.len()).find(|i| chars[*i] == ']')
        {
            let inner: String = chars[at + 1..close].iter().collect();
            out.push_str(&inner);
            at = close + 1;
            if chars.get(at) == Some(&'(')
                && let Some(end) = (at..chars.len()).find(|i| chars[*i] == ')')
            {
                at = end + 1;
            }
            continue;
        }
        out.push(c);
        at += 1;
    }
    out.replace("`super::", "`").replace("`crate::", "`")
}

/// Token text as a person writes it: `impl Into<Node>`, not `impl Into < Node >`.
pub fn tidy(tokens: &str) -> String {
    let mut text = tokens.to_owned();
    for (from, to) in [
        (" < ", "<"),
        ("< ", "<"),
        (" <", "<"),
        (" >", ">"),
        (" ,", ","),
        (" :: ", "::"),
        (":: ", "::"),
        (" ::", "::"),
        ("& ", "&"),
        (" (", "("),
        ("( ", "("),
        (" )", ")"),
        ("[ ", "["),
        (" ]", "]"),
        (" ;", ";"),
        ("' ", "'"),
        ("- >", "->"),
        (" : ", ": "),
    ] {
        while text.contains(from) {
            text = text.replace(from, to);
        }
    }
    // `impl Fn(&R) -> N`: the arrow keeps its spaces.
    text.replace(")->", ") ->")
        .replace("->", " -> ")
        .replace("  ", " ")
}

/// The methods `root_methods!` adds, read from its definition in
/// `components/mod.rs`.
fn root_methods(text: &str) -> Vec<Method> {
    let Some(start) = text.find("macro_rules! root_methods") else {
        return Vec::new();
    };
    let mut methods = Vec::new();
    let mut docs: Vec<String> = Vec::new();
    for line in text[start..].lines().skip(1) {
        let line = line.trim();
        if line.starts_with("macro_rules!") || line == "pub(crate) use root_methods;" {
            break;
        }
        if let Some(doc) = line.strip_prefix("///") {
            docs.push(doc.trim().to_owned());
        } else if let Some(signature) = line.strip_prefix("pub fn ") {
            let name = signature.split('(').next().unwrap_or_default().to_owned();
            let args = signature
                .split_once('(')
                .and_then(|(_, rest)| rest.rsplit_once(')'))
                .map(|(args, _)| args)
                .unwrap_or_default()
                .trim_start_matches("mut self")
                .trim_start_matches(',')
                .trim()
                .replace("$crate::node::", "");
            methods.push(Method {
                name,
                generics: String::new(),
                args,
                doc: paragraph(&docs),
            });
            docs.clear();
        } else if !line.starts_with('#') {
            docs.clear();
        }
    }
    methods
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_read_as_written() {
        assert_eq!(tidy("impl Into < Node >"), "impl Into<Node>");
        assert_eq!(tidy("impl Fn (& R) -> N + 'a"), "impl Fn(&R) -> N + 'a");
        assert_eq!(tidy("& 'static str"), "&'static str");
        assert_eq!(
            tidy("impl Into < crate :: node :: AttrValue >"),
            "impl Into<crate::node::AttrValue>"
        );
        assert_eq!(tidy("< N : Into < Node > >"), "<N: Into<Node>>");
    }

    #[test]
    fn every_chart_has_the_builders_all_charts_share() {
        let api = read(|file| crate::read_repo(&format!("crates/navree-ui/src/{file}")));
        for name in [
            "Chart", "Donut", "Funnel", "Sparkline", "Ring", "Rings", "Gauge", "DayClock",
        ] {
            let doc = &api[name];
            for method in [
                "id", "heading", "locale", "table", "loading", "failed", "retry",
            ] {
                assert!(
                    doc.methods.iter().any(|m| m.name == method),
                    "{name} has no `{method}`"
                );
            }
            let locale = doc.methods.iter().find(|m| m.name == "locale");
            assert_eq!(locale.map(|m| m.args.as_str()), Some("tag: &str"));
            let table = doc.methods.iter().find(|m| m.name == "table");
            assert_eq!(table.map(|m| m.args.as_str()), Some("mode: TableMode"));
        }
    }

    #[test]
    fn links_become_code() {
        assert_eq!(
            unlink("Layout belongs to [`Stack`] and [`href`](Self::href)."),
            "Layout belongs to `Stack` and `href`."
        );
        assert_eq!(
            unlink("`#[col(skip)]` and [`super::EditCell`]"),
            "`#[col(skip)]` and `EditCell`"
        );
    }
}
