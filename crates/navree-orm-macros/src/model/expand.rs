//! The code `#[derive(Model)]` writes: the `Model` impl and the column
//! constants.

use heck::ToShoutySnakeCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Type;

use super::attributes::{self, MultiPart};
use super::{Column, KeyType, Model, option_inner, plain_name};
use crate::errors::Errors;

/// The type as written, without the token spacing (`Option<DateTime<Utc>>`).
fn type_text(ty: &Type) -> String {
    quote!(#ty)
        .to_string()
        .replace(" :: ", "::")
        .replace(":: ", "::")
        .replace(" <", "<")
        .replace("< ", "<")
        .replace(" >", ">")
        .replace(" ,", ",")
        .replace("& ", "&")
}

/// How one column is read, written, converted and typed.
struct Codec {
    /// `row -> field` (reading a row); `None` for the columns of a
    /// multi-column field after its first (the first reads them all).
    read: Option<TokenStream>,
    /// Pushes `(column, Value)` onto `values` (`None`: the first column of
    /// a multi-column field pushes them all).
    push: Option<TokenStream>,
    /// `&self -> Value` (`value(column)`).
    write: TokenStream,
    /// `value -> self` (`set_value`).
    convert: TokenStream,
    /// The column constant's type and initializer.
    constant_type: TokenStream,
    constant: TokenStream,
}

fn codec(model: &Model, column: &Column, krate: &TokenStream) -> Codec {
    if let Some(multi) = &column.field.multi {
        return multi_codec(model, column, multi, krate);
    }
    let private = quote!(#krate::__private);
    let ident = &column.ident;
    let ty = &column.ty;
    let name = &column.name;
    let table = &model.table;
    // The field's default when its type has one: what a column a partial
    // select left out reads as in lenient mode.
    let default = quote!({
        #[allow(unused_imports)]
        use #private::{ViaDefault as _, WithoutDefault as _};
        (&#private::DefaultOf::<#ty>::new()).default_fn()
    });
    let (read, write, convert, constant) = match (&column.cast, &column.inner) {
        (None, _) => {
            let read = if column.default {
                quote!(#private::read_or_default::<Self, #ty>(row, #name)?)
            } else {
                quote!(#private::read::<Self, #ty>(row, #name, #default)?)
            };
            (
                read,
                quote!(#krate::Field::to_value(&self.#ident)),
                quote!(self.#ident = #private::convert::<Self, #ty>(column, value)?;),
                quote!(#krate::Column::<Self, #ty>::field(#table, #name)),
            )
        }
        (Some(cast), None) => {
            let cast = attributes::cast_type(model, column, cast, krate);
            let read = if column.default {
                quote!(#private::read_cast_or_default::<Self, #cast, #ty>(row, #name)?)
            } else {
                quote!(#private::read_cast::<Self, #cast, #ty>(row, #name, #default)?)
            };
            (
                read,
                quote!(#private::write_cast::<Self, #cast, #ty>(#name, &self.#ident)?),
                quote!(self.#ident = #private::convert_cast::<Self, #cast, #ty>(column, value)?;),
                quote!(#krate::Column::<Self, #ty>::with_codec(
                    #table,
                    #name,
                    #private::encode::<#cast, #ty>,
                    #private::decode::<#cast, #ty>,
                )),
            )
        }
        (Some(cast), Some(inner)) => {
            let cast = attributes::cast_type(model, column, cast, krate);
            let read = if column.default {
                quote!(#private::read_cast_option_or_default::<Self, #cast, #inner>(row, #name)?)
            } else {
                quote!(#private::read_cast_option::<Self, #cast, #inner>(row, #name, #default)?)
            };
            (
                read,
                quote!(#private::write_cast_option::<Self, #cast, #inner>(#name, &self.#ident)?),
                quote!(self.#ident = #private::convert_cast_option::<Self, #cast, #inner>(column, value)?;),
                quote!(#krate::Column::<Self, #ty>::with_codec(
                    #table,
                    #name,
                    #private::encode_option::<#cast, #inner>,
                    #private::decode_option::<#cast, #inner>,
                )),
            )
        }
    };
    let push = quote!(values.push((#name, #write)););
    Codec {
        read: Some(read),
        push: Some(push),
        write,
        convert,
        constant_type: quote!(#krate::Column<Self, #ty>),
        constant,
    }
}

/// One column of a field stored across several columns.
fn multi_codec(model: &Model, column: &Column, multi: &MultiPart, krate: &TokenStream) -> Codec {
    let private = quote!(#krate::__private);
    let ident = &column.ident;
    let field_name = plain_name(ident);
    let name = &column.name;
    let table = &model.table;
    let cast = &multi.cast;
    let value_ty = multi.field_inner.as_ref().unwrap_or(&multi.field_ty);
    let names = &multi.names;
    let index = multi.index;
    let part_ty = &column.ty;
    let (read_fn, values, replace) = if multi.field_inner.is_some() {
        (
            quote!(read_multi_row_option),
            quote!(#private::write_multi_option::<Self, #cast, #value_ty>(#field_name, &self.#ident)?),
            quote!(replace_multi_option),
        )
    } else {
        (
            quote!(read_multi_row),
            quote!(#private::multi_values::<Self, #cast, #value_ty>(#field_name, &self.#ident)?),
            quote!(replace_multi),
        )
    };
    let first = index == 0;
    Codec {
        read: first.then(|| {
            quote!(#private::#read_fn::<Self, #cast, #value_ty>(row, #field_name, &[#(#names),*])?)
        }),
        push: first.then(|| {
            quote!(values.extend([#(#names),*].into_iter().zip(#values));)
        }),
        write: quote!(#values.swap_remove(#index)),
        convert: quote! {
            self.#ident = #private::#replace::<Self, #cast, #value_ty>(#field_name, &self.#ident, column, #index, value)?;
        },
        constant_type: quote!(#krate::Column<Self, #part_ty>),
        constant: quote!(#krate::Column::<Self, #part_ty>::field(#table, #name)),
    }
}

/// The `Model` impl, the column constants, the setters and the items
/// beside the model.
#[allow(clippy::too_many_lines)]
pub(crate) fn expand(model: &Model, krate: &TokenStream) -> syn::Result<TokenStream> {
    let ident = &model.ident;
    let vis = &model.vis;
    let private = quote!(#krate::__private);
    let codecs: Vec<Codec> = model
        .columns
        .iter()
        .map(|column| codec(model, column, krate))
        .collect();

    // ModelMeta.
    let model_name = ident.to_string();
    let table = &model.table;
    let column_count = model.columns.len();
    let column_metas = model.columns.iter().map(|column| {
        let name = &column.name;
        let field = plain_name(&column.ident);
        let rust_type = type_text(&column.ty);
        let mut meta = quote!(#krate::ColumnMeta::new(#name, #field, #rust_type));
        let nullable = match &column.field.multi {
            Some(multi) => multi.field_inner.is_some() || option_inner(&column.ty).is_some(),
            None => column.inner.is_some(),
        };
        if nullable {
            meta = quote!(#meta.nullable());
        }
        let cast_name = match (&column.field.multi, &column.cast) {
            (Some(multi), _) => {
                let cast = &multi.cast;
                Some(quote!(#cast).to_string().replace(' ', ""))
            }
            (None, Some(cast)) => Some(attributes::cast_name(cast)),
            (None, None) => None,
        };
        if let Some(cast_name) = cast_name {
            meta = quote!(#meta.cast(#cast_name));
        }
        let more = attributes::column_meta_calls(model, column, krate);
        quote!(#meta #more)
    });
    let key_column = &model.columns[model.key];
    let key_name = &key_column.name;
    let key_ident = &key_column.ident;
    let key_ty = &key_column.ty;
    let key_kind = match model.key_type {
        KeyType::Increments => quote!(#krate::KeyKind::Increments),
        KeyType::UuidV7 => quote!(#krate::KeyKind::UuidV7),
        KeyType::UuidV4 => quote!(#krate::KeyKind::UuidV4),
        KeyType::Ulid => quote!(#krate::KeyKind::Ulid),
        KeyType::Manual => quote!(#krate::KeyKind::Manual),
    };
    let mut meta = quote! {
        #krate::ModelMeta::new(#model_name, #table)
            .primary_key(#key_name, #key_kind)
            .columns(&COLUMNS)
    };
    if model.created_at.is_some() || model.updated_at.is_some() {
        let created = match &model.created_at {
            Some(name) => quote!(::core::option::Option::Some(#name)),
            None => quote!(::core::option::Option::None),
        };
        let updated = match &model.updated_at {
            Some(name) => quote!(::core::option::Option::Some(#name)),
            None => quote!(::core::option::Option::None),
        };
        meta = quote!(#meta.timestamps(#created, #updated));
    }
    if let Some(deleted_at) = &model.deleted_at {
        meta = quote!(#meta.soft_deletes(#deleted_at));
    }
    if let Some(connection) = &model.connection {
        meta = quote!(#meta.connection(#connection));
    }
    if let Some(per_page) = &model.per_page {
        meta = quote!(#meta.per_page(#per_page));
    }
    let lifecycle_meta = super::lifecycle::meta_additions(&model.lifecycle, ident, krate);
    meta = quote!(#meta #lifecycle_meta);
    let morph_name = &model.morph_name;
    let foreign_key = &model.foreign_key;
    meta = quote!(#meta.morph_name(#morph_name).foreign_key(#foreign_key));
    if !model.touches.is_empty() {
        let touches = &model.touches;
        meta = quote!(#meta.touches(&[#(#touches),*]));
    }
    if !model.with.is_empty() {
        let with = &model.with;
        meta = quote!(#meta.with(&[#(#with),*]));
    }
    if !model.with_count.is_empty() {
        let with_count = &model.with_count;
        meta = quote!(#meta.with_count(&[#(#with_count),*]));
    }
    if let Some((foreign_key, related_key)) = &model.pivot {
        meta = quote!(#meta.pivot_keys(#foreign_key, #related_key));
    }
    let attribute_calls = attributes::meta_calls(model, krate);
    meta = quote!(#meta #attribute_calls);

    // from_row.
    let reads = model
        .columns
        .iter()
        .zip(&codecs)
        .filter_map(|(column, codec)| {
            let field = &column.ident;
            codec.read.as_ref().map(|read| quote!(#field: #read))
        });
    let skipped = model
        .skipped
        .iter()
        .map(|field| quote!(#field: ::core::default::Default::default()));

    // values / value / set_value.
    let pushes = codecs.iter().filter_map(|codec| codec.push.as_ref());
    let value_arms = model.columns.iter().zip(&codecs).map(|(column, codec)| {
        let name = &column.name;
        let write = &codec.write;
        quote!(#name => ::core::option::Option::Some(#write),)
    });
    let set_arms = model.columns.iter().zip(&codecs).map(|(column, codec)| {
        let name = &column.name;
        let convert = &codec.convert;
        quote!(#name => {
            #convert
            ::core::result::Result::Ok(())
        })
    });

    // Timestamps: generated bodies stay lint-clean when there are none.
    let has_timestamps = model.timestamp_columns().next().is_some();
    let touch_body = if has_timestamps {
        let arms = model.timestamp_columns().map(|column| {
            let name = &column.name;
            let field = &column.ident;
            let ty = &column.ty;
            quote!(#name => {
                self.#field = <#ty as #krate::Timestamp>::from_now(now);
                ::core::result::Result::Ok(())
            })
        });
        quote!(match column {
            #(#arms)*
            _ => ::core::result::Result::Err(#private::not_a_timestamp::<Self>(column)),
        })
    } else {
        quote!({
            let _ = now;
            ::core::result::Result::Err(#private::not_a_timestamp::<Self>(column))
        })
    };
    let empty_body = if has_timestamps {
        let arms = model.timestamp_columns().map(|column| {
            let name = &column.name;
            let field = &column.ident;
            let ty = &column.ty;
            quote!(#name => <#ty as #krate::Timestamp>::is_empty(&self.#field),)
        });
        quote!(match column {
            #(#arms)*
            _ => false,
        })
    } else {
        quote!({
            let _ = column;
            false
        })
    };
    let clear_body = if has_timestamps {
        let arms = model.timestamp_columns().map(|column| {
            let name = &column.name;
            let field = &column.ident;
            let ty = &column.ty;
            quote!(#name => { self.#field = <#ty as #krate::Timestamp>::empty(); })
        });
        quote!(match column {
            #(#arms)*
            _ => {}
        })
    } else {
        quote!({
            let _ = column;
        })
    };
    let timestamp_value_body = if has_timestamps {
        let arms = model.timestamp_columns().map(|column| {
            let name = &column.name;
            let ty = &column.ty;
            quote!(#name => ::core::option::Option::Some(
                #krate::Field::to_value(&<#ty as #krate::Timestamp>::from_now(now))
            ),)
        });
        quote!(match column {
            #(#arms)*
            _ => ::core::option::Option::None,
        })
    } else {
        quote!({
            let _ = (column, now);
            ::core::option::Option::None
        })
    };

    let global_scopes = if model.global_scopes.is_empty() {
        quote!()
    } else {
        let scopes = &model.global_scopes;
        quote! {
            fn global_scopes() -> ::std::vec::Vec<#private::Arc<dyn #krate::GlobalScope<Self>>> {
                ::std::vec![#(#private::Arc::new(#scopes) as #private::Arc<dyn #krate::GlobalScope<Self>>),*]
            }
        }
    };

    // Lifecycle: the model's own hooks (observers, typed events, ...).
    let hook_entries = super::lifecycle::hook_entries(&model.lifecycle, krate);
    let hooks = super::lifecycle::hooks_fn(ident, &hook_entries, krate);
    let capabilities = super::lifecycle::capability_impls(ident, model.deleted_at.is_some(), krate);

    // Column constants: one per column, named after the field (after the
    // column for the columns of a multi-column field).
    let constants = model.columns.iter().zip(&codecs).map(|(column, codec)| {
        let constant = match &column.field.multi {
            Some(_) => attributes::part_constant(&column.name, column.ident.span()),
            None => format_ident!(
                "{}",
                plain_name(&column.ident).to_shouty_snake_case(),
                span = column.ident.span()
            ),
        };
        let constant_type = &codec.constant_type;
        let init = &codec.constant;
        let doc = format!(
            "The `{}` column (`{}.{}`).",
            plain_name(&column.ident),
            model.table,
            column.name
        );
        quote! {
            #[doc = #doc]
            #vis const #constant: #constant_type = #init;
        }
    });

    let attribute_items = attributes::trait_items(model, krate);
    let mut errors = Errors::default();
    let setters = attributes::inherent_items(model, &mut errors);
    errors.finish()?;
    let module_items = attributes::module_items(model, krate);

    Ok(quote! {
        #[automatically_derived]
        impl #krate::Model for #ident {
            type Key = #key_ty;

            fn meta() -> &'static #krate::ModelMeta {
                static COLUMNS: [#krate::ColumnMeta; #column_count] = [#(#column_metas),*];
                static META: #krate::ModelMeta = #meta;
                &META
            }

            fn key(&self) -> Self::Key {
                ::core::clone::Clone::clone(&self.#key_ident)
            }

            fn set_key(&mut self, key: Self::Key) {
                self.#key_ident = key;
            }

            fn from_row(row: &#krate::Row) -> #krate::Result<Self> {
                ::core::result::Result::Ok(Self {
                    #(#reads,)*
                    #(#skipped,)*
                })
            }

            #[allow(clippy::vec_init_then_push)]
            fn values(&self) -> #krate::Result<::std::vec::Vec<(&'static str, #krate::Value)>> {
                let mut values = ::std::vec::Vec::with_capacity(#column_count);
                #(#pushes)*
                ::core::result::Result::Ok(values)
            }

            fn value(&self, column: &str) -> #krate::Result<::core::option::Option<#krate::Value>> {
                ::core::result::Result::Ok(match column {
                    #(#value_arms)*
                    _ => ::core::option::Option::None,
                })
            }

            fn set_value(&mut self, column: &str, value: #krate::Value) -> #krate::Result<()> {
                match column {
                    #(#set_arms)*
                    _ => ::core::result::Result::Err(#private::unknown_column::<Self>(column)),
                }
            }

            fn touch_timestamp(&mut self, column: &str, now: #private::DateTime<#private::Utc>) -> #krate::Result<()> {
                #touch_body
            }

            fn timestamp_is_empty(&self, column: &str) -> bool {
                #empty_body
            }

            fn clear_timestamp(&mut self, column: &str) {
                #clear_body
            }

            fn timestamp_value(column: &str, now: #private::DateTime<#private::Utc>) -> ::core::option::Option<#krate::Value> {
                #timestamp_value_body
            }

            #global_scopes

            #hooks

            fn relation(name: &str) -> ::core::option::Option<#krate::relations::DynRelation<Self>> {
                // A `#[relations]` block's inherent function, else the
                // fallback (no declared relations); then the relations other
                // modules defined.
                #[allow(unused_imports)]
                use #private::relations::NoRelations as _;
                match Self::__navree_relations(name) {
                    ::core::option::Option::Some(relation) => ::core::option::Option::Some(relation),
                    ::core::option::Option::None => #private::relations::defined::<Self>(name),
                }
            }

            fn relation_names() -> ::std::vec::Vec<::std::borrow::Cow<'static, str>> {
                #[allow(unused_imports)]
                use #private::relations::NoRelations as _;
                let mut names: ::std::vec::Vec<::std::borrow::Cow<'static, str>> =
                    Self::__navree_relation_names()
                        .iter()
                        .map(|name| ::std::borrow::Cow::Borrowed(*name))
                        .collect();
                names.extend(#private::relations::defined_names::<Self>());
                names
            }

            #attribute_items
        }

        #capabilities

        #[automatically_derived]
        impl #ident {
            #(#constants)*

            #setters
        }

        #module_items
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_are_written_without_token_spacing() {
        let ty: Type = syn::parse_str("Option<chrono::DateTime<chrono::Utc>>").unwrap();
        assert_eq!(type_text(&ty), "Option<chrono::DateTime<chrono::Utc>>");
        let ty: Type = syn::parse_str("std::collections::HashMap<String, u8>").unwrap();
        assert_eq!(type_text(&ty), "std::collections::HashMap<String, u8>");
    }
}
