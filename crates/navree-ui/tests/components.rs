//! Snapshots of each component's rendered HTML (one element per line, so a
//! change shows up as a readable diff). Update them with
//! `INSTA_UPDATE=always cargo test -p navree-ui --test components` and
//! review the `.snap` files.

use navree_ui::{
    Action, Cell, Chart, Column, Crumb, Empty, FormErrors, MenuItem, NavGroup, NavItem, PageInfo,
    Row, SeriesTone, Shell, Sparkline, Table, Toast, Tone, UserMenu, views_with,
};
use serde_json::{Value, json};

/// Renders `body` (a template snippet with `ui` imported) with `ctx`.
fn render(body: &str, ctx: Value) -> String {
    let source = format!("{{% import \"ui/components.html\" as ui %}}{body}");
    let views = views_with([("test.html", source)]).expect("the snippet compiles");
    let html = views.render("test.html", ctx).expect("the snippet renders");
    lines(&html)
}

/// One tag per line.
fn lines(html: &str) -> String {
    html.replace("><", ">\n<").trim().to_owned()
}

#[test]
fn buttons() {
    insta::assert_snapshot!(render(
        r#"{{ ui.button("Save", variant="primary", type="submit") }}
{{ ui.button("Export", icon="download", href="/export?format=csv&all=1") }}
{{ ui.button("Search", icon="search", shortcut="mod+K", size="sm") }}
{{ ui.button("Delete", variant="danger", dialog="confirm-delete", disabled=true) }}
{{ ui.icon_button("ellipsis", "More actions") }}
{{ ui.action_button(action) }}"#,
        json!({ "action": Action::dialog("Invite", "invite").icon("user-plus").primary() }),
    ));
}

#[test]
fn badges_status_avatars_and_keys() {
    insta::assert_snapshot!(render(
        r#"{{ ui.badge("Admin") }}{{ ui.badge("Paid", "success", icon="check") }}{{ ui.badge("Live", "success", dot=true) }}
{{ ui.status("Degraded", "warning") }}
{{ ui.avatar("Ada Lovelace") }}{{ ui.avatar("Grace", src="/avatars/grace.png") }}
{{ ui.kbd("mod+shift+P") }}"#,
        json!({}),
    ));
}

#[test]
fn page_header_card_and_alerts() {
    insta::assert_snapshot!(render(
        r#"{% call ui.page_header("Users", "Everyone in <Acme>.") %}{{ ui.button("Invite", variant="primary") }}{% endcall %}
{{ ui.page_header("Settings") }}
{% call ui.card("Plan", "Business", link={"label": "Change", "href": "/billing"}, footer="Renews on 1 Dec") %}<p>Body</p>{% endcall %}
{{ ui.alert("Webhook failing", "Five failures.", tone="danger", items=["503 at 10:02", "503 at 10:07"]) }}
{{ ui.details([["Region", "eu-west-1"], {"term": "Plan", "value": "Business"}]) }}
{{ ui.progress(62, label="Storage used") }}"#,
        json!({}),
    ));
}

#[test]
fn stats_and_charts() {
    let bars = Chart::bars(["May", "Jun", "Jul"])
        .series("Sign-ups", SeriesTone::Muted, [180.0, 210.0, 320.0])
        .highlight(2);
    let lines_chart = Chart::lines(["Mon", "Tue", "Wed", "Thu"])
        .series("Requests", SeriesTone::Bright, [10.0, 30.0, 20.0, 40.0])
        .dashed_series("Last week", SeriesTone::Muted, [8.0, 12.0, 30.0, 25.0])
        .suffix(" rpm");
    insta::assert_snapshot!(render(
        r#"{{ ui.stat("Revenue", "$48,210", delta="+12.5%", trend="up", note="vs. last month", icon="wallet", spark=spark) }}
{{ ui.stat("Errors", "0.4%", delta="-0.1%", trend="down", tone="positive") }}
{{ ui.chart_card("Sign-ups", bars, description="Per month", value="320") }}
{{ ui.chart_card("Requests", lines_chart) }}"#,
        json!({
            "spark": Sparkline::new([1.0, 3.0, 2.0, 5.0]),
            "bars": bars,
            "lines_chart": lines_chart,
        }),
    ));
}

