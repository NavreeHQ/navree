//! What the website reads: a preview document per example, and three JSON
//! files (the catalog, the props tables, the toolkit's files and tokens).

use std::collections::BTreeMap;

use navree::ui::assets::{self, Asset};
use navree::ui::theme::Theme;
use navree::ui::{
    Input, LazyPage, Node, PAGE_CSP, Uses, View, components, el, icons, measure, render, tokens,
};
use serde::Serialize;

use crate::api::TypeDoc;
use crate::catalog::{COMPONENTS, Cells, Component, Entry, Example, GROUPS, Kind, Layout};

/// Where the preview documents are served from, under the site's root.
pub const PREVIEW_ROOT: &str = "/ui-preview";

/// A part of a component: its `data-slot`, its element and what it holds.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Slot {
    pub slot: String,
    pub tag: &'static str,
    /// How many siblings in a row have this slot (three radios, say).
    pub count: usize,
    pub children: Vec<Slot>,
}

/// The parts of `node`, nested as they are in the tree. Elements without a
/// `data-slot` are passed through.
pub fn slots(node: &Node) -> Vec<Slot> {
    let mut out = Vec::new();
    collect(node, &mut out);
    out
}

fn collect(node: &Node, out: &mut Vec<Slot>) {
    match node {
        Node::Element(element) => {
            let slot = element
                .get("data-slot")
                .and_then(|value| value.as_text().map(|text| text.into_owned()));
            match slot {
                Some(slot) => {
                    let mut children = Vec::new();
                    for child in element.child_nodes() {
                        collect(child, &mut children);
                    }
                    let part = Slot {
                        slot,
                        tag: element.tag(),
                        count: 1,
                        children,
                    };
                    // Siblings of one kind in a row (menu items, radios) are one
                    // part, drawn once with the richest of their insides.
                    match out.last_mut() {
                        Some(last) if last.slot == part.slot => {
                            last.count += 1;
                            if size(&part.children) > size(&last.children) {
                                last.children = part.children;
                            }
                        }
                        _ => out.push(part),
                    }
                }
                None => {
                    for child in element.child_nodes() {
                        collect(child, out);
                    }
                }
            }
        }
        Node::Fragment(nodes) => {
            for child in nodes {
                collect(child, out);
            }
        }
        _ => {}
    }
}

/// How many parts a list of parts holds, all the way down.
fn size(slots: &[Slot]) -> usize {
    slots.iter().map(|slot| 1 + size(&slot.children)).sum()
}

/// The file a toolkit asset is built from, in the repository.
fn asset_source(asset: &Asset) -> String {
    match (asset.name, asset.ext) {
        ("geist", _) => "crates/navree-ui/assets/fonts/geist.woff2".to_owned(),
        ("favicon", _) => "crates/navree-ui/assets/favicon.svg".to_owned(),
        (name, ext) => format!("crates/navree-ui/dist/{name}.{ext}"),
    }
}

/// `core-d67293dd5eb1.js`.
fn asset_file(asset: &Asset) -> String {
    format!("{}-{}.{}", asset.name, asset.hash, asset.ext)
}

/// The served name of the asset behind a toolkit URL.
fn file_of(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or(url)
}

