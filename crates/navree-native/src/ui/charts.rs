//! Builders for the charts and for the parts a chart is made of: a hint, a
//! zone, a reference line, what an axis measures.

use super::IntoNumber;
use crate::charts::{
    CalendarHeatmap, Category, Centre, ColorStrip, Colour, Curve, DayClock, HeatDay, Hint,
    HintMark, Hour, Hue, MatrixHeatmap, Measure, Meter, MiniColumn, MiniColumns, Point,
    PointSeries, RangeScale, Reference, ReferenceKind, Relation, Ring, RingValue, Rings,
    RingsCentre, Scatter, Segment, Stacking, StripCell, Zone,
};
use crate::node::{Badge, Chart, Direction, Figure, Gauge, Icon, Tone, Trend};

impl Hint {
    /// A note about `about` (the name of the thing it explains: `the sleep
    /// score`) that says `text`, opened from a question mark.
    pub fn about(about: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            about: about.into(),
            text: text.into(),
            ..Self::default()
        }
    }

    /// A heading for the note.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// It is opened from an "i", for more about the thing, and not from a
    /// question mark.
    #[must_use]
    pub fn info(mut self) -> Self {
        self.mark = HintMark::Info;
        self
    }
}

impl Zone {
    /// A stretch of a scale from `from` to `to`, in the scale's own units.
    pub fn new(from: impl IntoNumber, to: impl IntoNumber) -> Self {
        Self {
            from: from.into_number(),
            to: to.into_number(),
            ..Self::default()
        }
    }

    /// What the zone is: `Optimal`.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Its colour.
    #[must_use]
    pub fn colour(mut self, colour: impl Into<Colour>) -> Self {
        self.colour = Some(colour.into());
        self
    }
}

impl Reference {
    /// A line across a chart at the level `value`, named `label`: a goal, an
    /// average.
    pub fn level(value: impl IntoNumber, label: impl Into<String>) -> Self {
        Self {
            kind: ReferenceKind::Level,
            value: Some(value.into_number()),
            label: Some(label.into()),
            colour: None,
        }
    }

    /// The line where the two axes are equal, named `label`.
    pub fn diagonal(label: impl Into<String>) -> Self {
        Self {
            kind: ReferenceKind::Diagonal,
            value: None,
            label: Some(label.into()),
            colour: None,
        }
    }

    /// Its colour.
    #[must_use]
    pub fn colour(mut self, colour: impl Into<Colour>) -> Self {
        self.colour = Some(colour.into());
        self
    }
}

impl Measure {
    /// What an axis measures: its name.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ..Self::default()
        }
    }

    /// What it counts in: `bpm`, `h`.
    #[must_use]
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// The ends of the axis, when they are not the data's own.
    #[must_use]
    pub fn between(mut self, min: impl IntoNumber, max: impl IntoNumber) -> Self {
        self.min = Some(min.into_number());
        self.max = Some(max.into_number());
        self
    }
}

impl Category {
    /// A name of a key, in its hue.
    pub fn new(name: impl Into<String>, hue: Hue) -> Self {
        Self {
            name: name.into(),
            colour: Some(Colour::new(hue)),
            detail: None,
        }
    }

    /// A few words about it, said beside its name in the key: its share or
    /// its total, as the screen writes it (`56% of the night`).
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

impl Chart {
    /// What the labels are along the axis they stand on.
    #[must_use]
    pub fn x(mut self, measure: Measure) -> Self {
        self.x = Some(measure);
        self
    }

    /// What the value axis measures: its unit and its ends.
    #[must_use]
    pub fn y(mut self, measure: Measure) -> Self {
        self.y = Some(measure);
        self
    }

    /// Adds a line across the chart.
    #[must_use]
    pub fn reference(mut self, line: Reference) -> Self {
        self.references.push(line);
        self
    }

    /// Adds a band of the value axis: a target, a usual range.
    #[must_use]
    pub fn zone(mut self, zone: Zone) -> Self {
        self.zones.push(zone);
        self
    }

    /// The names of the values of the value axis, from value 0: the stages of
    /// sleep.
    #[must_use]
    pub fn levels<L: Into<String>>(mut self, names: impl IntoIterator<Item = L>) -> Self {
        self.levels = names.into_iter().map(Into::into).collect();
        self
    }

    /// How a line or an area runs from one value to the next.
    #[must_use]
    pub fn curve(mut self, curve: Curve) -> Self {
        self.curve = curve;
        self
    }