fn users() -> Table {
    Table::new("users")
        .caption("Users")
        .column(Column::new("name", "Name").sortable())
        .column(Column::new("role", "Role"))
        .column(Column::new("deals", "Deals").numeric().sortable())
        .column(Column::new("seen", "Last seen").hide_on_mobile())
        .row(
            Row::new("1")
                .cell(Cell::lines("Ada Lovelace", "ada@acme.test").href("/users/1"))
                .cell(Cell::badge("Admin", Tone::Neutral))
                .cell(Cell::positive("$1,200"))
                .cell(Cell::muted("Today"))
                .action(MenuItem::link("Edit", "/users/1/edit").icon("pencil"))
                .action(MenuItem::separator())
                .action(
                    MenuItem::dialog("Remove", "remove")
                        .icon("trash-2")
                        .danger(),
                ),
        )
        .row(
            Row::new("2")
                .cell(Cell::strong("Grace & Co"))
                .cell(Cell::status("Invited", Tone::Warning))
                .cell(Cell::number("0"))
                .cell(Cell::mono("u_2"))
                .selected(),
        )
        .bulk(Action::link("Export", "/users/export").icon("download"))
        .sort("-deals", "/users?q=a&sort={sort}")
        .page(PageInfo::new(2, 15, 1234), "/users?page={page}")
        .per_page("/users?per_page={per_page}", &[15, 50])
}

#[test]
fn data_table() {
    insta::assert_snapshot!(render(
        "{{ ui.data_table(table) }}",
        json!({ "table": users() })
    ));
}

#[test]
fn data_table_empty_and_loading() {
    let empty = Table::new("hooks")
        .column(Column::new("url", "Endpoint"))
        .empty(
            Empty::new("No endpoints")
                .description("Add one.")
                .icon("webhook")
                .action(Action::link("Add endpoint", "/hooks/new").icon("plus")),
        )
        .page(PageInfo::new(1, 15, 0), "?page={page}");
    let loading = Table::new("jobs")
        .column(Column::new("job", "Job"))
        .column(Column::new("ms", "Runtime").numeric())
        .selectable()
        .loading(2);
    insta::assert_snapshot!(render(
        "{{ ui.data_table(empty) }}\n{{ ui.data_table(loading) }}\n{{ ui.loading() }}",
        json!({ "empty": empty, "loading": loading }),
    ));
}

#[test]
fn pagination_takes_navree_page_meta() {
    // The `meta` of a `navree_db::Page`, as JSON.
    let meta = json!({ "current_page": 1, "per_page": 15, "total": 31, "last_page": 3 });
    insta::assert_snapshot!(render(
        r#"{{ ui.pagination(meta, "/deals?page={page}") }}"#,
        json!({ "meta": meta }),
    ));
}

#[test]
fn forms_show_navree_validation_errors() {
    let errors = FormErrors::from_body(&json!({
        "statusCode": 400,
        "message": ["email must be a valid email address"],
        "error": "Bad Request",
        "errors": { "email": ["email must be a valid email address"] }
    }));
    insta::assert_snapshot!(render(
        r#"{{ ui.form_errors(errors) }}
{{ ui.field("email", "Email", type="email", value="ada@", errors=errors, hint="Work address.", required=true) }}
{{ ui.field("site", "Website", icon="globe", optional=true, attrs={"data-x": "1"}) }}
{{ ui.textarea_field("bio", "Bio", value="Hi <there>", rows=2) }}
{{ ui.select_field("role", "Role", [["admin", "Admin"], ["rep", "Sales rep"]], value="rep") }}
{{ ui.select_field("tz", "Time zone", ["UTC", {"value": "Asia/Kolkata", "label": "India"}], placeholder="Select") }}
{{ ui.checkbox_field("terms", "I agree", errors={"terms": ["Confirm to continue"]}) }}
{{ ui.switch_field("mail", "Email", checked=true, hint="Mentions.") }}
{{ ui.search_input(value="ada") }}"#,
        json!({ "errors": errors }),
    ));
}

