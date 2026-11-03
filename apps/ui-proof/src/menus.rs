//! Tabs, toggle groups, dropdown menus and toasts in the states the conformance suite
//! runs against: `/menus`, the pages a lazy tab loads, and the posts that raise toasts.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use http::{Method, StatusCode};
use navree::http::extract::{Path, RawQuery, urlencoded};
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::*;

/// How many times each lazy tab's page was asked for.
static HITS: Mutex<BTreeMap<String, u32>> = Mutex::new(BTreeMap::new());

/// The first value of each key in a query or a form body.
fn pairs(input: &str) -> Vec<(String, String)> {
    urlencoded::parse(input.as_bytes())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

fn first<'a>(all: &'a [(String, String)], name: &str) -> Option<&'a str> {
    all.iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn section(id: &'static str, title: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("id", id)
        .children((Heading::h2(title), Stack::gap(3).children(content)))
}

fn line_tabs(query: &[(String, String)]) -> Tabs {
    Tabs::new("t-line", "Deal sections")
        .active_from(first(query, "tab"))
        .tab(Tab::new("overview", "Overview").panel((
            Text::new("The overview panel."),
            Button::outline("A button in the panel"),
        )))
        .tab(
            Tab::new("activity", "Activity")
                .count("12")
                .panel(Text::new("The activity panel.")),
        )
        .tab(
            Tab::new("files", "Files")
                .disabled(true)
                .panel(Text::new("Files you can't reach.")),
        )
        .tab(
            Tab::new("settings", "Settings")
                .icon(icons::SETTINGS)
                .panel(Text::new("The settings panel.")),
        )
}

fn tabs(query: &[(String, String)]) -> Element {
    let many = (1..=24).fold(
        Tabs::new("t-many", "Many tabs")
            .param("m")
            .active_from(first(query, "m")),
        |tabs, n| {
            tabs.tab(
                Tab::new(format!("t{n}"), format!("Section number {n}"))
                    .panel(Text::new(format!("Panel {n}."))),
            )
        },
    );
    let lazy = Tabs::new("t-lazy", "Loaded on demand")
        .param("lz")
        .active_from(first(query, "lz"))
        .tab(Tab::new("fast", "Fast").lazy("/menus/tab/fast"))
        .tab(Tab::new("slow", "Slow").lazy("/menus/tab/slow"))
        .tab(Tab::new("fresh", "Fresh").lazy("/menus/tab/fresh").fresh())
        .tab(
            Tab::new("given", "Given")
                .lazy("/menus/tab/given")
                .panel(Text::new("Drawn with the page.")),
        );
    section(
        "tabs",
        "Tabs",
        (
            line_tabs(query),
            Tabs::new("t-seg", "Range")
                .param("seg")
                .variant(TabsVariant::Segmented)
                .size(Size::Sm)
                .activation(Activation::Manual)
                .active_from(first(query, "seg"))
                .tab(
                    Tab::new("day", "Day")
                        .icon(icons::CALENDAR)
                        .panel(Text::new("A day.")),
                )
                .tab(Tab::new("week", "Week").panel(Text::new("A week.")))
                .tab(
                    Tab::new("month", "Month")
                        .count("3")
                        .panel(Text::new("A month.")),
                ),
            Tabs::new("t-vert", "Settings sections")
                .param("v")
                .vertical()
                .active_from(first(query, "v"))
                .tab(Tab::new("profile", "Profile").panel(Text::new("The profile.")))
                .tab(Tab::new("billing", "Billing").panel(Text::new("The billing.")))
                .tab(Tab::new("team", "Team").panel(Text::new("The team."))),
            many,
            lazy,
            Tabs::new("t-find", "Findable")
                .param("f")
                .findable()
                .active_from(first(query, "f"))
                .tab(Tab::new("a", "First").panel(Text::new("The first panel.")))
                .tab(
                    Tab::new("b", "Second").panel(Text::new("A pineapple is in the second panel.")),
                ),
            Tabs::new("t-stretch", "Stretched")
                .param("s")
                .stretch()
                .variant(TabsVariant::Segmented)
                .active_from(first(query, "s"))
                .tab(Tab::new("one", "One").panel(Text::new("One.")))
                .tab(Tab::new("two", "Two").panel(Text::new("Two."))),
        ),
    )
}

