//! `#[derive(ConfigSection)]` and `#[derive(ConfigTable)]`: describe the
//! fields of a configuration struct for navree-config.
//!
//! Both derives read the struct's fields, their rustdoc and their `serde`
//! names, and write `navree_config::ConfigTable`: the fields (name, type,
//! documentation, secret, required, environment fallback) and the value of
//! each field as JSON, which is how a section shows its defaults.
//! `ConfigSection` also writes `navree_config::ConfigSection`: the key, the
//! schema, the defaults and, when a `validate` function is named, the check.

use proc_macro_crate::FoundCrate;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{
    Attribute, Data, DeriveInput, Expr, Fields, GenericArgument, Ident, Lit, LitStr, Meta, Path,
    PathArguments, Token, Type,
};

use crate::attr::{Errors, key_name};
use crate::paths::CONFIG;

/// The path to navree-config: the crate itself when the crate being
/// compiled depends on it (also from inside), else the `navree` facade.
pub(crate) fn config_path(span: Span) -> syn::Result<TokenStream> {
    match proc_macro_crate::crate_name("navree-config") {
        Ok(FoundCrate::Itself) => Ok(quote!(::navree_config)),
        Ok(FoundCrate::Name(name)) => {
            let name = format_ident!("{name}");
            Ok(quote!(::#name))
        }
        Err(_) => CONFIG.path(span),
    }
}

/// Expands `#[derive(ConfigSection)]`.
pub(crate) fn expand_section(input: TokenStream) -> TokenStream {
    expand(input, true)
}

/// Expands `#[derive(ConfigTable)]`.
pub(crate) fn expand_table(input: TokenStream) -> TokenStream {
    expand(input, false)
}

fn expand(input: TokenStream, section: bool) -> TokenStream {
    let parsed = match parse(input, section) {
        Ok(parsed) => parsed,
        Err(error) => return error.into_compile_error(),
    };
    match config_path(Span::call_site()) {
        Ok(config) => generate(&parsed, &config),
        Err(error) => error.into_compile_error(),
    }
}

/// How the struct gets its defaults.
enum ContainerDefault {
    /// No `#[serde(default)]`: the struct publishes none.
    None,
    /// `#[serde(default)]`, or `#[config(default)]`: `Default::default()`.
    Trait,
    /// `#[serde(default = "path")]`: the function.
    Function(Path),
}

/// The struct-level `#[config(..)]` arguments of `ConfigSection`.
struct SectionArgs {
    key: LitStr,
    docs: Option<LitStr>,
    validate: Option<Path>,
}

/// What a field's type is, as far as the description goes.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Bool,
    Integer,
    Float,
    String,
    Duration,
    Any,
    List(Box<Kind>),
    Map(Box<Kind>),
    Other(String),
}

/// A field's type, read from its tokens.
#[derive(Clone, Debug)]
struct Shape {
    kind: Kind,
    /// Wrapped in `Option`.
    optional: bool,
    /// `Secret<..>` is in the type.
    secret: bool,
    /// The type, and what it holds, implement `DescribeValue`.
    describable: bool,
}

/// The field-level `#[config(..)]` arguments.
#[derive(Default)]
struct FieldArgs {
    secret: bool,
    public: bool,
    table: bool,
    skip: bool,
    env_fallback: Option<String>,
    one_of: Option<Vec<String>>,
    default: Option<String>,
}

/// The field-level `#[serde(..)]` arguments the description reads.
#[derive(Default)]
struct FieldSerde {
    rename: Option<String>,
    skip: bool,
    flatten: bool,
    default: bool,
}

struct FieldInfo {
    ident: Ident,
    name: String,
    doc: String,
    shape: Shape,
    /// The type with its `Option` taken off.
    base: Type,
    args: FieldArgs,
    flatten: bool,
    required: bool,
}

pub(crate) struct Parsed {
    ident: Ident,
    section: Option<SectionArgs>,
    doc: String,
    default: ContainerDefault,
    fields: Vec<FieldInfo>,
}

/// `#[serde(rename_all = "..")]`.
#[derive(Clone, Copy)]
enum RenameRule {
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    ScreamingSnake,
    Kebab,
    ScreamingKebab,
}

impl RenameRule {
    fn parse(rule: &str) -> Option<Self> {
        Some(match rule {
            "lowercase" => Self::Lower,
            "UPPERCASE" => Self::Upper,
            "PascalCase" => Self::Pascal,
            "camelCase" => Self::Camel,
            "snake_case" => Self::Snake,
            "SCREAMING_SNAKE_CASE" => Self::ScreamingSnake,
            "kebab-case" => Self::Kebab,
            "SCREAMING-KEBAB-CASE" => Self::ScreamingKebab,
            _ => return None,
        })
    }

    /// A field's name (`snake_case` in Rust), as serde renames it.
    fn field(self, name: &str) -> String {
        match self {
            Self::Lower | Self::Snake => name.to_owned(),
            Self::Upper | Self::ScreamingSnake => name.to_ascii_uppercase(),
            Self::Pascal => pascal(name),
            Self::Camel => {
                let pascal = pascal(name);
                let mut chars = pascal.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            }
            Self::Kebab => name.replace('_', "-"),
            Self::ScreamingKebab => name.to_ascii_uppercase().replace('_', "-"),
        }
    }
}

fn pascal(name: &str) -> String {
    name.split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Reads the struct and its attributes.
pub(crate) fn parse(input: TokenStream, section: bool) -> syn::Result<Parsed> {
    let input: DeriveInput = syn::parse2(input)?;
    let derive = if section {
        "ConfigSection"
    } else {
        "ConfigTable"
    };
    let mut errors = Errors::default();
    let named = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => Some(named),
            other => {
                errors.push(syn::Error::new(
                    other.span(),
                    format!("#[derive({derive})] needs a struct with named fields"),
                ));
                None
            }
        },
        _ => {
            errors.push(syn::Error::new(
                input.ident.span(),
                format!("#[derive({derive})] goes on a struct whose fields are the settings"),
            ));
            None
        }
    };
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        errors.push(syn::Error::new_spanned(
            &input.generics,
            format!("#[derive({derive})] cannot describe a generic struct"),
        ));
    }

    let mut rename_all = None;
    let mut default = ContainerDefault::None;
    let mut key = None;
    let mut docs = None;
    let mut validate = None;
    for attr in &input.attrs {
        if attr.path().is_ident("serde") {
            errors.keep(container_serde(attr, &mut rename_all, &mut default));
        } else if attr.path().is_ident("config") {
            errors.keep(container_config(
                attr,
                section,
                &mut key,
                &mut docs,
                &mut validate,
                &mut default,
            ));
        }
    }
    let section_args = if section {
        match key {
            Some(key) => Some(SectionArgs {
                key,
                docs,
                validate,
            }),
            None => {
                errors.push(syn::Error::new(
                    input.ident.span(),
                    "name the section: #[config(key = \"..\")]",
                ));
                None
            }
        }
    } else {
        None
    };

    let container_default = !matches!(default, ContainerDefault::None);
    let mut fields = Vec::new();
    if let Some(named) = named {
        for field in &named.named {
            let Some(ident) = field.ident.clone() else {
                continue;
            };
            let serde = match errors.keep(field_serde(&field.attrs)) {
                Some(serde) => serde,
                None => continue,
            };
            let args = match errors.keep(field_config(&field.attrs)) {
                Some(args) => args,
                None => continue,
            };
            if serde.skip || args.skip {
                continue;
            }
            let (base, optional) = strip_option(&field.ty);
            let shape = shape(&field.ty);
            let name = match serde.rename {
                Some(name) => name,
                None => match rename_all {
                    Some(rule) => RenameRule::field(rule, &ident.unraw().to_string()),
                    None => ident.unraw().to_string(),
                },
            };
            if serde.flatten && !args.table {
                // A flattened field has no name of its own and its type is
                // not described: leave it out.
                continue;
            }
            fields.push(FieldInfo {
                name,
                doc: first_paragraph(&field.attrs),
                required: !container_default && !serde.default && !optional,
                shape,
                base: base.clone(),
                args,
                flatten: serde.flatten,
                ident,
            });
        }
    }
    errors.finish()?;
    Ok(Parsed {
        ident: input.ident,
        section: section_args,
        doc: first_paragraph(&input.attrs),
        default,
        fields,
    })
}

