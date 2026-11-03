//! `#[derive(Form)]` and `#[derive(Options)]`.

use heck::ToKebabCase;
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::ext::IdentExt;
use syn::{Data, DeriveInput, Fields, Ident, LitInt, LitStr};

use crate::shared::{
    apply_case, doc_line, many_inner, named_fields, option_inner, serde_rename, serde_rename_all,
    type_name, ui_path, words,
};

/// What `#[form(...)]` says about a field.
#[derive(Default)]
struct FieldHints {
    label: Option<LitStr>,
    placeholder: Option<LitStr>,
    help: Option<LitStr>,
    prompt: Option<LitStr>,
    error: Option<LitStr>,
    addon: Option<LitStr>,
    rows: Option<LitInt>,
    max: Option<LitInt>,
    widget: Option<Ident>,
    autofocus: bool,
    wide: bool,
    skip: bool,
}

fn hints(attrs: &[syn::Attribute]) -> syn::Result<FieldHints> {
    let mut out = FieldHints::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("form")) {
        attr.parse_nested_meta(|meta| {
            let key = meta.path.get_ident().map(ToString::to_string).unwrap_or_default();
            match key.as_str() {
                "label" => out.label = Some(meta.value()?.parse()?),
                "placeholder" => out.placeholder = Some(meta.value()?.parse()?),
                "help" => out.help = Some(meta.value()?.parse()?),
                "prompt" => out.prompt = Some(meta.value()?.parse()?),
                "error" => out.error = Some(meta.value()?.parse()?),
                "addon" => out.addon = Some(meta.value()?.parse()?),
                "rows" => out.rows = Some(meta.value()?.parse()?),
                "max" => out.max = Some(meta.value()?.parse()?),
                "widget" => out.widget = Some(meta.value()?.parse()?),
                "autofocus" => out.autofocus = true,
                "wide" => out.wide = true,
                "skip" => out.skip = true,
                _ => {
                    return Err(meta.error(format!(
                        "unknown `form` option `{key}`: expected label, placeholder, help, prompt, error, addon, rows, max, widget, autofocus, wide or skip"
                    )));
                }
            }
            Ok(())
        })?;
    }
    Ok(out)
}

