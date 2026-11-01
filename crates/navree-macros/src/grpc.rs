//! `#[grpc_service]`: serves a tonic service impl through navree-grpc's call
//! pipeline and implements `navree_grpc::GrpcService` for its struct.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{
    Expr, FnArg, GenericArgument, Ident, ImplItem, ImplItemFn, Item, ItemImpl, LitStr, Pat,
    PathArguments, ReceiverKind, ReturnType, Token, Type,
};

use crate::attr::Errors;
use crate::controller::access_calls;
use crate::paths::{ACCESS, GRPC};
use crate::route::{RouteAttrs, Site};

/// Expands `#[grpc_service(server = X, interceptor = f)] impl Trait for S { .. }`.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    let mut item = match parse_impl(item.clone()) {
        Ok(item) => item,
        Err(error) => {
            let mut out = error.into_compile_error();
            out.extend(item);
            return out;
        }
    };
    let mut errors = Errors::default();
    let args = errors.keep(syn::parse2::<Args>(args));
    reject_http_only(&mut item.attrs, &mut errors);
    let shared = RouteAttrs::take(&mut item.attrs, Site::Impl, &mut errors);
    if let (Some(_), Some((restriction, at))) = (&shared.access.public, shared.access.restriction())
    {
        errors.push(at.error(format!(
            "the impl block is #[public] but also declares {restriction}: a public method needs no authentication or authorization; remove one of them"
        )));
    }
    let methods = collect_methods(&mut item, &shared, &mut errors);
    let paths = errors.keep(Paths::resolve(&methods));
    let mut out = errors
        .finish()
        .err()
        .map(syn::Error::into_compile_error)
        .unwrap_or_default();
    match (args, paths) {
        (Some(args), Some(paths)) => out.extend(generate(&args, item, &methods, &paths)),
        _ => out.extend(item.into_token_stream()),
    }
    out
}

/// The annotated item must be a trait impl (of a generated service trait).
fn parse_impl(item: TokenStream) -> syn::Result<ItemImpl> {
    match syn::parse2::<Item>(item)? {
        Item::Impl(item) if item.trait_.is_some() => Ok(item),
        Item::Impl(item) => Err(syn::Error::new_spanned(
            &item.self_ty,
            "#[grpc_service] goes on the impl of a generated service trait: \
             `#[grpc_service(server = DealsServer)] impl Deals for DealsGrpc { .. }`",
        )),
        other => Err(syn::Error::new(
            other.span(),
            "#[grpc_service] goes on an impl block: \
             `#[grpc_service(server = DealsServer)] impl Deals for DealsGrpc { .. }`",
        )),
    }
}

/// `server = DealsServer`, `interceptor = expr`.
struct Args {
    server: syn::Path,
    interceptor: Option<Expr>,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut server = None;
        let mut interceptor = None;
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            match key.to_string().as_str() {
                "server" if server.is_none() => server = Some(input.parse()?),
                "interceptor" if interceptor.is_none() => interceptor = Some(input.parse()?),
                "server" | "interceptor" => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("duplicate `{key}` key"),
                    ));
                }
                _ => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!(
                            "unknown #[grpc_service] key `{key}`: expected `server = DealsServer` or `interceptor = fn_name`"
                        ),
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        let server = server.ok_or_else(|| {
            input.error("#[grpc_service] needs the generated server type: #[grpc_service(server = DealsServer)]")
        })?;
        Ok(Args {
            server,
            interceptor,
        })
    }
}

/// The route attributes that mean nothing for gRPC.
const HTTP_ONLY: [&str; 12] = [
    "get",
    "post",
    "put",
    "patch",
    "delete",
    "throttle",
    "timeout",
    "transactional",
    "no_compression",
    "audit",
    "name",
    "api",
];

/// Removes the HTTP-only route attributes from `attrs`, each an error
/// pointing at it.
fn reject_http_only(attrs: &mut Vec<syn::Attribute>, errors: &mut Errors) {
    attrs.retain(|attr| {
        let path = attr.path();
        let ours = match path.segments.len() {
            1 => path.leading_colon.is_none(),
            2 => matches!(
                path.segments[0].ident.to_string().as_str(),
                "navree" | "navree_macros"
            ),
            _ => false,
        };
        let name = path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default();
        if ours && HTTP_ONLY.contains(&name.as_str()) {
            errors.push(syn::Error::new_spanned(
                attr,
                format!(
                    "#[{name}] does not apply to gRPC methods: they take #[public], #[authenticated], #[roles], #[permissions], #[can] and #[guard]"
                ),
            ));
            false
        } else {
            true
        }
    });
}