/// `#[serde(..)]` on the struct: `rename_all` and `default`.
fn container_serde(
    attr: &Attribute,
    rename_all: &mut Option<RenameRule>,
    default: &mut ContainerDefault,
) -> syn::Result<()> {
    attr.parse_nested_meta(|meta| {
        match key_name(&meta.path).as_str() {
            "rename_all" => {
                if meta.input.peek(Token![=]) {
                    let value: LitStr = meta.value()?.parse()?;
                    *rename_all = RenameRule::parse(&value.value());
                } else {
                    meta.parse_nested_meta(|inner| {
                        let value: LitStr = inner.value()?.parse()?;
                        if inner.path.is_ident("deserialize") {
                            *rename_all = RenameRule::parse(&value.value());
                        }
                        Ok(())
                    })?;
                }
            }
            "default" => {
                if meta.input.peek(Token![=]) {
                    let value: LitStr = meta.value()?.parse()?;
                    *default = ContainerDefault::Function(value.parse()?);
                } else {
                    *default = ContainerDefault::Trait;
                }
            }
            _ => skip_value(&meta)?,
        }
        Ok(())
    })
}

/// `#[config(..)]` on the struct.
fn container_config(
    attr: &Attribute,
    section: bool,
    key: &mut Option<LitStr>,
    docs: &mut Option<LitStr>,
    validate: &mut Option<Path>,
    default: &mut ContainerDefault,
) -> syn::Result<()> {
    attr.parse_nested_meta(|meta| {
        let name = key_name(&meta.path);
        match name.as_str() {
            "key" if section => *key = Some(meta.value()?.parse()?),
            "docs" if section => *docs = Some(meta.value()?.parse()?),
            "validate" if section => *validate = Some(meta.value()?.parse()?),
            "default" => {
                if !matches!(*default, ContainerDefault::Function(_)) {
                    *default = ContainerDefault::Trait;
                }
            }
            _ => {
                let expected = if section {
                    "`key = \"..\"`, `docs = \"..\"`, `validate = path` or `default`"
                } else {
                    "`default`"
                };
                return Err(meta.error(format!(
                    "unknown #[config] key `{name}` on a struct: expected {expected}"
                )));
            }
        }
        Ok(())
    })
}

