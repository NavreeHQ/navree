//! Axes and gridlines: what each axis draws ([`Axis`]), the lines behind the marks
//! ([`Gridlines`]), which labels an axis keeps ([`Ticks`]), and the range of the value axis
//! ([`Domain`]).
//!
//! The defaults are the chart as it always was: value labels at the side, x labels
//! under the plot, solid hairlines across at each value tick, no axis lines and no tick
//! marks, a value axis that starts at zero. Each option turns one thing on or off, so a
//! bare hero chart is `.x_axis(Axis::hidden()).y_axis(Axis::hidden()).grid(Gridlines::none())`.

use super::value::IntoValue;

/// What one axis draws: its labels (on by default), its line and its tick marks (off by
/// default).
///
/// ```
/// use navree_ui::charts::{Axis, Chart, Series};
///
/// let chart = Chart::line("Heart rate")
///     .categories(["06:00", "12:00", "18:00"])
///     .series(Series::new("Heart rate", [62, 88, 74]))
///     .y_axis(Axis::hidden())
///     .x_axis(Axis::new().line().ticks());
/// # let _ = navree_ui::render(&chart.into());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub struct Axis {
    pub(crate) labels: bool,
    pub(crate) line: bool,
    pub(crate) ticks: bool,
}

impl Default for Axis {
    fn default() -> Self {
        Self::new()
    }
}

impl Axis {
    /// Labels, and no line or tick marks: the default.
    pub const fn new() -> Self {
        Self {
            labels: true,
            line: false,
            ticks: false,
        }
    }

    /// Nothing at all: no labels, no line, no tick marks. The plot takes the room.
    pub const fn hidden() -> Self {
        Self {
            labels: false,
            line: false,
            ticks: false,
        }
    }

    /// Draws the axis line along the plot's edge.
    pub const fn line(mut self) -> Self {
        self.line = true;
        self
    }

    /// Draws a short tick mark at each label.
    pub const fn ticks(mut self) -> Self {
        self.ticks = true;
        self
    }

    /// Leaves the labels out, keeping the line and tick marks if any.
    pub const fn no_labels(mut self) -> Self {
        self.labels = false;
        self
    }

    /// Whether it draws anything.
    pub(crate) const fn shows(self) -> bool {
        self.labels || self.line || self.ticks
    }
}

/// The gridlines behind a chart's marks: across at the value ticks (the default),
/// down at the x labels, both or none; solid hairlines, or dashed.
///
/// ```
/// use navree_ui::charts::Gridlines;
///
/// let quiet = Gridlines::horizontal().dashed();
/// let none = Gridlines::none();
/// let full = Gridlines::both().dashed();
/// # let _ = (quiet, none, full);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub struct Gridlines {
    pub(crate) across: bool,
    pub(crate) down: bool,
    pub(crate) dashed: bool,
}

impl Default for Gridlines {
    fn default() -> Self {
        Self::horizontal()
    }
}

impl Gridlines {
    /// No gridlines. A zero line is still drawn where values cross zero.
    pub const fn none() -> Self {
        Self {
            across: false,
            down: false,
            dashed: false,
        }
    }

    /// Lines across the plot at each value tick: the default.
    pub const fn horizontal() -> Self {
        Self {
            across: true,
            down: false,
            dashed: false,
        }
    }

    /// Lines down the plot at each x label.
    pub const fn vertical() -> Self {
        Self {
            across: false,
            down: true,
            dashed: false,
        }
    }

    /// Lines both ways.
    pub const fn both() -> Self {
        Self {
            across: true,
            down: true,
            dashed: false,
        }
    }

    /// Draws the lines dashed (3 on, 3 off).
    pub const fn dashed(mut self) -> Self {
        self.dashed = true;
        self
    }
}

/// Which labels an axis keeps: all that fit (the default, thinned on a narrow chart),
/// every n-th, or at most n, with or without the first and the last.
///
/// ```
/// use navree_ui::charts::Ticks;
///
/// // Every third hour, and always the first and the last.
/// let hours = Ticks::every(3).keep_ends();
/// // At most five labels, spread evenly.
/// let few = Ticks::most(5);
/// # let _ = (hours, few);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct Ticks {
    pub(crate) every: Option<usize>,
    pub(crate) most: Option<usize>,
    pub(crate) ends: bool,
}

