//! `#[derive(ToSchema)]`: the JSON Schema of a struct or an enum, from its
//! fields, its serde attributes, its doc comments and `#[schema(..)]`.

use std::cell::Cell;

use proc_macro2::{Delimiter, Span, TokenStream, TokenTree};
use quote::{ToTokens, quote};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprLit, Field, Fields, GenericArgument, Generics, Ident,
    Lit, LitStr, Member, Path, PathArguments, Token, Type,
};

use crate::attr::Errors;
use crate::paths::OPENAPI;

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    match try_expand(input) {
        Ok(tokens) => tokens,
        Err(error) => error.to_compile_error(),
    }
}

fn try_expand(input: TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = syn::parse2(input)?;
    let krate = OPENAPI.path(Span::call_site())?;
    let serde = SerdeContainer::parse(&input.attrs);
    let container = ContainerArgs::parse(&input.attrs)?;
    refuse_what_replaces_the_json(&input)?;
    let cx = Context {
        krate: &krate,
        root: &input.ident,
        generics: &input.generics,
        inlines: Cell::new(false),
    };
    let body = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) if serde.transparent => {
                let kept: Punctuated<Field, Token![,]> = fields
                    .named
                    .iter()
                    .filter(|field| !SerdeField::parse(&field.attrs).skip)
                    .cloned()
                    .collect();
                cx.tuple_struct(&kept)?
            }
            Fields::Named(fields) => cx.named_struct(&fields.named, &serde, true)?,
            Fields::Unnamed(fields) => cx.tuple_struct(&fields.unnamed)?,
            Fields::Unit => Body {
                schema: quote!(#krate::Schema::null().default_value(#krate::__private::serde_json::Value::Null)),
                components: TokenStream::new(),
            },
        },
        Data::Enum(data) => cx.enumeration(&data.variants, &serde)?,
        Data::Union(_) => {
            return Err(syn::Error::new(
                input.ident.span(),
                "#[derive(ToSchema)] goes on a struct or an enum",
            ));
        }
    };

    let description = container
        .description
        .map(|text| text.value())
        .or_else(|| doc_comment(&input.attrs));
    let describe = description.map(|text| quote!(.description(#text)));
    let name = match &container.name {
        Some(path) => path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>()
            .join("."),
        None => input.ident.to_string(),
    };
    let ident = &input.ident;
    let mut generics = input.generics.clone();
    let params: Vec<Ident> = generics.type_params().map(|p| p.ident.clone()).collect();
    if !params.is_empty() {
        let where_clause = generics.make_where_clause();
        for param in &params {
            where_clause
                .predicates
                .push(syn::parse_quote!(#param: #krate::ToSchema));
        }
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let schema = body.schema;
    let components = body.components;
    let name_of = if params.is_empty() {
        quote!(::std::borrow::Cow::Borrowed(#name))
    } else {
        quote! {{
            let mut name = ::std::string::String::from(#name);
            #(
                name.push('_');
                name.push_str(&<#params as #krate::ToSchema>::name());
            )*
            ::std::borrow::Cow::Owned(name)
        }}
    };
    let guard = cx.inlines.get().then(|| {
        quote! {
            let _inlining = #krate::__private::InlineGuard::enter(::std::any::type_name::<Self>());
        }
    });
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #krate::ToSchema for #ident #ty_generics #where_clause {
            fn name() -> ::std::borrow::Cow<'static, str> {
                #name_of
            }

            fn schema() -> #krate::Schema {
                #guard
                (#schema) #describe
            }

            fn schemas(out: &mut ::std::vec::Vec<(::std::string::String, #krate::Schema)>) {
                let _ = &out;
                #components
            }
        }
    })
}

/// A compile error for serde attributes that replace a type's JSON, or a
/// field's, with something the derive can't see.
fn refuse_what_replaces_the_json(input: &DeriveInput) -> syn::Result<()> {
    for (key, _) in serde_entries(&input.attrs) {
        if matches!(key.as_str(), "from" | "try_from" | "into" | "remote") {
            return Err(syn::Error::new(
                input.ident.span(),
                format!(
                    "#[serde({key} = ..)] replaces this type's JSON, which #[derive(ToSchema)] can't see: write `impl ToSchema` by hand"
                ),
            ));
        }
    }
    let check_field = |field: &Field| -> syn::Result<()> {
        let args = FieldArgs::parse(&field.attrs)?;
        if args.value_type.is_some() || args.schema_with.is_some() {
            return Ok(());
        }
        for (key, _) in serde_entries(&field.attrs) {
            if matches!(key.as_str(), "with" | "serialize_with" | "getter") {
                return Err(syn::Error::new(
                    field.span(),
                    format!(
                        "#[serde({key} = ..)] changes this field's JSON, which #[derive(ToSchema)] can't see: say what it is with #[schema(value_type = ..)] or #[schema(schema_with = ..)]"
                    ),
                ));
            }
        }
        Ok(())
    };
    match &input.data {
        Data::Struct(data) => data.fields.iter().try_for_each(check_field),
        Data::Enum(data) => data
            .variants
            .iter()
            .flat_map(|variant| variant.fields.iter())
            .try_for_each(check_field),
        Data::Union(_) => Ok(()),
    }
}

/// What the derive makes of a struct, an enum or a variant.
struct Body {
    /// An expression: the schema.
    schema: TokenStream,
    /// Statements adding to `out` the components the schema refers to.
    components: TokenStream,
}

/// The derive's input.
struct Context<'a> {
    krate: &'a TokenStream,
    root: &'a Ident,
    generics: &'a Generics,
    /// Whether a field says `#[schema(inline)]`.
    inlines: Cell<bool>,
}

impl Context<'_> {
    /// An object of the fields, or with flattened fields an `allOf`.
    fn named_struct(
        &self,
        fields: &Punctuated<Field, Token![,]>,
        serde: &SerdeContainer,
        with_defaults: bool,
    ) -> syn::Result<Body> {
        let krate = self.krate;
        let mut errors = Errors::default();
        let mut object = quote!(#krate::Schema::object());
        let mut flattened = Vec::new();
        let mut flattened_map = None;
        let mut components = TokenStream::new();
        let mut has_properties = false;
        for field in fields {
            let Some(parsed) = errors.keep(self.field(field, serde, with_defaults)) else {
                continue;
            };
            let Some(parsed) = parsed else { continue };
            components.extend(parsed.components);
            let schema = parsed.schema;
            if parsed.flatten {
                if parsed.is_map {
                    flattened_map = Some(schema);
                } else {
                    flattened.push(schema);
                }
            } else {
                has_properties = true;
                let required = parsed.required;
                for side in parsed.sides {
                    let name = side.name;
                    let direction = side.direction;
                    object = quote!(#object.property(#name, #schema #direction, #required));
                }
            }
        }
        errors.finish()?;
        if let Some(map) = flattened_map {
            object = quote! {{
                let mut object = #object;
                object.additional_properties = (#map).additional_properties;
                object
            }};
        }
        let schema = if flattened.is_empty() {
            if serde.deny_unknown_fields {
                object = quote!(#object.no_additional_properties());
            }
            object
        } else {
            if has_properties {
                flattened.push(object);
            }
            quote!(#krate::Schema::all_of(::std::vec![#(#flattened),*]))
        };
        Ok(Body { schema, components })
    }

    /// One field of a struct or a variant; `None` when serde skips it.
    fn field(
        &self,
        field: &Field,
        serde: &SerdeContainer,
        with_defaults: bool,
    ) -> syn::Result<Option<ParsedField>> {
        let krate = self.krate;
        let rules = SerdeField::parse(&field.attrs);
        let args = FieldArgs::parse(&field.attrs)?;
        if rules.skip {
            return Ok(None);
        }
        let ident = field
            .ident
            .as_ref()
            .ok_or_else(|| syn::Error::new(field.span(), "a named field has a name"))?;
        let rust_name = ident.to_string();
        let rust_name = rust_name.strip_prefix("r#").unwrap_or(&rust_name);
        let sides = sides(&rules, &args, serde, rust_name);
        let ty = args.value_type.as_ref().unwrap_or(&field.ty);
        let optional = is_option(ty);
        let required = args.required.unwrap_or(
            !optional && !rules.skip_serializing_if && !(serde.default || rules.default),
        );
        let is_map = rules.flatten && is_map(ty);

        if let Some(function) = &args.schema_with {
            return Ok(Some(ParsedField {
                sides,
                schema: quote!(#function()),
                components: TokenStream::new(),
                required,
                flatten: rules.flatten,
                is_map,
            }));
        }

        let keep_null = args.nullable != Some(false);
        let peeled = if keep_null { ty } else { peel_option(ty) };
        if args.inline && mentions(peeled, self.root) {
            return Err(syn::Error::new(
                field.span(),
                format!(
                    "#[schema(inline)] would put `{}` inside itself: a type that contains itself needs a `$ref`, so remove `inline`",
                    self.root
                ),
            ));
        }
        let reference = if args.inline {
            self.inline_schema(peeled)
        } else {
            let ty = self.resolved(peeled);
            quote!(<#ty as #krate::ToSchema>::reference())
        };
        let mut schema = reference;
        if args.nullable == Some(true) {
            schema = quote!(#schema.nullable());
        }
        if let Some(format) = &args.format {
            schema = quote!(#schema.format(#format));
        }
        if let Some(value) = &args.minimum {
            schema = quote!(#schema.minimum(#krate::__private::serde_json::json!(#value)));
        }
        if let Some(value) = &args.maximum {
            schema = quote!(#schema.maximum(#krate::__private::serde_json::json!(#value)));
        }
        if let Some(value) = &args.example {
            schema = quote!(#schema.example(#krate::__private::serde_json::json!(#value)));
        }
        if let Some(text) = &args.title {
            schema = quote!(#schema.title(#text));
        }
        // A value of any shape, or any object, has no default worth writing down.
        if serde.default && with_defaults && !is_untyped(peel_option(ty)) {
            let member = Member::Named(ident.clone());
            let instance = match &serde.default_path {
                Some(path) => {
                    let path: Path = syn::parse_str(path)
                        .map_err(|error| syn::Error::new(field.span(), error.to_string()))?;
                    quote!(::std::option::Option::<Self>::Some(#path()))
                }
                None => quote! {{
                    use #krate::__private::{HasDefault as _, NoDefault as _};
                    (&#krate::__private::DefaultOf::<Self>::new()).get()
                }},
            };
            // No default to show when the type has none, or the field can't be
            // written (generic types, and types that are only read).
            schema = quote! {{
                use #krate::__private::{HasJson as _, NoJson as _};
                let schema = #schema;
                match #instance
                    .and_then(|instance| (&#krate::__private::JsonOf(&instance.#member)).get())
                {
                    ::std::option::Option::Some(value) => schema.default_value(value),
                    ::std::option::Option::None => schema,
                }
            }};
        }
        if has_deprecated(&field.attrs) {
            schema = quote!(#schema.deprecated());
        }
        if let Some(text) = doc_comment(&field.attrs) {
            schema = if args.inline && !optional {
                // A component's own description stays: this one sits next to it.
                quote! {
                    #krate::Schema::all_of(::std::vec![
                        #schema,
                        #krate::Schema::object().description(#text),
                    ])
                }
            } else if args.inline {
                quote!(#schema.description(#text))
            } else {
                quote!(#schema.described(#text))
            };
        }
        Ok(Some(ParsedField {
            sides,
            schema,
            components: self.field_components(peeled, args.inline),
            required,
            flatten: rules.flatten,
            is_map,
        }))
    }

    /// `#[schema(inline)]`: the component's schema instead of a `$ref`, through
    /// `Option` and `Vec`.
    fn inline_schema(&self, ty: &Type) -> TokenStream {
        let krate = self.krate;
        self.inlines.set(true);
        if let Some(inner) = wrapped(ty, "Option") {
            let inner = self.inline_schema(inner);
            return quote!(#krate::Schema::one_of(::std::vec![#inner, #krate::Schema::null()]));
        }
        if let Some(inner) = wrapped(ty, "Vec") {
            let inner = self.inline_schema(inner);
            return quote!(#krate::Schema::array(#inner));
        }
        let ty = self.resolved(ty);
        quote!(<#ty as #krate::ToSchema>::schema())
    }

    /// The statements adding what a field refers to. A generic parameter's
    /// own schema isn't one of them: the app registers `Note` of `Page<Note>`
    /// with `api_schema`, or the document refuses to build.
    fn field_components(&self, ty: &Type, inline: bool) -> TokenStream {
        let krate = self.krate;
        let params: Vec<&Ident> = self.generics.type_params().map(|p| &p.ident).collect();
        let mentioned: Vec<&&Ident> = params.iter().filter(|p| mentions(ty, p)).collect();
        if !mentioned.is_empty() {
            return quote!(#(<#mentioned as #krate::ToSchema>::schemas(out);)*);
        }
        let ty = self.resolved(ty);
        if inline {
            quote!(<#ty as #krate::ToSchema>::schemas(out);)
        } else {
            quote!(<#ty as #krate::ToSchema>::components(out);)
        }
    }

    /// `ty` with the `Object` of `value_type = Object` named by its path.
    fn resolved(&self, ty: &Type) -> Type {
        let mut ty = ty.clone();
        rewrite_object(&mut ty, self.krate);
        ty
    }

    /// `struct Name(T)` is `T`'s schema; several fields are an array of as many
    /// items.
    fn tuple_struct(&self, fields: &Punctuated<Field, Token![,]>) -> syn::Result<Body> {
        let krate = self.krate;
        let mut iter = fields.iter();
        let Some(first) = iter.next() else {
            return Ok(Body {
                schema: quote!(#krate::Schema::array(#krate::Schema::default())),
                components: TokenStream::new(),
            });
        };
        let args = FieldArgs::parse(&first.attrs)?;
        let ty = args.value_type.as_ref().unwrap_or(&first.ty);
        if args.inline && mentions(ty, self.root) {
            return Err(syn::Error::new(
                first.span(),
                format!(
                    "#[schema(inline)] would put `{}` inside itself: a type that contains itself needs a `$ref`, so remove `inline`",
                    self.root
                ),
            ));
        }
        let single = fields.len() == 1;
        let same = fields.iter().all(|field| {
            field.ty.to_token_stream().to_string() == first.ty.to_token_stream().to_string()
        });
        let item = if same {
            if args.inline {
                self.inline_schema(ty)
            } else {
                let ty = self.resolved(ty);
                quote!(<#ty as #krate::ToSchema>::reference())
            }
        } else {
            quote!(#krate::Schema::object())
        };
        let components = if same {
            self.field_components(ty, args.inline)
        } else {
            TokenStream::new()
        };
        let schema = if single {
            item
        } else {
            let count = fields.len() as u64;
            quote! {{
                let mut array = #krate::Schema::array(#item);
                array.max_items = ::std::option::Option::Some(#count);
                array.min_items = ::std::option::Option::Some(#count);
                array
            }}
        };
        Ok(Body { schema, components })
    }

    fn enumeration(
        &self,
        variants: &Punctuated<syn::Variant, Token![,]>,
        serde: &SerdeContainer,
    ) -> syn::Result<Body> {
        let krate = self.krate;
        let kept: Vec<(&syn::Variant, SerdeField)> = variants
            .iter()
            .map(|variant| (variant, SerdeField::parse(&variant.attrs)))
            .filter(|(_, rules)| !rules.skip)
            .collect();
        let all_unit = kept
            .iter()
            .all(|(variant, _)| matches!(variant.fields, Fields::Unit));
        if all_unit {
            return self.plain_enum(&kept, serde);
        }
        let mut items = Vec::new();
        let mut components = TokenStream::new();
        for (variant, rules) in &kept {
            let body = self.variant(variant, rules, serde)?;
            items.push(body.schema);
            components.extend(body.components);
        }
        Ok(Body {
            schema: quote!(#krate::Schema::one_of(::std::vec![#(#items),*])),
            components,
        })
    }

    /// An enum of unit variants, in serde's representation.
    fn plain_enum(
        &self,
        variants: &[(&syn::Variant, SerdeField)],
        serde: &SerdeContainer,
    ) -> syn::Result<Body> {
        let krate = self.krate;
        let names = variants
            .iter()
            .map(|(variant, rules)| variant_name(variant, rules, serde))
            .collect::<syn::Result<Vec<String>>>()?;
        let schema = match &serde.repr {
            Repr::External if names.is_empty() => {
                quote!(#krate::Schema::never())
            }
            Repr::External => quote!(#krate::Schema::string_enum([#(#names),*])),
            Repr::Untagged => {
                quote!(#krate::Schema::null().default_value(#krate::__private::serde_json::Value::Null))
            }
            Repr::Internal(tag) => {
                let items = names.iter().map(|name| {
                    quote! {
                        #krate::Schema::object().property(#tag, #krate::Schema::string_enum([#name]), true)
                    }
                });
                quote!(#krate::Schema::one_of(::std::vec![#(#items),*]))
            }
            Repr::Adjacent { tag, content } => {
                let items = names.iter().map(|name| {
                    quote! {
                        #krate::Schema::object()
                            .property(#content, #krate::Schema::string_enum([#name]), true)
                            .property(#tag, #krate::Schema::string_enum([#content]), true)
                    }
                });
                quote!(#krate::Schema::one_of(::std::vec![#(#items),*]))
            }
        };
        Ok(Body {
            schema,
            components: TokenStream::new(),
        })
    }

    /// One variant of an enum that has data, in serde's representation.
    fn variant(
        &self,
        variant: &syn::Variant,
        rules: &SerdeField,
        serde: &SerdeContainer,
    ) -> syn::Result<Body> {
        let krate = self.krate;
        let name = variant_name(variant, rules, serde)?;
        let doc = doc_comment(&variant.attrs);
        let describe = doc.as_ref().map(|text| quote!(.description(#text)));
        let tagged =
            |tag: &str| quote!(.property(#tag, #krate::Schema::string_enum([#name]), true));
        let (schema, components) = match &variant.fields {
            Fields::Unit => {
                let schema = match &serde.repr {
                    Repr::External => quote!(#krate::Schema::string_enum([#name])),
                    Repr::Untagged => quote! {
                        #krate::Schema::null().default_value(#krate::__private::serde_json::Value::Null)
                    },
                    Repr::Internal(tag) | Repr::Adjacent { tag, .. } => {
                        let tagged = tagged(tag);
                        quote!(#krate::Schema::object() #tagged)
                    }
                };
                (quote!(#schema #describe), TokenStream::new())
            }
            Fields::Named(fields) => {
                let mut inner = SerdeContainer::parse(&variant.attrs);
                if inner.rename_all.is_none() {
                    inner.rename_all = serde.rename_all_fields.clone();
                }
                let body = self.named_struct(&fields.named, &inner, false)?;
                let schema = body.schema;
                let inner_schema = quote!((#schema) #describe);
                let schema = match &serde.repr {
                    Repr::External => quote! {
                        #krate::Schema::object().property(#name, #inner_schema, true) #describe
                    },
                    Repr::Untagged => inner_schema,
                    Repr::Internal(tag) => {
                        let tagged = tagged(tag);
                        quote!(#inner_schema #tagged)
                    }
                    Repr::Adjacent { tag, content } => {
                        let tagged = tagged(tag);
                        quote! {
                            #krate::Schema::object().property(#content, #inner_schema, true) #tagged #describe
                        }
                    }
                };
                (schema, body.components)
            }
            Fields::Unnamed(fields) => {
                let body = self.tuple_struct(&fields.unnamed)?;
                let inner = body.schema;
                let is_reference = fields.unnamed.iter().any(|field| is_component(&field.ty));
                let inner_schema = match &doc {
                    Some(text) => quote!((#inner).described(#text)),
                    None => inner,
                };
                let schema = match &serde.repr {
                    Repr::External => quote! {
                        #krate::Schema::object().property(#name, #inner_schema, true) #describe
                    },
                    Repr::Untagged => inner_schema,
                    Repr::Internal(tag) => {
                        let tagged = tagged(tag);
                        if is_reference {
                            quote! {
                                #krate::Schema::all_of(::std::vec![
                                    #inner_schema,
                                    #krate::Schema::object() #tagged,
                                ]) #describe
                            }
                        } else {
                            quote!(#inner_schema #tagged)
                        }
                    }
                    Repr::Adjacent { tag, content } => {
                        let tagged = tagged(tag);
                        quote! {
                            #krate::Schema::object().property(#content, #inner_schema, true) #tagged #describe
                        }
                    }
                };
                (schema, body.components)
            }
        };
        Ok(Body { schema, components })
    }
}

/// A name a field has in the JSON, and in which direction: a field whose name
/// differs between responses and requests has two.
struct Side {
    name: String,
    /// `.read_only()` or `.write_only()`, when it is one way.
    direction: TokenStream,
}

/// The names of a field, by what serde reads and writes.
fn sides(
    rules: &SerdeField,
    args: &FieldArgs,
    serde: &SerdeContainer,
    rust_name: &str,
) -> Vec<Side> {
    let name_for = |one_way: &Option<String>, rule: &Option<RenameRule>| {
        one_way
            .clone()
            .or_else(|| args.rename.clone())
            .unwrap_or_else(|| match rule {
                Some(rule) => rule.field(rust_name),
                None => rust_name.to_owned(),
            })
    };
    let written = name_for(&rules.rename.serialize, &serde.rename_all.serialize);
    let read = name_for(&rules.rename.deserialize, &serde.rename_all.deserialize);
    let one_way = |name: String, method: &str| {
        let method = Ident::new(method, Span::call_site());
        Side {
            name,
            direction: quote!(.#method()),
        }
    };
    if rules.skip_deserializing {
        vec![one_way(written, "read_only")]
    } else if rules.skip_serializing {
        vec![one_way(read, "write_only")]
    } else if written == read {
        vec![Side {
            name: written,
            direction: TokenStream::new(),
        }]
    } else {
        vec![one_way(written, "read_only"), one_way(read, "write_only")]
    }
}

/// A field, ready to go in the object.
struct ParsedField {
    sides: Vec<Side>,
    schema: TokenStream,
    components: TokenStream,
    required: bool,
    flatten: bool,
    is_map: bool,
}

/// `#[schema(..)]` on a struct or an enum.
#[derive(Default)]
struct ContainerArgs {
    name: Option<Path>,
    description: Option<LitStr>,
}

impl ContainerArgs {
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut args = Self::default();
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("schema")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("as") {
                    args.name = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("description") {
                    args.description = Some(meta.value()?.parse()?);
                } else {
                    return Err(meta.error(
                        "#[schema(..)] on a type takes `as = Name` and `description = \"..\"`",
                    ));
                }
                Ok(())
            })?;
        }
        Ok(args)
    }
}

/// `#[schema(..)]` on a field or a variant.
#[derive(Default)]
struct FieldArgs {
    value_type: Option<Type>,
    inline: bool,
    nullable: Option<bool>,
    required: Option<bool>,
    rename: Option<String>,
    format: Option<String>,
    example: Option<TokenStream>,
    title: Option<LitStr>,
    minimum: Option<Lit>,
    maximum: Option<Lit>,
    schema_with: Option<Path>,
}

impl FieldArgs {
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut args = Self::default();
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("schema")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("value_type") {
                    args.value_type = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("inline") {
                    args.inline = flag(&meta)?;
                } else if meta.path.is_ident("nullable") {
                    args.nullable = Some(flag(&meta)?);
                } else if meta.path.is_ident("required") {
                    args.required = Some(flag(&meta)?);
                } else if meta.path.is_ident("rename") {
                    let name: LitStr = meta.value()?.parse()?;
                    args.rename = Some(name.value());
                } else if meta.path.is_ident("format") {
                    args.format = Some(parse_format(meta.value()?)?);
                } else if meta.path.is_ident("example") {
                    args.example = Some(parse_json(meta.value()?)?);
                } else if meta.path.is_ident("title") {
                    args.title = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("minimum") {
                    args.minimum = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("maximum") {
                    args.maximum = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("schema_with") {
                    args.schema_with = Some(meta.value()?.parse()?);
                } else {
                    return Err(meta.error(
                        "#[schema(..)] on a field takes `value_type`, `inline`, `nullable`, `required`, `rename`, `format`, `example`, `title`, `minimum`, `maximum` and `schema_with`",
                    ));
                }
                Ok(())
            })?;
        }
        Ok(args)
    }
}

/// `name` or `name = true`.
fn flag(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<bool> {
    if meta.input.peek(Token![=]) {
        let value: syn::LitBool = meta.value()?.parse()?;
        Ok(value.value)
    } else {
        Ok(true)
    }
}

/// `format = Uuid`, or `format = "custom"`.
fn parse_format(input: syn::parse::ParseStream<'_>) -> syn::Result<String> {
    if input.peek(LitStr) {
        let format: LitStr = input.parse()?;
        return Ok(format.value());
    }
    let ident: Ident = input.parse()?;
    let format = match ident.to_string().as_str() {
        "Int32" => "int32",
        "Int64" => "int64",
        "Float" => "float",
        "Double" => "double",
        "Byte" => "byte",
        "Binary" => "binary",
        "Date" => "date",
        "DateTime" => "date-time",
        "Duration" => "duration",
        "Password" => "password",
        "Uuid" => "uuid",
        "Email" => "email",
        "Uri" => "uri",
        "Url" => "url",
        "Hostname" => "hostname",
        "Ipv4" => "ipv4",
        "Ipv6" => "ipv6",
        _ => {
            return Err(syn::Error::new(
                ident.span(),
                "unknown format: use a known name (`Uuid`, `Date`, `DateTime`, `Password`, ..) or a string",
            ));
        }
    };
    Ok(format.to_owned())
}

/// `example = "text"`, a number, `true`, or `json!(..)`: tokens for `json!`.
fn parse_json(input: syn::parse::ParseStream<'_>) -> syn::Result<TokenStream> {
    let expr: Expr = input.parse()?;
    if let Expr::Macro(mac) = &expr
        && mac.mac.path.is_ident("json")
    {
        return Ok(mac.mac.tokens.clone());
    }
    Ok(expr.to_token_stream())
}

/// The serde attributes of a struct, an enum or a variant.
#[derive(Default)]
struct SerdeContainer {
    rename_all: Directed<RenameRule>,
    rename_all_fields: Directed<RenameRule>,
    default: bool,
    /// `default = "path"`.
    default_path: Option<String>,
    transparent: bool,
    deny_unknown_fields: bool,
    repr: Repr,
}

/// How serde writes an enum.
#[derive(Default)]
enum Repr {
    #[default]
    External,
    Internal(String),
    Adjacent {
        tag: String,
        content: String,
    },
    Untagged,
}

impl SerdeContainer {
    fn parse(attrs: &[Attribute]) -> Self {
        let mut out = Self::default();
        let mut tag = None;
        let mut content = None;
        let rules = |key: &str| {
            let names = directed(attrs, key);
            Directed {
                serialize: names.serialize.as_deref().and_then(RenameRule::parse),
                deserialize: names.deserialize.as_deref().and_then(RenameRule::parse),
            }
        };
        out.rename_all = rules("rename_all");
        out.rename_all_fields = rules("rename_all_fields");
        for (key, value) in serde_entries(attrs) {
            match (key.as_str(), value) {
                ("tag", Some(name)) => tag = Some(name),
                ("content", Some(name)) => content = Some(name),
                ("untagged", _) => out.repr = Repr::Untagged,
                ("transparent", _) => out.transparent = true,
                ("default", path) => {
                    out.default = true;
                    out.default_path = path;
                }
                ("deny_unknown_fields", _) => out.deny_unknown_fields = true,
                _ => {}
            }
        }
        match (tag, content) {
            (Some(tag), Some(content)) => out.repr = Repr::Adjacent { tag, content },
            (Some(tag), None) => out.repr = Repr::Internal(tag),
            _ => {}
        }
        out
    }
}

/// What serde says for writing and for reading.
#[derive(Clone)]
struct Directed<T> {
    serialize: Option<T>,
    deserialize: Option<T>,
}

impl<T> Default for Directed<T> {
    fn default() -> Self {
        Directed {
            serialize: None,
            deserialize: None,
        }
    }
}

impl<T> Directed<T> {
    fn is_none(&self) -> bool {
        self.serialize.is_none() && self.deserialize.is_none()
    }
}

/// `key = "both"`, or `key(serialize = "..", deserialize = "..")`, of
/// `#[serde(..)]`.
fn directed(attrs: &[Attribute], key: &str) -> Directed<String> {
    let mut out = Directed::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        let Ok(list) = attr.meta.require_list() else {
            continue;
        };
        let tokens: Vec<TokenTree> = list.tokens.clone().into_iter().collect();
        for (at, token) in tokens.iter().enumerate() {
            if !matches!(token, TokenTree::Ident(ident) if ident == key) {
                continue;
            }
            match tokens.get(at + 1) {
                Some(TokenTree::Punct(punct)) if punct.as_char() == '=' => {
                    let value = tokens.get(at + 2).cloned().and_then(string_literal);
                    out.serialize.clone_from(&value);
                    out.deserialize = value;
                }
                Some(TokenTree::Group(group)) => {
                    let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                    for (at, token) in inner.iter().enumerate() {
                        let TokenTree::Ident(ident) = token else {
                            continue;
                        };
                        let value = inner.get(at + 2).cloned().and_then(string_literal);
                        if ident == "serialize" {
                            out.serialize = value;
                        } else if ident == "deserialize" {
                            out.deserialize = value;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// The serde attributes of a field or a variant.
#[derive(Default)]
struct SerdeField {
    skip: bool,
    skip_serializing: bool,
    skip_deserializing: bool,
    rename: Directed<String>,
    default: bool,
    flatten: bool,
    skip_serializing_if: bool,
}

impl SerdeField {
    fn parse(attrs: &[Attribute]) -> Self {
        let mut out = Self::default();
        for (key, _) in serde_entries(attrs) {
            match key.as_str() {
                "skip" => out.skip = true,
                "skip_serializing" => out.skip_serializing = true,
                "skip_deserializing" => out.skip_deserializing = true,
                "skip_serializing_if" => out.skip_serializing_if = true,
                "flatten" => out.flatten = true,
                "default" => out.default = true,
                _ => {}
            }
        }
        out.skip |= out.skip_serializing && out.skip_deserializing;
        out.rename = directed(attrs, "rename");
        out
    }
}

/// The `key` and `key = "value"` entries of `#[serde(..)]`; a list
/// (`rename(serialize = "x")`) has no value.
fn serde_entries(attrs: &[Attribute]) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        let Ok(list) = attr.meta.require_list() else {
            continue;
        };
        let mut tokens = list.tokens.clone().into_iter().peekable();
        while let Some(token) = tokens.next() {
            let TokenTree::Ident(ident) = token else {
                continue;
            };
            let is_value =
                matches!(tokens.peek(), Some(TokenTree::Punct(punct)) if punct.as_char() == '=');
            if !is_value {
                out.push((ident.to_string(), None));
                continue;
            }
            tokens.next();
            let value = tokens.next().and_then(string_literal);
            out.push((ident.to_string(), value));
        }
    }
    out
}

/// The text of a string literal token; a macro's `$value:literal` arrives in an
/// invisible group.
fn string_literal(token: TokenTree) -> Option<String> {
    match token {
        TokenTree::Literal(literal) => syn::parse_str::<LitStr>(&literal.to_string())
            .ok()
            .map(|text| text.value()),
        TokenTree::Group(group) if group.delimiter() == Delimiter::None => {
            group.stream().into_iter().next().and_then(string_literal)
        }
        _ => None,
    }
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

    /// A variant's name (`PascalCase` in Rust), as serde renames it.
    fn variant(self, name: &str) -> String {
        match self {
            Self::Lower => name.to_ascii_lowercase(),
            Self::Upper => name.to_ascii_uppercase(),
            Self::Pascal => name.to_owned(),
            Self::Camel => {
                let mut chars = name.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            }
            Self::Snake => {
                let mut out = String::new();
                for (index, ch) in name.char_indices() {
                    if index > 0 && ch.is_uppercase() {
                        out.push('_');
                    }
                    out.push(ch.to_ascii_lowercase());
                }
                out
            }
            Self::ScreamingSnake => Self::Snake.variant(name).to_ascii_uppercase(),
            Self::Kebab => Self::Snake.variant(name).replace('_', "-"),
            Self::ScreamingKebab => Self::ScreamingSnake.variant(name).replace('_', "-"),
        }
    }

    /// A field's name (`snake_case` in Rust), as serde renames it.
    fn field(self, name: &str) -> String {
        match self {
            Self::Lower | Self::Snake => name.to_owned(),
            Self::Upper | Self::ScreamingSnake => name.to_ascii_uppercase(),
            Self::Pascal => {
                let mut out = String::new();
                let mut capitalize = true;
                for ch in name.chars() {
                    if ch == '_' {
                        capitalize = true;
                    } else if capitalize {
                        out.push(ch.to_ascii_uppercase());
                        capitalize = false;
                    } else {
                        out.push(ch);
                    }
                }
                out
            }
            Self::Camel => {
                let pascal = Self::Pascal.field(name);
                let mut chars = pascal.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            }
            Self::Kebab => name.replace('_', "-"),
            Self::ScreamingKebab => Self::ScreamingSnake.field(name).replace('_', "-"),
        }
    }
}

/// A variant's name in JSON.
fn variant_name(
    variant: &syn::Variant,
    rules: &SerdeField,
    serde: &SerdeContainer,
) -> syn::Result<String> {
    let args = FieldArgs::parse(&variant.attrs)?;
    let rust_name = variant.ident.to_string();
    let rust_name = rust_name.strip_prefix("r#").unwrap_or(&rust_name);
    let name_for = |one_way: &Option<String>, rule: &Option<RenameRule>| {
        one_way
            .clone()
            .or_else(|| args.rename.clone())
            .unwrap_or_else(|| match rule {
                Some(rule) => rule.variant(rust_name),
                None => rust_name.to_owned(),
            })
    };
    let written = name_for(&rules.rename.serialize, &serde.rename_all.serialize);
    let read = name_for(&rules.rename.deserialize, &serde.rename_all.deserialize);
    if written != read {
        return Err(syn::Error::new(
            variant.span(),
            format!(
                "this variant is `{written}` in responses and `{read}` in requests, which one schema can't say: use one name, or write `impl ToSchema` by hand"
            ),
        ));
    }
    Ok(written)
}

/// The doc comment: the lines, with their common indentation gone, joined with
/// newlines. `None` without one.
fn doc_comment(attrs: &[Attribute]) -> Option<String> {
    let mut lines = Vec::new();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("doc")) {
        if let syn::Meta::NameValue(pair) = &attr.meta
            && let Expr::Lit(ExprLit {
                lit: Lit::Str(text),
                ..
            }) = &pair.value
        {
            lines.push(text.value().trim_end().to_owned());
        }
    }
    let indent = lines
        .iter()
        .filter(|line| !line.is_empty())
        .map(|line| line.len() - line.trim_ascii_start().len())
        .min()
        .unwrap_or(0);
    for line in &mut lines {
        if !line.is_empty() {
            line.drain(..indent);
        }
    }
    let text = lines.join("\n");
    (!text.is_empty()).then_some(text)
}

fn has_deprecated(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("deprecated"))
}

/// The last segment of a path type, and its first generic type argument.
fn last_segment(ty: &Type) -> Option<(&Ident, Option<&Type>)> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    let argument = match &segment.arguments {
        PathArguments::AngleBracketed(arguments) => {
            arguments.args.iter().find_map(|argument| match argument {
                GenericArgument::Type(ty) => Some(ty),
                _ => None,
            })
        }
        _ => None,
    };
    Some((&segment.ident, argument))
}

/// The `T` of `name<T>`.
fn wrapped<'a>(ty: &'a Type, name: &str) -> Option<&'a Type> {
    match last_segment(ty)? {
        (ident, Some(argument)) if ident == name => Some(argument),
        _ => None,
    }
}

fn is_option(ty: &Type) -> bool {
    wrapped(ty, "Option").is_some()
}

fn peel_option(ty: &Type) -> &Type {
    match wrapped(ty, "Option") {
        Some(inner) => peel_option(inner),
        None => ty,
    }
}

fn is_map(ty: &Type) -> bool {
    last_segment(ty).is_some_and(|(ident, _)| {
        ident == "BTreeMap" || ident == "HashMap" || ident == "Map" || ident == "IndexMap"
    })
}

/// `Object` of `value_type = Object`, or `serde_json::Value`.
fn is_untyped(ty: &Type) -> bool {
    last_segment(ty).is_some_and(|(ident, _)| ident == "Object" || ident == "Value")
}

/// A type that has its own component, by its name: not a primitive, a list, a
/// map or `Value`.
fn is_component(ty: &Type) -> bool {
    match last_segment(ty) {
        Some((ident, argument)) => {
            let name = ident.to_string();
            match name.as_str() {
                "String" | "str" | "char" | "bool" | "usize" | "u8" | "u16" | "u32" | "u64"
                | "u128" | "isize" | "i8" | "i16" | "i32" | "i64" | "i128" | "f32" | "f64"
                | "Uuid" | "DateTime" | "NaiveDate" | "NaiveDateTime" | "NaiveTime" | "Value"
                | "Vec" | "BTreeMap" | "HashMap" | "Map" | "IndexMap" | "HashSet" | "BTreeSet"
                | "Object" => false,
                "Option" | "Box" | "Arc" | "Rc" | "Cow" => argument.is_some_and(is_component),
                _ => true,
            }
        }
        None => false,
    }
}

/// Whether `ty` names the generic parameter `param`.
fn mentions(ty: &Type, param: &Ident) -> bool {
    ty.to_token_stream()
        .into_iter()
        .any(|token| mentions_token(&token, param))
}

fn mentions_token(token: &TokenTree, param: &Ident) -> bool {
    match token {
        TokenTree::Ident(ident) => ident == param,
        TokenTree::Group(group) => group
            .stream()
            .into_iter()
            .any(|token| mentions_token(&token, param)),
        _ => false,
    }
}

/// Replaces the bare type `Object` with `navree_openapi::Object`.
fn rewrite_object(ty: &mut Type, krate: &TokenStream) {
    let Type::Path(path) = ty else {
        return;
    };
    if path.qself.is_none() && path.path.is_ident("Object") {
        *ty = syn::parse_quote!(#krate::Object);
        return;
    }
    for segment in &mut path.path.segments {
        if let PathArguments::AngleBracketed(arguments) = &mut segment.arguments {
            for argument in &mut arguments.args {
                if let GenericArgument::Type(ty) = argument {
                    rewrite_object(ty, krate);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_rename_as_serde_does() {
        let cases = [
            (RenameRule::Camel, "status_code", "statusCode"),
            (RenameRule::Pascal, "status_code", "StatusCode"),
            (RenameRule::Kebab, "status_code", "status-code"),
            (RenameRule::ScreamingSnake, "status_code", "STATUS_CODE"),
            (RenameRule::ScreamingKebab, "status_code", "STATUS-CODE"),
            (RenameRule::Upper, "status_code", "STATUS_CODE"),
            (RenameRule::Lower, "status_code", "status_code"),
            (RenameRule::Snake, "status_code", "status_code"),
        ];
        for (rule, name, expected) in cases {
            assert_eq!(rule.field(name), expected);
        }
    }

    #[test]
    fn variants_rename_as_serde_does() {
        let cases = [
            (RenameRule::Camel, "InProgress", "inProgress"),
            (RenameRule::Pascal, "InProgress", "InProgress"),
            (RenameRule::Snake, "InProgress", "in_progress"),
            (RenameRule::Kebab, "InProgress", "in-progress"),
            (RenameRule::ScreamingSnake, "InProgress", "IN_PROGRESS"),
            (RenameRule::ScreamingKebab, "InProgress", "IN-PROGRESS"),
            (RenameRule::Lower, "InProgress", "inprogress"),
            (RenameRule::Upper, "InProgress", "INPROGRESS"),
        ];
        for (rule, name, expected) in cases {
            assert_eq!(rule.variant(name), expected);
        }
    }

    #[test]
    fn doc_comments_lose_their_common_indentation() {
        let input: DeriveInput = syn::parse_quote! {
            /// First line.
            ///
            ///   Indented.
            struct Note;
        };
        assert_eq!(
            doc_comment(&input.attrs).as_deref(),
            Some("First line.\n\n  Indented.")
        );
    }

    #[test]
    fn serde_entries_read_values_and_skip_lists() {
        let input: DeriveInput = syn::parse_quote! {
            #[serde(rename = "x", default, rename(serialize = "y"), tag = "kind")]
            struct Note;
        };
        let entries = serde_entries(&input.attrs);
        assert_eq!(entries[0], ("rename".to_owned(), Some("x".to_owned())));
        assert_eq!(entries[1], ("default".to_owned(), None));
        assert!(entries.contains(&("tag".to_owned(), Some("kind".to_owned()))));
    }

    #[test]
    fn an_unknown_option_is_an_error() {
        let input = quote! {
            struct Note {
                #[schema(colour = "red")]
                body: String,
            }
        };
        let parsed: DeriveInput = syn::parse2(input).unwrap();
        let Data::Struct(data) = &parsed.data else {
            unreachable!()
        };
        let field = data.fields.iter().next().unwrap();
        let error = FieldArgs::parse(&field.attrs).err().map(|e| e.to_string());
        assert!(error.unwrap().contains("takes `value_type`"));
    }
}