/// `#[serde(..)]` on a field.
fn field_serde(attrs: &[Attribute]) -> syn::Result<FieldSerde> {
    let mut out = FieldSerde::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        attr.parse_nested_meta(|meta| {
            match key_name(&meta.path).as_str() {
                "rename" => {
                    if meta.input.peek(Token![=]) {
                        let value: LitStr = meta.value()?.parse()?;
                        out.rename = Some(value.value());
                    } else {
                        meta.parse_nested_meta(|inner| {
                            let value: LitStr = inner.value()?.parse()?;
                            if inner.path.is_ident("deserialize") {
                                out.rename = Some(value.value());
                            }
                            Ok(())
                        })?;
                    }
                }
                "skip" | "skip_deserializing" => out.skip = true,
                "flatten" => out.flatten = true,
                "default" => {
                    out.default = true;
                    skip_value(&meta)?;
                }
                _ => skip_value(&meta)?,
            }
            Ok(())
        })?;
    }
    Ok(out)
}

/// `#[config(..)]` on a field.
fn field_config(attrs: &[Attribute]) -> syn::Result<FieldArgs> {
    let mut out = FieldArgs::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("config")) {
        attr.parse_nested_meta(|meta| {
            let name = key_name(&meta.path);
            match name.as_str() {
                "secret" => out.secret = true,
                "public" => out.public = true,
                "table" => out.table = true,
                "skip" => out.skip = true,
                "env_fallback" => {
                    let value: LitStr = meta.value()?.parse()?;
                    out.env_fallback = Some(value.value());
                }
                "one_of" => {
                    let value: LitStr = meta.value()?.parse()?;
                    out.one_of = Some(
                        value
                            .value()
                            .split(',')
                            .map(|item| item.trim().to_owned())
                            .filter(|item| !item.is_empty())
                            .collect(),
                    );
                }
                "default" => {
                    let value: LitStr = meta.value()?.parse()?;
                    out.default = Some(value.value());
                }
                _ => {
                    return Err(meta.error(format!(
                        "unknown #[config] key `{name}` on a field: expected `secret`, \
                         `public`, `table`, `skip`, `env_fallback = \"..\"`, \
                         `one_of = \"a, b\"` or `default = \"..\"`"
                    )));
                }
            }
            Ok(())
        })?;
    }
    if out.secret && out.public {
        return Err(syn::Error::new(
            Span::call_site(),
            "a field cannot be both `secret` and `public`",
        ));
    }
    Ok(out)
}