/// The page policy without `frame-ancestors`, which a `<meta>` can't carry: the
/// site frames these documents itself.
fn preview_csp() -> String {
    PAGE_CSP
        .split(';')
        .map(str::trim)
        .filter(|part| !part.starts_with("frame-ancestors"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// What an example draws, ready to render, and the captions of a gallery. A
/// page's body comes from `page`, the page the example drew.
fn stage(example: &Example, page: Option<&View>) -> (Node, Vec<String>) {
    let single = |node: Node| -> Node {
        el("div")
            .flag("data-preview-stage", true)
            .attr_opt("data-live", example.streaming)
            .child(node)
            .into()
    };
    match &example.kind {
        Kind::Single(draw) => (single(draw()), Vec::new()),
        Kind::Page(_) => (
            single(page.map_or(Node::Empty, |view| view.body_node().clone())),
            Vec::new(),
        ),
        Kind::Search { label, noun, draw } => {
            (search_stage(label, noun, draw(), example.cells), Vec::new())
        }
        Kind::Gallery(draw) => {
            let cells = draw();
            let captions: Vec<String> = cells
                .iter()
                .map(|(caption, _)| (*caption).to_owned())
                .collect();
            let captions_len = captions.len();
            let figures: Vec<Node> = cells
                .into_iter()
                .map(|(caption, node)| {
                    el("figure")
                        .flag("data-preview-cell", true)
                        .children((
                            el("div").flag("data-preview-cell-body", true).child(node),
                            el("figcaption").child(caption),
                        ))
                        .into()
                })
                .collect();
            (
                el("div")
                    .flag("data-preview-stage", true)
                    .attr_opt("data-live", example.streaming)
                    .child(
                        el("div")
                            .attr("data-preview-gallery", example.cells.as_str())
                            .attr("data-preview-cols", columns(example.cells, captions_len))
                            .child(figures),
                    )
                    .into(),
                captions,
            )
        }
    }
}

/// A search box over cells, and the cells: the preview script filters them as
/// someone types. The cells are plain markup, so the page works unfiltered
/// without the script.
fn search_stage(label: &str, noun: &str, entries: Vec<Entry>, cells: Cells) -> Node {
    let total = entries.len();
    let figures: Vec<Node> = entries
        .into_iter()
        .map(|entry| {
            el("figure")
                .flag("data-preview-cell", true)
                .attr("data-terms", entry.terms)
                .children((
                    el("div")
                        .flag("data-preview-cell-body", true)
                        .child(entry.node),
                    el("figcaption").children((
                        el("span").child(entry.caption),
                        entry.detail.map(|detail| el("small").child(detail)),
                    )),
                ))
                .into()
        })
        .collect();
    el("div")
        .flag("data-preview-stage", true)
        .children((
            el("form")
                .flag("data-preview-search", true)
                .attr("role", "search")
                .attr("data-noun", noun.to_owned())
                .attr("data-total", i64::try_from(total).unwrap_or(0))
                .children((
                    el("div").flag("data-preview-search-box", true).children((
                        components::icon(icons::SEARCH),
                        Input::search("q")
                            .placeholder(label.to_owned())
                            .attr("aria-label", label.to_owned())
                            .attr("autocomplete", "off"),
                    )),
                    el("p")
                        .flag("data-preview-count", true)
                        .attr("role", "status")
                        .child(format!("{total} {noun}")),
                )),
            el("div")
                .attr("data-preview-gallery", cells.as_str())
                .attr("data-preview-cols", 0_i64)
                .child(figures),
            el("p")
                .flag("data-preview-empty", true)
                .flag("hidden", true)
                .child(format!("No {noun} match. Try a shorter word.")),
        ))
        .into()
}

/// How many columns a gallery of `n` cells takes on a wide screen: full rows
/// where it can, so no cell is left alone on the last one.
fn columns(cells: Cells, n: usize) -> i64 {
    let n = i64::try_from(n).unwrap_or(1).max(1);
    match cells {
        Cells::Wide => 1,
        Cells::Icon | Cells::Flag => 0,
        Cells::Narrow if n <= 5 => n,
        Cells::Medium if n <= 4 => n,
        Cells::Narrow | Cells::Medium if n <= 6 => 3,
        Cells::Narrow | Cells::Medium => 4,
    }
}

/// The module scripts a preview loads: what the page would load, less the
/// core runtime unless the example runs commands, and less the live module,
/// since no preview follows a stream. The core boosts links and forms, which
/// go nowhere in a preview.
fn scripts(example: &Example, uses: Uses) -> Vec<String> {
    assets::scripts(uses)
        .filter(|url| example.live || *url != assets::CORE_URL)
        .filter(|url| *url != assets::LIVE_URL)
        .map(|url| file_of(url).to_owned())
        .collect()
}

/// The module stylesheets a preview links after the toolkit's own, as a page
/// with the same parts does: the theme builder's, say.
fn sheets(uses: Uses) -> Vec<String> {
    assets::stylesheets(uses)
        .map(|url| file_of(url).to_owned())
        .collect()
}

/// Where a preview finds the stylesheet of a theme of your own, under the
/// previews: what an app serves at the theme's `stylesheet()`, named by the
/// theme's key. The address an app uses names the theme by all its values, too
/// long for a file's name.
fn theme_sheet_path(theme: &Theme) -> String {
    format!("_toolkit/theme/{}.css", theme.key())
}

/// The pages a preview's lists are answered from, as templates the preview
/// script reads. Inert markup: nothing in a template runs or loads.
fn answers(example: &Example) -> String {
    let mut out = String::new();
    for answer in (example.answers)() {
        let html = LazyPage::new(answer.items, answer.more).to_html();
        out.push_str(&format!(
            "<template data-preview-answer data-src=\"{}\" data-dir=\"{}\" data-more=\"{}\">{html}</template>",
            escape(answer.src),
            escape(answer.dir),
            if answer.more { "1" } else { "0" },
        ));
    }
    out
}

/// What a preview needs besides its body: the page it draws, if it is one, and
/// the module scripts and stylesheets its parts use.
struct Parts<'a> {
    page: Option<&'a View>,
    scripts: &'a [String],
    sheets: &'a [String],
}

/// The preview document of one example.
fn document(example: &Example, stage: &Node, parts: &Parts<'_>, component: &Component) -> String {
    let mut head = String::new();
    head.push_str("<!doctype html>\n<html lang=\"en\"");
    // The page's look, as the page writes it: a theme's key, a mode's id and a
    // density's id, never text from outside.
    for (name, value) in parts.page.map(View::look).unwrap_or_default() {
        head.push_str(&format!(" {name}=\"{}\"", escape(&value)));
    }
    head.push_str(">\n<head>\n<meta charset=\"utf-8\">\n");
    head.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n");
    head.push_str(&format!(
        "<meta name=\"color-scheme\" content=\"{}\">\n",
        parts.page.map_or("dark", View::color_scheme)
    ));
    head.push_str(&format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{}\">\n",
        preview_csp()
    ));
    head.push_str("<meta name=\"robots\" content=\"noindex\">\n");
    head.push_str(&format!(
        "<title>{}: {}</title>\n",
        escape(component.name),
        escape(example.title)
    ));
    head.push_str(&format!(
        "<link rel=\"preload\" as=\"font\" type=\"font/woff2\" crossorigin href=\"../_toolkit/{}\">\n",
        file_of(assets::GEIST_URL)
    ));
    head.push_str(&format!(
        "<link rel=\"stylesheet\" href=\"../_toolkit/{}\">\n",
        file_of(assets::NAVREE_UI_URL)
    ));
    for sheet in parts.sheets {
        head.push_str(&format!(
            "<link rel=\"stylesheet\" href=\"../_toolkit/{sheet}\">\n"
        ));
    }
    // A theme of your own: its sheet after the toolkit's, so its rules come later.
    for theme in parts.page.map(View::own_themes).unwrap_or_default() {
        head.push_str(&format!(
            "<link rel=\"stylesheet\" href=\"../{}\" data-ui-theme>\n",
            theme_sheet_path(theme)
        ));
    }
    head.push_str("<link rel=\"stylesheet\" href=\"../_frame/preview.css\">\n");
    head.push_str("<script type=\"module\" src=\"../_frame/preview.js\"></script>\n");
    for script in parts.scripts {
        head.push_str(&format!(
            "<script type=\"module\" src=\"../_toolkit/{script}\"></script>\n"
        ));
    }
    head.push_str("</head>\n");
    let show = match (example.show, example.open) {
        (true, true) => " data-preview-show data-preview-open",
        (true, false) => " data-preview-show",
        (false, true) => " data-preview-open",
        (false, false) => "",
    };
    // A static page has no server to stream from: a region that follows a stream would open
    // one, to an address the site does not have, and keep trying.
    let body = without_streams(&render(stage));
    let body = if example.sidebar_open {
        with_sidebar_open(&body)
    } else {
        body
    };
    let answers = answers(example);
    format!(
        "{head}<body data-preview=\"{}\"{show}>{body}{answers}</body>\n</html>\n",
        example.layout.as_str(),
    )
}

/// `html` with its app shell's sidebar open, the way pressing the top bar's button on a phone
/// leaves it: the shell's wrapper is marked `data-mobile-open`.
fn with_sidebar_open(html: &str) -> String {
    html.replacen(
        "data-slot=\"sidebar-wrapper\"",
        "data-slot=\"sidebar-wrapper\" data-mobile-open",
        1,
    )
}

/// `html` with no `data-ui-live-src` or `data-ui-live-event` attribute: the regions
/// and tables in it follow no stream. Every preview is drawn this way.
fn without_streams(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some((at, name)) = [" data-ui-live-src=\"", " data-ui-live-event=\""]
        .iter()
        .filter_map(|name| rest.find(name).map(|at| (at, name.len())))
        .min()
    {
        out.push_str(&rest[..at]);
        let value = &rest[at + name..];
        let end = value.find('"').map_or(value.len(), |end| end + 1);
        rest = &value[end..];
    }
    out.push_str(rest);
    out
}

#[derive(Serialize)]
struct ExampleJson<'a> {
    id: String,
    component: &'static str,
    slug: &'static str,
    title: &'static str,
    kind: &'static str,
    /// The Rust that drew it.
    code: &'a str,
    /// The preview document's URL.
    src: String,
    height: u32,
    layout: &'static str,
    cells: &'static str,
    show: bool,
    /// Draws a drawer or dialog open over its page.
    open: bool,
    live: bool,
    /// Drawn this wide and scaled to fit.
    width: Option<u32>,
    /// The frame grows to what it shows.
    fit: bool,
    /// The toolkit modules the preview loads.
    scripts: Vec<String>,
    captions: Vec<String>,
    /// The parts, for a single tree.
    slots: Vec<Slot>,
}

