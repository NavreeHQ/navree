//! ErrorPage, Forbidden, NotFound, SignInRequired, SessionEnded, EmptyState and the
//! states of a page.

use navree::ui::*;

use crate::catalog::{Example, gallery, page, single};

const SOURCE: &str = include_str!("states.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("errors-and-states", "kinds", "Every kind", errors_kinds).wide(),
        single!(
            "errors-and-states",
            "inline",
            "In a card, a tab or a list",
            errors_inline
        )
        .start(),
        single!("error-page", "thumb", "A page not found", error_page_thumb).height(240),
        single!(
            "error-page",
            "default",
            "A page not found",
            error_page_default
        )
        .height(380),
        gallery!("error-page", "kinds", "Every kind", error_page_kinds).wide(),
        page!("error-page", "shell", "In the app shell", error_page_shell)
            .fill()
            .width(1100)
            .height(600),
        single!("forbidden", "thumb", "No access", forbidden_thumb),
        single!(
            "forbidden",
            "default",
            "No access to a report",
            forbidden_default
        )
        .start(),
        single!(
            "forbidden",
            "request-access",
            "With a way to ask",
            forbidden_request_access
        )
        .start(),
        single!("forbidden", "row", "A row the user lost", forbidden_row).start(),
        single!("not-found", "thumb", "A deal that is gone", not_found_thumb),
        single!(
            "not-found",
            "default",
            "A deal that is gone",
            not_found_default
        )
        .start(),
        single!("not-found", "compact", "One line", not_found_compact).start(),
        single!(
            "sign-in-required",
            "thumb",
            "Sign in to see this",
            sign_in_thumb
        ),
        single!("sign-in-required", "default", "In a card", sign_in_default).start(),
        single!(
            "sign-in-required",
            "notice",
            "After a session ends",
            sign_in_notice
        )
        .start(),
        single!("empty-state", "thumb", "Nothing yet", empty_state_thumb),
        single!("empty-state", "default", "Nothing yet", empty_state_default).height(240),
        single!(
            "empty-state",
            "no-results",
            "A filter found nothing",
            empty_state_no_results
        )
        .height(240),
        single!("empty-state", "caught-up", "Done", empty_state_caught_up).height(200),
        single!("empty-state", "row", "In a table", empty_state_row).start(),
    ]
}

fn errors_kinds() -> Grid {
    Grid::cols(2).gap(3).children(
        [
            StateKind::SignIn,
            StateKind::Forbidden,
            StateKind::NotFound,
            StateKind::Conflict,
            StateKind::Expired,
            StateKind::RateLimited,
            StateKind::Failed,
            StateKind::Unavailable,
        ]
        .map(|kind| {
            ErrorPage::of(kind)
                .sign_in("/sign-in")
                .reference("01hx7k2m9q")
                .state(None, false)
                .size(StateSize::Panel)
        })
        .into_iter()
        .collect::<Vec<_>>(),
    )
}

fn errors_inline() -> Stack {
    Stack::gap(3).children((
        Card::titled("Revenue").body(Forbidden::new().what("report")),
        Card::titled("Owner").body(NotFound::new().what("deal")),
        Card::titled("Your deals").body(SignInRequired::new("/sign-in").what("your deals")),
    ))
}

fn error_page_thumb() -> State {
    ErrorPage::not_found()
        .reference("01hx7k2m9q")
        .state(None, false)
        .size(StateSize::Panel)
}

fn error_page_default() -> State {
    ErrorPage::not_found()
        .reference("01hx7k2m9q")
        .state(None, false)
        .size(StateSize::Page)
}

fn error_page_kinds() -> impl IntoIterator<Item = (&'static str, State)> {
    [
        StateKind::SignIn,
        StateKind::Forbidden,
        StateKind::NotFound,
        StateKind::MethodNotAllowed,
        StateKind::Conflict,
        StateKind::Gone,
        StateKind::Expired,
        StateKind::Invalid,
        StateKind::RateLimited,
        StateKind::Failed,
        StateKind::Unavailable,
    ]
    .map(|kind| {
        (
            kind.name(),
            ErrorPage::of(kind)
                .sign_in("/sign-in")
                .retry_after(12)
                .reference("01hx7k2m9q")
                .state(None, false)
                .size(StateSize::Panel),
        )
    })
}

fn error_page_shell() -> AppPage {
    let shell = Shell::new("Acme CRM")
        .context("Production")
        .group(
            NavGroup::new("Sales")
                .item(NavItem::new("Overview", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE))
                .item(NavItem::new("Contacts", "/contacts").icon(icons::USERS)),
        )
        .user(UserMenu::new("Ada Lovelace", "ada@example.com").action("Sign out", "/sign-out"));
    shell.page("Page not found", "/deals/42/notes").body(
        ErrorPage::not_found()
            .reference("01hx7k2m9q")
            .state(None, true),
    )
}

fn forbidden_thumb() -> Forbidden {
    Forbidden::new().what("report")
}

fn forbidden_default() -> Card {
    Card::titled("Revenue").body(Forbidden::new().what("report"))
}

fn forbidden_request_access() -> Card {
    Card::titled("Contracts").body(
        Forbidden::new()
            .what("contract")
            .request_access("/access/contracts"),
    )
}

fn forbidden_row() -> Table {
    Table::new()
        .caption("Deals")
        .head((TableHead::new("Name"), TableHead::new("Amount").numeric()))
        .rows((
            TableRowEl::new((TableCell::new("Acme"), TableCell::new("1,200.00").numeric())),
            Forbidden::new().row(2),
            TableRowEl::new((
                TableCell::new("Globex"),
                TableCell::new("3,400.00").numeric(),
            )),
        ))
}

fn not_found_thumb() -> NotFound {
    NotFound::new().what("deal")
}

fn not_found_default() -> Card {
    Card::titled("Owner").body(NotFound::new().what("deal"))
}

fn not_found_compact() -> Card {
    Card::titled("Related").body(Stack::gap(2).children((
        NotFound::new().what("deal").compact(),
        Forbidden::new().what("contact").compact(),
    )))
}

fn sign_in_thumb() -> SignInRequired {
    SignInRequired::new("/sign-in").what("your deals")
}

fn sign_in_default() -> Card {
    let here = ReturnTo::parse("/deals?tab=notes");
    let state = SignInRequired::new("/sign-in").what("your deals");
    Card::titled("Your deals").body(match &here {
        Some(here) => state.return_to(here),
        None => state,
    })
}

fn sign_in_notice() -> Stack {
    Stack::gap(3).children((
        SessionEnded::new(),
        Field::new("name", "Email").control(Input::email("email")),
        Button::primary("Sign in"),
    ))
}

fn empty_state_thumb() -> EmptyState {
    EmptyState::nothing_yet("deals")
}

fn empty_state_default() -> EmptyState {
    EmptyState::nothing_yet("deals")
        .description("Create a deal to start tracking your pipeline.")
        .action(Button::primary("New deal").icon(icons::PLUS))
}

fn empty_state_no_results() -> EmptyState {
    EmptyState::no_results().clear_filters("/deals")
}

fn empty_state_caught_up() -> EmptyState {
    EmptyState::caught_up()
}

fn empty_state_row() -> Table {
    Table::new()
        .caption("Deals")
        .head((TableHead::new("Name"), TableHead::new("Amount").numeric()))
        .rows(EmptyState::no_results().clear_filters("/deals").row(2))
}
