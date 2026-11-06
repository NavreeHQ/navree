//! The actions of a table row are a real menu: the roles and keys of a menu, a button that
//! says whose actions they are, links and posts as before, and nothing that grows a form
//! with the rows.

use navree_ui::{
    DataTable, RowAction, TableLabels, TableQuery, TableRow, Uses, View, icons, render,
};

#[derive(Clone, TableRow)]
#[table(per = 1000, per_pages(10, 1000))]
struct Row {
    #[col(skip)]
    id: i64,
    #[col(sortable, search, filter = text)]
    title: String,
    #[col(sortable, numeric, filter = range)]
    amount: i64,
}

fn rows(count: i64) -> Vec<Row> {
    (1..=count)
        .map(|id| Row {
            id,
            title: format!("Deal {id}"),
            amount: id * 100_000,
        })
        .collect()
}

fn actions(row: &Row) -> Vec<RowAction> {
    vec![
        RowAction::link("View", format!("/deals/{}", row.id)).icon(icons::EYE),
        RowAction::post("Archive", format!("/deals/{}/archive", row.id)).icon(icons::ARCHIVE),
        RowAction::post("Delete", format!("/deals/{}/delete", row.id))
            .icon(icons::TRASH)
            .destructive(),
        RowAction::post("Waive the fee", format!("/deals/{}/waive", row.id)).disabled(true),
    ]
}

fn table<'a>(
    query: &'a TableQuery<Row>,
    rows: &'a [Row],
    with: impl Fn(&Row) -> Vec<RowAction> + 'a,
) -> DataTable<'a, Row> {
    DataTable::new("deals", query, rows)
        .row_actions(with)
        .csrf("tok")
}

fn draw(rows: &[Row], with: impl Fn(&Row) -> Vec<RowAction>) -> String {
    let query = TableQuery::<Row>::parse("/deals", "per=1000");
    render(&table(&query, rows, with).into())
}

