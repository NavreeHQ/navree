//! # navree-i18n-macros
//!
//! `embed_locales!("locales")`: embeds every Fluent file under
//! `locales/<locale>/` in the binary, for navree-i18n's
//! `I18nOptions::files`. Use it through `navree_i18n::embed_locales` (or
//! `navree::i18n::embed_locales`).
//!
//! The directory is relative to the crate's `Cargo.toml` (an absolute path
//! also works). Each subdirectory is a locale (`en`, `fr`, `pt-BR`), and
//! every `.ftl` file in it, at any depth, is embedded with
//! `include_str!`, so editing a file rebuilds the crate. A new file is
//! picked up on the next build of the crate (touch a source file after
//! adding one). Every file is parsed while the crate builds: a syntax error
//! is a compile error naming the file and line.
//!
//! The macro expands to a value that converts into `LocaleFiles` (a slice
//! of `(locale, path, source)`), so it fits the `files` field directly:
//!
//! ```ignore
//! I18nModule::for_root(I18nOptions {
//!     files: embed_locales!("locales"),
//!     ..I18nOptions::default()
//! })
//! ```
#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

use proc_macro::TokenStream;
use quote::quote;
use syn::{LitStr, parse_macro_input};

/// Embeds the Fluent files of a locales directory (see the crate docs).
#[proc_macro]
pub fn embed_locales(input: TokenStream) -> TokenStream {
    let dir = parse_macro_input!(input as LitStr);
    let base = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_default();
    match collect(&base.join(dir.value()), &dir.value()) {
        Ok(files) => expand(&files).into(),
        Err(errors) => {
            let message = errors.join("\n");
            syn::Error::new(dir.span(), message)
                .to_compile_error()
                .into()
        }
    }
}

/// One embedded file.
#[derive(Debug, PartialEq, Eq)]
struct EmbeddedFile {
    /// The locale directory's name.
    locale: String,
    /// The path shown in messages: `{dir}/{locale}/{file}`, `/`-separated.
    display: String,
    /// The absolute path, for `include_str!`.
    path: PathBuf,
}

/// The `.ftl` files under `root/<locale>/`, sorted, each parsed; every
/// problem when any file is unreadable or has a syntax error.
fn collect(root: &Path, shown: &str) -> Result<Vec<EmbeddedFile>, Vec<String>> {
    let shown = shown.trim_end_matches(['/', '\\']).replace('\\', "/");
    let entries = fs::read_dir(root).map_err(|error| {
        vec![format!(
            "embed_locales!: cannot read `{}`: {error}",
            root.display()
        )]
    })?;
    let mut locales: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            (!name.starts_with('.')).then(|| (name, entry.path()))
        })
        .collect();
    locales.sort();
    let mut files = Vec::new();
    let mut errors = Vec::new();
    for (locale, dir) in locales {
        let mut found = Vec::new();
        ftl_files(&dir, &mut found);
        found.sort();
        for path in found {
            let relative = path
                .strip_prefix(&dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let display = format!("{shown}/{locale}/{relative}");
            match fs::read_to_string(&path) {
                Ok(source) => errors.extend(syntax_errors(&display, &source)),
                Err(error) => errors.push(format!("{display}: cannot read the file: {error}")),
            }
            let path = path.canonicalize().unwrap_or(path);
            files.push(EmbeddedFile {
                locale: locale.clone(),
                display,
                path,
            });
        }
    }
    if errors.is_empty() {
        Ok(files)
    } else {
        Err(errors)
    }
}

/// Every `.ftl` file under `dir`, recursively (hidden entries skipped).
fn ftl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let hidden = entry.file_name().to_string_lossy().starts_with('.');
        if hidden {
            continue;
        }
        if path.is_dir() {
            ftl_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "ftl") {
            out.push(path);
        }
    }
}

/// `{display}:{line}: {message}` for each syntax error in `source`, at the
/// line where the broken entry starts.
fn syntax_errors(display: &str, source: &str) -> Vec<String> {
    match fluent_syntax::parser::parse(source) {
        Ok(_) => Vec::new(),
        Err((_, errors)) => errors
            .iter()
            .map(|error| {
                let at = error
                    .slice
                    .as_ref()
                    .map_or(error.pos.start, |slice| slice.start);
                let line = source[..at.min(source.len())].matches('\n').count() + 1;
                format!("{display}:{line}: syntax error: {}", error.kind)
            })
            .collect(),
    }
}

/// The expansion: `&[(locale, path, include_str!(..)), ..]` converted with
/// `Into::into`, so it becomes the `LocaleFiles` the field expects.
fn expand(files: &[EmbeddedFile]) -> proc_macro2::TokenStream {
    let entries = files.iter().map(|file| {
        let locale = &file.locale;
        let display = &file.display;
        let path = file.path.to_string_lossy().into_owned();
        quote! { (#locale, #display, ::core::include_str!(#path)) }
    });
    quote! {
        ::core::convert::Into::into(
            &[#(#entries),*] as &'static [(&'static str, &'static str, &'static str)]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, relative: &str, source: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    #[test]
    fn collects_ftl_files_by_locale_in_order() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "fr/main.ftl", "hello = Bonjour\n");
        write(dir.path(), "en/main.ftl", "hello = Hello\n");
        write(dir.path(), "en/mail/welcome.ftl", "welcome = Welcome\n");
        write(dir.path(), "en/notes.txt", "not fluent");
        write(dir.path(), "en/.hidden.ftl", "skipped = yes\n");
        write(dir.path(), "README.md", "not a locale");

        let files = collect(dir.path(), "locales/").unwrap();
        let shown: Vec<(&str, &str)> = files
            .iter()
            .map(|file| (file.locale.as_str(), file.display.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                ("en", "locales/en/mail/welcome.ftl"),
                ("en", "locales/en/main.ftl"),
                ("fr", "locales/fr/main.ftl"),
            ]
        );
        assert!(files.iter().all(|file| file.path.is_absolute()));
    }

    #[test]
    fn syntax_errors_name_the_file_and_line() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "en/main.ftl",
            "ok = Fine\n\nbroken = { $count\n",
        );
        write(dir.path(), "de/main.ftl", "also-broken\n");

        let errors = collect(dir.path(), "locales").unwrap_err();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(
            errors[0].starts_with("locales/de/main.ftl:1: syntax error"),
            "{errors:?}"
        );
        assert!(
            errors[1].starts_with("locales/en/main.ftl:3: syntax error"),
            "{errors:?}"
        );
    }

    #[test]
    fn a_missing_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let errors = collect(&dir.path().join("nope"), "nope").unwrap_err();
        assert!(errors[0].contains("cannot read"), "{errors:?}");
    }

    #[test]
    fn expands_to_a_typed_slice() {
        let files = [EmbeddedFile {
            locale: "en".to_owned(),
            display: "locales/en/main.ftl".to_owned(),
            path: PathBuf::from("/app/locales/en/main.ftl"),
        }];
        let code = expand(&files).to_string();
        assert!(code.contains("\"en\""), "{code}");
        assert!(code.contains("include_str !"), "{code}");
        assert!(code.contains("Into :: into"), "{code}");
    }
}
