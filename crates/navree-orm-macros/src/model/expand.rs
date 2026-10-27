//! The code `#[derive(Model)]` writes: the `Model` impl and the column
//! constants.

use heck::ToShoutySnakeCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Type;

use super::{Cast, Column, KeyType, Model, plain_name};

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

/// The path of a cast type.
fn cast_type(cast: &Cast, krate: &TokenStream) -> TokenStream {
    match cast {
        Cast::Json => quote!(#krate::cast::AsJson),
        Cast::StringEnum => quote!(#krate::cast::StringEnum),
        Cast::IntEnum => quote!(#krate::cast::IntEnum),
        Cast::Path(path) => quote!(#path),
    }
}

/// How one column is read, written, converted and typed.
struct Codec {
    /// `row -> field` (reading a row).
    read: TokenStream,
    /// `&self.field -> Value`.
    write: TokenStream,
    /// `value -> field` (`set_value`).
    convert: TokenStream,
    /// The column constant's initializer.
    constant: TokenStream,
}

fn codec(model: &Model, column: &Column, krate: &TokenStream) -> Codec {
    let private = quote!(#krate::__private);
    let ident = &column.ident;
    let ty = &column.ty;
    let name = &column.name;
    let table = &model.table;
    match (&column.cast, &column.inner) {
        (None, _) => {
            let read = if column.default {
                quote!(#private::read_or_default::<Self, #ty>(row, #name)?)
            } else {
                quote!(#private::read::<Self, #ty>(row, #name)?)
            };
            Codec {
                read,
                write: quote!(#krate::Field::to_value(&self.#ident)),
                convert: quote!(#private::convert::<Self, #ty>(column, value)?),
                constant: quote!(#krate::Column::<Self, #ty>::field(#table, #name)),
            }
        }
        (Some(cast), None) => {
            let cast = cast_type(cast, krate);
            let read = if column.default {
                quote!(#private::read_cast_or_default::<Self, #cast, #ty>(row, #name)?)
            } else {
                quote!(#private::read_cast::<Self, #cast, #ty>(row, #name)?)
            };
            Codec {
                read,
                write: quote!(#private::write_cast::<Self, #cast, #ty>(#name, &self.#ident)?),
                convert: quote!(#private::convert_cast::<Self, #cast, #ty>(column, value)?),
                constant: quote!(#krate::Column::<Self, #ty>::with_codec(
                    #table,
                    #name,
                    #private::encode::<#cast, #ty>,
                    #private::decode::<#cast, #ty>,
                )),
            }
        }
        (Some(cast), Some(inner)) => {
            let cast = cast_type(cast, krate);
            let read = if column.default {
                quote!(#private::read_cast_option_or_default::<Self, #cast, #inner>(row, #name)?)
            } else {
                quote!(#private::read_cast_option::<Self, #cast, #inner>(row, #name)?)
            };
            Codec {
                read,
                write: quote!(#private::write_cast_option::<Self, #cast, #inner>(#name, &self.#ident)?),
                convert: quote!(#private::convert_cast_option::<Self, #cast, #inner>(column, value)?),
                constant: quote!(#krate::Column::<Self, #ty>::with_codec(
                    #table,
                    #name,
                    #private::encode_option::<#cast, #inner>,
                    #private::decode_option::<#cast, #inner>,
                )),
            }
        }
    }
}

/// The `Model` impl and the column constants.
pub(crate) fn expand(model: &Model, krate: &TokenStream) -> TokenStream {
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
        if column.inner.is_some() {
            meta = quote!(#meta.nullable());
        }
        if let Some(cast) = &column.cast {
            let cast_name = match cast {
                Cast::Json => "json".to_owned(),
                Cast::StringEnum => "string_enum".to_owned(),
                Cast::IntEnum => "int_enum".to_owned(),
                Cast::Path(path) => quote!(#path).to_string().replace(' ', ""),
            };
            meta = quote!(#meta.cast(#cast_name));
        }
        meta
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

    // from_row.
    let reads = model.columns.iter().zip(&codecs).map(|(column, codec)| {
        let field = &column.ident;
        let read = &codec.read;
        quote!(#field: #read)
    });
    let skipped = model
        .skipped
        .iter()
        .map(|field| quote!(#field: ::core::default::Default::default()));

    // values / value / set_value.
    let values = model.columns.iter().zip(&codecs).map(|(column, codec)| {
        let name = &column.name;
        let write = &codec.write;
        quote!((#name, #write))
    });
    let value_arms = model.columns.iter().zip(&codecs).map(|(column, codec)| {
        let name = &column.name;
        let write = &codec.write;
        quote!(#name => ::core::option::Option::Some(#write),)
    });
    let set_arms = model.columns.iter().zip(&codecs).map(|(column, codec)| {
        let name = &column.name;
        let field = &column.ident;
        let convert = &codec.convert;
        quote!(#name => {
            self.#field = #convert;
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

    // Column constants.
    let constants = model.columns.iter().zip(&codecs).map(|(column, codec)| {
        let constant = format_ident!(
            "{}",
            plain_name(&column.ident).to_shouty_snake_case(),
            span = column.ident.span()
        );
        let ty = &column.ty;
        let init = &codec.constant;
        let doc = format!("The `{}` column (`{}.{}`).", plain_name(&column.ident), model.table, column.name);
        quote! {
            #[doc = #doc]
            #vis const #constant: #krate::Column<Self, #ty> = #init;
        }
    });

    quote! {
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

            fn values(&self) -> #krate::Result<::std::vec::Vec<(&'static str, #krate::Value)>> {
                ::core::result::Result::Ok(::std::vec![#(#values),*])
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
        }

        #[automatically_derived]
        impl #ident {
            #(#constants)*
        }
    }
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
