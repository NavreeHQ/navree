//! Grids and points: [`CalendarHeatmap`] (a day per cell, weeks across), [`MatrixHeatmap`]
//! (one measure against another, such as a correlation matrix), [`RelationList`] (the same
//! pairs as a ranked list) and [`Scatter`] (a point per pair of values).
//!
//! ```
//! use chrono::NaiveDate;
//! use navree_ui::charts::*;
//! use navree_ui::render;
//!
//! let first = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
//! let calendar = CalendarHeatmap::new("Training load")
//!     .days((0..60).map(|d| (first + chrono::Duration::days(d), (d * 7 % 40) as f64)));
//! let html = render(&calendar.into());
//! assert!(html.contains("data-slot=\"heat-cell\""));
//! assert!(html.contains("<table"), "every chart carries its data as a table");
//! ```
//!
//! **What they have in common with the other charts.** The same figure: a title, a legend,
//! the drawing, a text summary and the data as a table behind a switch. The drawing is one
//! tab stop. Inside it the arrow keys move from cell to cell (or point to point), Home and
//! End go to the ends of a row, Page Up and Page Down to the ends of a column, and a
//! tooltip says what is under the pointer or the keys. All of it reads with no script: the
//! cells are in the page, and so is the table.
//!
//! **Colour.** Never written in a component. A heat map takes an [`Ink`] (a chart colour, a
//! tone or the accent) and steps it through the page's own surface with the stylesheet, one
//! ramp for the light palette and another for the dark one, so every theme gets a ramp that
//! is right for it. A day with no data is a hollow dashed cell, not the faintest step: no
//! data and zero are different things.
//!
//! **Words.** [`GridLabels`] holds every word these charts write, in English by default.
//! Months, weekdays and dates come from the chart's [locale](CalendarHeatmap::locale), and
//! numbers are grouped by it (`en-IN` writes lakh and crore).

mod calendar;
mod matrix;
mod relations;
mod scatter;

pub use calendar::{CalendarHeatmap, HeatSteps};
pub use matrix::{MatrixHeatmap, Relation};
pub use relations::{MAX_RELATIONS, RelationList};
pub use scatter::{PointSeries, Scatter};

use std::borrow::Cow;

use super::paint::Ink;
use crate::node::{Element, Node, Uses, el};

/// The words the grid charts write, in English by default. Templates name their blanks in
/// braces.
///
/// ```
/// use navree_ui::charts::{CalendarHeatmap, GridLabels};
///
/// let mut words = GridLabels::english();
/// words.less = "Moins".into();
/// words.more = "Plus".into();
/// let chart = CalendarHeatmap::new("Activité").words(words);
/// # let _ = navree_ui::render(&chart.into());
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct GridLabels {
    /// The low end of a calendar's legend.
    pub less: Cow<'static, str>,
    /// The high end of a calendar's legend.
    pub more: Cow<'static, str>,
    /// A day with nothing recorded.
    pub no_data: Cow<'static, str>,
    /// A pair whose link is not significant.
    pub not_significant: Cow<'static, str>,
    /// A pair never tested.
    pub untested: Cow<'static, str>,
    /// A link that moves the same way.
    pub positive: Cow<'static, str>,
    /// A link that moves the opposite way.
    pub negative: Cow<'static, str>,
    /// The first column of a calendar's table.
    pub date: Cow<'static, str>,
    /// A value column with no other name.
    pub value: Cow<'static, str>,
    /// The column of a matrix table that says what a pair came to.
    pub result: Cow<'static, str>,
    /// The column of a matrix table that holds a note.
    pub note: Cow<'static, str>,
    /// The first column of a matrix table: `{a}`, `{b}`.
    pub pair: Cow<'static, str>,
    /// The column of a scatter's table that names the series.
    pub series: Cow<'static, str>,
    /// The first column of a scatter's table: a point with no name of its own, `{n}`.
    pub point: Cow<'static, str>,
    /// A pair's name: `{a}`, `{b}`.
    pub pair_name: Cow<'static, str>,
    /// A calendar's summary: `{days}`, `{first}`, `{last}`, `{active}`, `{max}`, `{max_date}`,
    /// `{total}`.
    pub calendar_summary: Cow<'static, str>,
    /// Added to a calendar's summary when days have nothing recorded: `{missing}`.
    pub calendar_missing: Cow<'static, str>,
    /// Under a calendar that shows fewer days than it was given: `{shown}`, `{total}`.
    pub calendar_cut: Cow<'static, str>,
    /// A matrix's summary: `{metrics}`, `{pairs}`, `{tested}`, `{significant}`.
    pub matrix_summary: Cow<'static, str>,
    /// Added to a matrix's or a relation list's summary when a pair is significant:
    /// `{pair}`, `{value}`.
    pub matrix_strongest: Cow<'static, str>,
    /// A relation list's summary: `{pairs}`, `{significant}`.
    pub relations_summary: Cow<'static, str>,
    /// Under a relation list that shows fewer rows than it has pairs: `{shown}`, `{total}`.
    pub relations_cut: Cow<'static, str>,
    /// A scatter series' summary: `{series}`, `{count}`, `{x}`, `{x_min}`, `{x_max}`,
    /// `{x_mean}`, `{y}`, `{y_min}`, `{y_max}`, `{y_mean}`.
    pub scatter_summary: Cow<'static, str>,
    /// Added to it when the series has a correlation: `{r}`.
    pub scatter_correlation: Cow<'static, str>,
    /// Under a scatter that draws fewer points than it was given: `{shown}`, `{total}`.
    pub scatter_cut: Cow<'static, str>,
}

