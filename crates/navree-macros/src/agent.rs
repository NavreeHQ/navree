//! `#[agent]`: collects the `#[tool]` methods of an `impl Agent for X`
//! block into `Agent::tool_specs` and `Agent::dispatch`, and moves them to
//! an inherent impl.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{
    Attribute, Expr, ExprLit, FnArg, Ident, ImplItem, ImplItemFn, ItemImpl, Lit, LitStr, Meta,
    Type,
};

use crate::attr::Errors;
use crate::paths::{ACCESS, AI};
use crate::route::{At, Can, Name, strings};

/// One `#[tool]` method.
struct Tool {
    method: ImplItemFn,
    /// The arguments type (`None`: the tool takes none).
    args: Option<Type>,
    read_only: bool,
    needs_approval: bool,
    description: String,
    access: Vec<AccessRule>,
}

/// An access attribute on a tool.
enum AccessRule {
    Can(Box<Can>),
    Permissions(Vec<LitStr>),
    Roles(Vec<LitStr>),
}

pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    expand_inner(args, item).unwrap_or_else(syn::Error::into_compile_error)
}

fn expand_inner(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(args, "#[agent] takes no arguments"));
    }
    let mut block: ItemImpl = syn::parse2(item)?;
    let is_agent = block
        .trait_
        .as_ref()
        .and_then(|(path, _)| path.segments.last())
        .is_some_and(|segment| segment.ident == "Agent");
    if !is_agent {
        return Err(syn::Error::new(
            block.impl_token.span,
            "#[agent] goes on an `impl Agent for YourAgent` block",
        ));
    }
    let mut errors = Errors::default();
    let mut tools = Vec::new();
    let mut kept = Vec::new();
    for item in std::mem::take(&mut block.items) {
        match item {
            ImplItem::Fn(method) if has_attr(&method.attrs, "tool") => {
                if let Some(tool) = errors.keep(tool(method)) {
                    tools.push(tool);
                }
            }
            ImplItem::Fn(method) => {
                for attr in &method.attrs {
                    if ["can", "permissions", "roles"].iter().any(|name| is(attr, name)) {
                        errors.push(At::of(attr).error(format!(
                            "#[{}] only applies to #[tool] methods",
                            attr.path().segments[0].ident
                        )));
                    }
                }
                kept.push(ImplItem::Fn(method));
            }
            other => kept.push(other),
        }
    }
    errors.finish()?;

    // Misuse is reported first; only valid tools need the runtime crate.
    let ai = AI.path(block.impl_token.span)?;
    let needs_access = tools.iter().any(|tool| {
        tool.access
            .iter()
            .any(|rule| matches!(rule, AccessRule::Can(can) if can.uses_markers()))
    });
    let access_crate = if needs_access {
        Some(ACCESS.path(block.impl_token.span)?)
    } else {
        None
    };

    let specs = tools.iter().map(|tool| spec(tool, &ai, access_crate.as_ref()));
    let arms = tools.iter().map(|tool| arm(tool, &ai));
    let methods = tools.iter().map(|tool| &tool.method);

    let (impl_generics, _, where_clause) = block.generics.split_for_impl();
    let self_ty = &block.self_ty;
    let attrs = &block.attrs;
    let unsafety = &block.unsafety;
    let (trait_path, for_token) = block.trait_.as_ref().expect("checked above");
    let tool_specs = if tools.is_empty() {
        quote!()
    } else {
        quote! {
            fn tool_specs() -> ::std::vec::Vec<#ai::ToolSpec> {
                ::std::vec![#(#specs),*]
            }

            fn dispatch<'navree_agent>(
                &'navree_agent self,
                name: &'navree_agent str,
                run: &'navree_agent #ai::AgentRun,
                arguments: #ai::__private::Value,
            ) -> #ai::__private::BoxFuture<'navree_agent, #ai::__private::ToolOutcome> {
                ::std::boxed::Box::pin(async move {
                    match name {
                        #(#arms)*
                        _ => #ai::__private::ToolOutcome::unknown(name),
                    }
                })
            }
        }
    };
    Ok(quote! {
        #(#attrs)*
        #unsafety impl #impl_generics #trait_path #for_token #self_ty #where_clause {
            #(#kept)*
            #tool_specs
        }

        impl #impl_generics #self_ty #where_clause {
            #(#methods)*
        }
    })
}

