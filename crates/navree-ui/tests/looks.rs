//! The looks a component takes only when a page asks for one: what each writes, what it
//! links, how assistive technology reads it, and that the sheet they share draws nothing for a
//! part drawn as it always was (`component_looks.rs` holds that markup).

use navree_ui::assets::{self, get};
use navree_ui::*;

fn html(node: impl Into<Node>) -> String {
    render(&node.into())
}

/// The stylesheet's source the looks are drawn by.
fn looks_source() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles/39-looks.css");
    std::fs::read_to_string(path)
        .expect("the looks sheet")
        .replace("\r\n", "\n")
}

/// The attribute contract every component keeps, and the sheet a look links.
fn keeps_the_contract(name: &str, node: Node) {
    let out = render(&node);
    assert!(!out.contains("style="), "{name}: {out}");
    assert!(!out.contains("class="), "{name}: {out}");
    assert!(!out.contains(" onclick"), "{name}: {out}");
    let page = View::new("x").body(node);
    assert!(
        page.uses().has(Uses::LOOKS),
        "{name} links the looks sheet: {:?}",
        page.uses()
    );
    assert!(page.to_html().contains(assets::LOOKS_URL), "{name}");
}

fn mode(current: &str) -> ButtonGroup {
    ButtonGroup::new("Mode").segmented().block().children((
        Button::new("Mine")
            .href("/deals/mine")
            .current(current == "mine"),
        Button::new("Team")
            .href("/deals/team")
            .current(current == "team"),
    ))
}