/// One method of the service.
struct Method {
    ident: Ident,
    attrs: RouteAttrs,
    /// The method's attributes besides ours (docs, `cfg`, `allow`).
    other_attrs: Vec<syn::Attribute>,
    /// The type tonic passes: `Request<T>`.
    request_ty: Type,
    /// The message type inside it, when it is not a stream.
    message_ty: Option<Type>,
    /// The first parameter as written: its pattern and type, and whether it
    /// is `Valid<Request<T>>`.
    first: (Box<Pat>, Box<Type>, bool),
    /// The extractors after it.
    extractors: Vec<(Box<Pat>, Box<Type>)>,
    /// `Response<T>`, from `Result<Response<T>, ..>`.
    ok_ty: Type,
    /// The declared return type.
    output: Type,
    block: syn::Block,
}

fn collect_methods(item: &mut ItemImpl, shared: &RouteAttrs, errors: &mut Errors) -> Vec<Method> {
    let mut methods = Vec::new();
    let mut kept = Vec::new();
    for impl_item in std::mem::take(&mut item.items) {
        let ImplItem::Fn(mut method) = impl_item else {
            kept.push(impl_item);
            continue;
        };
        let ident = method.sig.ident.clone();
        reject_http_only(&mut method.attrs, errors);
        let mut attrs = RouteAttrs::take(&mut method.attrs, Site::Method(&ident), errors);
        if let Some((restriction, at)) = attrs.access.public_conflict(&shared.access) {
            errors.push(at.error(format!(
                "method `{ident}` is #[public] but also declares {restriction}: a public method needs no authentication or authorization; remove one of them"
            )));
        }
        if let (Some((_, at)), Some(_)) = (&attrs.access.can, &shared.access.can) {
            errors.push(at.error(format!(
                "method `{ident}` has #[can] here and on the impl block: a method checks one ability"
            )));
        }
        attrs.inherit(shared);
        if !attrs.access.is_declared() && !attrs.access.malformed {
            let guard_note = if attrs.guards.is_empty() {
                ""
            } else {
                "; a #[guard] alone is not an access declaration"
            };
            errors.push(syn::Error::new(
                ident.span(),
                format!(
                    "gRPC method `{ident}` has no access declaration: add #[public], #[authenticated], #[roles], #[permissions] or #[can], on the method or the impl block (deny by default{guard_note})"
                ),
            ));
        }
        match parse_signature(&method) {
            Ok(parsed) => {
                let (request_ty, message_ty, first, extractors, ok_ty, output) = parsed;
                if let (Some((can, at)), None) = (&attrs.access.can, &message_ty)
                    && can.from.is_some()
                {
                    errors.push(at.error(format!(
                        "`from = ..` reads the record id from the request message, but `{ident}` receives a stream"
                    )));
                }
                methods.push(Method {
                    ident,
                    attrs,
                    other_attrs: method.attrs,
                    request_ty,
                    message_ty,
                    first,
                    extractors,
                    ok_ty,
                    output,
                    block: method.block,
                });
            }
            Err(error) => {
                errors.push(error);
                kept.push(ImplItem::Fn(method));
            }
        }
    }
    item.items = kept;
    methods
}

type Signature = (
    Type,
    Option<Type>,
    (Box<Pat>, Box<Type>, bool),
    Vec<(Box<Pat>, Box<Type>)>,
    Type,
    Type,
);

