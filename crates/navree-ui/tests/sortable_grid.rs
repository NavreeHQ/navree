//! The sortable grid's markup: what the runtime reads, what a page with no script posts,
//! and what a screen reader hears.

use navree_ui::{GridFit, GridOrder, GridTile, SortableGrid, SortableGridLabels, render};

fn grid() -> SortableGrid {
    SortableGrid::new("tiles", "Widget tiles")
        .post("/widgets/7/tiles")
        .columns(2)
        .tile(GridTile::new("health", "Health score").fixed())
        .tile(GridTile::new("recovery", "Recovery"))
        .tile(GridTile::new("sleep", "Sleep"))
}

fn html(grid: SortableGrid) -> String {
    render(&grid.into())
}

/// The `<li>` of the tile `key`.
fn tile<'a>(html: &'a str, key: &str) -> &'a str {
    let start = html
        .find(&format!(r#"data-key="{key}""#))
        .unwrap_or_else(|| panic!("no tile {key}: {html}"));
    let start = html[..start].rfind("<li").unwrap();
    let end = start + html[start..].find("</li>").unwrap();
    &html[start..end]
}

#[test]
fn a_grid_that_posts_is_a_named_group_with_its_hooks_words_and_order_form() {
    let out = html(grid());
    assert!(
        out.starts_with(r#"<div data-slot="sortable-grid" id="tiles" role="group" aria-label="Widget tiles" data-columns="2" data-ui-grid tabindex="-1" data-ui-autosave="500" data-l=""#),
        "{out}"
    );
    // The words the runtime says, with their gaps for the tile and its place.
    assert!(
        out.contains("Picked up {item}. Position {position} of {total}."),
        "{out}"
    );
    assert!(out.contains("Removed {item}. {total} left."), "{out}");
    // The instructions every handle is described by, and the live region.
    assert!(out.contains(
        r#"<p data-slot="sortable-grid-help" id="tiles-help" hidden>Press Space to pick it up"#
    ));
    assert!(out.contains(r#"<div data-slot="sortable-grid-status" aria-live="assertive" aria-atomic="true" data-ui-sr></div>"#));
    // One form, posted by the core runtime, with every key in the order drawn.
    assert!(out.contains(r#"<form data-slot="sortable-grid-form" id="tiles-form" method="post" action="/widgets/7/tiles" data-ui-form data-ui-deadline="15000"><input type="hidden" name="order" value="health"><input type="hidden" name="order" value="recovery"><input type="hidden" name="order" value="sleep"></form>"#), "{out}");
    // An ordered list, so a screen reader reads each tile's number.
    assert!(out.contains(r#"<ol data-slot="sortable-grid-items">"#));
    assert!(out.contains(r#"<p data-slot="sortable-grid-empty">Nothing here yet</p>"#));
}

#[test]
fn each_tile_has_a_handle_steps_and_a_remove_button_tied_to_the_form() {
    let out = html(grid());
    let recovery = tile(&out, "recovery");
    assert!(recovery.starts_with(r#"<li data-slot="sortable-grid-item" id="tiles-tile-recovery" data-key="recovery" data-name="Recovery">"#), "{recovery}");
    assert!(recovery.contains(r#"<button data-slot="sortable-grid-handle" type="button" tabindex="0" aria-label="Move Recovery" aria-describedby="tiles-help">"#), "{recovery}");
    assert!(recovery.contains(r#"<button data-slot="sortable-grid-step" type="submit" form="tiles-form" name="up" value="recovery" aria-label="Move Recovery earlier">"#), "{recovery}");
    assert!(recovery.contains(r#"<button data-slot="sortable-grid-step" type="submit" form="tiles-form" name="down" value="recovery" aria-label="Move Recovery later">"#), "{recovery}");
    assert!(recovery.contains(r#"<button data-slot="sortable-grid-remove" type="submit" form="tiles-form" name="remove" value="recovery" aria-label="Remove Recovery">"#), "{recovery}");
    // With no body, the name is what shows.
    assert!(
        recovery.contains(r#"<div data-slot="sortable-grid-body"><span>Recovery</span></div>"#)
    );
}

#[test]
fn the_first_tile_moves_no_earlier_and_the_last_no_later() {
    let out = html(grid());
    let first = tile(&out, "health");
    assert!(
        first.contains(
            r#"name="up" value="health" aria-label="Move Health score earlier" disabled>"#
        ),
        "{first}"
    );
    assert!(
        !first.contains(
            r#"name="down" value="health" aria-label="Move Health score later" disabled"#
        )
    );
    let last = tile(&out, "sleep");
    assert!(
        last.contains(r#"name="down" value="sleep" aria-label="Move Sleep later" disabled>"#),
        "{last}"
    );
    assert!(!last.contains(r#"name="up" value="sleep" aria-label="Move Sleep earlier" disabled"#));
    // One tile alone moves neither way.
    let alone = html(
        SortableGrid::new("g", "G")
            .post("/g")
            .tile(GridTile::new("a", "A")),
    );
    assert_eq!(alone.matches(r#" disabled>"#).count(), 2, "{alone}");
}

#[test]
fn a_fixed_tile_has_no_remove_button() {
    let out = html(grid());
    let fixed = tile(&out, "health");
    assert!(fixed.contains("data-fixed"), "{fixed}");
    assert!(!fixed.contains("sortable-grid-remove"), "{fixed}");
    assert!(
        fixed.contains("sortable-grid-handle"),
        "a fixed tile still moves"
    );
}

#[test]
fn a_grid_with_nowhere_to_post_draws_tiles_only() {
    let out = html(
        SortableGrid::new("g", "Tiles")
            .tile(GridTile::new("a", "A").body("Body of A"))
            .tile(GridTile::new("b", "B")),
    );
    for absent in [
        "data-ui-grid",
        "<form",
        "<button",
        "data-l=",
        "aria-live",
        "tabindex",
    ] {
        assert!(!out.contains(absent), "{absent} in {out}");
    }
    assert!(out.contains(r#"data-fit="medium""#), "{out}");
    assert!(out.contains(r#"<div data-slot="sortable-grid-body">Body of A</div>"#));
}

#[test]
fn columns_are_one_to_six_and_a_fit_names_its_width() {
    let columns = |n| html(SortableGrid::new("g", "G").columns(n));
    assert!(columns(0).contains(r#"data-columns="1""#));
    assert!(columns(4).contains(r#"data-columns="4""#));
    assert!(columns(9).contains(r#"data-columns="6""#));
    for (fit, name) in [
        (GridFit::Narrow, "narrow"),
        (GridFit::Medium, "medium"),
        (GridFit::Wide, "wide"),
    ] {
        let out = html(SortableGrid::new("g", "G").columns(3).fit(fit));
        assert!(out.contains(&format!(r#"data-fit="{name}""#)), "{out}");
        assert!(!out.contains("data-columns"), "{out}");
    }
}

#[test]
fn the_pause_the_deadline_the_token_and_the_words_are_the_callers() {
    let mut labels = SortableGridLabels::default();
    labels.handle = "Verschieben: {item}".into();
    labels.empty = "Noch leer".into();
    let out = html(
        SortableGrid::new("g", "G")
            .post("/g")
            .autosave_ms(0)
            .deadline_ms(10)
            .csrf("tok-123")
            .labels(labels)
            .tile(GridTile::new("a", "Schlaf")),
    );
    assert!(out.contains(r#"data-ui-autosave="0""#), "{out}");
    assert!(
        out.contains(r#"data-ui-deadline="1000""#),
        "a deadline is at least 1 s: {out}"
    );
    assert!(out.contains(r#"<input type="hidden" name="_csrf" value="tok-123"><input type="hidden" name="order" value="a">"#), "{out}");
    assert!(out.contains(r#"aria-label="Verschieben: Schlaf""#));
    assert!(out.contains(">Noch leer</p>"));
    let long = html(SortableGrid::new("g", "G").post("/g").autosave_ms(60_000));
    assert!(long.contains(r#"data-ui-autosave="10000""#), "{long}");
}

#[test]
fn hostile_keys_and_names_stay_text_and_ids_stay_plain() {
    let out = html(
        SortableGrid::new("g", "G")
            .post("/g")
            .tile(GridTile::new(
                r#""><script>x</script>"#,
                "<img src=x onerror=alert(1)>",
            ))
            .tile(GridTile::new("a b", "Second")),
    );
    assert!(!out.contains("<script>"), "{out}");
    assert!(!out.contains("<img"), "{out}");
    assert!(out.contains("&lt;img src=x onerror=alert(1)&gt;"));
    // Keys that can't be an id are hashed into one; the value keeps the key itself.
    assert!(!out.contains(r#"id="g-tile-a b""#), "{out}");
    assert!(out.contains(r#"name="order" value="a b""#), "{out}");
}

#[test]
fn the_order_a_page_posts_reads_back_as_drawn() {
    // What a page with no script sends when Recovery's "earlier" button is pressed.
    let order = GridOrder::from_form(b"order=health&order=recovery&order=sleep&up=recovery")
        .expect("an order");
    let arranged = order
        .keep(["health"])
        .arrange(&["health", "recovery", "sleep"]);
    assert_eq!(arranged.order, ["recovery", "health", "sleep"]);
    // And a removal of the fixed tile, forged, changes nothing.
    let forged = GridOrder::from_form(b"order=health&order=recovery&order=sleep&remove=health")
        .unwrap()
        .keep(["health"])
        .arrange(&["health", "recovery", "sleep"]);
    assert!(!forged.changed);
}