#[derive(Serialize)]
struct ComponentJson {
    id: &'static str,
    name: &'static str,
    url: String,
    /// The first type's doc comment.
    summary: String,
    group: Option<&'static str>,
    types: &'static [&'static str],
    thumb: String,
    anatomy: Option<String>,
    examples: Vec<String>,
}

#[derive(Serialize)]
struct GroupJson {
    id: &'static str,
    title: &'static str,
    components: &'static [&'static str],
}

#[derive(Serialize)]
struct Catalog<'a> {
    #[serde(rename = "$comment")]
    comment: &'static str,
    groups: Vec<GroupJson>,
    components: Vec<ComponentJson>,
    examples: Vec<ExampleJson<'a>>,
}

#[derive(Serialize)]
struct AssetJson {
    name: &'static str,
    ext: &'static str,
    /// The served name, with its content hash.
    file: String,
    /// What it's built from, in the repository.
    source: String,
    bytes: usize,
    gzip: Option<usize>,
    brotli: Option<usize>,
}

#[derive(Serialize)]
struct TokenJson {
    name: &'static str,
    value: &'static str,
}

#[derive(Serialize)]
struct Toolkit {
    #[serde(rename = "$comment")]
    comment: &'static str,
    /// The hash of every asset together.
    build: &'static str,
    /// The content security policy of every page.
    csp: &'static str,
    /// Where the files are served.
    mount: &'static str,
    assets: Vec<AssetJson>,
    /// The dark theme, the one that ships.
    tokens: Vec<TokenJson>,
    icons: usize,
    flags: usize,
    /// What the flags weigh: the flags page says it from these numbers.
    flag_weights: FlagWeights,
}