/// Whether `attr` is `#[name]` (or `#[navree::name]`).
fn is(attr: &Attribute, name: &str) -> bool {
    let path = attr.path();
    path.segments.last().is_some_and(|segment| segment.ident == name)
        && (path.segments.len() == 1
            || (path.segments.len() == 2
                && matches!(
                    path.segments[0].ident.to_string().as_str(),
                    "navree" | "navree_macros"
                )))
}

fn has_attr(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|attr| is(attr, name))
}

/// Parses a `#[tool]` method and strips our attributes from it.
fn tool(mut method: ImplItemFn) -> syn::Result<Tool> {
    let mut errors = Errors::default();
    let mut read_only = false;
    let mut needs_approval = false;
    let mut access = Vec::new();
    let mut retained = Vec::new();
    for attr in std::mem::take(&mut method.attrs) {
        if is(&attr, "tool") {
            errors.keep(tool_flags(&attr, &mut read_only, &mut needs_approval));
        } else if is(&attr, "can") {
            let at = At::of(&attr);
            if let Some(can) = errors.keep(Can::parse_args(&attr, &at)) {
                if can.from.is_some() {
                    errors.push(at.error(
                        "a tool checks records with `run.authorize::<E, A>(id)`, not `from`",
                    ));
                }
                access.push(AccessRule::Can(Box::new(can)));
            }
        } else if is(&attr, "permissions") {
            if let Some(list) = errors.keep(strings(&attr, "\"deals.update\"")) {
                access.push(AccessRule::Permissions(list));
            }
        } else if is(&attr, "roles") {
            if let Some(list) = errors.keep(strings(&attr, "\"admin\", \"manager\"")) {
                access.push(AccessRule::Roles(list));
            }
        } else {
            retained.push(attr);
        }
    }
    method.attrs = retained;
    let description = doc_comment(&method.attrs);
    let ident = method.sig.ident.clone();
    if description.is_empty() {
        errors.push(syn::Error::new(
            ident.span(),
            format!("the tool `{ident}` needs a doc comment: the model chooses tools by it"),
        ));
    }
    let args = errors.keep(signature(&method)).flatten().flatten();
    errors.finish()?;
    Ok(Tool {
        method,
        args,
        read_only,
        needs_approval,
        description,
        access,
    })
}

/// `#[tool]`, `#[tool(read_only)]`, `#[tool(needs_approval)]`, or both.
fn tool_flags(attr: &Attribute, read_only: &mut bool, needs_approval: &mut bool) -> syn::Result<()> {
    match &attr.meta {
        Meta::Path(_) => Ok(()),
        Meta::List(_) => attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("read_only") {
                *read_only = true;
                Ok(())
            } else if meta.path.is_ident("needs_approval") {
                *needs_approval = true;
                Ok(())
            } else {
                Err(meta.error(format!(
                    "unknown #[tool] option `{}`: expected `read_only` or `needs_approval`",
                    meta.path.to_token_stream()
                )))
            }
        }),
        Meta::NameValue(_) => Err(At::of(attr).error(
            "expected #[tool], #[tool(read_only)] or #[tool(needs_approval)]",
        )),
    }
}

/// Checks `async fn name(&self, run: &AgentRun[, args: T])` and returns `T`.
fn signature(method: &ImplItemFn) -> syn::Result<Option<Option<Type>>> {
    let sig = &method.sig;
    let ident = &sig.ident;
    let expected = || {
        syn::Error::new(
            ident.span(),
            format!(
                "a #[tool] is `async fn {ident}(&self, run: &AgentRun, args: Args) -> Result<T>` \
                 (or without `args`)"
            ),
        )
    };
    if sig.asyncness.is_none() {
        return Err(syn::Error::new(ident.span(), format!("the tool `{ident}` must be `async`")));
    }
    if !sig.generics.params.is_empty() {
        return Err(syn::Error::new(
            sig.generics.span(),
            format!("the tool `{ident}` must not be generic"),
        ));
    }
    let mut inputs = sig.inputs.iter();
    match inputs.next() {
        Some(FnArg::Receiver(receiver))
            if matches!(receiver.kind, syn::ReceiverKind::Reference(_, _, None)) => {}
        _ => return Err(expected()),
    }
    match inputs.next() {
        Some(FnArg::Typed(_)) => {}
        _ => return Err(expected()),
    }
    let args = match inputs.next() {
        Some(FnArg::Typed(typed)) => Some((*typed.ty).clone()),
        Some(FnArg::Receiver(_)) => return Err(expected()),
        None => None,
    };
    if inputs.next().is_some() {
        return Err(expected());
    }
    Ok(Some(args))
}