#[test]
fn a_rows_actions_are_a_menu_opened_by_a_button_that_names_the_row() {
    let html = draw(&rows(2), actions);
    // The button: an icon button that opens a menu, named for its row.
    assert!(html.contains(r#"id="deals-a-1-t""#), "{html}");
    assert!(html.contains(r#"popovertarget="deals-a-1""#), "{html}");
    assert!(html.contains(r#"aria-haspopup="menu""#), "{html}");
    assert!(html.contains(r#"aria-expanded="false""#), "{html}");
    assert!(
        html.contains(r#"aria-label="Actions for Deal 1""#),
        "{html}"
    );
    assert!(
        html.contains(r#"aria-label="Actions for Deal 2""#),
        "{html}"
    );
    // The menu: the markup the menus module gives the keys of a menu.
    assert!(
        html.contains(r#"id="deals-a-1" popover="auto" role="menu""#),
        "{html}"
    );
    assert!(html.contains(r#"aria-labelledby="deals-a-1-t""#), "{html}");
    assert!(html.contains("data-ui-menu"), "{html}");
    // The row's popover is no longer a dialog of links.
    assert!(
        !html.contains(r#"id="deals-a-1" popover="auto" role="dialog""#),
        "{html}"
    );
    // Four items a row: a link, two posts, and one that cannot be taken.
    assert_eq!(html.matches(r#"role="menuitem""#).count(), 8, "{html}");
}

#[test]
fn a_link_is_a_menu_item_and_a_post_is_a_small_form_with_the_token_around_a_menu_item() {
    let html = draw(&rows(1), actions);
    assert!(
        html.contains(r#"<a data-slot="menu-item" role="menuitem" href="/deals/1">"#),
        "{html}"
    );
    // A post keeps its own small form inside the row: the page's one post form would tie a
    // submit control to a form for every row.
    assert!(
        html.contains(r#"<form method="post" action="/deals/1/archive""#),
        "{html}"
    );
    assert!(html.contains(r#"role="none""#), "{html}");
    assert!(html.contains(r#"name="_csrf""#), "{html}");
    assert!(
        html.contains(r#"<button data-slot="menu-item" role="menuitem" type="submit">"#),
        "{html}"
    );
    assert!(
        html.contains(
            r#"<button data-slot="menu-item" role="menuitem" type="submit" data-destructive>"#
        ),
        "{html}"
    );
    // The destructive post is not a link: a click away, a prefetch or a crawler can't do it.
    assert!(!html.contains(r#"href="/deals/1/delete""#), "{html}");
    assert!(!html.contains(r#"form="ui-post""#), "{html}");
}

#[test]
fn an_action_that_cannot_be_taken_stays_in_the_menu_and_does_nothing() {
    let html = draw(&rows(1), actions);
    let at = html.find("Waive the fee").expect("the item");
    let start = html[..at].rfind("<button").expect("its tag");
    let tag = &html[start..at];
    assert!(tag.contains(r#"aria-disabled="true""#), "{tag}");
    assert!(tag.contains("data-disabled"), "{tag}");
    assert!(!tag.contains(r#"type="submit""#), "{tag}");
    assert!(!html.contains("/deals/1/waive"), "{html}");
}

#[test]
fn the_buttons_words_are_the_tables_and_come_with_the_rows_first_column() {
    let query = TableQuery::<Row>::parse("/deals", "per=1000");
    let data = rows(1);
    let labels = TableLabels {
        row_actions: "{label} ke kaam".into(),
        ..TableLabels::default()
    };
    let html = render(&table(&query, &data, actions).labels(&labels).into());
    assert!(html.contains(r#"aria-label="Deal 1 ke kaam""#), "{html}");
}

#[test]
fn text_from_the_row_and_from_the_actions_is_escaped_and_an_unsafe_link_goes_nowhere() {
    let mut data = rows(1);
    data[0].title = r#""><img src=x onerror=alert(1)>"#.to_owned();
    let html = draw(&data, |row| {
        vec![
            RowAction::link("<i>Open</i>", "javascript:alert(1)"),
            RowAction::post("<b>Remove</b>", format!("/deals/{}/remove", row.id)),
        ]
    });
    for bad in ["<img", "<i>", "<b>", "javascript:"] {
        assert!(!html.contains(bad), "{bad}: {html}");
    }
    assert!(html.contains("&lt;i&gt;Open&lt;/i&gt;"), "{html}");
}

#[test]
fn a_row_with_no_actions_draws_no_menu_at_all() {
    let html = draw(&rows(2), |row| {
        if row.id == 1 {
            Vec::new()
        } else {
            vec![RowAction::link("View", "/deals/2")]
        }
    });
    assert!(!html.contains(r#"id="deals-a-1""#), "{html}");
    assert!(html.contains(r#"id="deals-a-2""#), "{html}");
    // The cell is still there, so the columns line up.
    assert_eq!(
        html.matches(r#"data-slot="cell-actions""#).count(),
        3,
        "{html}"
    );
}

/// What the page asks of a form: ` form="` attributes that are not the table's own.
fn tied(html: &str) -> usize {
    html.matches(" form=\"").count()
}

#[test]
fn rows_add_no_form_attribute_and_no_control_to_a_shared_form() {
    let few = draw(&rows(3), actions);
    let many = draw(&rows(300), actions);
    assert_eq!(tied(&many), tied(&few), "a row tied a control to a form");
    assert!(!many.contains(r#"form="ui-post""#));
    // Two posts a row can take (the third is shown but cannot be taken), each in a form of
    // its own that holds one submit button.
    assert_eq!(many.matches("<form method=\"post\"").count(), 300 * 2);
}

#[test]
fn a_thousand_rows_keep_every_id_apart_and_every_button_tied_to_its_menu() {
    let html = draw(&rows(1000), actions);
    assert_eq!(html.matches("role=\"menu\"").count(), 1000);
    for id in [1, 500, 1000] {
        assert!(html.contains(&format!(r#"id="deals-a-{id}-t""#)), "{id}");
        assert!(
            html.contains(&format!(r#"aria-labelledby="deals-a-{id}-t""#)),
            "{id}"
        );
    }
    let mut ids: Vec<&str> = html
        .match_indices(" id=\"deals-a-")
        .map(|(at, _)| {
            let rest = &html[at + 5..];
            &rest[..rest.find('"').unwrap_or(rest.len())]
        })
        .collect();
    assert_eq!(ids.len(), 2000, "a button and a menu for each row");
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 2000, "an id is used twice");
}

#[test]
fn a_table_loads_the_menus_module_only_when_its_rows_have_a_menu() {
    let query = TableQuery::<Row>::parse("/deals", "per=1000");
    let data = rows(2);
    let with = View::new("x").body(table(&query, &data, actions));
    let uses = with.uses();
    for module in [Uses::CORE, Uses::TABLE, Uses::MENUS, Uses::OVERLAY] {
        assert!(uses.has(module), "{uses:?}");
    }
    // It is the menus module and the overlay that places it, and nothing from the new menus.
    assert!(!uses.has(Uses::MENUBAR), "{uses:?}");
    let without = View::new("x").body(DataTable::new("deals", &query, &data));
    assert!(!without.uses().has(Uses::MENUS), "{:?}", without.uses());
}