    /// How the series of a bar or area chart sit on each other.
    #[must_use]
    pub fn stacked(mut self, stacking: Stacking) -> Self {
        self.stacking = stacking;
        self
    }

    /// Its bars lie along rows, each with its label.
    #[must_use]
    pub fn horizontal(mut self) -> Self {
        self.horizontal = true;
        self
    }

    /// What the values of the series added last count, when it is not what
    /// the value axis counts.
    #[must_use]
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        if let Some(series) = self.series.last_mut() {
            series.unit = Some(unit.into());
        }
        self
    }

    /// Adds a series named `label` in which a label may have no value: a gap.
    #[must_use]
    pub fn series_with_gaps<V: IntoNumber>(
        mut self,
        label: impl Into<String>,
        values: impl IntoIterator<Item = Option<V>>,
    ) -> Self {
        self.series.push(crate::node::Series {
            label: label.into(),
            values: values
                .into_iter()
                .map(|value| value.map(IntoNumber::into_number))
                .collect(),
            ..crate::node::Series::default()
        });
        self
    }

    /// A note about what the chart means, shown when asked.
    #[must_use]
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }
}

impl Gauge {
    /// `value` on a scale from `min` to `max`.
    pub fn of(value: impl IntoNumber, min: impl IntoNumber, max: impl IntoNumber) -> Self {
        Self {
            value: value.into_number(),
            min: min.into_number(),
            max: max.into_number(),
            ..Self::default()
        }
    }

    /// What it measures.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The value in the screen's own words: `1.12`, `72%`.
    #[must_use]
    pub fn shown(mut self, shown: impl Into<String>) -> Self {
        self.shown = Some(shown.into());
        self
    }

    /// A quieter line after the label.
    #[must_use]
    pub fn caption(mut self, caption: impl Into<String>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    /// Adds a zone of the scale, with its name and its colour. The value is
    /// in the last zone that holds it, and a client says that zone's name.
    #[must_use]
    pub fn zone(mut self, zone: Zone) -> Self {
        self.zones.push(zone);
        self
    }

    /// What its colour means, when it has no zones to take it from.
    #[must_use]
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = Some(tone);
        self
    }

    /// Its colour by hue, when it has no zones to take it from.
    #[must_use]
    pub fn colour(mut self, colour: impl Into<Colour>) -> Self {
        self.colour = Some(colour.into());
        self
    }

    /// One sentence that says the whole gauge to a reader who can't see it.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// A note about what the gauge means, shown when asked.
    #[must_use]
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }
}

impl Figure {
    /// What its colour means.
    #[must_use]
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = Some(tone);
        self
    }

    /// A note about what the figure means, shown when asked.
    #[must_use]
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }
}

impl Ring {
    /// `icon` stands in its middle in place of the value.
    #[must_use]
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self.centre = Centre::Icon;
        self
    }

    /// Nothing stands in its middle.
    #[must_use]
    pub fn bare(mut self) -> Self {
        self.centre = Centre::Nothing;
        self
    }

    /// How the value stands, in a word with its tone: `Good`.
    #[must_use]
    pub fn status(mut self, status: Badge) -> Self {
        self.status = Some(status);
        self
    }

    /// Which way the value is going, by how much in the screen's words
    /// (`4% vs your last 7 days`), and whether that is good news.
    #[must_use]
    pub fn trend(mut self, direction: Direction, text: impl Into<String>, good: bool) -> Self {
        let trend = Trend::new(direction, text);
        self.trend = Some(if good { trend.good() } else { trend.bad() });
        self
    }

    /// Which way the value is going, as a [`Trend`] built on its own: one
    /// that is neither good nor bad news.
    #[must_use]
    pub fn trend_of(mut self, trend: Trend) -> Self {
        self.trend = Some(trend);
        self
    }
}

impl RingValue {
    /// One ring of rings inside each other: `value` against `max`. It is
    /// written as the number is until [`shown`](Self::shown) says how the
    /// screen writes it.
    pub fn new(name: impl Into<String>, value: impl IntoNumber, max: impl IntoNumber) -> Self {
        let value = value.into_number();
        Self {
            name: name.into(),
            shown: Some(value.to_string()),
            value: Some(value),
            max: max.into_number(),
            unit: None,
            colour: None,
        }
    }

    /// A ring with no reading: the client draws the empty ring and a dash.
    pub fn empty(name: impl Into<String>, max: impl IntoNumber) -> Self {
        Self {
            name: name.into(),
            shown: None,
            value: None,
            max: max.into_number(),
            unit: None,
            colour: None,
        }
    }