/// Consumes `= value` or `(..)` after a serde key the derive ignores.
fn skip_value(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    if meta.input.peek(Token![=]) {
        let _: Expr = meta.value()?.parse()?;
    } else if meta.input.peek(syn::token::Paren) {
        let _ = meta.input.step(|cursor| {
            let mut rest = *cursor;
            if let Some((_, _, next)) = rest.group(proc_macro2::Delimiter::Parenthesis) {
                rest = next;
            }
            Ok(((), rest))
        });
    }
    Ok(())
}

/// The first paragraph of the doc comment, on one line.
fn first_paragraph(attrs: &[Attribute]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("doc")) {
        let Meta::NameValue(pair) = &attr.meta else {
            continue;
        };
        let Expr::Lit(lit) = &pair.value else {
            continue;
        };
        let Lit::Str(text) = &lit.lit else {
            continue;
        };
        for line in text.value().lines() {
            lines.push(line.trim().to_owned());
        }
    }
    let mut paragraph: Vec<String> = Vec::new();
    for line in lines {
        if line.starts_with("```") {
            break;
        }
        if line.is_empty() {
            if paragraph.is_empty() {
                continue;
            }
            break;
        }
        paragraph.push(line);
    }
    paragraph.join(" ")
}

/// The type inside `Option<..>`, and whether there was one.
fn strip_option(ty: &Type) -> (&Type, bool) {
    let ty = unwrap_group(ty);
    if let Some((ident, args)) = type_arguments(ty)
        && ident == "Option"
        && let Some(inner) = args.first()
    {
        return (unwrap_group(inner), true);
    }
    (ty, false)
}

fn unwrap_group(ty: &Type) -> &Type {
    match ty {
        Type::Group(group) => unwrap_group(&group.elem),
        Type::Paren(paren) => unwrap_group(&paren.elem),
        other => other,
    }
}

