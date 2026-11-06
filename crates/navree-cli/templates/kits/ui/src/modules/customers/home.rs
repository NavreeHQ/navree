//! The home page: a few figures that stay current, and the newest customers.
//!
//! | Route | Answers |
//! |---|---|
//! | `GET /` | the page |
//! | `GET /fragments/figures` | the figures alone, which the page fetches again when the stream speaks |

use http::Method;
use navree::http::{Controller, RouteDef};
use navree::prelude::*;
use navree::ui::*;

use super::entity::Customer;
use super::service::{CustomerService, Figures};
use crate::ui::format::{day, grouped, rupees};
use crate::ui::live::{TOPIC, stream};
use crate::ui::shell::Desk;

/// The four figures across the top.
fn figures_grid(figures: &Figures) -> Grid {
    let paying = figures.monthly_paise;
    Grid::cols(4).gap(3).children((
        Stat::new("Customers", grouped(figures.customers)).trend(
            format!("{} active", grouped(figures.active)),
            figures.active > 0,
        ),
        Stat::new("Active", grouped(figures.active)).trend("Paying every month", true),
        Stat::new("On trial", grouped(figures.trials))
            .trend("Could join a plan", figures.trials > 0),
        Stat::new("Monthly revenue", rupees(paying)).trend("From active customers", paying > 0),
    ))
}

/// The figures, live: they are fetched again when the stream says a customer changed.
fn live_figures(figures: &Figures) -> Region {
    Region::new("figures")
        .src("/fragments/figures")
        .live_on(stream().on(TOPIC))
        .children(figures_grid(figures))
}

/// The newest customers, each a link.
fn newest(customers: &[Customer]) -> Card {
    let rows: Vec<Row> = customers
        .iter()
        .map(|customer| {
            Row::gap(2).align(Align::Center).children((
                Button::new(customer.name.clone())
                    .variant(ButtonVariant::Link)
                    .href(format!("/customers/{}", customer.id)),
                Text::muted(format!("{}, since {}", customer.city, day(customer.since))).inline(),
            ))
        })
        .collect();
    Card::titled("Newest customers").body(Stack::gap(2).children(rows))
}

/// `GET /`.
async fn home(desk: Desk, uow: Uow, service: Scoped<CustomerService>) -> Result<AppPage> {
    let figures = service.figures(&uow, desk.workspace()).await?;
    let customers = service.newest(&uow, desk.workspace(), 5).await?;
    Ok(desk
        .page("Home")
        .actions(
            Button::outline("All customers")
                .icon(icons::USERS)
                .href("/customers"),
        )
        .body(Stack::gap(4).children((live_figures(&figures), newest(&customers)))))
}

/// `GET /fragments/figures`.
async fn figures_fragment(
    desk: Desk,
    uow: Uow,
    service: Scoped<CustomerService>,
) -> Result<View> {
    let figures = service.figures(&uow, desk.workspace()).await?;
    Ok(View::new("Figures").body(figures_grid(&figures)))
}

/// The routes above.
#[injectable]
pub struct HomePages;

impl Controller for HomePages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/", home)
                .name("home")
                .authenticated()
                .page(),
            RouteDef::on(Method::GET, "!/fragments/figures", figures_fragment)
                .name("fragments.figures")
                .authenticated(),
        ]
    }
}