    /// What the value counts, written after it: `kcal`.
    #[must_use]
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// The value as the screen writes it, without its unit: `12,450`.
    #[must_use]
    pub fn shown(mut self, shown: impl Into<String>) -> Self {
        self.shown = Some(shown.into());
        self
    }

    /// Its colour.
    #[must_use]
    pub fn colour(mut self, colour: impl Into<Colour>) -> Self {
        self.colour = Some(colour.into());
        self
    }
}

impl Hour {
    /// An hour of a day's clock with no reading yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// What happened in the hour: its steps, its active minutes.
    #[must_use]
    pub fn value(mut self, value: impl IntoNumber) -> Self {
        self.value = Some(value.into_number());
        self
    }

    /// How strong the hour is, 0 to 1, when its value against the busiest
    /// hour's does not say it.
    #[must_use]
    pub fn level(mut self, level: f64) -> Self {
        self.level = Some(level.into_number());
        self
    }

    /// What the hour was: the place of one of the clock's key, from 0. An
    /// hour may have a kind and no value: an hour asleep.
    #[must_use]
    pub fn category(mut self, category: u32) -> Self {
        self.category = Some(category);
        self
    }
}

impl Rings {
    /// Adds a ring built on its own, the first outside: one whose value the
    /// screen writes its own way, or one with no reading.
    #[must_use]
    pub fn value(mut self, ring: RingValue) -> Self {
        self.rings.push(ring);
        self
    }

    /// `icon` stands in the middle.
    #[must_use]
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self.centre = RingsCentre::Icon;
        self
    }

    /// A note about what the rings mean, shown when asked.
    #[must_use]
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }
}

impl DayClock {
    /// Adds a kind of hour to the key, built on its own: one that says its
    /// total (`7 h`) beside its name.
    #[must_use]
    pub fn kind(mut self, category: Category) -> Self {
        self.key.push(category);
        self
    }

    /// The colour of the dial where an hour has no kind, and of its middle.
    #[must_use]
    pub fn colours(mut self, dial: impl Into<Colour>, hub: impl Into<Colour>) -> Self {
        self.colour = Some(dial.into());
        self.hub = Some(hub.into());
        self
    }

    /// The icon in its middle.
    #[must_use]
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The most an hour can hold, when it is not the busiest hour's value.
    #[must_use]
    pub fn most(mut self, max: impl IntoNumber) -> Self {
        self.max = Some(max.into_number());
        self
    }

    /// A note about what the clock means, shown when asked.
    #[must_use]
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }
}

impl Meter {
    /// One bar of segments that add up to the whole, none yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// What the bar shows.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds a segment: its name, its amount, the amount in the screen's own
    /// words with its unit (`1 h 23 min`), and its hue.
    #[must_use]
    pub fn segment(
        mut self,
        name: impl Into<String>,
        value: impl IntoNumber,
        shown: impl Into<String>,
        hue: Hue,
    ) -> Self {
        self.segments.push(Segment {
            name: name.into(),
            value: Some(value.into_number()),
            shown: Some(shown.into()),
            detail: None,
            colour: Some(Colour::new(hue)),
        });
        self
    }

    /// A line about the segment added last: `usually 1 h 10 min`.
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        if let Some(segment) = self.segments.last_mut() {
            segment.detail = Some(detail.into());
        }
        self
    }

    /// One sentence that says the whole bar to a reader who can't see it.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// A note about what the bar means, shown when asked.
    #[must_use]
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }
}

impl MiniColumns {
    /// Small columns, each split into the same parts, none yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// What the columns show.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds a part every column is split into, in its hue: the first is at
    /// the foot.
    #[must_use]
    pub fn part(mut self, name: impl Into<String>, hue: Hue) -> Self {
        self.parts.push(Category::new(name, hue));
        self
    }

    /// Adds a part built on its own: one that says its total or its share
    /// beside its name.
    #[must_use]
    pub fn kind(mut self, category: Category) -> Self {
        self.parts.push(category);
        self
    }

    /// Adds a column: its label, a value for each part, and its total in the
    /// screen's own words.
    #[must_use]
    pub fn column<V: IntoNumber>(
        mut self,
        label: impl Into<String>,
        values: impl IntoIterator<Item = V>,
        shown: impl Into<String>,
    ) -> Self {
        self.columns.push(MiniColumn {
            label: label.into(),
            values: values
                .into_iter()
                .map(|value| Some(value.into_number()))
                .collect(),
            shown: Some(shown.into()),
        });
        self
    }