impl Default for GridLabels {
    fn default() -> Self {
        Self::english()
    }
}

impl GridLabels {
    /// The English words.
    pub const fn english() -> Self {
        Self {
            less: Cow::Borrowed("Less"),
            more: Cow::Borrowed("More"),
            no_data: Cow::Borrowed("No data"),
            not_significant: Cow::Borrowed("Not significant"),
            untested: Cow::Borrowed("Not tested"),
            positive: Cow::Borrowed("Positive"),
            negative: Cow::Borrowed("Negative"),
            date: Cow::Borrowed("Date"),
            value: Cow::Borrowed("Value"),
            result: Cow::Borrowed("Result"),
            note: Cow::Borrowed("Note"),
            pair: Cow::Borrowed("Pair"),
            series: Cow::Borrowed("Series"),
            point: Cow::Borrowed("Point {n}"),
            pair_name: Cow::Borrowed("{a} \u{2194} {b}"),
            calendar_summary: Cow::Borrowed(
                "{days} days from {first} to {last}, {active} with activity. Highest {max} on {max_date}, total {total}.",
            ),
            calendar_missing: Cow::Borrowed(" {missing} days have no data."),
            calendar_cut: Cow::Borrowed("Showing the latest {shown} of {total} days."),
            matrix_summary: Cow::Borrowed(
                "{metrics} measures, {pairs} pairs: {tested} tested, {significant} significant.",
            ),
            matrix_strongest: Cow::Borrowed(" Strongest: {pair}, {value}."),
            relations_summary: Cow::Borrowed("{pairs} pairs, {significant} significant."),
            relations_cut: Cow::Borrowed(
                "The strongest {shown} of {total} pairs are listed; the table has every one.",
            ),
            scatter_summary: Cow::Borrowed(
                "{series}: {count} points. {x} from {x_min} to {x_max}, average {x_mean}. {y} from {y_min} to {y_max}, average {y_mean}.",
            ),
            scatter_correlation: Cow::Borrowed(" Correlation {r}."),
            scatter_cut: Cow::Borrowed(
                "The drawing shows {shown} of {total} points; the table lists the points drawn.",
            ),
        }
    }
}

/// A legend: swatches and words in a row, for the eye only (the summary and the table say
/// the same in words). `ink` colours the swatches that have none of their own.
pub(crate) fn heat_legend(ink: Option<Ink>, items: Vec<Node>) -> Node {
    let list = el("ul").slot("heat-legend").attr("aria-hidden", "true");
    let list = match ink {
        Some(ink) => ink.on(list),
        None => list,
    };
    list.uses(Uses::CHART_GRIDS).child(items).into()
}

/// A swatch of the legend, painted like a cell with the same attributes.
pub(crate) fn swatch(cell: Element) -> Element {
    cell.slot("heat-key")
}

/// A count written with the chart marks: `7,000`, or `7,00,000` in `en-IN`.
pub(crate) fn count(n: usize, numbers: &super::NumberLocale) -> String {
    super::NumberFormat::integer().format(n as f64, numbers)
}
