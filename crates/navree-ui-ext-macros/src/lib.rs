//! # navree-ui-ext-macros
//!
//! The two file macros of Navree UI's extensions, `extension_script!` and `extension_sheet!`: a
//! component's script and stylesheet, read, checked and hashed when a plugin's crate compiles.
//! Use them through `navree_ui` (with its `extension-files` feature) or `navree::ui` (with the
//! facade's `ui-extension-files`), not from this crate.
//!
//! Both read their file relative to the folder of the calling crate's `Cargo.toml` and expand to an
//! `ExtensionFile`, which `UiExtension::script` and `UiExtension::sheet` take. Both name the file
//! to cargo, so a change to it builds the crate again, and both stop the build when the file is
//! longer than the ceiling given as the last argument.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, LitStr, Token};

mod file;
mod sheet;

use file::Ceiling;

/// A plugin's script, read and hashed when the crate compiles: an `ExtensionFile` for
/// `UiExtension::script`.
///
/// The first argument is the file's path, relative to the folder of your crate's `Cargo.toml`. The
/// second is the most the file may be, in bytes, written as any expression: `200 * 1024`, or a
/// constant. A longer file stops the build and says how long it is. The file is hashed while the
/// macro runs, so a file of several megabytes costs no compile-time evaluation, and a file that
/// cannot be read stops the build with its path and the reason.
///
/// ```ignore
/// const MAP_SCRIPT: ExtensionFile = extension_script!("assets/map.js", 200 * 1024);
/// ```
///
/// The tests of `plugins/geo` prove this example: its script is read, hashed and held to its
/// ceiling when the plugin compiles.
#[proc_macro]
pub fn extension_script(input: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(input as ScriptArgs);
    expand_script(&args)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A plugin's stylesheet, read, checked, scoped to its namespace and hashed when the crate
/// compiles: an `ExtensionFile` for `UiExtension::sheet`.
///
/// The arguments are the file's path, relative to the folder of your crate's `Cargo.toml`; the
/// plugin's namespace, as a string; and the most the sheet may be once it is scoped, in bytes,
/// written as any expression. A longer sheet, a path that cannot be read and a namespace that is
/// not one (a lower-case letter followed by up to 63 lower-case letters, digits or `_`) stop the
/// build.
///
/// ```ignore
/// const MAP_SHEET: ExtensionFile = extension_sheet!("assets/map.css", "geo", 32 * 1024);
/// ```
///
/// The tests of `plugins/geo` prove this example: its sheet is scoped and hashed when the plugin
/// compiles. The scoper's rules are proved by this crate's own tests.
///
/// The sheet is read with the tokenizer browsers use, and what is served is what was read, written
/// again: names, strings and addresses with the escapes a browser expects, numbers as they were
/// written, white space as one space, comments never. The served sheet is read once more, and the
/// build stops unless it holds exactly what was checked.
///
/// Every rule is served inside `:where([data-slot^="geo:"])` (for the namespace `geo`), which adds
/// no specificity, so a rule applies to an element that is a component's root or inside one. To
/// keep that true, the build stops, naming the file and the line, for:
///
/// - a selector that can style outside the component: once the rule's own `&` is added where the
///   selector writes none, `&` must appear exactly once, never inside `:is()`, `:where()`,
///   `:not()`, `:has()`, `:nth-child(.. of ..)` or any other function, and the compound that holds
///   it may not be followed by `+` or `~`. What comes before `&` (`.dark &`, `body.ltr & .x`) is
///   allowed;
/// - an at-rule outside the list: `@media`, `@supports`, `@container`, `@starting-style` and
///   `@layer` blocks stay inside the wrapper, and `@keyframes`, `@property`, `@font-face`,
///   `@counter-style` and `@layer` statements are hoisted out of it, from the top level only.
///   `@import`, `@charset`, `@namespace`, `@page`, `@scope`, `@position-try`, `@view-transition`,
///   `@font-feature-values` and `@font-palette-values` are refused, and so is every other;
/// - a global name outside the namespace: a hoisted rule's name must be the namespace, two dashes
///   and the rest (`geo--spin`); an `@property` name, and each value of `anchor-name`,
///   `position-anchor`, `view-transition-name`, `scroll-timeline-name`, `view-timeline-name`,
///   `timeline-scope` and `anchor-scope`, must be two dashes, the namespace, two dashes and the
///   rest (`--geo--pin`). These properties also take `none`, `auto`, `match-element` and the
///   CSS-wide keywords, and the last two take `all`;
/// - an address that is not a `data:` URL, in `url()`, `src()`, `image()` and `image-set()`: ship a
///   picture as an `AppImage` and pass its address instead;
/// - blocks that do not balance, and anything that would be read differently once it is served.
///
/// A selector whose whole first compound is `:root`, that writes no `&` and is not inside another
/// rule (it may be inside `@media` and the other groups), means the component's root and is served
/// as `&`. Where a browser would drop a declaration (`*zoom: 1`), the declaration is not served.
#[proc_macro]
pub fn extension_sheet(input: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(input as SheetArgs);
    expand_sheet(&args)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// The path and the ceiling of `extension_script!`.
struct ScriptArgs {
    path: LitStr,
    most: Expr,
}

impl Parse for ScriptArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let path: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;
        let most: Expr = input.parse()?;
        Ok(ScriptArgs { path, most })
    }
}

/// The path, the namespace and the ceiling of `extension_sheet!`.
struct SheetArgs {
    path: LitStr,
    namespace: LitStr,
    most: Expr,
}

impl Parse for SheetArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let path: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;
        let namespace: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;
        let most: Expr = input.parse()?;
        Ok(SheetArgs {
            path,
            namespace,
            most,
        })
    }
}

/// The expansion of `extension_script!`, or the error to report at its path.
fn expand_script(args: &ScriptArgs) -> syn::Result<TokenStream2> {
    let ui = file::ui_path(args.path.span())?;
    let manifest = file::manifest_dir(args.path.span())?;
    let (absolute, bytes) = file::read(&manifest, &args.path, Ceiling::Script)?;
    Ok(file::script_tokens(
        &ui,
        &args.path.value(),
        &absolute,
        &bytes,
        &args.most,
    ))
}