#[test]
fn tabs() {
    insta::assert_snapshot!(render(
        r#"{% call(tab) ui.tabs("deal", [{"id": "overview", "label": "Overview"}, {"id": "files", "label": "Files", "count": 3}], selected="files") %}<p>{{ tab.label }} panel</p>{% endcall %}
{{ ui.link_tabs([{"label": "General", "href": "/s", "active": true}, {"label": "Members", "href": "/s/m", "count": 24}], variant="line") }}"#,
        json!({}),
    ));
}

#[test]
fn menus_dialogs_and_toasts() {
    insta::assert_snapshot!(render(
        r#"{{ ui.dropdown("view", "View", items, icon="sliders-horizontal", align="end") }}
{{ ui.row_actions("row-1", items) }}
{% call ui.dialog("invite", "Invite a user", "They get an email.") %}<div class="ui-dialog__body">Form</div>{% endcall %}
{{ ui.confirm_dialog("remove", "Remove Grace?", "She loses access.", confirm="Remove", action="/users/2/delete", csrf={"name": "_csrf", "value": "t0k"}) }}
{{ ui.toast(toast) }}"#,
        json!({
            "items": [
                MenuItem::label("Columns"),
                MenuItem::link("Name", "#").checked(true),
                MenuItem::link("Export", "/export").icon("download").shortcut("Ctrl E"),
                MenuItem::separator(),
                MenuItem::dialog("Delete", "remove").icon("trash-2").danger(),
            ],
            "toast": Toast::error("Could not save").description("Try again."),
        }),
    ));
}

#[test]
fn values_are_escaped() {
    let html = render(
        r#"{{ ui.badge(text) }}{{ ui.button(text, href=text) }}{{ ui.field("x", text, value=text) }}"#,
        json!({ "text": "<script>alert(\"x\")</script>" }),
    );
    assert!(!html.contains("<script>"), "{html}");
    assert!(html.contains("&lt;script&gt;"), "{html}");
}

#[test]
fn unknown_icons_fail_the_render() {
    let views = views_with([(
        "test.html",
        r#"{% import "ui/components.html" as ui %}{{ ui.icon("no-such-icon") }}"#,
    )])
    .unwrap();
    assert!(views.render("test.html", json!({})).is_err());
}

#[test]
fn the_shell() {
    let shell = Shell::new("Helm")
        .context("Acme Inc.")
        .home("/helm")
        .assets("/_navree/ui/v")
        .switch(
            MenuItem::link("Helm", "/helm")
                .icon("ship-wheel")
                .checked(true),
        )
        .switch(
            MenuItem::link("Fleet", "/fleet")
                .icon("boxes")
                .checked(false),
        )
        .group(
            NavGroup::new("Records")
                .item(NavItem::new("Dashboard", "/helm").icon("layout-dashboard"))
                .item(
                    NavItem::new("Users", "/helm/users")
                        .icon("users")
                        .badge("12")
                        .child("All users", "/helm/users")
                        .child_with_badge("Invitations", "/helm/users/invitations", "3"),
                ),
        )
        .user(
            UserMenu::new("Ada Lovelace", "ada@acme.test")
                .item(MenuItem::link("Sign out", "/helm/logout").icon("log-out")),
        )
        .crumb(Crumb::link("Users", "/helm/users"))
        .crumb(Crumb::page("Invitations"))
        .title("Invitations")
        .toast(Toast::success("Saved"))
        .current("/helm/users/invitations");
    let views = views_with([(
        "page.html",
        r#"{% extends "ui/app.html" %}{% block content %}<p>Page</p>{% endblock %}"#,
    )])
    .unwrap();
    let html = navree_ui::render(&views, "page", &shell, ()).unwrap().0;
    insta::assert_snapshot!(lines(&html));
}