/// The widget a field's hint names.
fn named_widget(ui: &TokenStream, widget: &Ident, rows: u32) -> syn::Result<TokenStream> {
    let input = |kind: &str| {
        let kind = Ident::new(kind, widget.span());
        quote!(#ui::form::Widget::Input(#ui::InputKind::#kind))
    };
    Ok(match widget.unraw().to_string().as_str() {
        "text" => input("Text"),
        "email" => input("Email"),
        "url" => input("Url"),
        "tel" => input("Tel"),
        "password" => input("Password"),
        "search" => input("Search"),
        "number" => input("Number"),
        "date" => input("Date"),
        "time" => input("Time"),
        "datetime" => input("DateTime"),
        "date_picker" | "datepicker" => quote!(#ui::form::Widget::DatePicker),
        "time_picker" | "timepicker" => quote!(#ui::form::Widget::TimePicker),
        "hidden" => input("Hidden"),
        "textarea" => quote!(#ui::form::Widget::Textarea { rows: #rows }),
        "select" => quote!(#ui::form::Widget::Select),
        "listbox" => quote!(#ui::form::Widget::Listbox),
        "combobox" => quote!(#ui::form::Widget::Combobox),
        "multiselect" => quote!(#ui::form::Widget::Multi),
        "radio" => quote!(#ui::form::Widget::Radio),
        "checkbox" => quote!(#ui::form::Widget::Checkbox),
        "switch" => quote!(#ui::form::Widget::Switch),
        "money" => quote!(#ui::form::Widget::Money),
        other => {
            return Err(syn::Error::new(
                widget.span(),
                format!(
                    "unknown widget `{other}`: expected text, email, url, tel, password, search, number, date, time, datetime, date_picker, time_picker, hidden, textarea, select, listbox, combobox, multiselect, radio, checkbox, switch or money"
                ),
            ));
        }
    })
}

/// The widget a field's type implies; `None` means an enum with `Options`.
fn inferred_widget(ui: &TokenStream, name: Option<&str>) -> Option<TokenStream> {
    let input = |kind: &str| {
        let kind = Ident::new(kind, proc_macro2::Span::call_site());
        quote!(#ui::form::Widget::Input(#ui::InputKind::#kind))
    };
    Some(match name? {
        "String" | "str" | "Uuid" => input("Text"),
        "bool" => quote!(#ui::form::Widget::Checkbox),
        "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize" | "f32"
        | "f64" | "Decimal" => input("Number"),
        "NaiveDate" => input("Date"),
        "NaiveTime" => input("Time"),
        "NaiveDateTime" | "DateTime" => input("DateTime"),
        "Money" => quote!(#ui::form::Widget::Money),
        _ => return None,
    })
}

fn opt(value: Option<impl ToTokens>) -> TokenStream {
    match value {
        Some(value) => quote!(::core::option::Option::Some(#value)),
        None => quote!(::core::option::Option::None),
    }
}

pub(crate) fn form(input: DeriveInput) -> syn::Result<TokenStream> {
    let ui = ui_path(input.ident.span())?;
    let fields = named_fields(&input, "Form")?;
    let case = serde_rename_all(&input.attrs);
    let mut id = input.ident.to_string().to_kebab_case();
    for attr in input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("form"))
    {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("id") {
                id = meta.value()?.parse::<LitStr>()?.value();
                Ok(())
            } else {
                Err(meta.error("unknown `form` option on a type: expected `id = \"...\"`"))
            }
        })?;
    }
    let mut specs = Vec::new();
    let mut values = Vec::new();
    for field in &fields.named {
        let hint = hints(&field.attrs)?;
        if hint.skip {
            continue;
        }
        let Some(ident) = &field.ident else { continue };
        let rust_name = ident.unraw().to_string();
        let name =
            serde_rename(&field.attrs).unwrap_or_else(|| apply_case(&rust_name, case.as_deref()));
        let label = hint
            .label
            .as_ref()
            .map(LitStr::value)
            .or_else(|| doc_line(&field.attrs))
            .unwrap_or_else(|| words(&rust_name));
        let optional = option_inner(&field.ty).is_some();
        let inner = option_inner(&field.ty).unwrap_or(&field.ty);
        let rows: u32 = match &hint.rows {
            Some(rows) => rows.base10_parse()?,
            None => 3,
        };
        // A `Many<T>` is a multi-select; its options come from the element's type.
        let many = many_inner(inner);
        let element = many.unwrap_or(inner);
        let inferred = inferred_widget(&ui, type_name(element).as_deref());
        let widget = match &hint.widget {
            Some(widget) => named_widget(&ui, widget, rows)?,
            None if many.is_some() => quote!(#ui::form::Widget::Multi),
            None => inferred
                .clone()
                .unwrap_or_else(|| quote!(#ui::form::Widget::Select)),
        };
        let options = if inferred.is_none()
            && hint.widget.as_ref().is_none_or(|w| {
                matches!(
                    w.to_string().as_str(),
                    "select" | "radio" | "listbox" | "combobox" | "multiselect"
                )
            }) {
            quote!(<#element as #ui::form::Options>::OPTIONS)
        } else {
            quote!(&[])
        };
        let (placeholder, help, prompt, error, addon) = (
            opt(hint.placeholder),
            opt(hint.help),
            opt(hint.prompt),
            opt(hint.error),
            opt(hint.addon),
        );
        let (autofocus, wide) = (hint.autofocus, hint.wide);
        let max = opt(hint.max);
        specs.push(quote! {
            #ui::form::FormField {
                name: #name,
                label: #label,
                widget: #widget,
                optional: #optional,
                placeholder: #placeholder,
                help: #help,
                prompt: #prompt,
                error: #error,
                addon: #addon,
                autofocus: #autofocus,
                wide: #wide,
                options: #options,
                max: #max,
            },
        });
        values.push(quote! {
            for value in #ui::form::FieldValue::field_values(&self.#ident) {
                out.push((#name, value));
            }
        });
    }
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #ui::form::Form for #ident #ty_generics #where_clause {
            const ID: &'static str = #id;
            const FIELDS: &'static [#ui::form::FormField] = &[#(#specs)*];
            const RULES: &'static [#ui::form::FieldRules] = <Self as #ui::form::RuleTable>::FIELDS;

            fn values(&self) -> ::std::vec::Vec<(&'static str, ::std::string::String)> {
                let mut out = ::std::vec::Vec::new();
                #(#values)*
                out
            }
        }
    })
}

pub(crate) fn options(input: DeriveInput) -> syn::Result<TokenStream> {
    let ui = ui_path(input.ident.span())?;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "`#[derive(Options)]` works on enums of unit variants",
        ));
    };
    let case = serde_rename_all(&input.attrs);
    let mut pairs = Vec::new();
    let mut arms = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                variant,
                "`#[derive(Options)]` works on unit variants",
            ));
        }
        let mut label = None;
        for attr in variant
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("option"))
        {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("label") {
                    label = Some(meta.value()?.parse::<LitStr>()?.value());
                    Ok(())
                } else {
                    Err(meta.error("unknown `option` option: expected `label = \"...\"`"))
                }
            })?;
        }
        let name = variant.ident.to_string();
        let value =
            serde_rename(&variant.attrs).unwrap_or_else(|| apply_case(&name, case.as_deref()));
        let label = label
            .or_else(|| doc_line(&variant.attrs))
            .unwrap_or_else(|| words(&name));
        let variant_ident = &variant.ident;
        pairs.push(quote!((#value, #label),));
        arms.push(quote!(Self::#variant_ident => #value,));
    }
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #ui::form::Options for #ident #ty_generics #where_clause {
            const OPTIONS: &'static [(&'static str, &'static str)] = &[#(#pairs)*];

            fn option_value(&self) -> &'static str {
                match self { #(#arms)* }
            }
        }

        #[automatically_derived]
        impl #impl_generics #ui::form::FieldValue for #ident #ty_generics #where_clause {
            fn field_value(&self) -> ::core::option::Option<::std::string::String> {
                ::core::option::Option::Some(::std::string::ToString::to_string(#ui::form::Options::option_value(self)))
            }
        }

        #[automatically_derived]
        impl #impl_generics #ui::table::CellValue for #ident #ty_generics #where_clause {
            fn write_cell(&self, out: &mut #ui::table::CellWriter<'_>) {
                out.plain(#ui::table::option_label(<Self as #ui::form::Options>::OPTIONS, #ui::form::Options::option_value(self)));
            }

            fn write_text(&self, out: &mut ::std::string::String) {
                out.push_str(#ui::table::option_label(<Self as #ui::form::Options>::OPTIONS, #ui::form::Options::option_value(self)));
            }
        }

        #[automatically_derived]
        impl #impl_generics #ui::table::ColumnValue for #ident #ty_generics #where_clause {
            const KIND: #ui::table::ValueKind = #ui::table::ValueKind::Text;
        }
    })
}