impl Ticks {
    /// As many as fit: the chart picks, and a narrow chart drops every other one.
    pub const fn auto() -> Self {
        Self {
            every: None,
            most: None,
            ends: false,
        }
    }

    /// Every `n`-th label, from the first (`every(3)`: the 1st, 4th, 7th...).
    pub const fn every(n: usize) -> Self {
        Self {
            every: Some(if n == 0 { 1 } else { n }),
            most: None,
            ends: false,
        }
    }

    /// At most `n` labels (2 or more), spread evenly from the first.
    pub const fn most(n: usize) -> Self {
        Self {
            every: None,
            most: Some(if n < 2 { 2 } else { n }),
            ends: false,
        }
    }

    /// Always writes the first and the last label, dropping a label too close to them.
    pub const fn keep_ends(mut self) -> Self {
        self.ends = true;
        self
    }

    /// Whether the chart picks.
    pub(crate) const fn is_auto(self) -> bool {
        self.every.is_none() && self.most.is_none() && !self.ends
    }

    /// The indices of `count` labels to keep, in order, `auto` being the step when the
    /// chart picks.
    pub(crate) fn pick(self, count: usize, auto: usize) -> Vec<usize> {
        if count == 0 {
            return Vec::new();
        }
        let step = match (self.every, self.most) {
            (Some(every), _) => every.max(1),
            (None, Some(most)) => count
                .div_ceil(most.max(2) - usize::from(self.ends && most > 2))
                .max(1),
            (None, None) => auto.max(1),
        };
        let mut out: Vec<usize> = (0..count).step_by(step).collect();
        if self.ends && count > 1 {
            let last = count - 1;
            // A label too close to the last makes room for it: closer than half a step.
            while out
                .last()
                .is_some_and(|i| *i != last && last - *i < step.div_ceil(2))
            {
                out.pop();
            }
            if out.last() != Some(&last) {
                out.push(last);
            }
        }
        out
    }
}

/// The range of the value axis.
///
/// ```
/// use navree_ui::charts::Domain;
///
/// let hr = Domain::padded(10);          // lowest - 10 to highest + 10
/// let trend = Domain::data();           // lowest to highest, not from zero
/// let score = Domain::fixed(0, 10);     // 0 to 10, ticks inside it
/// let roomy = Domain::relative(0.1);    // a tenth of the spread above and below
/// # let _ = (hr, trend, score, roomy);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[must_use]
pub struct Domain {
    pub(crate) kind: DomainKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum DomainKind {
    /// From zero (bars, areas and stacks always; lines unless `y_zero(false)`), to nice
    /// numbers past the data.
    #[default]
    Auto,
    /// The data's own lowest and highest, widened to nice numbers.
    Data,
    /// The data's lowest and highest, widened by this much each way.
    Padded(f64),
    /// The data's lowest and highest, widened by this share of their spread each way.
    Relative(f64),
    /// Exactly from and to these, widened only if a value falls outside.
    Fixed(f64, f64),
}

impl Domain {
    /// The chart decides: from zero for bars, areas and stacks, and for lines unless
    /// told otherwise; ends on nice numbers.
    pub const fn auto() -> Self {
        Self {
            kind: DomainKind::Auto,
        }
    }

    /// From the lowest value to the highest, not from zero; ends on nice numbers.
    pub const fn data() -> Self {
        Self {
            kind: DomainKind::Data,
        }
    }

    /// From the lowest value less `by` to the highest plus `by`, exactly, with the ticks
    /// inside it.
    pub fn padded(by: impl IntoValue) -> Self {
        Self {
            kind: DomainKind::Padded(by.into_value().unwrap_or(0.0).abs()),
        }
    }

    /// From the lowest value to the highest, with `share` (0.1 is a tenth) of their
    /// spread added each way, exactly, with the ticks inside it.
    pub fn relative(share: f64) -> Self {
        let share = if share.is_finite() {
            share.abs().min(10.0)
        } else {
            0.0
        };
        Self {
            kind: DomainKind::Relative(share),
        }
    }