/// A gRPC method is a non-generic `async fn(&self, request, extractors..)
/// -> Result<Response<T>, ..>`.
fn parse_signature(method: &ImplItemFn) -> syn::Result<Signature> {
    let sig = &method.sig;
    let ident = &sig.ident;
    if sig.asyncness.is_none() {
        return Err(syn::Error::new(
            ident.span(),
            format!("gRPC method `{ident}` must be an `async fn`"),
        ));
    }
    match sig.receiver() {
        Some(receiver) if matches!(receiver.kind, ReceiverKind::Reference(_, _, None)) => {}
        _ => {
            return Err(syn::Error::new(
                ident.span(),
                format!("gRPC method `{ident}` must take `&self`"),
            ));
        }
    }
    if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &sig.generics,
            format!("gRPC method `{ident}` cannot be generic"),
        ));
    }
    let mut typed = sig.inputs.iter().filter_map(|arg| match arg {
        FnArg::Typed(arg) => Some((arg.pat.clone(), arg.ty.clone())),
        FnArg::Receiver(_) => None,
    });
    let Some((pat, ty)) = typed.next() else {
        return Err(syn::Error::new(
            ident.span(),
            format!("gRPC method `{ident}` takes the request first: `request: Request<T>`"),
        ));
    };
    let (request_ty, valid) = match single_generic(&ty, "Valid") {
        Some(inner) => (inner, true),
        None => ((*ty).clone(), false),
    };
    let message_ty = single_generic(&request_ty, "Request")
        .filter(|message| single_generic(message, "Streaming").is_none());
    let extractors = typed.collect();
    let output = match &sig.output {
        ReturnType::Type(_, ty) => (**ty).clone(),
        ReturnType::Default => {
            return Err(syn::Error::new(
                ident.span(),
                format!("gRPC method `{ident}` returns `Result<Response<T>>`"),
            ));
        }
    };
    let ok_ty = first_generic(&output, "Result").ok_or_else(|| {
        syn::Error::new_spanned(
            &output,
            "a gRPC method returns `Result<Response<T>>` (an `AppError`) or `Result<Response<T>, Status>`",
        )
    })?;
    Ok((
        request_ty,
        message_ty,
        (pat, ty, valid),
        extractors,
        ok_ty,
        output,
    ))
}

/// `T` when `ty` is `Name<T>` (the last path segment is `name`, with one
/// type argument).
fn single_generic(ty: &Type, name: &str) -> Option<Type> {
    let args = generics_of(ty, name)?;
    (args.len() == 1).then(|| args[0].clone())
}

/// The first type argument of `Name<A, ..>`.
fn first_generic(ty: &Type, name: &str) -> Option<Type> {
    generics_of(ty, name)?.into_iter().next()
}