fn toggles(query: &[(String, String)]) -> Element {
    let view = first(query, "view").unwrap_or("list");
    let filter = el("form")
        .attr("method", "get")
        .attr("action", "/menus")
        .attr("data-ui-target", "g-result")
        .children((
            ToggleGroup::single("view", "Show as")
                .variant(ToggleVariant::Outline)
                .size(Size::Sm)
                .joined()
                .item(ToggleItem::new("list", "List").icon(icons::LIST))
                .item(ToggleItem::new("grid", "Grid").icon(icons::LAYOUT_GRID))
                .item(ToggleItem::new("chart", "Chart"))
                .value(view),
            Button::outline("Apply").submit(),
        ));
    let days = el("form")
        .attr("id", "g-days-form")
        .attr("method", "post")
        .attr("action", "/menus/echo")
        .flag("data-ui-form", true)
        .children((
            ToggleGroup::multiple("days", "Working days")
                .variant(ToggleVariant::Outline)
                .size(Size::Sm)
                .item(ToggleItem::new("mon", "Mon"))
                .item(ToggleItem::new("tue", "Tue"))
                .item(ToggleItem::new("wed", "Wed"))
                .item(ToggleItem::new("thu", "Thu").disabled(true))
                .item(ToggleItem::new("fri", "Fri"))
                .values(["mon", "wed"]),
            Button::primary("Save days").submit(),
        ));
    let plain_days = el("form")
        .attr("id", "g-plain-form")
        .attr("method", "post")
        .attr("action", "/menus/echo")
        .children((
            ToggleGroup::multiple("plain", "Plain form days")
                .item(ToggleItem::new("a", "A"))
                .item(ToggleItem::new("b", "B")),
            ToggleGroup::single("size", "Size")
                .item(ToggleItem::new("s", "S"))
                .item(ToggleItem::new("m", "M"))
                .value("m"),
            Button::primary("Post").submit(),
        ));
    section(
        "toggles",
        "Toggle groups",
        (
            filter,
            el("div")
                .attr("id", "g-result")
                .child(Text::new(format!("Showing as {view}."))),
            days,
            plain_days,
            Row::gap(4).wrap().children((
                ToggleGroup::single("layout", "Layout")
                    .deselectable()
                    .vertical()
                    .joined()
                    .variant(ToggleVariant::Outline)
                    .item(ToggleItem::icon_only("list", icons::LIST, "As a list"))
                    .item(ToggleItem::icon_only(
                        "grid",
                        icons::LAYOUT_GRID,
                        "As a grid",
                    ))
                    .item(ToggleItem::icon_only(
                        "settings",
                        icons::SETTINGS,
                        "Settings",
                    ))
                    .value("list"),
                ToggleGroup::multiple("stacked", "Stacked days")
                    .vertical()
                    .variant(ToggleVariant::Outline)
                    .item(ToggleItem::new("early", "Early"))
                    .item(ToggleItem::new("late", "Late"))
                    .values(["late"]),
                ToggleGroup::single("all-off", "All disabled")
                    .disabled(true)
                    .item(ToggleItem::new("x", "X"))
                    .item(ToggleItem::new("y", "Y")),
            )),
        ),
    )
}

fn basic_menu() -> DropdownMenu {
    DropdownMenu::new("m-basic", "Actions")
        .caret()
        .item(
            MenuItem::link("Edit", "/menus?edited=1")
                .icon(icons::PENCIL)
                .shortcut("E"),
        )
        .item(
            MenuItem::action("Duplicate")
                .icon(icons::COPY)
                .on_click(Cmd::toast(ToastTone::Success, "Duplicated."))
                .shortcut("Ctrl+D"),
        )
        .item(MenuItem::action("Archive").disabled(true))
        .item(MenuItem::action("Rename").opens_dialog("m-dialog-box"))
        .separator()
        .group(
            MenuGroup::new("Share")
                .item(
                    MenuItem::action("Copy link")
                        .on_click(Cmd::toast(ToastTone::Info, "Link copied.")),
                )
                .item(MenuItem::link("Email", "/menus?mail=1")),
        )
        .separator()
        .item(
            MenuItem::post("Delete", "/menus/echo")
                .icon(icons::TRASH_2)
                .destructive(),
        )
}