/// One flag's artwork, in bytes (the 4 by 3 art).
#[derive(Serialize)]
struct FlagWeight {
    code: &'static str,
    name: &'static str,
    bytes: usize,
}

/// What the flags weigh, measured in the committed artwork.
#[derive(Serialize)]
struct FlagWeights {
    flags: usize,
    median: usize,
    over_2_kb: usize,
    over_10_kb: usize,
    over_50_kb: usize,
    /// The four heaviest, heaviest first.
    heaviest: Vec<FlagWeight>,
    /// Flags people use, for scale.
    plain: Vec<FlagWeight>,
}

fn flag_weights() -> FlagWeights {
    use navree::ui::flags;
    let weigh = |code: flags::CountryCode| FlagWeight {
        code: code.as_str(),
        name: code.name(),
        bytes: code.rect_shapes().len(),
    };
    let mut all: Vec<FlagWeight> = flags::ALL.iter().map(|code| weigh(*code)).collect();
    let mut sizes: Vec<usize> = all.iter().map(|weight| weight.bytes).collect();
    sizes.sort_unstable();
    let over = |bytes: usize| sizes.iter().filter(|size| **size > bytes).count();
    let (over_2_kb, over_10_kb, over_50_kb) = (over(2 * 1024), over(10 * 1024), over(50 * 1024));
    let median = sizes[sizes.len() / 2];
    all.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.code.cmp(b.code)));
    all.truncate(4);
    FlagWeights {
        flags: sizes.len(),
        median,
        over_2_kb,
        over_10_kb,
        over_50_kb,
        heaviest: all,
        plain: vec![weigh(flags::FR), weigh(flags::US)],
    }
}

