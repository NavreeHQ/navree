//! `#[workflow]`, `#[trigger]`, `#[condition]` and `#[action]`: implement
//! navree-workflows' `Workflow`, `Trigger`, `Condition` and `Action` traits
//! from plain functions (and `#[trigger]` also from a configuration
//! struct).
//!
//! Each attribute keeps the item as written and adds the trait impl next to
//! it, on the type the function takes: the workflow's input, or the block's
//! configuration. The doc comment becomes the description people see.

use heck::ToKebabCase;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{
    Attribute, Expr, FnArg, GenericArgument, Item, ItemFn, ItemStruct, Lit, LitBool, LitInt,
    LitStr, Meta, PathArguments, ReturnType, Type,
};

use crate::attr::{Errors, key_name};
use crate::paths::WORKFLOWS;

/// The longest workflow name (navree-workflows' `MAX_NAME_LEN`).
const MAX_NAME_LEN: usize = 100;

/// `Box<dyn Error + Send + Sync>`: navree-kernel's `BoxError`, spelled out
/// so the expansion needs no path to the kernel.
fn box_error() -> TokenStream {
    quote!(
        ::std::boxed::Box<
            dyn ::std::error::Error + ::core::marker::Send + ::core::marker::Sync + 'static,
        >
    )
}

/// Emits `item` unchanged next to `error`, so nothing else cascades.
fn failed(error: syn::Error, item: TokenStream) -> TokenStream {
    let mut out = error.into_compile_error();
    out.extend(item);
    out
}

/// Runs `generate` with the path to navree-workflows, or reports that the
/// crate depends on neither the facade nor navree-workflows.
fn with_runtime(
    item: &impl ToTokens,
    generate: impl FnOnce(&TokenStream) -> TokenStream,
) -> TokenStream {
    match WORKFLOWS.path(Span::call_site()) {
        Ok(workflows) => generate(&workflows),
        Err(error) => failed(error, item.to_token_stream()),
    }
}

// ---------------------------------------------------------------------------
// #[workflow]
// ---------------------------------------------------------------------------

/// A parsed `#[workflow]`.
pub(crate) struct ParsedWorkflow {
    item: ItemFn,
    input: Type,
    output: Type,
    name: String,
    version: Option<u32>,
    min_version: Option<u32>,
    description: String,
}

/// Expands `#[workflow(args)] async fn f(ctx: WorkflowCtx, input: In) -> Result<Out, E>`.
pub(crate) fn expand_workflow(args: TokenStream, item: TokenStream) -> TokenStream {
    match parse_workflow(args, item.clone()) {
        Ok(parsed) => with_runtime(&parsed.item, |w| generate_workflow(&parsed, w)),
        Err(error) => failed(error, item),
    }
}

pub(crate) fn parse_workflow(args: TokenStream, item: TokenStream) -> syn::Result<ParsedWorkflow> {
    let item: ItemFn = syn::parse2(item).map_err(|error| {
        syn::Error::new(
            error.span(),
            "#[workflow] goes on an async fn: \
             `async fn onboarding(ctx: WorkflowCtx, input: Onboarding) -> Result<(), BoxError>`",
        )
    })?;
    let mut errors = Errors::default();
    check_plain_fn(&item, "#[workflow]", true, &mut errors);
    let params = errors.keep(typed_params(&item)).unwrap_or_default();
    let mut input = None;
    if params.len() == 2 {
        if !last_segment_is(&params[0], "WorkflowCtx") {
            errors.push(syn::Error::new_spanned(
                &params[0],
                "the first argument of a workflow is its `WorkflowCtx` (by value)",
            ));
        }
        if matches!(params[1], Type::Reference(_)) {
            errors.push(syn::Error::new_spanned(
                &params[1],
                "a workflow takes its input by value: `input: Onboarding`",
            ));
        } else {
            input = Some(params[1].clone());
        }
    } else {
        errors.push(syn::Error::new_spanned(
            &item.sig.inputs,
            "a workflow takes two arguments: `(ctx: WorkflowCtx, input: Onboarding)`",
        ));
    }
    let output = match result_ok(&item.sig.output) {
        Some(output) => Some(output),
        None => {
            errors.push(syn::Error::new_spanned(
                return_span(&item),
                "a workflow returns `Result<T, E>` (T is stored as the run's output; E converts \
                 into BoxError)",
            ));
            None
        }
    };
    let args = errors.keep(WorkflowArgs::parse(args)).unwrap_or_default();
    let name = match &args.name {
        Some(name) => {
            errors.keep(
                check_workflow_name(&name.value()).map_err(|m| syn::Error::new(name.span(), m)),
            );
            name.value()
        }
        None => item.sig.ident.to_string().to_kebab_case(),
    };
    let version = args.version.as_ref().and_then(|v| errors.keep(v.base10_parse::<u32>()));
    let min_version = args
        .min_version
        .as_ref()
        .and_then(|v| errors.keep(v.base10_parse::<u32>()));
    if let (Some(0), Some(literal)) = (version, &args.version) {
        errors.push(syn::Error::new(literal.span(), "a workflow version is at least 1"));
    }
    if let (Some(min), Some(literal)) = (min_version, &args.min_version)
        && min > version.unwrap_or(1)
    {
        errors.push(syn::Error::new(
            literal.span(),
            format!(
                "min_version ({min}) cannot be above version ({})",
                version.unwrap_or(1)
            ),
        ));
    }
    errors.finish()?;
    Ok(ParsedWorkflow {
        description: doc_text(&item.attrs),
        input: input.expect("checked above"),
        output: output.expect("checked above"),
        name,
        version,
        min_version,
        item,
    })
}