fn view_menu() -> DropdownMenu {
    DropdownMenu::new("m-view", "View")
        .checkbox(MenuCheckbox::new("archived", "Show archived").checked(true))
        .checkbox(MenuCheckbox::new("owner", "Show owner").keep_open())
        .radios(
            MenuRadios::new("sort", "Sort by")
                .option("name", "Name")
                .option("date", "Date")
                .option("size", "Size")
                .value("name"),
        )
}

fn sub_menu() -> DropdownMenu {
    DropdownMenu::new("m-sub", "Move")
        .item(MenuItem::action("Rename"))
        .submenu(
            MenuSub::new("Move to")
                .icon(icons::FOLDER)
                .item(
                    MenuItem::action("Inbox")
                        .on_click(Cmd::toast(ToastTone::Info, "Moved to Inbox.")),
                )
                .item(MenuItem::action("Archive"))
                .submenu(
                    MenuSub::new("Label as")
                        .item(
                            MenuItem::action("Urgent")
                                .on_click(Cmd::toast(ToastTone::Info, "Labelled Urgent.")),
                        )
                        .item(MenuItem::action("Later")),
                ),
        )
        .submenu(
            MenuSub::new("Disabled submenu")
                .disabled(true)
                .item(MenuItem::action("x")),
        )
        .item(MenuItem::action("Delete").destructive())
}

fn long_menu() -> DropdownMenu {
    (1..=60).fold(DropdownMenu::new("m-long", "Sixty items"), |menu, n| {
        menu.item(MenuItem::action(format!("Item {n}")))
    })
}

fn menus() -> Element {
    let table = Table::new()
        .caption("Rows with a menu")
        .head((TableHead::new("Deal"), TableHead::new("Actions")))
        .rows([1, 2].map(|n| {
            TableRowEl::new((
                TableCell::new(format!("Deal {n}")),
                TableCell::new(
                    DropdownMenu::icon(
                        format!("m-row-{n}"),
                        icons::ELLIPSIS,
                        format!("Actions for deal {n}"),
                    )
                    .align(PopupAlign::End)
                    .size(Size::Sm)
                    .item(MenuItem::link("Open", format!("/menus?open={n}")))
                    .item(MenuItem::post("Remove", "/menus/echo").destructive()),
                ),
            ))
        }));
    section(
        "menus",
        "Dropdown menus",
        (
            Row::gap(2).wrap().children((
                basic_menu(),
                el("form")
                    .attr("id", "m-view-form")
                    .attr("method", "post")
                    .attr("action", "/menus/echo")
                    .flag("data-ui-form", true)
                    .children((view_menu(), Button::primary("Save view").submit())),
                el("form")
                    .attr("id", "m-post-form")
                    .attr("method", "post")
                    .attr("action", "/menus/echo")
                    .flag("data-ui-form", true)
                    .children((
                        el("input")
                            .attr("type", "hidden")
                            .attr("name", "note")
                            .attr("value", "outer"),
                        DropdownMenu::new("m-post", "Row actions")
                            .item(MenuItem::post("Archive", "/menus/archive"))
                            .item(MenuItem::link("Open", "/menus?open=9")),
                        Button::primary("Save outer").submit(),
                    )),
                sub_menu(),
                long_menu(),
                DropdownMenu::icon("m-right", icons::ELLIPSIS_VERTICAL, "Menu at the edge")
                    .align(PopupAlign::Start)
                    .item(MenuItem::action(
                        "Wide item with a long label that needs room",
                    ))
                    .item(MenuItem::action("Another")),
            )),
            table,
            Dialog::new("m-dialog-box", "Rename deal")
                .description("A menu item opened this.")
                .body(
                    Stack::gap(3).children((
                        Row::gap(2).children((
                            DropdownMenu::new("m-in-dialog", "Menu in a dialog")
                                .item(
                                    MenuItem::action("One")
                                        .on_click(Cmd::toast(ToastTone::Info, "From the dialog.")),
                                )
                                .item(MenuItem::action("Two")),
                            Button::outline("Raise a toast")
                                .attr("id", "m-dialog-toast")
                                .on_click(Cmd::toast(
                                    ToastTone::Success,
                                    "Raised over the dialog.",
                                )),
                        )),
                        el("form")
                            .attr("id", "m-dialog-form")
                            .attr("method", "post")
                            .attr("action", "/menus/leave")
                            .flag("data-ui-form", true)
                            .child(
                                Button::primary("Save and leave")
                                    .attr("id", "m-dialog-save")
                                    .submit(),
                            ),
                    )),
                )
                .footer(Button::outline("Close").closes_dialog("m-dialog-box")),
            Button::outline("Open a sheet").opens_dialog("m-sheet-box"),
            Sheet::new("m-sheet-box", "Menu in a sheet").body(
                DropdownMenu::new("m-in-sheet", "Menu in a sheet")
                    .item(MenuItem::action("One"))
                    .item(MenuItem::action("Two")),
            ),
        ),
    )
}

