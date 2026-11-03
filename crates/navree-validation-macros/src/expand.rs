//! `#[derive(Validate)]`: one `validate_at` that runs every field's rules
//! in order and records each failure at the field's path.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, Ident, Member};

use crate::paths;
use crate::rules::{Container, FieldRules, Kind, Rule};
use crate::serde_names;

pub(crate) fn derive(input: DeriveInput) -> syn::Result<TokenStream> {
    let mut container = Container::default();
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("validate")) {
        container.parse_into(attr)?;
    }
    let nv = match &container.krate {
        Some(path) => quote!(#path),
        None => paths::validation(input.ident.span())?,
    };
    let rename_all = serde_names::rename_all(&input.attrs);

    let mut table = quote!();
    let body = match &input.data {
        Data::Struct(data) => {
            let fields = fields(&data.fields, &container, rename_all.as_deref())?;
            table = rule_table(&nv, &fields);
            let checks = checks(&nv, &fields, |field| {
                let member = &field.member;
                quote!(&self.#member)
            })?;
            quote!(#(#checks)*)
        }
        Data::Enum(data) => {
            let mut arms = Vec::new();
            for variant in &data.variants {
                let variant_case =
                    serde_names::rename_all(&variant.attrs).or_else(|| rename_all.clone());
                let fields = fields(&variant.fields, &container, variant_case.as_deref())?;
                let name = &variant.ident;
                let bindings: Vec<Ident> = (0..fields.len())
                    .map(|i| format_ident!("__navree_field_{i}"))
                    .collect();
                let members = fields.iter().map(|field| &field.member);
                let pattern = match &variant.fields {
                    Fields::Named(_) => quote!(Self::#name { #(#members: #bindings),* }),
                    Fields::Unnamed(_) => quote!(Self::#name(#(#bindings),*)),
                    Fields::Unit => quote!(Self::#name),
                };
                let checks = checks(&nv, &fields, |field| {
                    let binding = &bindings[field.position];
                    quote!(#binding)
                })?;
                arms.push(quote! {
                    #[allow(unused_variables)]
                    #pattern => { #(#checks)* }
                });
            }
            if arms.is_empty() {
                quote!()
            } else {
                quote!(match self { #(#arms)* })
            }
        }
        Data::Union(data) => {
            return Err(syn::Error::new(
                data.union_token.span(),
                "`#[derive(Validate)]` works on structs and enums, not unions",
            ));
        }
    };

    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #nv::Validate for #ident #ty_generics #where_clause {
            #[allow(unused_variables, clippy::needless_borrow, clippy::redundant_closure_call)]
            fn validate_at(
                &self,
                __navree_at: &#nv::FieldPath<'_>,
                __navree_out: &mut #nv::Violations,
            ) {
                #body
            }
        }

        #[automatically_derived]
        impl #impl_generics #nv::RuleTable for #ident #ty_generics #where_clause {
            const FIELDS: &'static [#nv::FieldRules] = &[#table];
        }
    })
}

/// The fields with rules, as data for the `RuleTable` impl.
fn rule_table(nv: &TokenStream, fields: &[Field]) -> TokenStream {
    let entries = fields
        .iter()
        .filter(|field| !field.skipped)
        .filter_map(|field| {
            let name = field.name.as_ref()?;
            let rules = rule_list(nv, &field.rules.rules);
            Some(quote!(#nv::FieldRules { name: #name, rules: &[#(#rules),*] },))
        });
    quote!(#(#entries)*)
}

fn rule_list(nv: &TokenStream, rules: &[Rule]) -> Vec<TokenStream> {
    rules
        .iter()
        .map(|rule| match &rule.kind {
            Kind::Required => quote!(#nv::Rule::Required),
            Kind::Length { bytes, min, max } => {
                let min = constant_usize(min.as_ref());
                let max = constant_usize(max.as_ref());
                quote!(#nv::Rule::Length { min: #min, max: #max, bytes: #bytes })
            }
            Kind::Range { min, max } => {
                let min = literal_text(min.as_ref());
                let max = literal_text(max.as_ref());
                quote!(#nv::Rule::Range { min: #min, max: #max })
            }
            Kind::Text(name) => quote!(#nv::Rule::Format(#name)),
            Kind::TextWith(name, text) => match *name {
                "contains" => quote!(#nv::Rule::Contains(#text)),
                "starts_with" => quote!(#nv::Rule::StartsWith(#text)),
                _ => quote!(#nv::Rule::EndsWith(#text)),
            },
            Kind::Pattern(pattern) => quote!(#nv::Rule::Pattern(#pattern)),
            Kind::SameAs(other) => {
                let other = other.unraw().to_string();
                quote!(#nv::Rule::SameAs(#other))
            }
            Kind::Custom(_) => quote!(#nv::Rule::Custom),
            Kind::Nested => quote!(#nv::Rule::Nested),
            Kind::Each(inner) => {
                let inner = rule_list(nv, inner);
                quote!(#nv::Rule::Each(&[#(#inner),*]))
            }
        })
        .collect()
}

/// A `length` bound usable in a constant: a literal or a path to a constant.
/// A bound computed at run time isn't stated.
fn constant_usize(bound: Option<&syn::Expr>) -> TokenStream {
    match bound {
        Some(expr @ (syn::Expr::Lit(_) | syn::Expr::Path(_))) => {
            quote!(::core::option::Option::Some(#expr))
        }
        _ => quote!(::core::option::Option::None),
    }
}

/// A `range` bound as written, when it is a literal: its type depends on the
/// field, and text is all a reader of the table needs.
fn literal_text(bound: Option<&syn::Expr>) -> TokenStream {
    fn text(expr: &syn::Expr) -> Option<String> {
        match expr {
            syn::Expr::Lit(lit) => match &lit.lit {
                syn::Lit::Int(int) => Some(int.base10_digits().to_owned()),
                syn::Lit::Float(float) => Some(float.base10_digits().to_owned()),
                _ => None,
            },
            syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Neg(_)) => {
                text(&unary.expr).map(|digits| format!("-{digits}"))
            }
            _ => None,
        }
    }
    match bound.and_then(text) {
        Some(text) => quote!(::core::option::Option::Some(#text)),
        None => quote!(::core::option::Option::None),
    }
}

/// A field with rules.
struct Field {
    /// Its place among the fields (the enum binding's index).
    position: usize,
    member: Member,
    /// Its name in paths (after renames); `None` for a flattened field,
    /// whose rules report at the parent's path.
    name: Option<String>,
    rules: FieldRules,
    /// `skip`, or unmarked under `skip_unmarked`: no checks.
    skipped: bool,
    span: Span,
}

/// Every field, in order, with its rules.
fn fields(fields: &Fields, container: &Container, case: Option<&str>) -> syn::Result<Vec<Field>> {
    let single = fields.len() == 1;
    let mut out = Vec::new();
    for (position, field) in fields.iter().enumerate() {
        let mut rules = FieldRules::default();
        let mut marked = false;
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("validate")) {
            rules.parse_into(attr)?;
            marked = true;
        }
        let span = field
            .ident
            .as_ref()
            .map_or_else(|| field.ty.span(), Ident::span);
        if !marked && !container.skip_unmarked {
            let what = match &field.ident {
                Some(ident) => format!("field `{}`", ident.unraw()),
                None => format!("field {position}"),
            };
            return Err(syn::Error::new(
                span,
                format!(
                    "{what} has no validation rules: add `#[validate(...)]` with its rules, or `#[validate(skip)]` (or put `#[validate(skip_unmarked)]` on the type)"
                ),
            ));
        }
        rules.check()?;
        // Unmarked fields (with `skip_unmarked`) and skipped ones have no
        // rules, but `same_as` may still compare with them.
        let skipped = !marked || rules.skip;
        let serde = serde_names::field(&field.attrs);
        let name = if let Some(rename) = &rules.rename {
            Some(rename.value())
        } else if serde.flatten {
            None
        } else if let Some(rename) = serde.rename {
            Some(rename)
        } else {
            match &field.ident {
                Some(ident) => {
                    let name = ident.unraw().to_string();
                    Some(match case {
                        Some(case) => serde_names::apply_case(&name, case),
                        None => name,
                    })
                }
                // A newtype is its one field; tuple fields are numbered.
                None if single => None,
                None => Some(position.to_string()),
            }
        };
        let member = match &field.ident {
            Some(ident) => Member::Named(ident.clone()),
            None => Member::Unnamed(syn::Index::from(position)),
        };
        out.push(Field {
            position,
            member,
            name,
            rules,
            skipped,
            span,
        });
    }
    Ok(out)
}

/// The checks of every field; `access` gives a field's value as a reference.
fn checks(
    nv: &TokenStream,
    fields: &[Field],
    access: impl Fn(&Field) -> TokenStream,
) -> syn::Result<Vec<TokenStream>> {
    fields
        .iter()
        .filter(|field| !field.skipped)
        .map(|field| {
            let value = access(field);
            let path = match &field.name {
                Some(name) => quote!(&__navree_at.field(#name)),
                None => quote!(__navree_at),
            };
            let rules = field
                .rules
                .rules
                .iter()
                .map(|rule| check(nv, rule, Some((fields, field)), &access))
                .collect::<syn::Result<Vec<_>>>()?;
            Ok(quote_spanned! {field.span=>
                {
                    let __navree_path: &#nv::FieldPath<'_> = #path;
                    let __navree_value = #value;
                    #(#rules)*
                }
            })
        })
        .collect()
}

/// One rule on `__navree_value` at `__navree_path`. `field` is the field
/// and its siblings (`None` inside `each`).
fn check(
    nv: &TokenStream,
    rule: &Rule,
    field: Option<(&[Field], &Field)>,
    access: &impl Fn(&Field) -> TokenStream,
) -> syn::Result<TokenStream> {
    let span = rule.span;
    let private = quote!(#nv::__private);
    let call = match &rule.kind {
        Kind::Required => quote_spanned!(span=> #private::required(__navree_value)),
        Kind::Length { bytes, min, max } => {
            let unit = if *bytes {
                quote!(#private::Unit::Bytes)
            } else {
                quote!(#private::Unit::Chars)
            };
            let min = bound_usize(min.as_ref());
            let max = bound_usize(max.as_ref());
            quote_spanned!(span=> #private::length(__navree_value, #unit, #min, #max))
        }
        Kind::Range { min, max } => {
            let min = bound_ref(min.as_ref());
            let max = bound_ref(max.as_ref());
            quote_spanned!(span=> #private::range(__navree_value, #min, #max))
        }
        Kind::Text(name) => {
            let function = Ident::new(name, span);
            quote_spanned!(span=> #private::#function(__navree_value))
        }
        Kind::TextWith(name, text) => {
            let function = Ident::new(name, span);
            quote_spanned!(span=> #private::#function(__navree_value, #text))
        }
        Kind::Pattern(pattern) => {
            return Ok(quote_spanned! {span=>
                {
                    static __NAVREE_PATTERN: #private::Pattern = #private::Pattern::new(#pattern);
                    if let ::core::result::Result::Err(__navree_violation) =
                        #private::pattern(__navree_value, &__NAVREE_PATTERN)
                    {
                        __navree_out.push(__navree_path, __navree_violation);
                    }
                }
            });
        }
        Kind::SameAs(other) => {
            let Some((siblings, _)) = field else {
                return Err(syn::Error::new(span, "`same_as` can't apply to items"));
            };
            let wanted = other.unraw().to_string();
            let Some(sibling) = siblings.iter().find(|sibling| {
                matches!(&sibling.member, Member::Named(ident) if ident.unraw() == wanted)
            }) else {
                return Err(syn::Error::new(
                    other.span(),
                    format!(
                        "`same_as({other})`: there is no field `{other}` here"
                    ),
                ));
            };
            let other_value = access(sibling);
            let other_name = sibling.name.clone().unwrap_or(wanted);
            quote_spanned! {span=>
                #private::same_as(__navree_value, #other_value, &__navree_at.field(#other_name))
            }
        }
        Kind::Custom(expr) => {
            quote_spanned!(expr.span()=> #private::custom((#expr)(__navree_value)))
        }
        Kind::Nested => {
            return Ok(quote_spanned! {span=>
                #nv::Validate::validate_at(__navree_value, __navree_path, __navree_out);
            });
        }
        Kind::Each(rules) => {
            let inner = rules
                .iter()
                .map(|rule| check(nv, rule, None, access))
                .collect::<syn::Result<Vec<_>>>()?;
            return Ok(quote_spanned! {span=>
                #private::Each::each(
                    __navree_value,
                    __navree_path,
                    &mut |__navree_path: &#nv::FieldPath<'_>, __navree_value| { #(#inner)* },
                );
            });
        }
    };
    Ok(quote_spanned! {span=>
        if let ::core::result::Result::Err(__navree_violation) = #call {
            __navree_out.push(__navree_path, __navree_violation);
        }
    })
}

/// A `length` bound: `Some(expr)` as a `usize`, or `None`.
fn bound_usize(bound: Option<&syn::Expr>) -> TokenStream {
    match bound {
        Some(expr) => quote_spanned!(expr.span()=> ::core::option::Option::Some::<usize>(#expr)),
        None => quote!(::core::option::Option::None),
    }
}

/// A `range` bound: `Some(&expr)`, typed by the field, or `None`.
fn bound_ref(bound: Option<&syn::Expr>) -> TokenStream {
    match bound {
        Some(expr) => quote_spanned!(expr.span()=> ::core::option::Option::Some(&(#expr))),
        None => quote!(::core::option::Option::None),
    }
}