const GENERATED: &str = "Generated by `cargo run -p ui-docs` from crates/navree-ui. Do not edit.";

fn json(value: &impl Serialize) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text
}

/// Every file, by its path under `docs/ui-data`.
pub fn files(examples: &[Example], api: &BTreeMap<String, TypeDoc>) -> BTreeMap<String, String> {
    check(examples, api);
    let mut files = BTreeMap::new();
    let mut rendered = Vec::new();
    for example in examples {
        let component = COMPONENTS
            .iter()
            .find(|component| component.id == example.component)
            .unwrap_or_else(|| panic!("{}: no such component", example.id()));
        let page = match &example.kind {
            Kind::Page(draw) => Some(draw()),
            Kind::Single(_) | Kind::Gallery(_) | Kind::Search { .. } => None,
        };
        let (node, captions) = stage(example, page.as_ref());
        let uses = measure(&node).uses;
        let scripts = scripts(example, uses);
        let sheets = sheets(uses);
        // A page in a theme of your own links its sheet, which an app serves from
        // the theme's values: here it is written next to the toolkit's files.
        for theme in page.as_ref().map(View::own_themes).unwrap_or_default() {
            if let Some(css) = theme.css() {
                files.insert(format!("previews/{}", theme_sheet_path(theme)), css);
            }
        }
        let parts = Parts {
            page: page.as_ref(),
            scripts: &scripts,
            sheets: &sheets,
        };
        files.insert(
            format!("previews/{}/{}.html", example.component, example.slug),
            document(example, &node, &parts, component),
        );
        let slots = match &example.kind {
            Kind::Single(draw) => slots(&draw()),
            Kind::Page(_) => page
                .as_ref()
                .map(|view| slots(view.body_node()))
                .unwrap_or_default(),
            Kind::Gallery(_) | Kind::Search { .. } => Vec::new(),
        };
        rendered.push(ExampleJson {
            id: example.id(),
            component: example.component,
            slug: example.slug,
            title: example.title,
            kind: match example.kind {
                Kind::Single(_) | Kind::Page(_) => "single",
                Kind::Gallery(_) => "gallery",
                Kind::Search { .. } => "search",
            },
            code: &example.code,
            src: format!("{PREVIEW_ROOT}/{}/{}.html", example.component, example.slug),
            height: example.height,
            layout: example.layout.as_str(),
            cells: example.cells.as_str(),
            show: example.show,
            open: example.open,
            live: example.live,
            width: example.width,
            fit: example.fit,
            scripts,
            captions,
            slots,
        });
    }
    let components = COMPONENTS
        .iter()
        .map(|component| ComponentJson {
            id: component.id,
            name: component.name,
            url: component.url(),
            summary: component.about.map(str::to_owned).unwrap_or_else(|| {
                component
                    .types
                    .first()
                    .and_then(|name| api.get(*name))
                    .map(|doc| doc.summary.clone())
                    .unwrap_or_default()
            }),
            group: GROUPS
                .iter()
                .find(|group| group.components.contains(&component.id))
                .map(|group| group.id),
            types: component.types,
            thumb: format!("{}/{}", component.id, component.thumb),
            anatomy: component
                .anatomy
                .map(|slug| format!("{}/{slug}", component.id)),
            examples: examples
                .iter()
                .filter(|example| example.component == component.id)
                .map(Example::id)
                .collect(),
        })
        .collect();
    files.insert(
        "catalog.json".to_owned(),
        json(&Catalog {
            comment: GENERATED,
            groups: GROUPS
                .iter()
                .map(|group| GroupJson {
                    id: group.id,
                    title: group.title,
                    components: group.components,
                })
                .collect(),
            components,
            examples: rendered,
        }),
    );
    let wanted: BTreeMap<&str, &TypeDoc> = COMPONENTS
        .iter()
        .flat_map(|component| component.types.iter())
        .map(|name| {
            (
                *name,
                api.get(*name)
                    .unwrap_or_else(|| panic!("{name} is not a public type of navree-ui")),
            )
        })
        .collect();
    files.insert("api.json".to_owned(), json(&wanted));
    files.insert(
        "toolkit.json".to_owned(),
        json(&Toolkit {
            comment: GENERATED,
            build: assets::BUILD,
            csp: PAGE_CSP,
            mount: assets::MOUNT,
            assets: assets::FILES
                .iter()
                .map(|asset| AssetJson {
                    name: asset.name,
                    ext: asset.ext,
                    file: asset_file(asset),
                    source: checked_source(asset),
                    bytes: asset.bytes.len(),
                    gzip: asset.gzip.map(<[u8]>::len),
                    brotli: asset.brotli.map(<[u8]>::len),
                })
                .collect(),
            tokens: tokens::TOKENS
                .iter()
                .map(|token| TokenJson {
                    name: token.name,
                    value: token.dark,
                })
                .collect(),
            icons: icons::ALL.len(),
            flags: navree::ui::flags::ALL.len(),
            flag_weights: flag_weights(),
        }),
    );
    files.insert(
        "previews/_frame/preview.css".to_owned(),
        include_str!("../frame/preview.css").replace("\r\n", "\n"),
    );
    files.insert(
        "previews/_frame/preview.js".to_owned(),
        include_str!("../frame/preview.js").replace("\r\n", "\n"),
    );
    files
}