    /// The column at this place stands out, and the others are drawn quieter.
    #[must_use]
    pub fn highlight(mut self, column: u32) -> Self {
        self.highlight = Some(column);
        self
    }

    /// One sentence that says all the columns to a reader who can't see them.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// A note about what the columns mean, shown when asked.
    #[must_use]
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }
}

impl ColorStrip {
    /// A row of cells, each in the colour of its kind, none yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// What the strip shows.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds a kind of cell to the key, in its hue: the first is place 0.
    #[must_use]
    pub fn key(mut self, name: impl Into<String>, hue: Hue) -> Self {
        self.key.push(Category::new(name, hue));
        self
    }

    /// Adds a kind of cell built on its own: one that says its share of the
    /// strip in the screen's own words (`56% of the night`), so no client
    /// works it out.
    #[must_use]
    pub fn kind(mut self, category: Category) -> Self {
        self.key.push(category);
        self
    }

    /// Adds a cell named `label` of the kind at the place `category` of the
    /// key; with none it has no reading and is drawn quiet.
    #[must_use]
    pub fn cell(mut self, label: impl Into<String>, category: Option<u32>) -> Self {
        self.cells.push(StripCell {
            label: label.into(),
            category,
            value: None,
        });
        self
    }

    /// Adds a cell that is a reading: its label, the band the reading is in
    /// (a place of the key) and the reading itself.
    #[must_use]
    pub fn reading(
        mut self,
        label: impl Into<String>,
        category: u32,
        value: impl IntoNumber,
    ) -> Self {
        self.cells.push(StripCell {
            label: label.into(),
            category: Some(category),
            value: Some(value.into_number()),
        });
        self
    }

    /// What the readings measure and what they count in.
    #[must_use]
    pub fn measured(mut self, label: impl Into<String>, unit: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self.unit = Some(unit.into());
        self
    }

    /// The key says each kind's share of the cells.
    #[must_use]
    pub fn shares(mut self) -> Self {
        self.shares = true;
        self
    }

    /// One sentence that says the whole strip to a reader who can't see it.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// A note about what the strip means, shown when asked.
    #[must_use]
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }
}

impl RangeScale {
    /// Where `value` sits on a scale from `min` to `max`.
    pub fn of(value: impl IntoNumber, min: impl IntoNumber, max: impl IntoNumber) -> Self {
        let value = value.into_number();
        Self {
            shown: Some(value.to_string()),
            value: Some(value),
            min: min.into_number(),
            max: max.into_number(),
            ..Self::default()
        }
    }

    /// A scale from `min` to `max` with no reading on it.
    pub fn empty(min: impl IntoNumber, max: impl IntoNumber) -> Self {
        Self {
            min: min.into_number(),
            max: max.into_number(),
            ..Self::default()
        }
    }

    /// What the scale shows.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// What the value counts in.
    #[must_use]
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// The value as the screen writes it, without its unit.
    #[must_use]
    pub fn shown(mut self, shown: impl Into<String>) -> Self {
        self.shown = Some(shown.into());
        self
    }

    /// Words under the two ends in place of their numbers: `Slower`, `Faster`.
    #[must_use]
    pub fn ends(mut self, low: impl Into<String>, high: impl Into<String>) -> Self {
        self.low = Some(low.into());
        self.high = Some(high.into());
        self
    }

    /// Adds a zone along the scale.
    #[must_use]
    pub fn zone(mut self, zone: Zone) -> Self {
        self.zones.push(zone);
        self
    }

    /// One sentence that says the whole scale to a reader who can't see it.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// A note about what the scale means, shown when asked.
    #[must_use]
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }
}

impl CalendarHeatmap {
    /// A calendar of days, each shaded by its value: a day (`YYYY-MM-DD`)
    /// with no value is a day with no data, and not a day of zero.
    pub fn of<D, V>(days: impl IntoIterator<Item = (D, Option<V>)>) -> Self
    where
        D: Into<String>,
        V: IntoNumber,
    {
        Self {
            days: days
                .into_iter()
                .map(|(date, value)| HeatDay {
                    date: date.into(),
                    value: value.map(IntoNumber::into_number),
                })
                .collect(),
            ..Self::default()
        }
    }

    /// What the calendar shows.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The first and the last day it draws (`YYYY-MM-DD`), when they are not
    /// the first and the last day it holds.
    #[must_use]
    pub fn between(mut self, first: impl Into<String>, last: impl Into<String>) -> Self {
        self.first = Some(first.into());
        self.last = Some(last.into());
        self
    }