fn toasts() -> Element {
    let form = |id: &'static str,
                action: &'static str,
                label: &'static str,
                js: bool,
                fields: Vec<(&'static str, &'static str)>| {
        let mut form = el("form")
            .attr("id", id)
            .attr("method", "post")
            .attr("action", action)
            .flag("data-ui-form", js);
        for (name, value) in fields {
            form = form.child(
                el("input")
                    .attr("type", "hidden")
                    .attr("name", name)
                    .attr("value", value),
            );
        }
        form.child(Button::outline(label).submit())
    };
    section(
        "toasts",
        "Toasts",
        (
            Row::gap(2).wrap().children([
                ("n-info", ToastTone::Info, "Info"),
                ("n-success", ToastTone::Success, "Success"),
                ("n-warning", ToastTone::Warning, "Warning"),
                ("n-danger", ToastTone::Danger, "Error"),
            ].map(|(id, tone, label)| {
                Button::outline(label)
                    .attr("id", id)
                    .on_click(Cmd::toast(tone, format!("{label} from a command")))
            })),
            Row::gap(2).wrap().children((
                Button::outline("Six at once").attr("id", "n-six").on_click(
                    (1..=6)
                        .map(|n| Cmd::toast(ToastTone::Info, format!("Toast number {n}")))
                        .collect::<Vec<_>>(),
                ),
                Button::outline("Hostile text").attr("id", "n-hostile").on_click(Cmd::toast(
                    ToastTone::Warning,
                    "<img src=x onerror=alert(1)> and a very long word: Pneumonoultramicroscopicsilicovolcanoconiosis_Pneumonoultramicroscopicsilicovolcanoconiosis",
                )),
            )),
            Row::gap(2).wrap().children((
                form("n-form-plain", "/menus/toast", "Server toast", true, vec![("mode", "plain")]),
                form("n-form-rich", "/menus/toast", "Rich toast", true, vec![("mode", "rich")]),
                form("n-form-sticky", "/menus/toast", "Sticky toast", true, vec![("mode", "sticky")]),
                form("n-form-id", "/menus/toast", "Saving then saved", true, vec![("mode", "id")]),
                form("n-form-fail", "/menus/toast", "Refused", true, vec![("mode", "fail")]),
            )),
            Row::gap(2).wrap().children((
                form("n-redirect", "/menus/redirect", "Post and redirect", true, vec![]),
                form("n-redirect-plain", "/menus/redirect", "Post and redirect, no script", false, vec![]),
                form("n-reload", "/menus/reload", "Toast then reload", true, vec![]),
                Button::outline("A real page load")
                    .href("/menus?hard=1")
                    .attr("id", "n-hard")
                    .attr("data-ui-noboost", ""),
            )),
        ),
    )
}

fn page(shell: &AppShell, query: &str) -> AppPage {
    let query = pairs(query);
    shell.page("Menus and toasts").body(Stack::gap(8).children((
        tabs(&query),
        toggles(&query),
        menus(),
        toasts(),
    )))
}

pub struct MenuPages;

impl Injectable for MenuPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

/// A lazy tab's page: a fragment that says which tab it is and how often it was asked for.
fn fragment(name: &str) -> View {
    let count = {
        let mut hits = HITS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = hits.entry(name.to_owned()).or_insert(0);
        *count += 1;
        *count
    };
    View::new("Tab").body(Stack::gap(2).children((
        Text::new(format!("The {name} tab, loaded {count} time(s).")),
        Button::outline("A button in the loaded panel"),
    )))
}