/// The last segment of a path type and its generic type arguments.
fn type_arguments(ty: &Type) -> Option<(&Ident, Vec<&Type>)> {
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let segment = path.path.segments.last()?;
    let args = match &segment.arguments {
        PathArguments::AngleBracketed(arguments) => arguments
            .args
            .iter()
            .filter_map(|argument| match argument {
                GenericArgument::Type(ty) => Some(ty),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    Some((&segment.ident, args))
}

/// Reads a field's type.
fn shape(ty: &Type) -> Shape {
    let ty = unwrap_group(ty);
    let other = |name: &str| Shape {
        kind: Kind::Other(name.to_owned()),
        optional: false,
        secret: false,
        describable: false,
    };
    let Some((ident, args)) = type_arguments(ty) else {
        return other("value");
    };
    let name = ident.to_string();
    let leaf = |kind: Kind, describable: bool| Shape {
        kind,
        optional: false,
        secret: false,
        describable,
    };
    match name.as_str() {
        "Option" => match args.first() {
            Some(inner) => Shape {
                optional: true,
                ..shape(inner)
            },
            None => other(&name),
        },
        "Secret" => match args.first() {
            Some(inner) => Shape {
                secret: true,
                ..shape(inner)
            },
            None => Shape {
                secret: true,
                ..leaf(Kind::String, true)
            },
        },
        "Box" | "Arc" | "Rc" | "Cow" => match args.first() {
            Some(inner) => shape(inner),
            None => other(&name),
        },
        "bool" => leaf(Kind::Bool, true),
        "u8" | "u16" | "u32" | "u64" | "usize" | "i8" | "i16" | "i32" | "i64" | "isize" => {
            leaf(Kind::Integer, true)
        }
        "u128" | "i128" => leaf(Kind::Integer, false),
        "f32" | "f64" => leaf(Kind::Float, true),
        "String" | "str" | "char" | "PathBuf" | "SocketAddr" | "IpAddr" | "Ipv4Addr"
        | "Ipv6Addr" => leaf(Kind::String, true),
        "Duration" => leaf(Kind::Duration, false),
        "Value" => leaf(Kind::Any, false),
        "Vec" | "VecDeque" | "HashSet" | "BTreeSet" => match args.first() {
            Some(inner) => {
                let inner = shape(inner);
                Shape {
                    kind: Kind::List(Box::new(inner.kind)),
                    optional: false,
                    secret: inner.secret,
                    describable: inner.describable,
                }
            }
            None => other(&name),
        },
        "HashMap" | "BTreeMap" => match args.get(1) {
            Some(value) => {
                let inner = shape(value);
                Shape {
                    kind: Kind::Map(Box::new(inner.kind)),
                    optional: false,
                    secret: inner.secret,
                    describable: inner.describable,
                }
            }
            None => other(&name),
        },
        _ => other(&name),
    }
}

/// The expression that builds the `FieldType` of a `Kind`.
fn kind_tokens(kind: &Kind, config: &TokenStream) -> TokenStream {
    match kind {
        Kind::Bool => quote!(#config::FieldType::Bool),
        Kind::Integer => quote!(#config::FieldType::Integer),
        Kind::Float => quote!(#config::FieldType::Float),
        Kind::String => quote!(#config::FieldType::String),
        Kind::Duration => quote!(#config::FieldType::Duration),
        Kind::Any => quote!(#config::FieldType::Any),
        Kind::List(item) => {
            let item = kind_tokens(item, config);
            quote!(#config::FieldType::list(#item))
        }
        Kind::Map(value) => {
            let value = kind_tokens(value, config);
            quote!(#config::FieldType::map(#value))
        }
        Kind::Other(name) => quote!(#config::FieldType::other(#name)),
    }
}

/// The statement that adds a field to `fields`.
fn field_statement(field: &FieldInfo, config: &TokenStream) -> TokenStream {
    if field.flatten {
        // Only a flattened table is described (see `parse`).
        let base = &field.base;
        return quote! {
            fields.extend(<#base as #config::ConfigTable>::fields());
        };
    }
    let name = &field.name;
    let doc = &field.doc;
    let ty = if field.args.table {
        quote!(#config::FieldType::Table)
    } else if let Some(values) = &field.args.one_of {
        quote!(#config::FieldType::choice(&[#(#values),*]))
    } else {
        kind_tokens(&field.shape.kind, config)
    };
    let secret = (field.shape.secret || field.args.secret).then(|| quote!(.secret()));
    let public = field.args.public.then(|| quote!(.public()));
    let required = field.required.then(|| quote!(.required()));
    let env = field
        .args
        .env_fallback
        .as_ref()
        .map(|variable| quote!(.env_fallback(#variable)));
    let nested = field.args.table.then(|| {
        let base = &field.base;
        quote!(.with_fields(<#base as #config::ConfigTable>::fields()))
    });
    quote! {
        fields.push(
            #config::FieldSchema::new(#name, #ty, #doc)
                #secret #public #required #env #nested
        );
    }
}

/// The statement that adds a field's value to `map`.
fn json_statement(field: &FieldInfo, config: &TokenStream) -> TokenStream {
    let ident = &field.ident;
    let value = quote!(#config::__private::Value);
    if field.flatten {
        return quote! {
            if let #value::Object(inner) = #config::ConfigTable::to_json(&self.#ident) {
                map.extend(inner);
            }
        };
    }
    let name = &field.name;
    // A `Secret<..>` describes itself as `null` (a list of them as a list of
    // `null`), so only a plain type marked `#[config(secret)]` is cut here.
    let expr = if field.args.secret {
        quote!(#value::Null)
    } else if let Some(text) = &field.args.default {
        quote!(#value::String(::std::string::String::from(#text)))
    } else if field.args.table {
        if field.shape.optional {
            quote! {
                match &self.#ident {
                    ::core::option::Option::Some(table) => #config::ConfigTable::to_json(table),
                    ::core::option::Option::None => #value::Null,
                }
            }
        } else {
            quote!(#config::ConfigTable::to_json(&self.#ident))
        }
    } else if field.shape.describable {
        quote!(#config::DescribeValue::describe(&self.#ident))
    } else if field.shape.optional {
        quote! {
            match &self.#ident {
                ::core::option::Option::Some(inner) => #config::debug_value(inner),
                ::core::option::Option::None => #value::Null,
            }
        }
    } else {
        quote!(#config::debug_value(&self.#ident))
    };
    quote! {
        map.insert(::std::string::String::from(#name), #expr);
    }
}

/// The impls, naming navree-config as `config`.
pub(crate) fn generate(parsed: &Parsed, config: &TokenStream) -> TokenStream {
    let ident = &parsed.ident;
    let fields = parsed
        .fields
        .iter()
        .map(|field| field_statement(field, config));
    let values = parsed
        .fields
        .iter()
        .map(|field| json_statement(field, config));
    let table = quote! {
        #[automatically_derived]
        impl #config::ConfigTable for #ident {
            fn fields() -> ::std::vec::Vec<#config::FieldSchema> {
                #[allow(unused_mut)]
                let mut fields = ::std::vec::Vec::new();
                #(#fields)*
                fields
            }

            fn to_json(&self) -> #config::__private::Value {
                #[allow(unused_mut)]
                let mut map = #config::__private::Map::new();
                #(#values)*
                #config::__private::Value::Object(map)
            }
        }
    };
    let Some(section) = &parsed.section else {
        return table;
    };
    let key = &section.key;
    let doc = &parsed.doc;
    let docs = section.docs.as_ref().map(|path| quote!(.with_docs(#path)));
    let validate = section.validate.as_ref().map(|path| {
        quote! {
            fn validate(&self) -> ::core::result::Result<(), ::std::string::String> {
                #path(self)
            }
        }
    });
    let defaults = match &parsed.default {
        ContainerDefault::None => quote!(::core::option::Option::None),
        ContainerDefault::Trait => quote! {
            ::core::option::Option::Some(#config::ConfigTable::to_json(
                &<Self as ::core::default::Default>::default(),
            ))
        },
        ContainerDefault::Function(path) => quote! {
            ::core::option::Option::Some(#config::ConfigTable::to_json(&#path()))
        },
    };
    quote! {
        #table

        #[automatically_derived]
        impl #config::ConfigSection for #ident {
            const KEY: &'static str = #key;

            #validate

            fn defaults() -> ::core::option::Option<#config::__private::Value> {
                #defaults
            }

            fn schema() -> ::core::option::Option<#config::SectionSchema> {
                ::core::option::Option::Some(
                    #config::SectionSchema::new(
                        #key,
                        #doc,
                        <Self as #config::ConfigTable>::fields(),
                    )
                    #docs
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text without whitespace, so the checks do not depend on how the
    /// token printer spaces tokens.
    fn squash(text: &str) -> String {
        text.split_whitespace().collect()
    }

    fn has(out: &str, expected: &str) -> bool {
        squash(out).contains(&squash(expected))
    }

    fn expand_with(input: TokenStream, section: bool) -> String {
        match parse(input, section) {
            Ok(parsed) => generate(&parsed, &quote!(::navree_config)).to_string(),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn a_section_gets_its_key_fields_defaults_and_schema() {
        let out = expand_with(
            quote! {
                /// Mail settings.
                ///
                /// More about mail.
                #[derive(Default, Deserialize)]
                #[config(key = "mail", docs = "/docs/mail/", validate = Self::check)]
                #[serde(default, rename_all = "camelCase")]
                pub struct MailOptions {
                    /// The sender.
                    pub from_address: String,
                    /// The API key.
                    pub api_key: Option<Secret<String>>,
                    #[serde(rename = "port")]
                    pub smtp_port: u16,
                    #[config(env_fallback = "REDIS_URL")]
                    pub url: Option<String>,
                }
            },
            true,
        );
        for expected in [
            "impl :: navree_config :: ConfigSection for MailOptions",
            "const KEY : & 'static str = \"mail\"",
            "fn validate (& self) -> :: core :: result :: Result < () , :: std :: string :: String > { Self :: check (self) }",
            ".with_docs (\"/docs/mail/\")",
            "\"Mail settings.\"",
            "FieldSchema :: new (\"fromAddress\" , :: navree_config :: FieldType :: String , \"The sender.\")",
            "FieldSchema :: new (\"apiKey\" , :: navree_config :: FieldType :: String , \"The API key.\") . secret ()",
            "FieldSchema :: new (\"port\" , :: navree_config :: FieldType :: Integer , \"\")",
            ". env_fallback (\"REDIS_URL\")",
            "< Self as :: core :: default :: Default > :: default ()",
        ] {
            assert!(has(&out, expected), "missing `{expected}` in {out}");
        }
        // The container has `serde(default)`: nothing is required.
        assert!(!has(&out, ". required ()"), "{out}");
        // A credential's default is `null`, never its value: a `Secret` field
        // describes itself, a field marked `#[config(secret)]` is cut.
        assert!(
            has(
                &out,
                ":: navree_config :: DescribeValue :: describe (& self . api_key)"
            ),
            "{out}"
        );
    }

    #[test]
    fn without_a_default_the_required_fields_are_marked() {
        let out = expand_with(
            quote! {
                #[config(key = "x")]
                struct X {
                    needed: String,
                    maybe: Option<String>,
                    #[serde(default)]
                    given: bool,
                }
            },
            true,
        );
        assert_eq!(squash(&out).matches(".required()").count(), 1, "{out}");
        assert!(
            has(
                &out,
                "fn defaults () -> :: core :: option :: Option < :: navree_config :: __private :: Value > { :: core :: option :: Option :: None }"
            ),
            "{out}"
        );
    }

    #[test]
    fn types_are_reduced_to_kinds() {
        let out = expand_with(
            quote! {
                #[config(key = "x")]
                #[serde(default)]
                struct X {
                    a: Vec<String>,
                    b: HashMap<String, u32>,
                    c: std::time::Duration,
                    d: CacheStoreKind,
                    e: f64,
                    #[config(one_of = "memory, redis")]
                    f: Mode,
                }
            },
            true,
        );
        for expected in [
            "FieldType :: list (:: navree_config :: FieldType :: String)",
            "FieldType :: map (:: navree_config :: FieldType :: Integer)",
            "FieldType :: Duration",
            "FieldType :: other (\"CacheStoreKind\")",
            "FieldType :: Float",
            "FieldType :: choice (& [\"memory\" , \"redis\"])",
            // Strings and numbers describe themselves; the rest print as `Debug`.
            ":: navree_config :: DescribeValue :: describe (& self . a)",
            ":: navree_config :: debug_value (& self . d)",
            ":: navree_config :: debug_value (& self . c)",
        ] {
            assert!(has(&out, expected), "missing `{expected}` in {out}");
        }
    }

    #[test]
    fn a_table_describes_its_own_fields() {
        let out = expand_with(
            quote! {
                #[config(key = "x")]
                #[serde(default)]
                struct X {
                    #[config(table)]
                    cors: Cors,
                    #[config(table)]
                    tls: Option<Tls>,
                    #[serde(flatten)]
                    #[config(table)]
                    extra: Extra,
                    #[serde(skip)]
                    cache: Cache,
                    #[config(skip)]
                    other: Other,
                }
            },
            true,
        );
        for expected in [
            ".with_fields (< Cors as :: navree_config :: ConfigTable > :: fields ())",
            ".with_fields (< Tls as :: navree_config :: ConfigTable > :: fields ())",
            "fields . extend (< Extra as :: navree_config :: ConfigTable > :: fields ())",
            ":: navree_config :: ConfigTable :: to_json (& self . cors)",
            "Some (table) => :: navree_config :: ConfigTable :: to_json (table)",
        ] {
            assert!(has(&out, expected), "missing `{expected}` in {out}");
        }
        assert!(!squash(&out).contains("cache"), "{out}");
        assert!(!squash(&out).contains("Other"), "{out}");
    }

    #[test]
    fn a_table_derive_writes_no_section() {
        let out = expand_with(
            quote! {
                struct Cors {
                    origins: Vec<String>,
                }
            },
            false,
        );
        assert!(has(&out, "ConfigTable for Cors"), "{out}");
        assert!(!has(&out, "ConfigSection"), "{out}");
    }

    #[test]
    fn rename_rules_follow_serde() {
        assert_eq!(RenameRule::Camel.field("max_connections"), "maxConnections");
        assert_eq!(
            RenameRule::Pascal.field("max_connections"),
            "MaxConnections"
        );
        assert_eq!(
            RenameRule::Kebab.field("max_connections"),
            "max-connections"
        );
        assert_eq!(
            RenameRule::ScreamingSnake.field("max_connections"),
            "MAX_CONNECTIONS"
        );
        assert_eq!(
            RenameRule::ScreamingKebab.field("max_connections"),
            "MAX-CONNECTIONS"
        );
        assert_eq!(
            RenameRule::Snake.field("max_connections"),
            "max_connections"
        );
    }

    #[test]
    fn misuse_is_reported() {
        for (input, section, message) in [
            (
                quote!(
                    struct X {
                        a: bool,
                    }
                ),
                true,
                "name the section",
            ),
            (
                quote!(
                    #[config(key = "x", colour = "red")]
                    struct X {
                        a: bool,
                    }
                ),
                true,
                "unknown #[config] key `colour` on a struct",
            ),
            (
                quote!(
                    #[config(key = "x")]
                    struct X(bool);
                ),
                true,
                "needs a struct with named fields",
            ),
            (
                quote!(
                    #[config(key = "x")]
                    enum X {
                        A,
                    }
                ),
                true,
                "goes on a struct",
            ),
            (
                quote!(
                    #[config(key = "x")]
                    struct X<T> {
                        a: T,
                    }
                ),
                true,
                "cannot describe a generic struct",
            ),
            (
                quote!(
                    #[config(key = "x")]
                    struct X {
                        #[config(colour)]
                        a: bool,
                    }
                ),
                true,
                "unknown #[config] key `colour` on a field",
            ),
            (
                quote!(
                    #[config(key = "x")]
                    struct X {
                        #[config(secret, public)]
                        a: String,
                    }
                ),
                true,
                "both `secret` and `public`",
            ),
            (
                quote!(
                    #[config(key = "x")]
                    struct Cors {
                        a: bool,
                    }
                ),
                false,
                "unknown #[config] key `key` on a struct",
            ),
        ] {
            let out = expand_with(input, section);
            assert!(out.contains(message), "expected `{message}` in {out}");
        }
    }

    #[test]
    fn the_first_paragraph_of_the_docs_is_the_description() {
        let input: DeriveInput = syn::parse_quote! {
            /// First line
            /// second line.
            ///
            /// Another paragraph.
            struct X { a: bool }
        };
        assert_eq!(first_paragraph(&input.attrs), "First line second line.");
        let input: DeriveInput = syn::parse_quote! {
            /// ```
            /// code
            /// ```
            struct X { a: bool }
        };
        assert_eq!(first_paragraph(&input.attrs), "");
    }
}