pub(crate) fn generate_workflow(parsed: &ParsedWorkflow, w: &TokenStream) -> TokenStream {
    let ParsedWorkflow {
        item,
        input,
        output,
        name,
        version,
        min_version,
        description,
    } = parsed;
    let function = &item.sig.ident;
    let version = version.map(|v| quote!(const VERSION: u32 = #v;));
    let min_version = min_version.map(|v| quote!(const MIN_VERSION: u32 = #v;));
    let box_error = box_error();
    quote! {
        #item

        impl #w::Workflow for #input {
            const NAME: &'static str = #name;
            #version
            #min_version
            const DESCRIPTION: &'static str = #description;
            type Output = #output;

            fn run(
                self,
                ctx: #w::WorkflowCtx,
            ) -> impl ::core::future::Future<
                Output = ::core::result::Result<Self::Output, #box_error>,
            > + ::core::marker::Send {
                async move {
                    let result: ::core::result::Result<Self::Output, #box_error> =
                        #function(ctx, self).await.map_err(::core::convert::Into::into);
                    result
                }
            }
        }
    }
}

/// `name = "..", version = N, min_version = N`.
#[derive(Default)]
struct WorkflowArgs {
    name: Option<LitStr>,
    version: Option<LitInt>,
    min_version: Option<LitInt>,
}

impl WorkflowArgs {
    fn parse(args: TokenStream) -> syn::Result<Self> {
        let mut parsed = WorkflowArgs::default();
        let mut seen: Vec<String> = Vec::new();
        let parser = syn::meta::parser(|meta| {
            let key = key_name(&meta.path);
            if seen.contains(&key) {
                return Err(meta.error(format!("duplicate `{key}` key")));
            }
            match key.as_str() {
                "name" => parsed.name = Some(meta.value()?.parse()?),
                "version" => parsed.version = Some(meta.value()?.parse()?),
                "min_version" => parsed.min_version = Some(meta.value()?.parse()?),
                _ => {
                    return Err(meta.error(format!(
                        "unknown #[workflow] key `{key}`: expected `name = \"..\"`, \
                         `version = N` or `min_version = N`"
                    )));
                }
            }
            seen.push(key);
            Ok(())
        });
        syn::parse::Parser::parse2(parser, args)?;
        Ok(parsed)
    }
}

/// navree-workflows' name rule.
fn check_workflow_name(name: &str) -> Result<(), String> {
    let valid =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.');
    let starts_well = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if name.is_empty() || name.len() > MAX_NAME_LEN || !starts_well || !name.chars().all(valid) {
        return Err(format!(
            "invalid workflow name `{name}`: use 1 to {MAX_NAME_LEN} lower-case letters, \
             digits, `-`, `_` and `.`, starting with a letter or a digit (`onboarding`)"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Blocks: #[trigger], #[condition], #[action]
// ---------------------------------------------------------------------------

/// The keys the block attributes share, and their own.
#[derive(Default)]
struct BlockArgs {
    block_type: Option<LitStr>,
    label: Option<LitStr>,
    /// `#[trigger]`: the subject field.
    subject: Option<LitStr>,
    /// `#[trigger]`: the workspace field (`Some(None)`: `workspace = false`).
    workspace: Option<Option<LitStr>>,
    /// `#[trigger]` on a struct: the event type.
    event: Option<Type>,
    /// `#[action]`: a `fn(&Config, &AutomationCtx) -> Value` preview.
    preview: Option<syn::Path>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Trigger,
    Condition,
    Action,
}

impl BlockKind {
    fn attribute(self) -> &'static str {
        match self {
            BlockKind::Trigger => "#[trigger]",
            BlockKind::Condition => "#[condition]",
            BlockKind::Action => "#[action]",
        }
    }

    fn keys(self) -> &'static str {
        match self {
            BlockKind::Trigger => {
                "`type = \"..\"`, `label = \"..\"`, `subject = \"..\"`, `workspace = \"..\"` \
                 (or `false`) or `event = Type`"
            }
            BlockKind::Condition => "`type = \"..\"` or `label = \"..\"`",
            BlockKind::Action => "`type = \"..\"`, `label = \"..\"` or `preview = path`",
        }
    }
}

impl BlockArgs {
    fn parse(kind: BlockKind, args: TokenStream) -> syn::Result<Self> {
        let mut parsed = BlockArgs::default();
        let mut seen: Vec<String> = Vec::new();
        let parser = syn::meta::parser(|meta| {
            let key = key_name(&meta.path);
            if seen.contains(&key) {
                return Err(meta.error(format!("duplicate `{key}` key")));
            }
            let unknown = || {
                meta.error(format!(
                    "unknown {} key `{key}`: expected {}",
                    kind.attribute(),
                    kind.keys()
                ))
            };
            match key.as_str() {
                "type" => parsed.block_type = Some(meta.value()?.parse()?),
                "label" => parsed.label = Some(meta.value()?.parse()?),
                "subject" if kind == BlockKind::Trigger => {
                    parsed.subject = Some(meta.value()?.parse()?);
                }
                "workspace" if kind == BlockKind::Trigger => {
                    parsed.workspace = Some(match meta.value()?.parse::<Lit>()? {
                        Lit::Str(path) => Some(path),
                        Lit::Bool(LitBool { value: false, .. }) => None,
                        other => {
                            return Err(syn::Error::new_spanned(
                                other,
                                "`workspace` is the event's workspace field (`\"team_id\"`), \
                                 or `false` when the event has none",
                            ));
                        }
                    });
                }
                "event" if kind == BlockKind::Trigger => {
                    parsed.event = Some(meta.value()?.parse()?);
                }
                "preview" if kind == BlockKind::Action => {
                    parsed.preview = Some(meta.value()?.parse()?);
                }
                _ => return Err(unknown()),
            }
            seen.push(key);
            Ok(())
        });
        syn::parse::Parser::parse2(parser, args)?;
        Ok(parsed)
    }
}

/// A parsed block attribute.
pub(crate) struct ParsedBlock {
    kind: BlockKind,
    item: Item,
    /// The configuration type the trait is implemented on.
    config: Type,
    block_type: LitStr,
    label: Option<LitStr>,
    description: String,
    subject: Option<LitStr>,
    workspace: Option<Option<LitStr>>,
    /// The trigger's event type.
    event: Option<Type>,
    /// The function, when the attribute is on one.
    function: Option<BlockFn>,
    preview: Option<syn::Path>,
}

/// What the expansion needs to know about the function of a block.
struct BlockFn {
    ident: syn::Ident,
    is_async: bool,
    /// For conditions and actions: whether it returns a `Result`.
    returns_result: bool,
    /// For actions: what it returns (the `T` of `Result<T, E>`).
    output: Option<Type>,
}

/// Expands `#[trigger]`, `#[condition]` or `#[action]`.
fn expand_block(kind: BlockKind, args: TokenStream, item: TokenStream) -> TokenStream {
    match parse_block(kind, args, item.clone()) {
        Ok(parsed) => with_runtime(&parsed.item, |w| generate_block(&parsed, w)),
        Err(error) => failed(error, item),
    }
}

pub(crate) fn expand_trigger(args: TokenStream, item: TokenStream) -> TokenStream {
    expand_block(BlockKind::Trigger, args, item)
}

pub(crate) fn expand_condition(args: TokenStream, item: TokenStream) -> TokenStream {
    expand_block(BlockKind::Condition, args, item)
}

pub(crate) fn expand_action(args: TokenStream, item: TokenStream) -> TokenStream {
    expand_block(BlockKind::Action, args, item)
}

fn parse_block(kind: BlockKind, args: TokenStream, item: TokenStream) -> syn::Result<ParsedBlock> {
    let attribute = kind.attribute();
    let shape = match kind {
        BlockKind::Trigger => {
            "fn deal_won(config: &DealWonTrigger, event: &DealWon) -> bool, or a configuration \
             struct with `event = DealWon`"
        }
        BlockKind::Condition => {
            "async fn amount_at_least(config: &AmountAtLeast, ctx: &AutomationCtx) \
             -> Result<bool, BoxError>"
        }
        BlockKind::Action => {
            "async fn send_note(config: SendNote, ctx: AutomationCtx) -> Result<Value, BoxError>"
        }
    };
    let item: Item = syn::parse2(item).map_err(|error| {
        syn::Error::new(error.span(), format!("{attribute} goes on {shape}"))
    })?;
    let mut errors = Errors::default();
    // Missing keys are only reported when the keys parsed.
    let parsed_args = errors.keep(BlockArgs::parse(kind, args));
    let args_parsed = parsed_args.is_some();
    let args = parsed_args.unwrap_or_default();
    let block_type = match &args.block_type {
        Some(block_type) => {
            if block_type.value().trim().is_empty() {
                errors.push(syn::Error::new(
                    block_type.span(),
                    "the block type is the stable name stored in definitions (`deal.won`)",
                ));
            }
            Some(block_type.clone())
        }
        None if args_parsed => {
            errors.push(syn::Error::new(
                Span::call_site(),
                format!(
                    "{attribute} needs `type = \"..\"`: the stable name stored in automation \
                     definitions (`deal.won`, `mail.send`)"
                ),
            ));
            None
        }
        None => None,
    };
    let (config, event, function, attrs) = match &item {
        Item::Struct(ItemStruct {
            ident,
            generics,
            attrs,
            ..
        }) if kind == BlockKind::Trigger => {
            if !generics.params.is_empty() {
                errors.push(syn::Error::new_spanned(
                    generics,
                    "a trigger's configuration cannot be generic",
                ));
            }
            if args_parsed && args.event.is_none() {
                errors.push(syn::Error::new(
                    Span::call_site(),
                    "#[trigger] on a configuration struct needs `event = EventType`",
                ));
            }
            let config: Type = syn::parse_quote!(#ident);
            (Some(config), args.event.clone(), None, attrs.clone())
        }
        Item::Fn(function) => {
            if kind == BlockKind::Trigger && args.event.is_some() {
                errors.push(syn::Error::new(
                    Span::call_site(),
                    "`event` is for #[trigger] on a struct; on a fn the event is the type of \
                     its second argument",
                ));
            }
            let parsed = errors.keep(block_fn(kind, function));
            match parsed {
                Some((config, event, block)) => {
                    (Some(config), event, Some(block), function.attrs.clone())
                }
                None => (None, None, None, function.attrs.clone()),
            }
        }
        other => {
            errors.push(syn::Error::new_spanned(
                other,
                format!("{attribute} goes on {shape}"),
            ));
            (None, None, None, Vec::new())
        }
    };
    errors.finish()?;
    Ok(ParsedBlock {
        kind,
        description: doc_text(&attrs),
        config: config.expect("checked above"),
        block_type: block_type.expect("checked above"),
        label: args.label,
        subject: args.subject,
        workspace: args.workspace,
        event,
        function,
        preview: args.preview,
        item,
    })
}

/// The configuration type, the trigger's event type and what the
/// expansion needs from a block function.
fn block_fn(kind: BlockKind, item: &ItemFn) -> syn::Result<(Type, Option<Type>, BlockFn)> {
    let attribute = kind.attribute();
    let mut errors = Errors::default();
    check_plain_fn(item, attribute, false, &mut errors);
    if kind == BlockKind::Trigger && item.sig.asyncness.is_some() {
        errors.push(syn::Error::new_spanned(
            item.sig.asyncness,
            "a trigger's match is a plain fn (it runs for every event): drop `async`",
        ));
    }
    let params = errors.keep(typed_params(item)).unwrap_or_default();
    let (config_shape, ctx_shape, by_ref) = match kind {
        BlockKind::Trigger => ("config: &DealWonTrigger", "event: &DealWon", true),
        BlockKind::Condition => ("config: &AmountAtLeast", "ctx: &AutomationCtx", true),
        BlockKind::Action => ("config: SendNote", "ctx: AutomationCtx", false),
    };
    let mut config = None;
    let mut event = None;
    if params.len() == 2 {
        let read = |ty: &Type, shape: &str, errors: &mut Errors| -> Option<Type> {
            match (ty, by_ref) {
                (Type::Reference(reference), true) if reference.mutability.is_none() => {
                    Some((*reference.elem).clone())
                }
                (Type::Reference(_), false) | (_, true) => {
                    errors.push(syn::Error::new_spanned(
                        ty,
                        format!("{attribute} takes `{shape}`"),
                    ));
                    None
                }
                (other, false) => Some(other.clone()),
            }
        };
        config = read(&params[0], config_shape, &mut errors);
        let second = read(&params[1], ctx_shape, &mut errors);
        match kind {
            BlockKind::Trigger => event = second,
            BlockKind::Condition | BlockKind::Action => {
                if let Some(ctx) = &second
                    && !last_segment_is(ctx, "AutomationCtx")
                {
                    errors.push(syn::Error::new_spanned(
                        &params[1],
                        format!("the second argument is `{ctx_shape}`"),
                    ));
                }
            }
        }
    } else {
        errors.push(syn::Error::new_spanned(
            &item.sig.inputs,
            format!("{attribute} takes two arguments: `({config_shape}, {ctx_shape})`"),
        ));
    }
    let ok = result_ok(&item.sig.output);
    let mut output = None;
    match kind {
        BlockKind::Trigger => {
            if !returns(&item.sig.output, "bool") {
                errors.push(syn::Error::new_spanned(
                    return_span(item),
                    "a trigger's match returns `bool`",
                ));
            }
        }
        BlockKind::Condition => {
            let is_bool = ok.as_ref().is_some_and(|ok| is_type(ok, "bool"))
                || returns(&item.sig.output, "bool");
            if !is_bool {
                errors.push(syn::Error::new_spanned(
                    return_span(item),
                    "a condition returns `bool` or `Result<bool, E>`",
                ));
            }
        }
        BlockKind::Action => match &ok {
            Some(ok) => output = Some(ok.clone()),
            None => errors.push(syn::Error::new_spanned(
                return_span(item),
                "an action returns `Result<T, E>` (T is stored and readable by later nodes)",
            )),
        },
    }
    errors.finish()?;
    Ok((
        config.expect("checked above"),
        event,
        BlockFn {
            ident: item.sig.ident.clone(),
            is_async: item.sig.asyncness.is_some(),
            returns_result: ok.is_some(),
            output,
        },
    ))
}

fn generate_block(parsed: &ParsedBlock, w: &TokenStream) -> TokenStream {
    let ParsedBlock {
        kind,
        item,
        config,
        block_type,
        label,
        description,
        ..
    } = parsed;
    let label = label
        .as_ref()
        .map(|label| quote!(const LABEL: &'static str = #label;));
    let common = quote! {
        const TYPE: &'static str = #block_type;
        #label
        const DESCRIPTION: &'static str = #description;
    };
    let box_error = box_error();
    let body = match kind {
        BlockKind::Trigger => {
            let subject = parsed.subject.as_ref().map(|subject| {
                quote!(const SUBJECT: ::core::option::Option<&'static str> =
                    ::core::option::Option::Some(#subject);)
            });
            let workspace = parsed.workspace.as_ref().map(|workspace| match workspace {
                Some(path) => quote!(const WORKSPACE: ::core::option::Option<&'static str> =
                    ::core::option::Option::Some(#path);),
                None => quote!(const WORKSPACE: ::core::option::Option<&'static str> =
                    ::core::option::Option::None;),
            });
            let event = parsed.event.as_ref().expect("a trigger has an event");
            let matches = parsed.function.as_ref().map(|function| {
                let ident = &function.ident;
                quote! {
                    fn matches(&self, event: &Self::Event) -> bool {
                        #ident(self, event)
                    }
                }
            });
            quote! {
                impl #w::Trigger for #config {
                    #common
                    #subject
                    #workspace
                    type Event = #event;
                    #matches
                }
            }
        }
        BlockKind::Condition => {
            let function = parsed.function.as_ref().expect("a condition has a fn");
            let ident = &function.ident;
            let call = if function.is_async {
                quote!(#ident(self, ctx).await)
            } else {
                quote!(#ident(self, ctx))
            };
            let result = if function.returns_result {
                quote!(#call.map_err(::core::convert::Into::into))
            } else {
                quote!(::core::result::Result::Ok(#call))
            };
            quote! {
                impl #w::Condition for #config {
                    #common

                    fn evaluate(
                        &self,
                        ctx: &#w::AutomationCtx,
                    ) -> impl ::core::future::Future<
                        Output = ::core::result::Result<bool, #box_error>,
                    > + ::core::marker::Send {
                        async move {
                            let result: ::core::result::Result<bool, #box_error> = #result;
                            result
                        }
                    }
                }
            }
        }
        BlockKind::Action => {
            let function = parsed.function.as_ref().expect("an action has a fn");
            let ident = &function.ident;
            let output = function.output.as_ref().expect("an action has an output");
            let call = if function.is_async {
                quote!(#ident(self, ctx).await)
            } else {
                quote!(#ident(self, ctx))
            };
            let preview = parsed.preview.as_ref().map(|preview| {
                quote! {
                    fn preview(&self, ctx: &#w::AutomationCtx) -> #w::__private::Value {
                        #preview(self, ctx)
                    }
                }
            });
            quote! {
                impl #w::Action for #config {
                    #common
                    type Output = #output;

                    fn run(
                        self,
                        ctx: #w::AutomationCtx,
                    ) -> impl ::core::future::Future<
                        Output = ::core::result::Result<Self::Output, #box_error>,
                    > + ::core::marker::Send {
                        async move {
                            let result: ::core::result::Result<Self::Output, #box_error> =
                                #call.map_err(::core::convert::Into::into);
                            result
                        }
                    }

                    #preview
                }
            }
        }
    };
    quote! {
        #item

        #body
    }
}

// ---------------------------------------------------------------------------
// Shared checks
// ---------------------------------------------------------------------------

/// A free fn without generics or `self` (and async when `must_be_async`).
fn check_plain_fn(item: &ItemFn, attribute: &str, must_be_async: bool, errors: &mut Errors) {
    if must_be_async && item.sig.asyncness.is_none() {
        errors.push(syn::Error::new_spanned(
            item.sig.fn_token,
            format!("{attribute} goes on an `async fn`"),
        ));
    }
    if !item.sig.generics.params.is_empty() || item.sig.generics.where_clause.is_some() {
        errors.push(syn::Error::new_spanned(
            &item.sig.generics,
            format!("{attribute} goes on a fn without generic parameters"),
        ));
    }
}

/// The types of the arguments; `self` is an error.
fn typed_params(item: &ItemFn) -> syn::Result<Vec<Type>> {
    item.sig
        .inputs
        .iter()
        .map(|input| match input {
            FnArg::Typed(typed) => Ok((*typed.ty).clone()),
            FnArg::Receiver(receiver) => Err(syn::Error::new_spanned(
                receiver,
                "this attribute goes on a free fn, not a method",
            )),
        })
        .collect()
}

/// Whether `ty` is a path whose last segment is `name`.
fn last_segment_is(ty: &Type, name: &str) -> bool {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == name),
        _ => false,
    }
}

/// Whether `ty` is exactly the type named `name` (`bool`).
fn is_type(ty: &Type, name: &str) -> bool {
    match ty {
        Type::Path(path) => path.qself.is_none() && path.path.is_ident(name),
        _ => false,
    }
}

/// Whether the fn returns the type named `name`.
fn returns(output: &ReturnType, name: &str) -> bool {
    match output {
        ReturnType::Type(_, ty) => is_type(ty, name),
        ReturnType::Default => false,
    }
}

/// The `T` of a `Result<T, ..>` return type (any path ending in `Result`).
fn result_ok(output: &ReturnType) -> Option<Type> {
    let ReturnType::Type(_, ty) = output else {
        return None;
    };
    let Type::Path(path) = &**ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "Result" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        GenericArgument::Type(ty) => Some(ty.clone()),
        _ => None,
    })
}

/// What an error about the return type points at.
fn return_span(item: &ItemFn) -> TokenStream {
    match &item.sig.output {
        ReturnType::Type(_, ty) => ty.to_token_stream(),
        ReturnType::Default => item.sig.ident.to_token_stream(),
    }
}

/// The doc comment, its lines trimmed and joined with spaces.
fn doc_text(attrs: &[Attribute]) -> String {
    let lines: Vec<String> = attrs
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
        .flat_map(|text| {
            text.lines()
                .map(|line| line.trim().to_owned())
                .collect::<Vec<_>>()
        })
        .filter(|line| !line.is_empty())
        .collect();
    lines.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow(args: TokenStream, item: TokenStream) -> String {
        match parse_workflow(args, item) {
            Ok(parsed) => generate_workflow(&parsed, &quote!(::navree::workflows)).to_string(),
            Err(error) => format!("error: {error}"),
        }
    }

    fn block(kind: BlockKind, args: TokenStream, item: TokenStream) -> String {
        match parse_block(kind, args, item) {
            Ok(parsed) => generate_block(&parsed, &quote!(::navree::workflows)).to_string(),
            Err(error) => format!("error: {error}"),
        }
    }

    #[test]
    fn a_workflow_is_implemented_on_its_input() {
        let out = workflow(
            quote!(),
            quote! {
                /// Welcomes a new user,
                /// then nudges them.
                pub async fn deal_follow_up(ctx: WorkflowCtx, input: FollowUp) -> Result<u32, BoxError> {
                    Ok(1)
                }
            },
        );
        assert!(
            out.contains("impl :: navree :: workflows :: Workflow for FollowUp"),
            "{out}"
        );
        assert!(out.contains("const NAME : & 'static str = \"deal-follow-up\""), "{out}");
        assert!(out.contains("\"Welcomes a new user, then nudges them.\""), "{out}");
        assert!(out.contains("type Output = u32"), "{out}");
        assert!(out.contains("deal_follow_up (ctx , self) . await"), "{out}");
        assert!(!out.contains("const VERSION"), "the trait default applies: {out}");
        assert!(out.contains("pub async fn deal_follow_up"), "the fn is kept: {out}");
    }

    #[test]
    fn workflow_arguments_set_the_name_and_versions() {
        let out = workflow(
            quote!(name = "onboarding", version = 3, min_version = 2),
            quote! {
                async fn onboarding_v3(ctx: navree::workflows::WorkflowCtx, input: OnboardingV3)
                    -> navree::Result<()> { Ok(()) }
            },
        );
        assert!(out.contains("\"onboarding\""), "{out}");
        assert!(out.contains("const VERSION : u32 = 3u32"), "{out}");
        assert!(out.contains("const MIN_VERSION : u32 = 2u32"), "{out}");
        assert!(out.contains("type Output = ()"), "{out}");
    }

    #[test]
    fn workflow_mistakes_are_reported() {
        let cases = [
            (quote!(), quote!(fn f(ctx: WorkflowCtx, i: In) -> Result<(), E> {}), "goes on an `async fn`"),
            (quote!(), quote!(struct S;), "#[workflow] goes on an async fn"),
            (quote!(), quote!(async fn f(i: In) -> Result<(), E> {}), "takes two arguments"),
            (quote!(), quote!(async fn f(ctx: &WorkflowCtx, i: In) -> Result<(), E> {}), "is its `WorkflowCtx`"),
            (quote!(), quote!(async fn f(ctx: WorkflowCtx, i: &In) -> Result<(), E> {}), "input by value"),
            (quote!(), quote!(async fn f(ctx: WorkflowCtx, i: In) -> u32 {}), "returns `Result<T, E>`"),
            (quote!(), quote!(async fn f<T>(ctx: WorkflowCtx, i: In) -> Result<(), E> {}), "without generic"),
            (quote!(name = "Bad Name"), quote!(async fn f(ctx: WorkflowCtx, i: In) -> Result<(), E> {}), "invalid workflow name"),
            (quote!(version = 0), quote!(async fn f(ctx: WorkflowCtx, i: In) -> Result<(), E> {}), "at least 1"),
            (quote!(version = 2, min_version = 3), quote!(async fn f(ctx: WorkflowCtx, i: In) -> Result<(), E> {}), "cannot be above version (2)"),
            (quote!(retries = 3), quote!(async fn f(ctx: WorkflowCtx, i: In) -> Result<(), E> {}), "unknown #[workflow] key `retries`"),
            (quote!(name = "a", name = "b"), quote!(async fn f(ctx: WorkflowCtx, i: In) -> Result<(), E> {}), "duplicate `name` key"),
        ];
        for (args, item, expected) in cases {
            let out = workflow(args, item);
            assert!(out.contains(expected), "expected `{expected}` in: {out}");
        }
    }

    #[test]
    fn a_trigger_fn_implements_matches() {
        let out = block(
            BlockKind::Trigger,
            quote!(type = "deal.won", label = "Deal won", subject = "owner_id", workspace = "team_id"),
            quote! {
                /// A deal moved to won.
                fn deal_won(config: &DealWonTrigger, event: &DealWon) -> bool { true }
            },
        );
        assert!(out.contains("impl :: navree :: workflows :: Trigger for DealWonTrigger"), "{out}");
        assert!(out.contains("const TYPE : & 'static str = \"deal.won\""), "{out}");
        assert!(out.contains("const LABEL : & 'static str = \"Deal won\""), "{out}");
        assert!(out.contains("\"A deal moved to won.\""), "{out}");
        assert!(out.contains("Some (\"owner_id\")"), "{out}");
        assert!(out.contains("Some (\"team_id\")"), "{out}");
        assert!(out.contains("type Event = DealWon"), "{out}");
        assert!(out.contains("deal_won (self , event)"), "{out}");
    }

    #[test]
    fn a_trigger_struct_names_its_event() {
        let out = block(
            BlockKind::Trigger,
            quote!(type = "task.completed", event = TaskCompleted, workspace = false),
            quote! {
                #[derive(Serialize)]
                pub struct TaskCompletedTrigger {}
            },
        );
        assert!(out.contains("Trigger for TaskCompletedTrigger"), "{out}");
        assert!(out.contains("type Event = TaskCompleted"), "{out}");
        assert!(out.contains("WORKSPACE : :: core :: option :: Option < & 'static str > = :: core :: option :: Option :: None"), "{out}");
        assert!(!out.contains("fn matches"), "{out}");
        assert!(!out.contains("const LABEL"), "{out}");
    }

    #[test]
    fn conditions_take_bools_or_results() {
        let out = block(
            BlockKind::Condition,
            quote!(type = "deal.amount_at_least"),
            quote! {
                async fn at_least(config: &AmountAtLeast, ctx: &AutomationCtx) -> Result<bool, BoxError> { Ok(true) }
            },
        );
        assert!(out.contains("Condition for AmountAtLeast"), "{out}");
        assert!(out.contains("at_least (self , ctx) . await . map_err"), "{out}");
        let sync = block(
            BlockKind::Condition,
            quote!(type = "deal.big"),
            quote! {
                fn big(config: &Big, ctx: &AutomationCtx) -> bool { true }
            },
        );
        assert!(sync.contains("Ok (big (self , ctx))"), "{sync}");
    }

    #[test]
    fn actions_store_their_output_and_may_preview() {
        let out = block(
            BlockKind::Action,
            quote!(type = "note.send", label = "Send a note", preview = note_preview),
            quote! {
                async fn send_note(config: SendNote, ctx: AutomationCtx) -> Result<NoteSent, BoxError> { todo!() }
            },
        );
        assert!(out.contains("Action for SendNote"), "{out}");
        assert!(out.contains("type Output = NoteSent"), "{out}");
        assert!(out.contains("send_note (self , ctx) . await"), "{out}");
        assert!(
            out.contains("fn preview (& self , ctx : & :: navree :: workflows :: AutomationCtx) -> :: navree :: workflows :: __private :: Value { note_preview (self , ctx) }"),
            "{out}"
        );
    }

    #[test]
    fn block_mistakes_are_reported() {
        let cases = [
            (BlockKind::Trigger, quote!(), quote!(fn t(c: &C, e: &E) -> bool { true }), "needs `type = \"..\"`"),
            (BlockKind::Trigger, quote!(type = "t"), quote!(struct C;), "needs `event = EventType`"),
            (BlockKind::Trigger, quote!(type = "t", event = E), quote!(fn t(c: &C, e: &E) -> bool { true }), "`event` is for #[trigger] on a struct"),
            (BlockKind::Trigger, quote!(type = "t"), quote!(async fn t(c: &C, e: &E) -> bool { true }), "drop `async`"),
            (BlockKind::Trigger, quote!(type = "t"), quote!(fn t(c: C, e: &E) -> bool { true }), "takes `config: &DealWonTrigger`"),
            (BlockKind::Trigger, quote!(type = "t"), quote!(fn t(c: &C, e: &E) -> u8 { 1 }), "returns `bool`"),
            (BlockKind::Trigger, quote!(type = "t", workspace = true), quote!(fn t(c: &C, e: &E) -> bool { true }), "or `false` when the event has none"),
            (BlockKind::Trigger, quote!(type = "t", preview = p), quote!(fn t(c: &C, e: &E) -> bool { true }), "unknown #[trigger] key `preview`"),
            (BlockKind::Condition, quote!(type = "c"), quote!(fn c(x: &C, ctx: &Other) -> bool { true }), "the second argument is `ctx: &AutomationCtx`"),
            (BlockKind::Condition, quote!(type = "c"), quote!(fn c(x: &C, ctx: &AutomationCtx) -> u8 { 1 }), "returns `bool` or `Result<bool, E>`"),
            (BlockKind::Condition, quote!(type = "c", subject = "x"), quote!(fn c(x: &C, ctx: &AutomationCtx) -> bool { true }), "unknown #[condition] key `subject`"),
            (BlockKind::Action, quote!(type = "a"), quote!(async fn a(x: &C, ctx: AutomationCtx) -> Result<(), E> { Ok(()) }), "takes `config: SendNote`"),
            (BlockKind::Action, quote!(type = "a"), quote!(async fn a(x: C, ctx: AutomationCtx) -> u8 { 1 }), "returns `Result<T, E>`"),
            (BlockKind::Action, quote!(type = "a"), quote!(async fn a(x: C) -> Result<(), E> { Ok(()) }), "takes two arguments"),
            (BlockKind::Action, quote!(type = "a"), quote!(enum A { X }), "#[action] goes on"),
            (BlockKind::Action, quote!(type = " "), quote!(async fn a(x: C, ctx: AutomationCtx) -> Result<(), E> { Ok(()) }), "the stable name stored"),
            (BlockKind::Action, quote!(type = "a", type = "b"), quote!(async fn a(x: C, ctx: AutomationCtx) -> Result<(), E> { Ok(()) }), "duplicate `type` key"),
        ];
        for (kind, args, item, expected) in cases {
            let out = block(kind, args, item);
            assert!(out.contains(expected), "expected `{expected}` in: {out}");
        }
    }
}