fn hits() -> Response {
    let hits = HITS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let body = hits
        .iter()
        .map(|(name, count)| format!("\"{name}\":{count}"))
        .collect::<Vec<_>>()
        .join(",");
    (
        [(http::header::CONTENT_TYPE, "application/json")],
        format!("{{{body}}}"),
    )
        .into_response()
}

/// The rich toast of the op, drawn by the component, so the runtime's can be held beside it.
fn server_toast() -> View {
    View::new("A toast").body(
        Toaster::new().toast(
            Toast::new(ToastTone::Success, "The deal moved to the archive.")
                .title("Archived")
                .action(ToastAction::post("Undo", "/menus/undo")),
        ),
    )
}

fn echo(body: &str) -> Reply {
    let all = pairs(body);
    let text = all
        .iter()
        .filter(|(name, _)| name != "_csrf")
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(", ");
    Reply::ops_or_redirect("/menus").toast(ToastTone::Success, format!("Posted: {text}"))
}

fn toast_post(body: &str) -> Reply {
    let all = pairs(body);
    match first(&all, "mode").unwrap_or("plain") {
        "rich" => Reply::ops_or_redirect("/menus").toast_with(
            Toast::new(ToastTone::Success, "The deal moved to the archive.")
                .title("Archived")
                .action(ToastAction::post("Undo", "/menus/undo")),
        ),
        "sticky" => Reply::ops_or_redirect("/menus").toast_with(
            Toast::new(ToastTone::Danger, "The import stopped at row 40.")
                .title("Import failed")
                .duration(ToastDuration::Sticky),
        ),
        "id" => Reply::ops_or_redirect("/menus")
            .toast_with(Toast::new(ToastTone::Info, "Saving…").id("save"))
            .toast_with(Toast::new(ToastTone::Success, "Saved.").id("save")),
        "fail" => Reply::ops_or_redirect("/menus")
            .toast(ToastTone::Danger, "That name is taken.")
            .status(StatusCode::UNPROCESSABLE_ENTITY),
        _ => Reply::ops_or_redirect("/menus").toast(ToastTone::Success, "Saved from the server."),
    }
}

impl Controller for MenuPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/menus",
                |shell: AppShell, RawQuery(query): RawQuery| async move {
                    page(&shell, query.as_deref().unwrap_or_default())
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/menus/tab/{name}",
                |Path(name): Path<String>| async move { fragment(&name) },
            )
            .public(),
            RouteDef::on(Method::GET, "!/menus/hits", || async { hits() }).public(),
            RouteDef::on(Method::GET, "!/menus/server-toast", || async {
                server_toast()
            })
            .public(),
            RouteDef::on(Method::POST, "!/menus/echo", |body: String| async move {
                echo(&body)
            })
            .public(),
            RouteDef::on(Method::POST, "!/menus/toast", |body: String| async move {
                toast_post(&body)
            })
            .public(),
            RouteDef::on(Method::POST, "!/menus/undo", || async {
                Reply::ops_or_redirect("/menus").toast(ToastTone::Info, "Undone.")
            })
            .public(),
            RouteDef::on(Method::POST, "!/menus/redirect", || async {
                Reply::redirect("/menus?landed=1").toast(ToastTone::Success, "Saved, and moved on.")
            })
            .public(),
            RouteDef::on(Method::POST, "!/menus/leave", || async {
                Reply::redirect("/menus/plain?landed=1").toast_with(
                    Toast::new(ToastTone::Success, "Saved, and moved on.")
                        .duration(ToastDuration::Sticky),
                )
            })
            .public(),
            RouteDef::on(Method::POST, "!/menus/archive", || async {
                Reply::ops_or_redirect("/menus").toast(ToastTone::Success, "Archived by the item.")
            })
            .public(),
            RouteDef::on(Method::GET, "!/menus/plain", |shell: AppShell| async move {
                shell
                    .page("A page without the dialog")
                    .body(Text::new("This page has no dialog."))
            })
            .public(),
            RouteDef::on(Method::POST, "!/menus/reload", || async {
                Reply::ops(vec![Op::Reload]).toast(ToastTone::Info, "Reloading now.")
            })
            .public(),
        ]
    }
}