#[test]
fn a_segmented_group_of_links_is_a_named_group_of_real_links_with_one_current() {
    let out = html(mode("team"));
    assert!(
        out.starts_with(r#"<div data-slot="button-group" role="group" aria-label="Mode" data-variant="segmented" data-block>"#),
        "{out}"
    );
    assert_eq!(out.matches("<a ").count(), 2, "{out}");
    assert_eq!(out.matches("<button").count(), 0, "{out}");
    assert!(out.contains(r#"href="/deals/mine">Mine</a>"#), "{out}");
    assert!(
        out.contains(r#"<a data-slot="button" aria-current="page" href="/deals/team">Team</a>"#),
        "{out}"
    );
    assert_eq!(out.matches("aria-current").count(), 1, "{out}");
    // No script: the links work as links, with a link's keys.
    let page = View::new("x").body(mode("mine"));
    assert_eq!(page.uses(), Uses::LOOKS);
    assert!(!page.to_html().contains("<script"));
    keeps_the_contract("segmented", mode("mine").into());
    keeps_the_contract(
        "block",
        ButtonGroup::new("View")
            .block()
            .children(Button::new("List"))
            .into(),
    );
}

#[test]
fn a_current_button_says_so_and_one_that_is_not_writes_nothing() {
    assert!(html(Button::new("Mine").href("/m").current(true)).contains(r#"aria-current="page""#));
    assert!(
        html(Button::new("Now").current(true))
            .starts_with(r#"<button data-slot="button" aria-current="page" type="button">"#)
    );
    assert_eq!(
        html(Button::new("Mine").href("/m").current(false)),
        html(Button::new("Mine").href("/m"))
    );
    // A current button on its own needs no sheet of its own.
    assert!(
        View::new("x")
            .body(Button::new("Mine").href("/m").current(true))
            .uses()
            .is_none()
    );
}

#[test]
fn the_segmented_group_reads_the_segment_roles_toggle_corners_and_control_heights() {
    let css = looks_source();
    for read in [
        "background:var(--segment-track,var(--ink-2))",
        "background:var(--segment-on,var(--ink-3))",
        "border-radius:var(--radius-toggle,var(--radius-lg))",
        "border-radius:var(--radius-toggle,var(--radius))",
        "height:calc(var(--control-md,2.25rem) - .375rem)",
        "height:calc(var(--control-sm,1.875rem) - .375rem)",
        "height:calc(var(--control-lg,2.625rem) - .375rem)",
    ] {
        assert!(css.contains(read), "{read}");
    }
    // The current one is raised by the rule every current button has.
    let core = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles/10-controls.css"),
    )
    .unwrap();
    assert!(core.contains(
        "&[aria-current]{background:var(--ink-3);color:var(--fg);box-shadow:var(--shadow-raised)}"
    ));
}

/// The hooks the looks sheet's rules are keyed on: each a value a page writes only when it asks
/// for that look, so none of them reaches a part drawn as it always was.
const HOOKS: [&str; 2] = [
    "[data-slot=button-group][data-variant=segmented]",
    "[data-slot=button-group][data-block]",
];

#[test]
fn every_rule_of_the_looks_sheet_is_keyed_on_a_look_a_page_asks_for() {
    let css = looks_source();
    let mut body = String::new();
    let mut rest = css.as_str();
    // The source without its comments.
    while let Some(at) = rest.find("/*") {
        body.push_str(&rest[..at]);
        rest = rest[at..].split_once("*/").map_or("", |(_, after)| after);
    }
    body.push_str(rest);
    // Each rule at the top, or at the top of a media or container block: its selector, up
    // to its block.
    let mut open: Vec<bool> = Vec::new();
    let mut selector = String::new();
    let mut rules = 0;
    for c in body.chars() {
        let top = open.iter().all(|at_rule| *at_rule);
        match c {
            '{' => {
                let found = selector.trim().to_owned();
                selector.clear();
                let at_rule = top && found.starts_with('@');
                if top && !at_rule {
                    rules += 1;
                    assert!(
                        found
                            .split(',')
                            .all(|one| HOOKS.iter().any(|hook| one.trim().starts_with(hook))),
                        "{found} is keyed on no look"
                    );
                }
                open.push(at_rule);
            }
            '}' => {
                open.pop();
                selector.clear();
            }
            ';' => selector.clear(),
            _ if top => selector.push(c),
            _ => {}
        }
    }
    assert!(rules >= 2, "the sheet was read: {rules}");
    // And the sheet every page loads holds none of them.
    let shared = std::str::from_utf8(get("navree-ui").unwrap().bytes).unwrap();
    for hook in HOOKS {
        assert!(
            !shared.contains(hook),
            "{hook} in the sheet every page loads"
        );
    }
}

// ---- The inverse surface: a card, a stat and a code block ----------------------------------

#[test]
fn an_inverse_card_stat_and_code_block_mark_the_inverse_surface_and_link_no_sheet_of_their_own() {
    let card = html(
        Card::titled("Security, in every mode")
            .variant(CardVariant::Inverse)
            .body(Text::new("Every team works in its own pipeline."))
            .footer(Button::primary("Request a quote")),
    );
    assert!(
        card.starts_with(r#"<div data-slot="card" data-variant="inverse" data-inverse>"#),
        "{card}"
    );
    let stat = html(
        Stat::new("Invoices due", "₹9,12,400")
            .caption("this week")
            .variant(StatVariant::Inverse),
    );
    assert!(
        stat.starts_with(r#"<div data-slot="stat" data-variant="inverse" data-inverse>"#),
        "{stat}"
    );
    let code = html(CodeBlock::new("sh", "navree dev").copy_button().inverse());
    assert!(
        code.starts_with(r#"<div data-slot="code-block" data-inverse>"#),
        "{code}"
    );
    // The surface needs no rule of its own: a card and a plain stat link nothing, a code block
    // its own sheet and a rich stat the app blocks' sheet, as they do on the page's palette.
    let uses = |node: Node| View::new("x").body(node).uses();
    assert!(uses(Card::titled("Seats").variant(CardVariant::Inverse).into()).is_none());
    assert!(
        uses(
            Stat::new("Revenue", "1")
                .variant(StatVariant::Inverse)
                .into()
        )
        .is_none()
    );
    assert_eq!(
        uses(
            Stat::new("Revenue", "1")
                .unit("INR")
                .variant(StatVariant::Inverse)
                .into()
        ),
        Uses::APP_BLOCKS
    );
    assert_eq!(
        uses(CodeBlock::new("sh", "x").inverse().into()),
        Uses::MARKDOWN
    );
    // Asking for the default writes nothing.
    assert_eq!(
        html(Stat::new("Open", "12").variant(StatVariant::Default)),
        html(Stat::new("Open", "12"))
    );
    assert_eq!(
        html(Card::new().variant(CardVariant::Default)),
        html(Card::new())
    );
    // A code block in Markdown is never on the inverse surface by itself.
    let markdown = html(Markdown::new("```sh\nnavree dev\n```\n"));
    assert!(markdown.contains("data-slot=\"code-block\""), "{markdown}");
    assert!(!markdown.contains("data-inverse"), "{markdown}");
}

#[test]
fn the_inverse_surface_is_the_stylesheets_own_and_cards_stats_and_code_paint_with_it() {
    // The flipped scheme is in the sheet every page loads: a card, a stat and a code block
    // marked `data-inverse` read their colours from it, with no rule of their own.
    let shared = std::str::from_utf8(get("navree-ui").unwrap().bytes).unwrap();
    assert!(shared.contains(":where([data-inverse]){color-scheme:light;color:var(--fg-1)}"));
    for slot in ["[data-slot=card]{", "[data-slot=stat]{"] {
        let at = shared.find(slot).unwrap_or_else(|| panic!("{slot}"));
        let rule = &shared[at..at + shared[at..].find('}').unwrap()];
        assert!(
            rule.contains("background:var(--card,var(--ink-1))"),
            "{rule}"
        );
    }
    let code = std::str::from_utf8(get("markdown").unwrap().bytes).unwrap();
    assert!(
        code.contains("[data-slot=code-block]{") && code.contains("background:var(--ink-1)"),
        "{code:.300}"
    );
}

/// With no colours of its own, the inverse surface is the theme's other palette. Every text
/// pair the toolkit draws, the colours of code on the code panel among them, reads there in
/// every built-in theme, on a light page and on a dark one.
#[test]
fn every_built_in_theme_reads_on_its_inverse_surface() {
    use navree_ui::theme::{Paint, PairKind, Side, Theme, Token, pairs};

    // The colours of code are pairs on the code panel's surface.
    for token in [
        Token::Success,
        Token::Amber,
        Token::Cyan,
        Token::Info,
        Token::AccentText,
        Token::Danger,
        Token::Fg,
        Token::Fg1,
        Token::Fg2,
        Token::Fg3,
    ] {
        assert!(
            pairs().iter().any(|pair| pair.kind == PairKind::Text
                && pair.text == Paint::Token(token)
                && pair.on == Paint::Token(Token::Ink1)),
            "{} on ink-1",
            token.name()
        );
    }
    for theme in Theme::BUILT_IN {
        for page in Side::BOTH {
            let other = match page {
                Side::Light => Side::Dark,
                Side::Dark => Side::Light,
            };
            let palette = theme.spec().palette(other);
            for pair in pairs().iter().filter(|pair| pair.kind == PairKind::Text) {
                let ratio = pair.ratio(palette, other);
                assert!(
                    ratio >= pair.min,
                    "{}: {} on {} is {ratio:.2}:1 on the inverse surface of a {} page",
                    theme.id(),
                    pair.text.describe(),
                    pair.on.describe(),
                    page.as_str()
                );
            }
        }
    }
}

/// A theme's own inverse surface is measured with every colour of code on it: one that would
/// make a colour of code unreadable is refused, named by the surface.
#[test]
fn a_theme_whose_inverse_surface_hides_a_colour_of_code_is_refused() {
    use navree_ui::theme::{ImportError, Inverse, Rgb, Roles, Theme, ThemeId};

    // A mid green panel on a light page: the dark palette's strings (amber) and numbers (cyan)
    // are lost on it.
    let roles = Roles::new().inverse(
        Inverse::new()
            .surface(Rgb::hex(0x1f6f3f), Rgb::hex(0xffffff))
            .text(Rgb::hex(0xffffff), Rgb::hex(0x0a0a0a)),
    );
    let theme = Theme::NAVREE.renamed(ThemeId::parse("agra").unwrap(), "Agra");
    let Err(ImportError::Fields(errors)) = theme.with_roles(roles) else {
        panic!("a green panel that hides the colours of code is refused");
    };
    for colour in ["amber", "cyan"] {
        assert!(
            errors.iter().any(|(at, what)| at == "inverse"
                && what.starts_with(&format!("{colour} on inverse is "))
                && what.contains(" in light, on the inverse surface;")),
            "{colour}: {errors:?}"
        );
    }
}