    /// From `from` to `to` exactly, with the ticks inside it. A value outside widens it,
    /// so a mark is never cut off.
    pub fn fixed(from: impl IntoValue, to: impl IntoValue) -> Self {
        let a = from.into_value().unwrap_or(0.0);
        let b = to.into_value().unwrap_or(1.0);
        Self {
            kind: DomainKind::Fixed(a.min(b), a.max(b)),
        }
    }

    /// The range for data from `lo` to `hi`, and whether it must hold zero: `None` when
    /// the chart's own rules decide.
    pub(crate) fn range(self, lo: f64, hi: f64) -> Option<(f64, f64)> {
        match self.kind {
            DomainKind::Auto => None,
            DomainKind::Data => Some((lo, hi)),
            DomainKind::Padded(by) => Some((lo - by, hi + by)),
            DomainKind::Relative(share) => {
                let spread = (hi - lo).abs().max(hi.abs().max(lo.abs()) * 0.01);
                Some((lo - spread * share, hi + spread * share))
            }
            DomainKind::Fixed(a, b) => Some((a.min(lo), b.max(hi))),
        }
    }

    /// Whether the range is kept as it is, with the ticks inside it, rather than widened
    /// to nice numbers past it.
    pub(crate) fn is_exact(self) -> bool {
        matches!(
            self.kind,
            DomainKind::Fixed(..) | DomainKind::Padded(_) | DomainKind::Relative(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_keep_what_they_say() {
        assert_eq!(Ticks::auto().pick(10, 2), [0, 2, 4, 6, 8]);
        assert_eq!(Ticks::every(3).pick(10, 1), [0, 3, 6, 9]);
        assert_eq!(Ticks::every(3).keep_ends().pick(9, 1), [0, 3, 6, 8]);
        // A label too close to the last gives way to it.
        assert_eq!(Ticks::every(4).keep_ends().pick(10, 1), [0, 4, 9]);
        assert_eq!(Ticks::most(4).pick(24, 1).len(), 4);
        let ends = Ticks::most(4).keep_ends().pick(24, 1);
        assert!(ends.len() <= 4, "{ends:?}");
        assert_eq!((ends[0], *ends.last().unwrap()), (0, 23));
        assert_eq!(Ticks::every(0).pick(3, 1), [0, 1, 2]);
        assert_eq!(Ticks::most(1).keep_ends().pick(1, 1), [0]);
        assert!(Ticks::auto().pick(0, 1).is_empty());
        assert!(Ticks::auto().is_auto() && !Ticks::every(2).is_auto());
    }

    #[test]
    fn domains_widen_the_data() {
        assert_eq!(Domain::auto().range(3.0, 9.0), None);
        assert_eq!(Domain::data().range(3.0, 9.0), Some((3.0, 9.0)));
        assert_eq!(Domain::padded(10).range(55.0, 120.0), Some((45.0, 130.0)));
        assert_eq!(Domain::padded(-2).range(0.0, 1.0), Some((-2.0, 3.0)));
        assert_eq!(Domain::relative(0.5).range(0.0, 10.0), Some((-5.0, 15.0)));
        // One value: a share of the value itself.
        let (lo, hi) = Domain::relative(1.0).range(50.0, 50.0).unwrap();
        assert!(lo < 50.0 && hi > 50.0);
        assert_eq!(Domain::fixed(10, 0).range(2.0, 8.0), Some((0.0, 10.0)));
        assert_eq!(Domain::fixed(0, 10).range(-1.0, 12.0), Some((-1.0, 12.0)));
        assert!(Domain::fixed(0, 1).is_exact() && Domain::padded(1).is_exact());
        assert!(!Domain::data().is_exact() && !Domain::auto().is_exact());
        assert_eq!(Domain::relative(f64::NAN).range(0.0, 1.0), Some((0.0, 1.0)));
    }

    #[test]
    fn axes_and_grids_say_what_they_draw() {
        assert!(Axis::new().shows() && !Axis::hidden().shows());
        assert!(Axis::hidden().line().shows());
        assert!(!Axis::new().no_labels().labels);
        assert_eq!(Gridlines::default(), Gridlines::horizontal());
        assert!(Gridlines::both().dashed().dashed && Gridlines::both().down);
    }
}
