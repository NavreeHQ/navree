//! `embed_templates!`: every file under a folder of the calling crate, embedded
//! with `include_str!` as (path, source) pairs, for a plugin's
//! `m.plugin_templates(..)`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use proc_macro2::TokenStream;
use quote::quote;
use syn::LitStr;

/// One embedded template.
#[derive(Debug)]
struct EmbeddedTemplate {
    /// The file's path relative to the folder, `/`-separated.
    name: String,
    /// The file's canonical path, for `include_str!`.
    path: PathBuf,
}

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    expand_inner(input).unwrap_or_else(syn::Error::into_compile_error)
}

fn expand_inner(input: TokenStream) -> syn::Result<TokenStream> {
    let dir: LitStr = syn::parse2(input)?;
    let crate_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_default();
    match collect(&crate_dir, &dir.value()) {
        Ok(files) => Ok(tokens(&files)),
        Err(problems) => Err(syn::Error::new(dir.span(), problems.join("\n"))),
    }
}

/// The files below `dir`, a folder of the crate in `crate_dir`, at any depth,
/// sorted by name, each read as UTF-8 text; every problem when the folder is
/// refused or any file is. Hidden entries are skipped, and a symbolic link
/// (or a junction) is never followed: it is a problem.
fn collect(crate_dir: &Path, dir: &str) -> Result<Vec<EmbeddedTemplate>, Vec<String>> {
    let dir = dir.trim_end_matches('/');
    if !is_safe_template_path(dir) {
        return Err(vec![format!(
            "embed_templates!: `{dir}` must be a folder of this crate, as a relative path (no `..`, no leading `/`, no `\\`, no `:`)"
        )]);
    }
    let cannot_read =
        |error: io::Error| vec![format!("embed_templates!: cannot read `{dir}`: {error}")];
    let crate_root = crate_dir.canonicalize().map_err(cannot_read)?;
    let root = crate_dir.join(dir).canonicalize().map_err(cannot_read)?;
    if !root.starts_with(&crate_root) {
        return Err(vec![format!(
            "embed_templates!: `{dir}` is outside this crate's folder"
        )]);
    }
    let mut collected = Collected {
        dir: dir.to_owned(),
        files: Vec::new(),
        problems: Vec::new(),
    };
    collected.walk(&root, "");
    if collected.problems.is_empty() {
        collected.files.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(collected.files)
    } else {
        collected.problems.sort();
        Err(collected
            .problems
            .into_iter()
            .map(|(_, problem)| problem)
            .collect())
    }
}

/// What the walk below a folder has found.
struct Collected {
    /// The folder as the macro was given it (trailing `/` trimmed), for messages.
    dir: String,
    files: Vec<EmbeddedTemplate>,
    /// Each problem with the name it belongs to, so the problems sort by name.
    problems: Vec<(String, String)>,
}

impl Collected {
    /// Reads `folder`, whose entries are named `{prefix}{entry}` in the result.
    fn walk(&mut self, folder: &Path, prefix: &str) {
        let entries = match fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(error) => {
                self.cannot_read(prefix.trim_end_matches('/'), &error);
                return;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.cannot_read(prefix.trim_end_matches('/'), &error);
                    continue;
                }
            };
            let file_name = entry.file_name();
            if file_name.to_string_lossy().starts_with('.') {
                continue;
            }
            let Some(file_name) = file_name.to_str() else {
                let lossy = file_name.to_string_lossy();
                let text = format!(
                    "embed_templates!: a file name in `{}` is not UTF-8: {lossy}",
                    self.dir
                );
                self.problems.push((format!("{prefix}{lossy}"), text));
                continue;
            };
            let name = format!("{prefix}{file_name}");
            let path = entry.path();
            let kind = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata.file_type(),
                Err(error) => {
                    self.cannot_read(&name, &error);
                    continue;
                }
            };
            if kind.is_symlink() {
                let text = format!(
                    "embed_templates!: `{}/{name}` is a symbolic link; put the file itself in the folder",
                    self.dir
                );
                self.problems.push((name, text));
            } else if kind.is_dir() {
                self.walk(&path, &format!("{name}/"));
            } else if !kind.is_file() {
                let text = format!(
                    "embed_templates!: `{}/{name}` is not a regular file",
                    self.dir
                );
                self.problems.push((name, text));
            } else if !is_safe_template_path(&name) {
                let text = format!(
                    "embed_templates!: `{}/{name}` is not a safe template path: no `\\`, `:`, `<`, `>`, `\"`, `|`, `?`, `*` or control character, no name ending in `.` or a space, no device name such as `con` or `nul`, at most 255 bytes in all and 128 per name",
                    self.dir
                );
                self.problems.push((name, text));
            } else {
                self.read_file(name, &path);
            }
        }
    }

    /// Keeps the file `name` at `path` when it reads as UTF-8 text.
    fn read_file(&mut self, name: String, path: &Path) {
        match fs::read_to_string(path) {
            Ok(_) => match path.canonicalize() {
                Ok(path) => self.files.push(EmbeddedTemplate { name, path }),
                Err(error) => self.cannot_read(&name, &error),
            },
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                let text = format!(
                    "embed_templates!: `{}/{name}` is not UTF-8 text: {error}",
                    self.dir
                );
                self.problems.push((name, text));
            }
            Err(error) => self.cannot_read(&name, &error),
        }
    }

    /// Records that `name` (empty: the folder itself) could not be read.
    fn cannot_read(&mut self, name: &str, error: &io::Error) {
        let shown = if name.is_empty() {
            self.dir.clone()
        } else {
            format!("{}/{name}", self.dir)
        };
        self.problems.push((
            name.to_owned(),
            format!("embed_templates!: cannot read `{shown}`: {error}"),
        ));
    }
}