/// The source file of an asset, checked to hold exactly the embedded bytes, so
/// the website copies what the toolkit serves.
fn checked_source(asset: &Asset) -> String {
    let source = asset_source(asset);
    let bytes =
        std::fs::read(crate::repo().join(&source)).unwrap_or_else(|e| panic!("{source}: {e}"));
    assert_eq!(
        assets::content_hash(&bytes),
        asset.hash,
        "{source} differs from the embedded asset: run `node build.mjs` in crates/navree-ui/runtime"
    );
    source
}

/// The catalog holds together: every id it names exists, once.
fn check(examples: &[Example], api: &BTreeMap<String, TypeDoc>) {
    let mut seen = std::collections::BTreeSet::new();
    for example in examples {
        assert!(seen.insert(example.id()), "{} twice", example.id());
        if example.code.trim().is_empty() {
            panic!("{} has no code", example.id());
        }
    }
    for group in GROUPS {
        for id in group.components {
            assert!(
                COMPONENTS.iter().any(|component| component.id == *id),
                "group {} names {id}, which isn't a component",
                group.id
            );
        }
    }
    for component in COMPONENTS {
        for name in component.types {
            assert!(api.contains_key(*name), "{}: no type {name}", component.id);
        }
        let named = std::iter::once(component.thumb).chain(component.anatomy);
        for slug in named {
            let id = format!("{}/{slug}", component.id);
            assert!(
                seen.contains(&id),
                "{} names {id}, which isn't an example",
                component.id
            );
        }
    }
    let _ = Layout::Center;
}

#[cfg(test)]
mod stream_tests {
    #[test]
    fn a_preview_follows_no_stream() {
        let html = r#"<div id="r" data-ui-src="/f" data-ui-live-src="/events" data-ui-live-event="stats"><table data-ui-live-src="/events"></table></div>"#;
        assert_eq!(
            super::without_streams(html),
            r#"<div id="r" data-ui-src="/f"><table></table></div>"#
        );
        assert_eq!(super::without_streams("<p>plain</p>"), "<p>plain</p>");
    }
}

#[cfg(test)]
mod weight_tests {
    #[test]
    fn the_weights_come_from_the_committed_artwork() {
        let weights = super::flag_weights();
        assert_eq!(weights.flags, 270);
        assert_eq!(weights.heaviest.len(), 4);
        assert_eq!(weights.heaviest[0].code, "RS");
        assert!(
            weights
                .heaviest
                .windows(2)
                .all(|pair| pair[0].bytes >= pair[1].bytes)
        );
        assert!(weights.median < 2_000, "{}", weights.median);
        assert!(weights.over_2_kb > weights.over_10_kb && weights.over_10_kb > weights.over_50_kb);
        assert_eq!(weights.plain[0].code, "FR");
        assert_eq!(weights.plain[1].code, "US");
    }

    #[test]
    fn the_flags_page_says_no_weight_of_its_own() {
        let page = crate::read_repo("docs/ui/flags.mdx");
        assert!(page.contains("<UiFlagWeights />"));
        for stale in ["median flag", "over 2 KB", "Serbia is"] {
            assert!(
                !page.contains(stale),
                "{stale}: the numbers come from the data"
            );
        }
    }
}