fn generics_of(ty: &Type, name: &str) -> Option<Vec<Type>> {
    let Type::Path(path) = ty else {
        return None;
    };
    let last = path.path.segments.last()?;
    if last.ident != name {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    Some(
        args.args
            .iter()
            .filter_map(|arg| match arg {
                GenericArgument::Type(ty) => Some(ty.clone()),
                _ => None,
            })
            .collect(),
    )
}

/// Paths to navree-grpc and, for `#[can]` with marker types, navree-access.
struct Paths {
    grpc: TokenStream,
    access: Option<TokenStream>,
}

impl Paths {
    fn resolve(methods: &[Method]) -> syn::Result<Paths> {
        let grpc = GRPC.path(Span::call_site())?;
        // Inside navree-grpc itself (its tests and docs) the expansion
        // names the crate by its name, which `extern crate self` provides.
        let grpc = if grpc.to_string() == "crate" {
            quote!(::navree_grpc)
        } else {
            grpc
        };
        let markers = methods.iter().find_map(|method| {
            method
                .attrs
                .access
                .can
                .as_ref()
                .filter(|(can, _)| can.uses_markers())
        });
        let access = match markers {
            Some((_, at)) => Some(ACCESS.path(at.span())?),
            None => None,
        };
        Ok(Paths { grpc, access })
    }
}

fn generate(args: &Args, mut item: ItemImpl, methods: &[Method], paths: &Paths) -> TokenStream {
    let grpc = &paths.grpc;
    for method in methods {
        let wrapper = wrapper(method, grpc);
        item.items.push(ImplItem::Verbatim(wrapper));
    }
    let self_ty = &item.self_ty;
    let (impl_generics, _, where_clause) = item.generics.split_for_impl();
    let list = Ident::new("methods", Span::mixed_site());
    let guard_var = Ident::new("guard", Span::mixed_site());
    let defs = methods.iter().map(|method| {
        let cfgs = method
            .other_attrs
            .iter()
            .filter(|attr| attr.path().is_ident("cfg"));
        let name = LitStr::new(&method.ident.to_string(), method.ident.span());
        let access = access_calls(&method.attrs, paths.access.as_ref());
        let guards = method.attrs.guards.iter().map(|guard| {
            quote_spanned! {guard.span()=>
                .guard({
                    let #guard_var: #grpc::__private::Arc<dyn #grpc::__private::Guard> = #guard;
                    #guard_var
                })
            }
        });
        quote! {
            #(#cfgs)*
            #list.push(#grpc::GrpcMethod::new(#name) #access #(#guards)*);
        }
    });
    let mut server = args.server.clone();
    if let Some(last) = server.segments.last_mut() {
        last.arguments = PathArguments::AngleBracketed(syn::parse_quote!(<Self>));
    }
    let constructor = &args.server;
    let routes = Ident::new("routes", Span::mixed_site());
    let limits = Ident::new("limits", Span::mixed_site());
    let service = Ident::new("service", Span::mixed_site());
    let add = match &args.interceptor {
        Some(interceptor) => quote! {
            #routes.add_service(#grpc::__private::InterceptedService::new(#service, #interceptor))
        },
        None => quote!(#routes.add_service(#service)),
    };
    let attribute: syn::Attribute = syn::parse_quote!(#[#grpc::async_trait]);
    item.attrs.push(attribute);
    quote! {
        #item

        impl #impl_generics #grpc::GrpcService for #self_ty #where_clause {
            const NAME: &'static str = <#server as #grpc::__private::NamedService>::NAME;

            fn methods(self: &#grpc::__private::Arc<Self>) -> ::std::vec::Vec<#grpc::GrpcMethod> {
                let mut #list = ::std::vec::Vec::new();
                #(#defs)*
                #list
            }

            fn add_to(
                self: #grpc::__private::Arc<Self>,
                #routes: #grpc::__private::Routes,
                #limits: &#grpc::ServiceLimits,
            ) -> #grpc::__private::Routes {
                let #service = #constructor::from_arc(self)
                    .max_decoding_message_size(#limits.max_message_bytes)
                    .max_encoding_message_size(#limits.max_message_bytes);
                #add
            }
        }
    }
}

/// The method tonic calls: the pipeline around the user's body.
fn wrapper(method: &Method, grpc: &TokenStream) -> TokenStream {
    let local = |name: &str| Ident::new(name, Span::mixed_site());
    let (request, parts, message) = (local("request"), local("parts"), local("message"));
    let ident = &method.ident;
    let name = LitStr::new(&ident.to_string(), ident.span());
    let attrs = &method.other_attrs;
    let request_ty = &method.request_ty;
    let ok_ty = &method.ok_ty;
    let output = &method.output;
    let block = &method.block;

    let record_key = match (&method.message_ty, method.attrs.access.can.as_ref()) {
        (Some(message_ty), Some((can, _))) if can.from.is_some() => {
            let from = can.from.as_ref().map(LitStr::value).unwrap_or_default();
            let span = can.from.as_ref().map_or_else(Span::call_site, LitStr::span);
            let field = format_ident!("{}", from, span = span);
            quote! {
                ::core::option::Option::Some(
                    (|#message: &#message_ty| #grpc::__private::RecordKey::record_key(&#message.#field))
                        as #grpc::__private::RecordKeyFn<#message_ty>
                )
            }
        }
        _ => quote!(::core::option::Option::None),
    };

    // The body runs once the call has passed its access check.
    let lift = method
        .message_ty
        .is_none()
        .then(|| quote!(#grpc::__private::lift_body_limit(&#parts);));
    let (first_pat, first_ty, valid) = &method.first;
    let first = if *valid {
        quote_spanned! {first_ty.span()=>
            let #first_pat: #first_ty = #grpc::__private::validate(#request, &#parts).await?;
        }
    } else {
        quote!(let #first_pat: #first_ty = #request;)
    };
    let extractors = method.extractors.iter().map(|(pat, ty)| {
        quote_spanned! {ty.span()=>
            let #pat: #ty = #grpc::__private::extract::<#ty, _>(&mut #parts).await?;
        }
    });
    quote! {
        #(#attrs)*
        async fn #ident(
            &self,
            #request: #request_ty,
        ) -> ::core::result::Result<#ok_ty, #grpc::Status> {
            #grpc::__private::call(
                #request,
                #name,
                #record_key,
                move |#request, mut #parts: #grpc::__private::Parts| async move {
                    #lift
                    #first
                    #(#extractors)*
                    let _ = &mut #parts;
                    #grpc::__private::finish(
                        #grpc::__private::returns::<#output, _>(async move #block).await,
                    )
                },
            )
            .await
        }
    }
}