/// The doc comment, lines joined with spaces and paragraphs with a blank
/// line.
fn doc_comment(attrs: &[Attribute]) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    for attr in attrs {
        let Meta::NameValue(meta) = &attr.meta else { continue };
        if !meta.path.is_ident("doc") {
            continue;
        }
        let Expr::Lit(ExprLit { lit: Lit::Str(text), .. }) = &meta.value else {
            continue;
        };
        let line = text.value();
        let line = line.trim();
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    paragraphs.join("\n\n")
}

/// The `ToolSpec` expression of `tool`.
fn spec(tool: &Tool, ai: &TokenStream, access_crate: Option<&TokenStream>) -> TokenStream {
    let name = LitStr::new(&tool.method.sig.ident.to_string(), tool.method.sig.ident.span());
    let description = LitStr::new(&tool.description, Span::call_site());
    let schema = match &tool.args {
        Some(args) => quote_spanned!(args.span()=> #ai::__private::tool_schema::<#args>()),
        None => quote!(#ai::__private::no_args_schema()),
    };
    let read_only = tool.read_only.then(|| quote!(.read_only()));
    let needs_approval = tool.needs_approval.then(|| quote!(.needs_approval()));
    let access = tool.access.iter().map(|rule| {
        let rule = match rule {
            AccessRule::Can(can) => {
                let name = |name: &Name, marker: &str, constant: &str| match name {
                    Name::Literal(literal) => literal.to_token_stream(),
                    Name::Type(ty) => {
                        let marker = Ident::new(marker, ty.span());
                        let constant = Ident::new(constant, ty.span());
                        quote_spanned!(ty.span()=> <#ty as #access_crate::#marker>::#constant)
                    }
                };
                let action = name(&can.action, "ActionMarker", "NAME");
                let subject = name(&can.subject, "Subject", "SUBJECT");
                quote!(#ai::ToolAccess::can(#action, #subject))
            }
            AccessRule::Permissions(list) => quote!(#ai::ToolAccess::permissions(&[#(#list),*])),
            AccessRule::Roles(list) => quote!(#ai::ToolAccess::roles(&[#(#list),*])),
        };
        quote!(.access(#rule))
    });
    quote! {
        #ai::ToolSpec::new(#name, #description, #schema) #read_only #needs_approval #(#access)*
    }
}

/// The dispatch arm of `tool`.
fn arm(tool: &Tool, ai: &TokenStream) -> TokenStream {
    let ident = &tool.method.sig.ident;
    let name = LitStr::new(&ident.to_string(), ident.span());
    match &tool.args {
        Some(args) => quote! {
            #name => #ai::__private::call_tool(arguments, move |args: #args| Self::#ident(self, run, args)).await,
        },
        None => quote! {
            #name => #ai::__private::call_tool_no_args(Self::#ident(self, run)).await,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_comments_join_lines_and_keep_paragraphs() {
        let method: ImplItemFn = syn::parse_quote! {
            /// Finds the deals the user can see,
            /// newest first.
            ///
            /// Returns at most 20.
            async fn search(&self, run: &AgentRun) -> Result<()> { Ok(()) }
        };
        assert_eq!(
            doc_comment(&method.attrs),
            "Finds the deals the user can see, newest first.\n\nReturns at most 20."
        );
    }

    #[test]
    fn signatures() {
        let ok: ImplItemFn = syn::parse_quote!(async fn a(&self, run: &AgentRun, args: Args) {});
        assert!(matches!(signature(&ok), Ok(Some(Some(_)))));
        let no_args: ImplItemFn = syn::parse_quote!(async fn a(&self, run: &AgentRun) {});
        assert!(matches!(signature(&no_args), Ok(Some(None))));
        let not_async: ImplItemFn = syn::parse_quote!(fn a(&self, run: &AgentRun) {});
        assert!(signature(&not_async).is_err());
        let owned_self: ImplItemFn = syn::parse_quote!(async fn a(self, run: &AgentRun) {});
        assert!(signature(&owned_self).is_err());
        let too_many: ImplItemFn = syn::parse_quote!(async fn a(&self, r: &AgentRun, a: A, b: B) {});
        assert!(signature(&too_many).is_err());
    }
}
