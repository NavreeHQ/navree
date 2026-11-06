//! Islands: a part of a page drawn in the browser by a script of the app's own.
//!
//! | Page | Shows |
//! |---|---|
//! | `/islands?city=` | a tally drawn from the server's props in a region whose links change the city, the same tally's fallback with no script, a forged island naming another site's script, and a link away |

use std::sync::Arc;

use http::Method;
use navree::http::extract::{RawQuery, urlencoded};
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::*;

/// The tally's script, compiled in: at most 4 KB.
pub static TALLY: IslandScript =
    IslandScript::new("tally", include_bytes!("../islands/tally.js"), 4 * 1024);

/// What the tally mounts with.
#[derive(serde::Serialize)]
struct Tally {
    city: &'static str,
    start: u32,
}

/// The depots and the parcels each has packed today.
const DEPOTS: [(&str, u32); 3] = [("Amritsar", 124), ("Jalandhar", 86), ("Patiala", 41)];

fn page(shell: &AppShell, query: &str) -> AppPage {
    let asked = urlencoded::parse(query.as_bytes())
        .find(|(name, _)| name == "city")
        .map(|(_, value)| value.into_owned());
    let (city, start) = DEPOTS
        .iter()
        .copied()
        .find(|(name, _)| asked.as_deref() == Some(*name))
        .unwrap_or(DEPOTS[0]);
    let links: Vec<Element> = DEPOTS
        .iter()
        .map(|(name, _)| {
            el("a")
                .href(format!("/islands?city={name}"))
                .attr("id", format!("depot-{}", name.to_lowercase()))
                .flag("data-ui-in-region", true)
                .child(*name)
        })
        .collect();
    shell.page("Islands").body(Stack::gap(6).children((
        Heading::h1("Islands"),
        Region::new("depot")
            .src(format!("/islands?city={city}"))
            .children(Stack::gap(3).children((
                Row::gap(3).children(links),
                Island::new("tally", &TALLY)
                    .label("Parcels packed today")
                    .props(&Tally { city, start })
                    .fallback(
                        el("p")
                            .attr("data-tally-fallback", "")
                            .child(format!("Parcels packed in {city}: {start}")),
                    ),
            ))),
        // Markup that names another site's script: the islands module imports nothing for it.
        el("div")
            .attr("id", "forged")
            .attr("data-ui-island", "https://attacker.example/island/x-000000000000.js")
            .attr("data-ui-props", "{}")
            .child(el("p").child("A forged island")),
        el("a").href("/").attr("id", "away").child("The overview, which has no island"),
    )))
}

pub struct IslandPages;

impl Injectable for IslandPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for IslandPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/islands",
                |shell: AppShell, RawQuery(query): RawQuery| async move {
                    page(&shell, query.as_deref().unwrap_or_default())
                },
            )
            .public(),
        ]
    }
}