/// Whether `path` is a safe relative path: the single-path rule the kernel
/// applies to every file a plugin ships (`is_safe_relative_path` in
/// navree-kernel, kept equal by a test). A name that follows it is also one a
/// file system accepts.
fn is_safe_template_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 255
        && !path.chars().any(is_refused_char)
        && path.split('/').all(is_safe_segment)
}

/// A character no template path holds: `\`, `:`, `<>"|?*` or a control
/// character.
fn is_refused_char(c: char) -> bool {
    c.is_control() || matches!(c, '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
}

/// Whether one segment of a template path is safe.
fn is_safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 128
        && segment != "."
        && segment != ".."
        && !segment.ends_with('.')
        && !segment.ends_with(' ')
        && !is_device_name(segment)
}

/// Whether `segment` names a Windows device: its part before the first `.`,
/// without trailing spaces, is `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`,
/// `CONOUT$`, `COM0` to `COM9`, `LPT0` to `LPT9`, or `COM` or `LPT` followed
/// by a superscript one, two or three, ignoring ASCII case.
fn is_device_name(segment: &str) -> bool {
    let stem = segment
        .split_once('.')
        .map_or(segment, |(stem, _)| stem)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    match stem.as_bytes() {
        b"CON" | b"PRN" | b"AUX" | b"NUL" | b"CONIN$" | b"CONOUT$" => true,
        [b'C', b'O', b'M', digit] | [b'L', b'P', b'T', digit] => digit.is_ascii_digit(),
        // The superscripts `¹` (U+00B9), `²` (U+00B2) and `³` (U+00B3) are
        // `C2 B9`, `C2 B2` and `C2 B3` in UTF-8.
        [b'C', b'O', b'M', 0xC2, last] | [b'L', b'P', b'T', 0xC2, last] => {
            matches!(*last, 0xB9 | 0xB2 | 0xB3)
        }
        _ => false,
    }
}