/// The expansion of `extension_sheet!`, or the error to report at its path or its namespace.
fn expand_sheet(args: &SheetArgs) -> syn::Result<TokenStream2> {
    let namespace = args.namespace.value();
    if let Some(text) = sheet::namespace_error(&namespace) {
        return Err(syn::Error::new(args.namespace.span(), text));
    }
    let ui = file::ui_path(args.path.span())?;
    let manifest = file::manifest_dir(args.path.span())?;
    let (absolute, bytes) = file::read(&manifest, &args.path, Ceiling::Sheet)?;
    let source = file::text(bytes, &args.path, Ceiling::Sheet)?;
    let path = args.path.value();
    let served = sheet::scope(&source, &namespace, &path)
        .map_err(|text| syn::Error::new(args.path.span(), text))?;
    Ok(file::sheet_tokens(
        &ui, &path, &absolute, &namespace, &served, &args.most,
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    use proc_macro2::Span;
    use quote::quote;
    use syn::{Expr, LitStr};

    use super::Ceiling;
    use super::{file, sheet};

    const FILE: &str = "assets/x.css";

    /// What every at-rule refusal says after the rule's name.
    const NOT_TAKEN: &str =
        "is not taken: a plugin's sheet styles its own components, and bundles what it needs";
    const OUTSIDE: &str = "can style outside the component: & appears once, outside :is/:where/:not/:has, and never before + or ~";
    const URL_TEXT: &str = "url(..) in a plugin's sheet is a data: URL; ship pictures as AppImage and pass their address";
    const PLAIN: &str = "the sheet does not parse the same once served; write it in plain CSS";

    fn scope(source: &str) -> Result<String, String> {
        sheet::scope(source, "geo", FILE)
    }

    /// The served sheet, squeezed; the source must be taken.
    fn served(source: &str) -> String {
        match scope(source) {
            Ok(text) => squeeze(&text),
            Err(error) => panic!("{source}: {error}"),
        }
    }

    /// The refusal of `source`, which must be refused.
    fn refused(source: &str) -> String {
        match scope(source) {
            Ok(text) => panic!("{source} was taken and served as {text}"),
            Err(error) => error,
        }
    }

    fn wrapped(inside: &str) -> String {
        format!(":where([data-slot^=\"geo:\"]){{{inside}}}")
    }

    /// The text of a refusal on `line`.
    fn at(line: u32, what: &str) -> String {
        format!("extension_sheet!: {FILE}:{line}: {what}")
    }

    fn selector_refusal(line: u32, selector: &str) -> String {
        at(line, &format!("the selector `{selector}` {OUTSIDE}"))
    }

    fn global_refusal(line: u32, name: &str, form: &str) -> String {
        at(
            line,
            &format!("`{name}` is a global name and must be {form}\u{2026}"),
        )
    }

    fn expr(text: &str) -> Expr {
        match syn::parse_str(text) {
            Ok(expr) => expr,
            Err(error) => panic!("{text}: {error}"),
        }
    }

    fn lit(text: &str) -> LitStr {
        LitStr::new(text, Span::call_site())
    }

    /// White space next to `{`, `}`, `;`, `,`, `:`, `(` and `)` dropped, and every other run of
    /// white space one space, so two texts that differ only in layout are equal.
    fn squeeze(text: &str) -> String {
        let mut collapsed = String::new();
        let mut in_space = false;
        for c in text.chars() {
            if c.is_whitespace() {
                in_space = true;
                continue;
            }
            if in_space && !collapsed.is_empty() {
                collapsed.push(' ');
            }
            in_space = false;
            collapsed.push(c);
        }
        loop {
            let mut next = collapsed.clone();
            for c in ['{', '}', ';', ',', ':', '(', ')'] {
                next = next
                    .replace(&format!(" {c}"), &c.to_string())
                    .replace(&format!("{c} "), &c.to_string());
            }
            if next == collapsed {
                return next;
            }
            collapsed = next;
        }
    }

    /// A fresh folder under the temp dir.
    fn temp_dir() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "navree-ui-ext-macros-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("a temporary folder");
        dir
    }

    // ------------------------------------------------------------------------- the file macros

    #[test]
    fn extension_script_hashes_a_two_megabyte_file_and_checks_its_ceiling() {
        // FNV-1a over 64 bits, 16 digits cut to 12: the published values for "" and "a".
        assert_eq!(file::hash(b""), "cbf29ce48422");
        assert_eq!(file::hash(b"a"), "af63dc4c8601");

        let big = vec![b'x'; 2_097_152];
        let mut expected: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in &big {
            expected ^= u64::from(*byte);
            expected = expected.wrapping_mul(0x0100_0000_01b3);
        }
        let started = Instant::now();
        let hashed = file::hash(&big);
        assert!(
            started.elapsed().as_secs() < 1,
            "hashing two megabytes took {:?}",
            started.elapsed()
        );
        assert_eq!(hashed.as_str(), &format!("{expected:016x}")[..12]);

        let tokens = file::ceiling(
            "assets/big.js",
            2_097_152,
            &expr("200 * 1024"),
            Ceiling::Script,
        )
        .to_string();
        assert!(tokens.contains("2097152 <="), "{tokens}");
        assert!(tokens.contains("(200 * 1024)"), "{tokens}");
        assert!(
            tokens.contains(
                "extension_script!: assets/big.js is 2097152 bytes, over its ceiling of 200 * 1024"
            ),
            "{tokens}"
        );

        // A sheet's ceiling is on the scoped bytes, and says so.
        let tokens =
            file::ceiling("assets/map.css", 40_000, &expr("32 * 1024"), Ceiling::Sheet).to_string();
        assert!(
            tokens.contains(
                "extension_sheet!: assets/map.css is 40000 bytes once scoped, over its ceiling of 32 * 1024"
            ),
            "{tokens}"
        );
    }

    #[test]
    fn a_brace_in_a_path_or_a_ceiling_is_written_twice_in_the_message() {
        let tokens = file::ceiling("assets/{a}.js", 9, &expr("{ 5 }"), Ceiling::Script).to_string();
        assert!(tokens.contains("assets/{{a}}.js"), "{tokens}");
        assert!(tokens.contains("{{ 5 }}"), "{tokens}");
    }

    #[test]
    fn a_file_the_macro_cannot_read_fails_with_its_path_and_the_reason() {
        let dir = temp_dir();
        let error = file::read(&dir, &lit("assets/missing.js"), Ceiling::Script)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("extension_script!: assets/missing.js cannot be read: "),
            "{error}"
        );
        let error = file::read(&dir, &lit("assets/missing.css"), Ceiling::Sheet)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("extension_sheet!: assets/missing.css cannot be read: "),
            "{error}"
        );

        // A sheet that is not text is a file that cannot be read.
        std::fs::write(dir.join("bad.css"), [0xff, 0xfe, b'a']).expect("a file");
        let (_, bytes) = file::read(&dir, &lit("bad.css"), Ceiling::Sheet).expect("it is there");
        let error = file::text(bytes, &lit("bad.css"), Ceiling::Sheet)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("extension_sheet!: bad.css cannot be read: "),
            "{error}"
        );

        // A file that is there is read as it is, from the folder of the crate.
        std::fs::write(dir.join("ok.js"), "export const contract = 1;").expect("a file");
        let (absolute, bytes) =
            file::read(&dir, &lit("ok.js"), Ceiling::Script).expect("it is there");
        assert_eq!(bytes, b"export const contract = 1;");
        assert!(absolute.ends_with("ok.js") && absolute.starts_with(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_expansions_name_the_file_the_hash_and_the_namespace() {
        let ui = quote!(::navree_ui);
        let script = file::script_tokens(
            &ui,
            "assets/map.js",
            &PathBuf::from("/crate/assets/map.js"),
            b"export const contract = 1;",
            &expr("200 * 1024"),
        )
        .to_string()
        .replace(' ', "");
        assert!(
            script.contains("::navree_ui::ExtensionFile::__new(::core::include_bytes!"),
            "{script}"
        );
        assert!(script.contains("\"/crate/assets/map.js\""), "{script}");
        assert!(
            script.contains(&format!(
                "\"{}\"",
                file::hash(b"export const contract = 1;")
            )),
            "{script}"
        );
        assert!(script.contains("::core::option::Option::None"), "{script}");
        assert!(script.contains("const_:()=::core::assert!"), "{script}");

        let served = ":where([data-slot^=\"geo:\"]){.pin{color:red}}";
        let sheet = file::sheet_tokens(
            &ui,
            "assets/map.css",
            &PathBuf::from("/crate/assets/map.css"),
            "geo",
            served,
            &expr("32 * 1024"),
        )
        .to_string()
        .replace(' ', "");
        assert!(
            sheet.contains("::navree_ui::ExtensionFile::__new(b\""),
            "{sheet}"
        );
        assert!(
            sheet.contains(&format!("\"{}\"", file::hash(served.as_bytes()))),
            "{sheet}"
        );
        assert!(
            sheet.contains("::core::option::Option::Some(\"geo\")"),
            "{sheet}"
        );
        // The source is named too, so that a change to it builds the crate again.
        assert!(
            sheet.contains("const_:&[u8]=::core::include_bytes!(\"/crate/assets/map.css\")"),
            "{sheet}"
        );
    }

    // ------------------------------------------------------------------------- what is served

    #[test]
    fn extension_sheet_wraps_rules_under_the_namespace_and_hoists_global_at_rules() {
        let source = r#".pin{color:red} @keyframes geo--pulse{from{opacity:0}to{opacity:1}} :root{--geo--size:8px} :root .pin{width:1px} @media (min-width:40rem){.pin{width:var(--geo--size)}} @property --geo--size{syntax:"<length>";inherits:false;initial-value:0px} @font-face{font-family:"geo--icons";src:url(data:font/woff2;base64,AAAA)} @counter-style geo--stars{system:cyclic;symbols:"*"}"#;
        let expected = r#"@keyframes geo--pulse{from{opacity:0}to{opacity:1}}@property --geo--size{syntax:"<length>";inherits:false;initial-value:0px}@font-face{font-family:"geo--icons";src:url(data:font/woff2;base64,AAAA)}@counter-style geo--stars{system:cyclic;symbols:"*"}:where([data-slot^="geo:"]){.pin{color:red}&{--geo--size:8px}& .pin{width:1px}@media(min-width:40rem){.pin{width:var(--geo--size)}}}"#;
        assert_eq!(served(source), squeeze(expected));
    }

    #[test]
    fn a_sheet_with_nothing_in_it_is_the_empty_wrapper() {
        assert_eq!(scope("").as_deref(), Ok(":where([data-slot^=\"geo:\"]){}"));
        assert_eq!(
            scope(" /* nothing */ ").as_deref(),
            Ok(":where([data-slot^=\"geo:\"]){}")
        );
    }

    #[test]
    fn white_space_is_one_space_and_a_comment_is_never_served() {
        assert_eq!(
            scope(".a\n  .b\t{\n color :  red ;\n /* gone */ margin: 0   auto ;\n}").as_deref(),
            Ok(wrapped(".a .b{color:red;margin:0 auto}").as_str())
        );
        assert_eq!(
            scope(".a,\n.b{color:red}").as_deref(),
            Ok(wrapped(".a, .b{color:red}").as_str())
        );
    }

    #[test]
    fn a_comment_between_two_tokens_keeps_them_apart_and_nothing_else_is_added() {
        // Without the empty comment these would be read as `1px-2px` (one unit) and `ab` (one
        // name).
        assert_eq!(
            scope(".a{margin:1px/**/-2px;font:a/**/b;color:red/**/;width:1px}").as_deref(),
            Ok(wrapped(".a{margin:1px/**/-2px;font:a/**/b;color:red;width:1px}").as_str())
        );
        // Tokens that were next to each other stay so: a unicode range is three tokens.
        assert_eq!(
            scope("@font-face{unicode-range:U+0025-00FF}").as_deref(),
            Ok(format!("@font-face{{unicode-range:U+0025-00FF}}{}", wrapped("")).as_str())
        );
    }

    #[test]
    fn numbers_are_served_as_they_were_written() {
        let values = [
            "33.3333%", "0.1%", "1.0", "1e3", "+.5", "-0", "00012", "1.50em", "1e3px", "10E2",
            "-.25rem", "100%", "0.75rem", "1e-3",
        ];
        for value in values {
            let source = format!(".a{{width:{value}}}");
            assert_eq!(
                scope(&source).as_deref(),
                Ok(wrapped(&format!(".a{{width:{value}}}")).as_str()),
                "{value}"
            );
        }
        // A unit that would be read as an exponent is escaped, as a browser writes it.
        assert_eq!(
            scope(".a{width:1\\65 }").as_deref(),
            Ok(wrapped(".a{width:1\\65 }").as_str())
        );
    }

    #[test]
    fn names_and_strings_are_written_again_with_their_escapes() {
        // `\26` is an ampersand inside a name, not the nesting selector.
        assert_eq!(
            scope(".a\\26 b{content:'x\"y'}").as_deref(),
            Ok(wrapped(".a\\&b{content:\"x\\\"y\"}").as_str())
        );
        assert_eq!(
            scope(".\\31 23{color:#FFF;background:#123456}").as_deref(),
            Ok(wrapped(".\\31 23{color:#FFF;background:#123456}").as_str())
        );
        assert_eq!(
            scope(".a{content:\"\u{20b9} 1,00,000\"}").as_deref(),
            Ok(wrapped(".a{content:\"\u{20b9} 1,00,000\"}").as_str())
        );
        // A byte order mark is not part of the first rule.
        assert_eq!(scope("\u{feff}.a{color:red}"), scope(".a{color:red}"));
    }

    #[test]
    fn a_declaration_a_browser_would_drop_is_not_served() {
        assert_eq!(
            scope(".a{*zoom:1;color:red;color red;_x:1;:b;}").as_deref(),
            Ok(wrapped(".a{color:red;_x:1}").as_str())
        );
    }

    #[test]
    fn a_value_of_white_space_only_and_important_are_kept() {
        assert_eq!(
            scope(".a{--off: ;--on:initial;color:red ! important}").as_deref(),
            Ok(wrapped(".a{--off: ;--on:initial;color:red!important}").as_str())
        );
        // Also last in its block, with a comment in it, and as `--off:;` (no value, no space).
        assert_eq!(
            scope(".a{--off:/* x */ }").as_deref(),
            Ok(wrapped(".a{--off: }").as_str())
        );
        assert_eq!(
            scope(".a{--off:;--on: }").as_deref(),
            Ok(wrapped(".a{--off:;--on: }").as_str())
        );
        // An ordinary property keeps no white space for a value, however it is written.
        assert_eq!(
            scope(".a{color: ;width:1px;height:\n}").as_deref(),
            Ok(wrapped(".a{color:;width:1px;height:}").as_str())
        );
    }

    #[test]
    fn a_selector_is_rewritten_to_the_root_only_when_root_is_its_whole_first_compound() {
        assert_eq!(
            served(":root{--geo--a:1} :root > .b{c:d} :ROOT .e{f:g} @media print{:root .h{i:j}}"),
            squeeze(&wrapped(
                "&{--geo--a:1}& > .b{c:d}& .e{f:g}@media print{& .h{i:j}}"
            ))
        );
        // Not the whole first compound, with a `&` of its own, or inside a rule: kept as written.
        assert_eq!(
            served(":root.dark .a{b:c} :root:not(.x) .y{b:c} :root &{b:c} .z{:root &{b:c}}"),
            squeeze(&wrapped(
                ":root.dark .a{b:c}:root:not(.x) .y{b:c}:root &{b:c}.z{:root &{b:c}}"
            ))
        );
        // The rewritten root is `&`, and a sibling after it is refused as one after `&` is.
        assert_eq!(
            refused(":root + .x{b:c}"),
            selector_refusal(1, ":root + .x")
        );
    }

    #[test]
    fn a_vendor_sheet_as_it_is_found_is_served_as_parsed_and_read_again_the_same() {
        let source = r#"/* required styles */
<!--
.pane,
.tile,
.marker-icon { position: absolute; left: 0; top: 0; }
.container { overflow: hidden; -webkit-tap-highlight-color: transparent; background: #ddd;
  font-family: "Helvetica Neue", Arial, Helvetica, sans-serif; font-size: 12px; font-size: 0.75rem; line-height: 1.5; }
.zoom a { filter: progid:DXImageTransform.Microsoft.gradient(startColorstr='#ffffff'); *zoom: 1; }
.anim .popup { opacity: 0; -webkit-transition: opacity 0.2s linear; -moz-transition: opacity 0.2s linear; transition: opacity 0.2s linear; }
.bar { box-shadow: 0 1px 5px rgba(0,0,0,0.65); border-radius: 4px; }
@media print { .control { -webkit-print-color-adjust: exact; print-color-adjust: exact; } }
.oldie .tip { -ms-filter: "progid:DXImageTransform.Microsoft.Matrix(M11=0.70710678, M12=0.70710678)"; }
.attribution a { background: url(data:image/png;base64,AAAA) no-repeat; }
.layer { max-width: none !important; max-height: none !important; }
:root { --geo--size: 8px; }
.a\26 b { color: red; } /* } */
-->
"#;
        let output = scope(source).expect("a sheet of the shapes vendors write is taken");
        for kept in [
            ".pane, .tile, .marker-icon{position:absolute;left:0;top:0}",
            "filter:progid:DXImageTransform.Microsoft.gradient(startColorstr=\"#ffffff\")",
            "font-family:\"Helvetica Neue\", Arial, Helvetica, sans-serif",
            "font-size:0.75rem",
            "-webkit-transition:opacity 0.2s linear",
            "box-shadow:0 1px 5px rgba(0,0,0,0.65)",
            "@media print{.control{-webkit-print-color-adjust:exact;print-color-adjust:exact}}",
            "-ms-filter:\"progid:DXImageTransform.Microsoft.Matrix(M11=0.70710678, M12=0.70710678)\"",
            "url(data:image/png;base64,AAAA) no-repeat",
            "max-width:none!important;max-height:none!important",
            "&{--geo--size:8px}",
            ".a\\&b{color:red}",
        ] {
            assert!(output.contains(kept), "{kept} is not in {output}");
        }
        for dropped in ["*zoom", "<!--", "-->", "/*"] {
            assert!(!output.contains(dropped), "{dropped} is in {output}");
        }
        // One wrapper, whose closing brace is the last character.
        assert!(
            output.starts_with(":where([data-slot^=\"geo:\"]){"),
            "{output}"
        );
        assert!(output.ends_with('}'), "{output}");
        assert_eq!(output.matches(":where([data-slot").count(), 1, "{output}");
        // And it reads back as what was checked.
        let checked = sheet::scoped(source, "geo", FILE).expect("checked");
        assert_eq!(sheet::rules(&output, FILE).as_ref(), Ok(&checked));
    }

    #[test]
    fn text_that_looks_like_a_closing_brace_cannot_close_the_wrapper() {
        // In an address, a string, a comment, a name's escape, a hash and a dropped declaration.
        let source = ".a{background:url(data:a}b);content:\"}\";x:y}\n.b\\7d {color:red}\n/* } */ .c{color:blue; *d:e}f{g:h} <!-- -->";
        let output = scope(source).expect("taken");
        assert!(
            output.starts_with(":where([data-slot^=\"geo:\"]){"),
            "{output}"
        );
        assert!(output.ends_with('}'), "{output}");
        assert!(output.contains(".b\\}{color:red}"), "{output}");
        // Read as a browser reads it, the served sheet is the one wrapper and nothing after it.
        let again = sheet::rules(&output, FILE).expect("reads");
        assert_eq!(again.len(), 1, "{output}");
        assert_eq!(sheet::scoped(source, "geo", FILE).expect("checked"), again);
    }

    // ------------------------------------------------------------------------- selectors

    #[test]
    fn extension_sheet_refuses_an_ampersand_inside_has_is_where_not_or_twice() {
        for selector in [
            ":has(&)",
            ":is(&)+form",
            ":where(&)",
            ":not(&)",
            "& &",
            "li:nth-child(2 of &)",
            "form:has(+ &)",
            "body:has(&) form",
            ".a[data-x=\"&\"]:is(.b, &)",
        ] {
            assert_eq!(
                refused(&format!("{selector}{{display:none}}")),
                selector_refusal(1, selector),
                "{selector}"
            );
        }
        // The same inside a rule: `&` is the rule's own selector there.
        assert_eq!(refused(".a{& .b &{}}"), selector_refusal(1, "& .b &"));
        assert_eq!(
            refused(".a{.b:has(&){x:y}}"),
            selector_refusal(1, ".b:has(&)")
        );
        // One part of a list is enough, and the line is the part's.
        assert_eq!(refused(".a,\n:has(&){}"), selector_refusal(2, ":has(&)"));

        // Context before `&` is allowed, because the subject is still `&` or inside it.
        for selector in [
            ".dark &",
            "body.ltr & .x",
            ":root:not([data-ui-js]) & [data-slot=\"geo:locate\"]",
            ".a &:hover > .b",
            "&",
            "&.x",
            ".a\\26 b",
        ] {
            assert!(
                scope(&format!("{selector}{{display:none}}")).is_ok(),
                "{selector}"
            );
        }
    }

    #[test]
    fn extension_sheet_refuses_a_sibling_after_the_root_at_any_level() {
        for selector in [
            "& + form",
            "&.x + form",
            "& ~ p",
            "+ form",
            "~ p",
            "&+form",
            "&:not(.y) ~ p",
        ] {
            assert_eq!(
                refused(&format!("{selector}{{}}")),
                selector_refusal(1, selector),
                "{selector}"
            );
        }
        assert_eq!(refused(".a{ + .b{} }"), selector_refusal(1, "+ .b"));
        assert_eq!(refused(".a{&.x ~ .y{}}"), selector_refusal(1, "&.x ~ .y"));
        assert_eq!(
            refused("@media print{.a{ ~ .b{} }}"),
            selector_refusal(1, "~ .b")
        );
        for selector in ["& > .x", ".x + .y", "&>.x", ".x ~ & .y"] {
            assert!(scope(&format!("{selector}{{}}")).is_ok(), "{selector}");
        }
        assert!(scope(".a{ > .b{} }").is_ok());
        assert!(scope(".a{ .b + .c{} }").is_ok());
    }

    #[test]
    fn a_selector_made_of_anything_else_is_refused() {
        for selector in [
            "",
            ".a,",
            ",.a",
            ".a;.b",
            ".a|b",
            "*|*",
            ".a \"x\"",
            "#1a",
            "(.a)",
            ".a 1",
            ".a % .b",
            "a <!-- b",
            ".a!",
            "--x:hover",
            "--x :hover",
            ".a:is({})",
            ".a:is(b;c, &)",
        ] {
            let error = refused(&format!("{selector}{{color:red}}"));
            assert!(
                error.starts_with(&format!("extension_sheet!: {FILE}:1: ")),
                "{selector}: {error}"
            );
            assert!(
                error.contains(OUTSIDE) || error.ends_with(PLAIN),
                "{selector}: {error}"
            );
        }
        // An attribute selector holds any text, and a function holds a selector list.
        assert!(scope("a[href^=\"https://x.example\" i], :is(.a, .b) > :not(.c){}").is_ok());
        assert!(scope("[data-a|=b] [data-c~=d] [e*=f] [g$=h]{}").is_ok());
    }

    // ------------------------------------------------------------------------- at-rules

    #[test]
    fn extension_sheet_refuses_at_rules_outside_the_list() {
        for (source, name) in [
            ("@import \"x.css\";", "import"),
            ("@charset \"utf-8\";", "charset"),
            ("@namespace svg url(x);", "namespace"),
            ("@page{}", "page"),
            ("@scope (.a){}", "scope"),
            ("@position-try --x{}", "position-try"),
            ("@view-transition{navigation:auto}", "view-transition"),
            ("@font-feature-values F{}", "font-feature-values"),
            ("@font-palette-values --x{}", "font-palette-values"),
            ("@document url-prefix(){}", "document"),
            ("@-webkit-keyframes geo--x{}", "-webkit-keyframes"),
            ("@IMPORT 'x';", "IMPORT"),
            ("@media print{@keyframes geo--x{}}", "keyframes"),
            ("@media print{@font-face{}}", "font-face"),
            ("@media print{@property --geo--x{}}", "property"),
            ("@supports (a:b){@counter-style geo--x{}}", "counter-style"),
            ("@layer geo--a{@keyframes geo--x{}}", "keyframes"),
            (".a{@keyframes geo--x{}}", "keyframes"),
            (".a{@import 'x';}", "import"),
            (".a{@media print{@layer geo--a{@import 'x';}}}", "import"),
            ("@keyframes geo--x{@media print{}}", "media"),
            ("@font-face{@media print{}}", "media"),
        ] {
            assert_eq!(
                refused(source),
                at(1, &format!("@{name} {NOT_TAKEN}")),
                "{source}"
            );
        }
        // The line is the at-rule's own.
        assert_eq!(
            refused("\n.a{}\n\n@import 'x';"),
            at(4, &format!("@import {NOT_TAKEN}"))
        );

        for source in [
            "@supports (display:grid){.a{}}",
            "@container (width>1px){.a{}}",
            "@starting-style{.a{opacity:0}}",
            ".a{@supports (display:grid){b:c}}",
            ".a{@starting-style{opacity:0}}",
        ] {
            assert!(scope(source).is_ok(), "{source}");
        }
        // They stay inside the wrapper, where the rules of a group are nested rules.
        assert_eq!(
            served("@supports (display:grid){.a{b:c}} @container (width>1px){.d{e:f}}"),
            squeeze(&wrapped(
                "@supports (display:grid){.a{b:c}}@container (width>1px){.d{e:f}}"
            ))
        );
        assert_eq!(
            served(".a{@starting-style{opacity:0}}"),
            squeeze(&wrapped(".a{@starting-style{opacity:0}}"))
        );
    }

    #[test]
    fn an_at_rule_without_the_block_it_needs_is_refused_in_plain_words() {
        for source in [
            "@media print;",
            "@keyframes geo--x;",
            "@font-face;",
            "@property --geo--x;",
            "@counter-style geo--x;",
            "@layer geo--a{@layer geo--b;}",
            ".a{@layer geo--b;}",
            "@font-face x{}",
            "@keyframes geo--x geo--y{}",
            "@property geo--x y{}",
            "@counter-style{}",
            "@layer;",
            "@layer geo--a,,geo--b;",
            "@layer geo--a, geo--b{}",
            "@layer \"geo--a\";",
            "@layer geo--a geo--b;",
        ] {
            assert_eq!(refused(source), at(1, PLAIN), "{source}");
        }
    }

    #[test]
    fn extension_sheet_keeps_layer_blocks_nested_and_hoists_layer_statements() {
        assert_eq!(
            served("@layer geo--base, geo--theme; @layer geo--base { .pin{color:red} }"),
            squeeze(&format!(
                "@layer geo--base,geo--theme;{}",
                wrapped("@layer geo--base{.pin{color:red}}")
            ))
        );
        // A sub-layer is a dot and a part; the first part carries the namespace.
        assert!(scope("@layer geo--a.b.c{} @layer geo--x.y, geo--z;").is_ok());
        // An anonymous block has no name to take.
        assert!(scope("@layer{.a{b:c}}").is_ok());
        for (source, name) in [
            ("@layer base{}", "base"),
            ("@layer base;", "base"),
            ("@layer geo--a, base;", "base"),
            ("@layer geo,geo--b;", "geo"),
            ("@layer base.geo--x{}", "base"),
        ] {
            assert_eq!(
                refused(source),
                global_refusal(1, name, "geo--"),
                "{source}"
            );
        }
        // A name that is only the namespace and two dashes names nothing.
        assert_eq!(
            refused("@layer geo--;"),
            global_refusal(1, "geo--", "geo--")
        );
    }

    #[test]
    fn at_rules_that_name_something_hold_declarations_frames_and_names_of_their_own() {
        assert_eq!(
            served("@keyframes \"geo--x\"{from{a:b}50%{a:c}10%,20%{a:d}to{a:e}}"),
            squeeze(&format!(
                "@keyframes \"geo--x\"{{from{{a:b}}50%{{a:c}}10%,20%{{a:d}}to{{a:e}}}}{}",
                wrapped("")
            ))
        );
        for source in [
            "@keyframes geo--x{.a{b:c}}",
            "@keyframes geo--x{from,{b:c}}",
            "@keyframes geo--x{from to{b:c}}",
            "@keyframes geo--x{entry 0%{b:c}}",
            "@keyframes geo--x{from{.a{b:c}}}",
            "@keyframes geo--x{from{@media print{b:c}}}",
            "@keyframes geo--x{b:c}",
            "@keyframes geo--x{from}",
            "@font-face{.a{b:c}}",
            "@property --geo--x{.a{b:c}}",
            "@counter-style geo--x{@media print{b:c}}",
            "@keyframes 1{}",
            "@keyframes geo--x,geo--y{}",
        ] {
            let error = refused(source);
            assert!(
                error.starts_with(&at(1, "")) && error.ends_with(PLAIN)
                    || error.contains("is not taken"),
                "{source}: {error}"
            );
        }
        // What a frame sets is checked as everything else is.
        assert_eq!(
            refused("@keyframes geo--x{to{anchor-name:--pin}}"),
            global_refusal(1, "--pin", "--geo--")
        );
        assert_eq!(
            refused("@keyframes geo--x{to{background:url(x.png)}}"),
            at(1, URL_TEXT)
        );
    }

    // ------------------------------------------------------------------------- global names

    #[test]
    fn extension_sheet_refuses_global_names_and_urls_outside_the_rules() {
        assert_eq!(
            refused("@keyframes pulse{}"),
            global_refusal(1, "pulse", "geo--")
        );
        assert_eq!(
            refused("@property --size{syntax:\"*\";inherits:false}"),
            global_refusal(1, "--size", "--geo--")
        );
        assert_eq!(
            refused("@font-face{font-family:Inter;src:url(data:font/woff2;base64,AA)}"),
            global_refusal(1, "Inter", "geo--")
        );
        assert_eq!(
            refused("@counter-style stars{system:cyclic;symbols:\"*\"}"),
            global_refusal(1, "stars", "geo--")
        );
        assert_eq!(
            refused(".a{anchor-name:--pin}"),
            global_refusal(1, "--pin", "--geo--")
        );
        assert_eq!(
            refused(".a{view-transition-name:hero}"),
            global_refusal(1, "hero", "--geo--")
        );
        assert!(scope(".a{position-anchor:--geo--pin}").is_ok());
        assert!(scope(".a{view-transition-name:none}").is_ok());

        for source in [
            ".a{background:url(images/layers.png)}",
            ".a{background:url(\"https://tiles.example/a.png\")}",
            ".a{background:image-set(\"x.png\" 1x)}",
            ".a{background:src(\"x.png\")}",
        ] {
            assert_eq!(refused(source), at(1, URL_TEXT), "{source}");
        }
        assert!(scope(".a{background:url(data:image/png;base64,AA)}").is_ok());
    }

    #[test]
    fn the_properties_that_name_things_across_the_page_take_names_under_the_namespace() {
        for source in [
            ".a{anchor-name:--geo--pin}",
            ".a{anchor-name:--geo--pin, --geo--other}",
            ".a{anchor-name:none}",
            ".a{anchor-name:REVERT-LAYER}",
            ".a{position-anchor:auto}",
            ".a{position-anchor:match-element}",
            ".a{view-transition-name:inherit}",
            ".a{view-transition-name:--geo--hero}",
            ".a{scroll-timeline-name:--geo--t}",
            ".a{view-timeline-name:--geo--v, --geo--w}",
            ".a{timeline-scope:all}",
            ".a{anchor-scope:all}",
            ".a{timeline-scope:--geo--a, none}",
            ".a{anchor-scope:--geo--a}",
            ".a{view-timeline:--geo--v block}",
            ".a{view-timeline:--geo--v inline 10px 20px, none}",
            ".a{scroll-timeline:--geo--t x}",
            ".a{scroll-timeline:none}",
            ".a{anchor-name:}",
        ] {
            assert!(scope(source).is_ok(), "{source}");
        }
        for (source, name, form) in [
            (".a{scroll-timeline-name:--t}", "--t", "--geo--"),
            (".a{view-timeline-name:--geo--v, --w}", "--w", "--geo--"),
            (".a{timeline-scope:--t}", "--t", "--geo--"),
            (".a{anchor-scope:--t}", "--t", "--geo--"),
            (".a{anchor-name:all}", "all", "--geo--"),
            (".a{view-transition-name:all}", "all", "--geo--"),
            (".a{anchor-name:var(--n)}", "var(--n)", "--geo--"),
            (".a{anchor-name:\"--geo--x\"}", "\"--geo--x\"", "--geo--"),
            (
                ".a{anchor-name:--geo--a --geo--b}",
                "--geo--a --geo--b",
                "--geo--",
            ),
            (".a{anchor-name:--GEO--x}", "--GEO--x", "--geo--"),
            (".a{anchor-name:--geo--}", "--geo--", "--geo--"),
            (".a{anchor-name:--other--x}", "--other--x", "--geo--"),
            (".a{anchor-name:geo--x}", "geo--x", "--geo--"),
            (".A{Anchor-Name:--pin}", "--pin", "--geo--"),
            (".a{anchor\\2d name:--pin}", "--pin", "--geo--"),
            (".a{view-timeline:--v block}", "--v block", "--geo--"),
            (".a{scroll-timeline:x}", "x", "--geo--"),
            (".a{view-timeline:--geo--v, --w x}", "--w x", "--geo--"),
        ] {
            assert_eq!(refused(source), global_refusal(1, name, form), "{source}");
        }
        // They are checked in a nested rule, a group and a custom property's neighbours alike.
        assert_eq!(
            refused(".a{.b{@media print{anchor-name:--pin}}}"),
            global_refusal(1, "--pin", "--geo--")
        );
        assert_eq!(
            refused("\n\n@media print{.a{view-transition-name:hero}}"),
            global_refusal(3, "hero", "--geo--")
        );
    }

    #[test]
    fn a_font_family_is_a_string_or_names_under_the_namespace() {
        for source in [
            "@font-face{font-family:geo--Open Sans}",
            "@font-face{font-family:\"geo--x y\"}",
            "@font-face{font-family:geo--icons;src:url(data:font/woff2;base64,AA)}",
            "@font-face{src:url(data:font/woff2;base64,AA)}",
        ] {
            assert!(scope(source).is_ok(), "{source}");
        }
        for (source, name) in [
            ("@font-face{font-family:Open Sans}", "Open Sans"),
            ("@font-face{font-family:\"Inter\"}", "Inter"),
            ("@font-face{font-family:var(--f)}", "var(--f)"),
            (
                "@font-face{font-family:\"geo--a\", \"geo--b\"}",
                "\"geo--a\", \"geo--b\"",
            ),
            ("@font-face{font-family:geo--}", "geo--"),
            ("@font-face{font-family:Geo--x}", "Geo--x"),
            ("@font-face{FONT-FAMILY:x}", "x"),
        ] {
            assert_eq!(
                refused(source),
                global_refusal(1, name, "geo--"),
                "{source}"
            );
        }
    }

    // ------------------------------------------------------------------------- addresses

    #[test]
    fn every_address_is_a_data_url_and_nothing_else_asks_for_one() {
        for source in [
            ".a{background:url(data:image/png;base64,AA)}",
            ".a{background:url( \"data:image/png;base64,AA\" )}",
            ".a{background:URL(data:image/png;base64,AA)}",
            ".a{background:url(DATA:image/png;base64,AA)}",
            ".a{background:url(\\64 ata:image/png;base64,AA)}",
            ".a{background:image-set(url(data:image/png;base64,AA) 1x, \"data:image/png;base64,BB\" 2x)}",
            ".a{background:-webkit-image-set(\"data:image/png;base64,AA\" 1x)}",
            ".a{background:image(\"data:image/png;base64,AA\")}",
            ".a{background:src(\"data:image/png;base64,AA\")}",
            ".a{content:\"https://a.example/x\";content:attr(data-label);content:attr(data-x, \"y\")}",
            ".a{font-family:\"Helvetica Neue\";quotes:\"a\" \"b\"}",
            "[href^=\"https://a.example\"]{x:y}",
            ".a{--u:url(data:image/png;base64,AA)}",
        ] {
            assert!(scope(source).is_ok(), "{source}");
        }
        for source in [
            ".a{background:url(a.png)}",
            ".a{background:url(//a.example/x.png)}",
            ".a{background:url(\"x.png\")}",
            ".a{background:url( x.png )}",
            ".a{background:url(\" data:image/png;base64,AA\")}",
            ".a{background:url(\"\")}",
            ".a{background:url()}",
            ".a{background:url(a b)}",
            ".a{background:url(a\"b)}",
            ".a{background:url(var(--u))}",
            ".a{background:url(\"data:x\" , \"y\") , url(z.png)}",
            ".a{background:image-set(\"x.png\" 1x)}",
            ".a{background:image-set(url(x.png) 1x)}",
            ".a{background:-webkit-image-set(\"x.png\" 1x)}",
            ".a{background:image-set(\"data:x\" 1x type(\"image/png\"))}",
            ".a{background:image-set(var(--u) 1x)}",
            ".a{background:image(\"x.png\")}",
            ".a{background:src(\"x.png\")}",
            ".a{background:src(var(--u))}",
            ".a{background:cross-fade(url(x.png), red)}",
            ".a{--u:url(https://a.example/x.png)}",
            ".a{content:attr(data-src url)}",
            ".a{background:attr(data-src type(<url>))}",
            "@supports (background:url(x.png)){.a{}}",
            "@media (min-width:1px) and (x:url(y)){.a{}}",
            "@font-face{src:url(https://a.example/f.woff2)}",
            "@font-face{src:local(Arial),url(f.woff2) format(\"woff2\")}",
            ".a{b:c(d(e(url(x.png))))}",
        ] {
            assert_eq!(refused(source), at(1, URL_TEXT), "{source}");
        }
    }

    // ------------------------------------------------------------------------- tokens and blocks

    #[test]
    fn blocks_that_do_not_balance_are_refused_without_a_line() {
        let text = format!("extension_sheet!: {FILE}: the blocks do not balance");
        for source in [
            ".a{",
            ".a{color:red",
            ".a{} .b{",
            ".a{}}",
            "}",
            ")",
            "]",
            ".a{x:(}",
            ".a{x:y)}",
            ".a{x:[}",
            ".a{x:y]}",
            ".a{x:f(}",
            ".a{x:f(a}",
            ".a:is(b{}",
            "@media print{.a{}",
            ".a{ /* }",
            ".a{x:\"y",
            ".a{x:'y",
            ".a{x:url(y",
        ] {
            assert_eq!(refused(source), text, "{source}");
        }
        assert!(scope(".a{x:f((a)[b]{c})}").is_ok());
    }

    #[test]
    fn a_token_that_would_not_be_read_the_same_is_refused_with_its_line() {
        // A block nested too deep for the tokenizer is not read at all.
        let deep = format!(".a{{x:{}{}}}", "(".repeat(100), ")".repeat(100));
        assert_eq!(refused(&deep), at(1, PLAIN));
        for source in [
            // A string that breaks across lines, a backslash that escapes nothing, a rule that
            // never opens a block, a declaration outside a rule, and the tokens that only the top
            // of a sheet may hold between its rules.
            ".a{content:\"x\n}",
            ".a{x:\\\ny}",
            ".a",
            ".a ;",
            "color:red;",
            ".a{} color:red;",
            ".a{x:<!--}",
            ".a{x:y -->}",
            "@media print{<!-- .a{} -->}",
            ".a <!-- .b{}",
        ] {
            assert_eq!(refused(source), at(1, PLAIN), "{source}");
        }
        assert_eq!(refused("\n\n.a{content:\"x\n}"), at(3, PLAIN));
    }

    #[test]
    fn a_namespace_outside_the_rules_is_refused_before_the_sheet_is_read() {
        let text = |ns: &str| {
            format!(
                "extension_sheet!: `{ns}` is not a namespace (a lower-case letter followed by up to 63 lower-case letters, digits or _)"
            )
        };
        let too_long = "a".repeat(65);
        for ns in [
            "",
            "Geo",
            "1geo",
            "geo-map",
            "geo.map",
            "geo map",
            "_geo",
            too_long.as_str(),
        ] {
            assert_eq!(sheet::scope(".a{}", ns, FILE), Err(text(ns)), "{ns:?}");
            assert_eq!(sheet::namespace_error(ns), Some(text(ns)), "{ns:?}");
        }
        let longest = "n".repeat(64);
        for ns in ["geo", "a", "khata_2", longest.as_str()] {
            assert!(sheet::namespace_error(ns).is_none(), "{ns}");
            assert!(sheet::scope(".a{}", ns, FILE).is_ok(), "{ns}");
        }
        // The namespace is the one the names and the wrapper carry.
        assert_eq!(
            sheet::scope(".a{b:c}@keyframes khata--x{}", "khata", FILE).as_deref(),
            Ok("@keyframes khata--x{}:where([data-slot^=\"khata:\"]){.a{b:c}}")
        );
        assert_eq!(
            sheet::scope(".a{anchor-name:--geo--x}", "khata", FILE),
            Err(global_refusal(1, "--geo--x", "--khata--"))
        );
    }

    // ------------------------------------------------------------------------- the second parse

    #[test]
    fn extension_sheet_serves_what_it_parsed() {
        let source = r#"/* } */
<!--
.a\26 b{}
.a{filter:progid:DXImageTransform.Microsoft.gradient(startColorstr='#ffffff');*zoom:1;-ms-filter:"x";color:red!important}
@media print{.a{-webkit-print-color-adjust:exact}}
-->"#;
        let output = scope(source).expect("taken");
        // Exactly one wrapper, whose closing brace is the last character.
        assert!(
            output.starts_with(":where([data-slot^=\"geo:\"]){"),
            "{output}"
        );
        assert!(output.ends_with('}'), "{output}");
        assert_eq!(output.matches(":where([data-slot").count(), 1, "{output}");
        assert!(!output.contains("*zoom") && !output.contains("<!--") && !output.contains("-->"));
        assert!(output.contains("color:red!important"), "{output}");
        // What was served reads back as what was checked.
        let checked = sheet::scoped(source, "geo", FILE).expect("checked");
        assert_eq!(sheet::rules(&output, FILE).as_ref(), Ok(&checked));

        assert_eq!(
            refused(".a{"),
            format!("extension_sheet!: {FILE}: the blocks do not balance")
        );
        assert_eq!(
            sheet::scope(".a{}", "Geo", FILE),
            Err("extension_sheet!: `Geo` is not a namespace (a lower-case letter followed by up to 63 lower-case letters, digits or _)".to_owned())
        );
    }

    #[test]
    fn two_sheets_are_the_same_when_they_hold_the_same_things_and_not_otherwise() {
        let reads = |css: &str| sheet::rules(css, FILE).expect(css);
        let reference = reads(".a b{c:d}");
        assert_eq!(reference, reads(".a   b { c : d } /* x */"));
        assert_eq!(reference, reads("\n.a\tb{c:d;}"));
        for other in [
            ".ab{c:d}",
            ".a>b{c:d}",
            ".a b{c:e}",
            ".a b{c:d !important}",
            ".a b{c:d;e:f}",
            ".a b{cc:d}",
            ".a .b{c:d}",
            ".a b{c:d}.x{}",
        ] {
            assert_ne!(reference, reads(other), "{other}");
        }
        assert_ne!(reads("a{b:c d}"), reads("a{b:cd}"));
        assert_ne!(reads("a{b:c(d)}"), reads("a{b:c (d)}"));
        assert_ne!(reads("a{b:1.0}"), reads("a{b:1}"));
        assert_ne!(reads("a{b:'c'}"), reads("a{b:c}"));
        assert_ne!(
            reads("@media print{.a{b:c}}"),
            reads("@media print{.a{b:c}.d{b:c}}")
        );
        assert_ne!(reads("@media a{}"), reads("@media b{}"));
        assert_ne!(reads("@layer a;"), reads("@layer a{}"));
        // Text that is not a sheet is not read at all.
        assert!(sheet::rules(".a{", FILE).is_err());
    }

    #[test]
    fn a_writer_that_loses_a_space_is_caught_by_the_second_parse() {
        // The two names are one name once the space between them is gone.
        let lossy = |rules: &[sheet::Rule]| sheet::write_rules(rules).replace(' ', "");
        assert_eq!(
            sheet::scope_written(".a .b{c:d}", "geo", FILE, lossy),
            Err(format!("extension_sheet!: {FILE}: {PLAIN}"))
        );
        assert!(sheet::scope_written(".a .b{c:d}", "geo", FILE, sheet::write_rules).is_ok());
    }

    #[test]
    fn a_sheet_with_blocks_sixty_deep_is_taken() {
        let deep = format!(".a{{x:{}{}}}", "(".repeat(60), ")".repeat(60));
        assert!(scope(&deep).is_ok());
    }
}
