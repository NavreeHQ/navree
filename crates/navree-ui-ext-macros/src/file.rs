//! What the two macros share: the file read from the calling crate, its hash, the path to the
//! toolkit as the calling crate names it, the ceiling check and the expansions.

use std::path::{Path, PathBuf};

use proc_macro_crate::FoundCrate;
use proc_macro2::{Literal, Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Expr, LitStr};

/// Which macro is expanding, for the texts of its errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ceiling {
    Script,
    Sheet,
}

impl Ceiling {
    fn macro_name(self) -> &'static str {
        match self {
            Ceiling::Script => "extension_script!",
            Ceiling::Sheet => "extension_sheet!",
        }
    }
}

/// The path to navree-ui as the crate being compiled sees it.
pub(crate) fn ui_path(span: Span) -> syn::Result<TokenStream> {
    if let Ok(found) = proc_macro_crate::crate_name("navree-ui") {
        return Ok(match found {
            FoundCrate::Itself => quote!(crate),
            FoundCrate::Name(name) => {
                let name = format_ident!("{name}");
                quote!(::#name)
            }
        });
    }
    if let Ok(found) = proc_macro_crate::crate_name("navree") {
        return Ok(match found {
            FoundCrate::Itself => quote!(crate::ui),
            FoundCrate::Name(name) => {
                let name = format_ident!("{name}");
                quote!(::#name::ui)
            }
        });
    }
    Err(syn::Error::new(
        span,
        "this macro needs the `navree` crate (or `navree-ui`) in the [dependencies] of your Cargo.toml",
    ))
}

/// The directory the crate being compiled lives in.
pub(crate) fn manifest_dir(span: Span) -> syn::Result<PathBuf> {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| syn::Error::new(span, "CARGO_MANIFEST_DIR is not set; build with cargo"))
}

/// The file at `path`, relative to `manifest`, and where it is.
pub(crate) fn read(
    manifest: &Path,
    path: &LitStr,
    what: Ceiling,
) -> syn::Result<(PathBuf, Vec<u8>)> {
    let absolute = manifest.join(path.value());
    match std::fs::read(&absolute) {
        Ok(bytes) => Ok((absolute, bytes)),
        Err(error) => Err(syn::Error::new(
            path.span(),
            format!(
                "{}: {} cannot be read: {error}",
                what.macro_name(),
                path.value()
            ),
        )),
    }
}

/// The text of a file, or the error that says it is not text.
pub(crate) fn text(bytes: Vec<u8>, path: &LitStr, what: Ceiling) -> syn::Result<String> {
    String::from_utf8(bytes).map_err(|error| {
        syn::Error::new(
            path.span(),
            format!(
                "{}: {} cannot be read: {error}",
                what.macro_name(),
                path.value()
            ),
        )
    })
}

/// The 12 hex digits a served file's name carries: FNV-1a over the bytes, 64 bits, written as 16
/// digits and cut to 12.
pub(crate) fn hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let mut out = format!("{hash:016x}");
    out.truncate(12);
    out
}

/// A `const` item that stops the build when `length` is more than `most`, with the file, the
/// length and the ceiling as the caller wrote it in the message. `most` is any expression that is
/// a number of bytes.
pub(crate) fn ceiling(file: &str, length: usize, most: &Expr, what: Ceiling) -> TokenStream {
    let shown = quote!(#most).to_string();
    let message = match what {
        Ceiling::Script => {
            format!("extension_script!: {file} is {length} bytes, over its ceiling of {shown}")
        }
        Ceiling::Sheet => format!(
            "extension_sheet!: {file} is {length} bytes once scoped, over its ceiling of {shown}"
        ),
    };
    // The message is a format string: a brace in a path or an expression is written twice.
    let message = message.replace('{', "{{").replace('}', "}}");
    let length = Literal::usize_unsuffixed(length);
    quote! {
        const _: () = ::core::assert!(#length <= (#most), #message);
    }
}

/// The expansion of `extension_script!`: the file's own bytes, its hash and no namespace.
pub(crate) fn script_tokens(
    ui: &TokenStream,
    file: &str,
    absolute: &Path,
    bytes: &[u8],
    most: &Expr,
) -> TokenStream {
    let ceiling = ceiling(file, bytes.len(), most, Ceiling::Script);
    let hash = hash(bytes);
    let absolute = absolute.to_string_lossy().into_owned();
    quote! {
        {
            #ceiling
            #ui::ExtensionFile::__new(
                ::core::include_bytes!(#absolute),
                #hash,
                ::core::option::Option::None,
            )
        }
    }
}

/// The expansion of `extension_sheet!`: the sheet as it is served, its hash and its namespace. The
/// source file is named too, so that a change to it makes cargo build the crate again.
pub(crate) fn sheet_tokens(
    ui: &TokenStream,
    file: &str,
    absolute: &Path,
    namespace: &str,
    served: &str,
    most: &Expr,
) -> TokenStream {
    let ceiling = ceiling(file, served.len(), most, Ceiling::Sheet);
    let hash = hash(served.as_bytes());
    let absolute = absolute.to_string_lossy().into_owned();
    let served = Literal::byte_string(served.as_bytes());
    quote! {
        {
            #ceiling
            const _: &[u8] = ::core::include_bytes!(#absolute);
            #ui::ExtensionFile::__new(
                #served,
                #hash,
                ::core::option::Option::Some(#namespace),
            )
        }
    }
}