/// The expansion: `&[(name, include_str!(..)), ..]` converted with
/// `Into::into`, so it becomes the value the registering method expects.
fn tokens(files: &[EmbeddedTemplate]) -> TokenStream {
    let entries = files.iter().map(|file| {
        let name = &file.name;
        let path = file.path.to_string_lossy().into_owned();
        quote! { (#name, ::core::include_str!(#path)) }
    });
    quote! {
        ::core::convert::Into::into(
            &[#(#entries),*] as &'static [(&'static str, &'static str)]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[cfg(unix)]
    fn unsafe_name(shown: &str) -> String {
        format!(
            "embed_templates!: `{shown}` is not a safe template path: no `\\`, `:`, `<`, `>`, `\"`, `|`, `?`, `*` or control character, no name ending in `.` or a space, no device name such as `con` or `nul`, at most 255 bytes in all and 128 per name"
        )
    }

    #[test]
    fn embed_templates_collects_files_in_order_and_refuses_unsafe_names() {
        let crate_dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let root = crate_dir.path();
        write(root, "views/list.html", b"<ul></ul>");
        write(root, "views/admin/edit.html", b"<form></form>");
        write(root, "views/.hidden.html", b"skipped");
        write(root, "views/.drafts/old.html", b"skipped");
        let found = collect(root, "views").unwrap();
        let names: Vec<&str> = found.iter().map(|file| file.name.as_str()).collect();
        assert_eq!(names, ["admin/edit.html", "list.html"]);
        assert!(found.iter().all(|file| file.path.is_absolute()));
        assert_eq!(collect(root, "views/").unwrap().len(), 2);

        for dir in ["../views", "/views", "views\\admin", "C:/views", ".", ""] {
            assert_eq!(
                collect(root, dir).unwrap_err(),
                [format!(
                    "embed_templates!: `{dir}` must be a folder of this crate, as a relative path (no `..`, no leading `/`, no `\\`, no `:`)"
                )]
            );
        }
        assert!(
            collect(root, "missing").unwrap_err()[0]
                .starts_with("embed_templates!: cannot read `missing`: ")
        );
        write(root, "binary/logo.html", &[0xff, 0xfe, 0x00]);
        assert!(
            collect(root, "binary").unwrap_err()[0]
                .starts_with("embed_templates!: `binary/logo.html` is not UTF-8 text: ")
        );

        #[cfg(unix)]
        {
            for name in [
                "unsafe/con.html",
                "unsafe/notes.",
                "unsafe/a\\b.html",
                "unsafe/ok.html",
            ] {
                write(root, name, b"x");
            }
            assert_eq!(
                collect(root, "unsafe").unwrap_err(),
                [
                    unsafe_name("unsafe/a\\b.html"),
                    unsafe_name("unsafe/con.html"),
                    unsafe_name("unsafe/notes.")
                ]
            );
            fs::create_dir_all(root.join("linked")).unwrap();
            std::os::unix::fs::symlink(root.join("views/list.html"), root.join("linked/list.html"))
                .unwrap();
            assert_eq!(
                collect(root, "linked").unwrap_err(),
                [
                    "embed_templates!: `linked/list.html` is a symbolic link; put the file itself in the folder"
                ]
            );
            let outside = tempfile::Builder::new()
                .prefix("navree-test-")
                .tempdir()
                .unwrap();
            std::os::unix::fs::symlink(outside.path(), root.join("elsewhere")).unwrap();
            assert_eq!(
                collect(root, "elsewhere").unwrap_err(),
                ["embed_templates!: `elsewhere` is outside this crate's folder"]
            );
        }
    }

    #[test]
    fn embed_templates_path_rule_agrees_with_the_kernels() {
        let longest = format!("{}/{}", "a".repeat(128), "b".repeat(126));
        let too_long = format!("{}/{}", "a".repeat(128), "b".repeat(127));
        let long_segment = "a".repeat(129);
        let cases = [
            "list.html",
            "admin/edit.html",
            "ਸੂਚਨਾ.html",
            "a b.html",
            "console.html",
            "com10.html",
            ".hidden",
            "",
            "/abs.html",
            "../x.html",
            "a/../b.html",
            "./a.html",
            "a//b.html",
            "a\\b.html",
            "C:x.html",
            "a:b.html",
            "a<b",
            "a>b",
            "a\"b",
            "a|b",
            "a?b",
            "a*b",
            "a\u{7}b",
            "name.",
            "name ",
            ". .",
            "con",
            "con.html",
            "CON",
            "con .txt",
            "conin$",
            "CONOUT$.html",
            "com1.css",
            "lpt9",
            "lpt\u{b2}.html",
            "com\u{b9}",
            "nul.d/x",
            longest.as_str(),
            too_long.as_str(),
            long_segment.as_str(),
        ];
        for path in cases {
            assert_eq!(
                is_safe_template_path(path),
                navree_kernel::__private::is_safe_relative_path(path),
                "{path:?}"
            );
        }
        assert!(is_safe_template_path("admin/edit.html") && !is_safe_template_path("con .txt"));
    }

    #[test]
    fn embed_templates_expands_to_a_typed_slice() {
        let files = [EmbeddedTemplate {
            name: "admin/edit.html".to_owned(),
            path: PathBuf::from("/srv/notices/views/admin/edit.html"),
        }];
        let code = tokens(&files).to_string();
        for needle in [
            "\"admin/edit.html\"",
            "include_str !",
            "Into :: into",
            "'static",
        ] {
            assert!(code.contains(needle), "{needle}: {code}");
        }
    }
}