    /// The hue its days are shaded in.
    #[must_use]
    pub fn colour(mut self, colour: impl Into<Colour>) -> Self {
        self.colour = Some(colour.into());
        self
    }

    /// Where the second, the third and the fourth shade begin. Left out, the
    /// shades are four equal parts from 0 to the highest value.
    #[must_use]
    pub fn steps(
        mut self,
        second: impl IntoNumber,
        third: impl IntoNumber,
        fourth: impl IntoNumber,
    ) -> Self {
        self.steps = vec![
            second.into_number(),
            third.into_number(),
            fourth.into_number(),
        ];
        self
    }

    /// What a day's value measures and what it counts in.
    #[must_use]
    pub fn measured(mut self, label: impl Into<String>, unit: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self.unit = Some(unit.into());
        self
    }

    /// One sentence that says the whole calendar to a reader who can't see it.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// A note about what the calendar means, shown when asked.
    #[must_use]
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }
}

impl MatrixHeatmap {
    /// Every one of `measures` against every other, no pair related yet.
    pub fn of<M: Into<String>>(measures: impl IntoIterator<Item = M>) -> Self {
        Self {
            measures: measures.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// What the matrix shows.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The measures at the places `a` and `b` relate by `value`: its sign
    /// says which way, its size how strongly.
    #[must_use]
    pub fn relation(mut self, a: u32, b: u32, value: impl IntoNumber) -> Self {
        self.relations.push(Relation {
            a,
            b,
            value: Some(value.into_number()),
            ..Relation::default()
        });
        self
    }

    /// The pair's relation may be chance: it is drawn neutral, with `note`
    /// to say why.
    #[must_use]
    pub fn chance(
        mut self,
        a: u32,
        b: u32,
        value: impl IntoNumber,
        note: impl Into<String>,
    ) -> Self {
        self.relations.push(Relation {
            a,
            b,
            value: Some(value.into_number()),
            significant: false,
            note: Some(note.into()),
        });
        self
    }

    /// The pair was never tested: its cell is almost blank.
    #[must_use]
    pub fn untested(mut self, a: u32, b: u32) -> Self {
        self.relations.push(Relation {
            a,
            b,
            value: None,
            ..Relation::default()
        });
        self
    }

    /// Only the half under the diagonal is drawn: the pairs are the same
    /// either way round.
    #[must_use]
    pub fn lower(mut self) -> Self {
        self.lower = true;
        self
    }

    /// The hue of a relation above zero and of one below it.
    #[must_use]
    pub fn colours(mut self, positive: impl Into<Colour>, negative: impl Into<Colour>) -> Self {
        self.positive = Some(positive.into());
        self.negative = Some(negative.into());
        self
    }

    /// The strength the deepest shade stands for.
    #[must_use]
    pub fn peak(mut self, peak: impl IntoNumber) -> Self {
        self.peak = Some(peak.into_number());
        self
    }

    /// What a relation's value is: `Correlation`.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// One sentence that says the whole matrix to a reader who can't see it.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// A note about what the matrix means, shown when asked.
    #[must_use]
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }
}

impl Scatter {
    /// Clouds of points on two numeric axes, none yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// What the plot shows.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds a cloud named `name`, a point for each pair, in its hue.
    #[must_use]
    pub fn series<X: IntoNumber, Y: IntoNumber>(
        mut self,
        name: impl Into<String>,
        points: impl IntoIterator<Item = (X, Y)>,
        hue: Hue,
    ) -> Self {
        self.series.push(PointSeries {
            name: name.into(),
            points: points
                .into_iter()
                .map(|(x, y)| Point {
                    x: x.into_number(),
                    y: y.into_number(),
                    label: None,
                })
                .collect(),
            colour: Some(Colour::new(hue)),
        });
        self
    }

    /// What the two axes measure.
    #[must_use]
    pub fn axes(mut self, x: Measure, y: Measure) -> Self {
        self.x = Some(x);
        self.y = Some(y);
        self
    }

    /// Both axes take one range and the plot is square, so the diagonal is
    /// the line of no change.
    #[must_use]
    pub fn equal_axes(mut self) -> Self {
        self.equal_axes = true;
        self
    }

    /// Adds a line across the plot.
    #[must_use]
    pub fn reference(mut self, line: Reference) -> Self {
        self.references.push(line);
        self
    }

    /// One sentence that says the whole plot to a reader who can't see it.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// A note about what the plot means, shown when asked.
    #[must_use]
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }
}
