//! Every icon the toolkit ships, read from `crates/navree-ui/src/icons.rs`.
//!
//! An `Icon`'s shapes are private to the toolkit, so the gallery reads the
//! constants from the generated file and draws each one with the markup of
//! `components::icon`; a test holds the two equal.

use navree::ui::{Node, el};
use syn::{Expr, Item, Lit};

use crate::catalog::{Example, Kind};

/// `(constant name, SVG shapes)` for every icon, in the file's order.
pub fn icons() -> Vec<(String, String)> {
    let text = crate::read_repo("crates/navree-ui/src/icons.rs");
    let file = syn::parse_file(&text).unwrap_or_else(|e| panic!("icons.rs: {e}"));
    file.items
        .into_iter()
        .filter_map(|item| {
            let Item::Const(item) = item else {
                return None;
            };
            let Expr::Call(call) = *item.expr else {
                return None;
            };
            let Some(Expr::Lit(lit)) = call.args.first() else {
                return None;
            };
            let Lit::Str(shapes) = &lit.lit else {
                return None;
            };
            Some((item.ident.to_string(), shapes.value()))
        })
        .collect()
}

/// An icon drawn as `components::icon` draws it.
pub fn icon_node(shapes: &str) -> Node {
    let shapes: &'static str = Box::leak(shapes.to_owned().into_boxed_str());
    el("svg")
        .slot("icon")
        .attr("viewBox", "0 0 24 24")
        .attr("aria-hidden", "true")
        .child(Node::Static(shapes))
        .into()
}

fn gallery() -> Vec<(&'static str, Node)> {
    icons()
        .into_iter()
        .map(|(name, shapes)| {
            let name: &'static str = Box::leak(name.into_boxed_str());
            (name, icon_node(&shapes))
        })
        .collect()
}

/// The icons page's gallery.
pub fn all_icons() -> Example {
    let mut example = Example::new(
        "icons",
        "all",
        "Every icon",
        Kind::Gallery(gallery),
        "use navree::ui::icons;\n\n// Each icon is a constant: pass it to a component.\nButton::primary(\"New deal\").icon(icons::PLUS)",
    );
    example.cells = crate::catalog::Cells::Icon;
    example.height = 600;
    example.layout = crate::catalog::Layout::Start;
    example
}

#[cfg(test)]
mod tests {
    use navree::ui::{components, icons, render};

    #[test]
    fn the_gallery_draws_an_icon_as_the_toolkit_does() {
        let shapes = super::icons()
            .into_iter()
            .find(|(name, _)| name == "PLUS")
            .map(|(_, shapes)| shapes)
            .expect("PLUS is an icon");
        assert_eq!(
            render(&super::icon_node(&shapes)),
            render(&components::icon(icons::PLUS))
        );
    }

    #[test]
    fn every_icon_is_read() {
        let count = crate::read_repo("crates/navree-ui/src/icons.rs")
            .matches("pub const ")
            .count();
        assert_eq!(super::icons().len(), count);
    }
}
