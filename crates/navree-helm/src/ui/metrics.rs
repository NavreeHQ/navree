//! Metric cards: a value and how it moved, a trend with its line, a partition as
//! bars, each drawn with Navree UI.

use navree_ui::charts::Sparkline;
use navree_ui::{
    Button, ButtonVariant, Card, Empty, Grid, Justify, Node, Progress, Row, Size, Stack, Stat,
    Text, el,
};

use crate::cx::Cx;
use crate::handle::ErasedResource;
use crate::metric::MetricResult;

/// The address of the page a card is on, with another range.
pub(super) type RangeLink<'a> = &'a (dyn Fn(&str) -> String + Sync);

/// A change as the card shows it: `+12.4%` (a tenth of a percent under 100 %,
/// whole percents above).
pub(super) fn change_text(change: f64) -> String {
    let percent = change * 100.0;
    if percent.abs() < 100.0 {
        format!("{percent:+.1}%")
    } else {
        format!("{percent:+.0}%")
    }
}

/// What the change is against, for the card's range.
pub(super) fn change_note(range: Option<&str>) -> String {
    match range {
        Some(days @ ("7" | "30" | "90")) => format!("On the previous {days} days"),
        _ => "On the previous period".to_owned(),
    }
}

/// The short name of a range on its button.
fn range_label(range: &str) -> String {
    match range {
        "month" => "MTD".to_owned(),
        "year" => "YTD".to_owned(),
        days => format!("{days}d"),
    }
}

/// The card of `result`. `range_link` gives the address of the same page with
/// another range, for a card whose value has one.
pub(super) fn card(result: &MetricResult, range_link: Option<RangeLink<'_>>) -> Node {
    let label = result.label.clone();
    let ranges = match (result.range, range_link) {
        (Some(current), Some(link)) if !result.ranges.is_empty() => Some(
            el("nav")
                .attr("aria-label", format!("Range of {label}"))
                .child(
                    Row::gap(1).children(
                        result
                            .ranges
                            .iter()
                            .map(|range| {
                                let here = range.value == current;
                                let button = Button::new(range_label(range.value))
                                    .variant(if here {
                                        ButtonVariant::Outline
                                    } else {
                                        ButtonVariant::Ghost
                                    })
                                    .size(Size::Xs)
                                    .href(link(range.value))
                                    .attr("title", range.label)
                                    .attr("aria-label", range.label);
                                if here {
                                    button.attr("aria-current", "true").into()
                                } else {
                                    button.into()
                                }
                            })
                            .collect::<Vec<Node>>(),
                    ),
                ),
        ),
        _ => None,
    };
    let body: Node = match result.kind {
        "trend" => {
            let stat = Stat::new(label.clone(), result.display.clone());
            if result.points.is_empty() {
                stat.into()
            } else {
                let first = &result.points[0];
                let last = &result.points[result.points.len() - 1];
                let summary = format!(
                    "{label}: from {} ({}) to {} ({})",
                    first.label, first.display, last.label, last.display
                );
                stat.chart(
                    Sparkline::new(summary, result.points.iter().map(|point| point.value))
                        .names(result.points.iter().map(|point| point.label.clone()))
                        .area(),
                )
                .into()
            }
        }
        "partition" => {
            let top = result
                .groups
                .iter()
                .map(|group| group.value)
                .fold(0.0_f64, f64::max);
            let rows: Vec<Node> = result
                .groups
                .iter()
                .map(|group| {
                    let percent = if top > 0.0 {
                        (group.value / top * 100.0).round().clamp(0.0, 100.0) as u32
                    } else {
                        0
                    };
                    Stack::gap(1)
                        .children((
                            Row::gap(2).justify(Justify::Between).children((
                                Text::new(group.label.clone()).inline(),
                                Text::muted(group.display.clone()).inline(),
                            )),
                            Progress::new(
                                percent,
                                100,
                                format!("{}: {}", group.label, group.display),
                            ),
                        ))
                        .into()
                })
                .collect();
            if rows.is_empty() {
                Empty::new("No data yet").into()
            } else {
                Stack::gap(3)
                    .children((Text::muted(label.clone()), rows))
                    .into()
            }
        }
        _ => {
            let mut stat = Stat::new(label.clone(), result.display.clone());
            if let Some(change) = result.change {
                stat = stat.trend(change_text(change), change >= 0.0);
            }
            Stack::gap(1)
                .children((
                    stat,
                    result
                        .change
                        .map(|_| Text::muted(change_note(result.range))),
                ))
                .into()
        }
    };
    let mut card = Card::new()
        .attr("role", "group")
        .attr("aria-label", label)
        .attr("data-metric", result.kind)
        .body(body);
    if let Some(ranges) = ranges {
        card = card.action(ranges);
    }
    card.into()
}

/// The cards of `resource`'s metrics (only those on the dashboard when
/// `dashboard_only`); a failed metric is logged and left out.
pub(super) async fn cards(
    cx: &Cx,
    resource: &dyn ErasedResource,
    range: crate::MetricRange,
    dashboard_only: bool,
    range_link: Option<RangeLink<'_>>,
) -> Vec<Node> {
    let mut cards = Vec::new();
    let info = resource.info();
    let readable = info.readable(cx.ability());
    for metric in &info.metrics {
        if (dashboard_only && !metric.on_dashboard) || !readable.metric(metric) {
            continue;
        }
        match resource.metric(cx, &metric.slug, range).await {
            Ok(result) => cards.push(card(&result, range_link)),
            Err(error) => tracing::warn!(
                target: "navree::helm",
                metric = %metric.slug,
                status = error.status.as_u16(),
                "a metric failed"
            ),
        }
    }
    cards
}

/// Cards side by side: three to a row on a wide screen, one on a phone.
pub(super) fn grid(cards: Vec<Node>) -> Node {
    Grid::cols(3).gap(4).children(cards).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_read_as_percents() {
        assert_eq!(change_text(0.124), "+12.4%");
        assert_eq!(change_text(-0.5), "-50.0%");
        assert_eq!(change_text(2.5), "+250%");
        assert_eq!(change_note(Some("7")), "On the previous 7 days");
        assert_eq!(change_note(Some("month")), "On the previous period");
        assert_eq!(range_label("30"), "30d");
        assert_eq!(range_label("year"), "YTD");
    }
}
