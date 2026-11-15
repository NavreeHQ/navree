using Navree.Protocol;

namespace Navree.Design.Tests;

/// <summary>
/// What every drawn kind does with what a server may send (a range the wrong way round, nothing, one point, a number that is not a
/// number) and with the size it is given (a surface of one pixel by one): the rules that decide the numbers a kind draws with never
/// throw and always give a size that can be drawn.
/// </summary>
[TestClass]
public sealed class KindsSafetyTests
{
    private static readonly double[] Hostile = [double.NaN, double.PositiveInfinity, double.NegativeInfinity, -5, -0.0, 0, 0.4, 1, 2, 3.5, 17, 480, 1e6, 1e308, -1e308];

    private static void Sound(double value, string what) =>
        Assert.IsTrue(double.IsFinite(value) && value >= 0, what + " was " + value);

    private static Models.Chart Chart(Models.ChartKind kind, params double?[][] series) =>
        new()
        {
            Kind = kind,
            Series = [.. series.Select((values, at) => new Models.Series { Label = "s" + at, Values = values })],
        };

    // ---- the guard -------------------------------------------------------------------------------------

    [TestMethod]
    public void A_clamp_takes_its_ends_in_either_order_and_never_throws()
    {
        Assert.AreEqual(5, Guard.Clamp(9, 5, 1));
        Assert.AreEqual(1, Guard.Clamp(-9, 5, 1));
        Assert.AreEqual(3, Guard.Clamp(3, 5, 1));
        Assert.AreEqual(1, Guard.Clamp(double.NaN, 5, 1));
        Assert.AreEqual(4, Guard.Clamp(9, double.NaN, 4));
        Assert.AreEqual(0, Guard.Clamp(9, double.NaN, double.NaN));
        foreach (double a in Hostile.Where(end => !double.IsNaN(end)))
        {
            foreach (double b in Hostile.Where(end => !double.IsNaN(end)))
            {
                double found = Guard.Clamp(1, a, b);
                Assert.IsTrue(found >= Math.Min(a, b) && found <= Math.Max(a, b), a + " " + b + " gave " + found);
            }
        }
    }

    [TestMethod]
    public void A_length_is_never_below_nothing_and_always_a_number()
    {
        Assert.AreEqual(0, Guard.Length(-3));
        Assert.AreEqual(0, Guard.Length(double.NaN));
        Assert.AreEqual(0, Guard.Length(double.PositiveInfinity));
        Assert.AreEqual(7.5, Guard.Length(7.5));
        Assert.AreEqual(2, Guard.Place(9, 3));
        Assert.AreEqual(-1, Guard.Place(0, 0));
        Assert.AreEqual(0, Guard.Fraction(double.NaN));
        Assert.AreEqual(1, Guard.Fraction(7));
    }

    [TestMethod]
    public void A_text_wider_than_its_room_shrinks_by_what_does_not_fit_and_never_by_a_number_that_is_not_one()
    {
        Assert.AreEqual(1, TextFit.Shrink(100, 80, 0.5));
        Assert.AreEqual(0.5, TextFit.Shrink(50, 100, 0.4), 1e-9);
        Assert.AreEqual(0.4, TextFit.Shrink(-20, 100, 0.4), 1e-9);
        Assert.AreEqual(1, TextFit.Shrink(10, 0, 0.5));
        Assert.AreEqual(0.5, TextFit.Shrink(double.NaN, 10, 0.5));
        foreach (double room in Hostile)
        {
            foreach (double wide in Hostile)
            {
                double share = TextFit.Shrink(room, wide, 0.4);
                Assert.IsTrue(share is >= 0.4 and <= 1, room + " " + wide + " gave " + share);
            }
        }
    }

    // ---- charts: bars, lines, areas, steps, donuts, sparklines, lanes --------------------------------------

    [TestMethod]
    public void A_position_on_an_axis_is_a_place_between_nought_and_one_for_every_value_and_every_axis()
    {
        Assert.AreEqual(0.5, AxisRules.Position(5, 0, 10), 1e-9);
        Assert.AreEqual(0, AxisRules.Position(5, 10, 0));
        foreach (double value in Hostile)
        {
            foreach (double low in Hostile)
            {
                foreach (double high in Hostile)
                {
                    double place = AxisRules.Position(value, low, high);
                    Assert.IsTrue(place is >= 0 and <= 1, value + " in " + low + ".." + high + " gave " + place);
                }
            }
        }

        // Two ends too far apart to subtract.
        Assert.AreEqual(0.5, AxisRules.Position(0, -1e308, 1e308), 1e-9);
    }

    [TestMethod]
    public void The_ends_of_an_axis_are_numbers_for_data_that_is_not()
    {
        foreach (double low in Hostile)
        {
            foreach (double high in Hostile)
            {
                AxisRules.Bounds nice = AxisRules.Nice(low, high);
                Assert.IsTrue(double.IsFinite(nice.Low) && double.IsFinite(nice.High), low + " " + high);
                IReadOnlyList<double> marks = AxisRules.Marks(nice, 4);
                Assert.IsTrue(marks.Count >= 1 && marks.All(double.IsFinite), low + " " + high);
            }
        }

        foreach (double wanted in Hostile)
        {
            Assert.IsTrue(double.IsFinite(AxisRules.NiceStep(wanted)) && AxisRules.NiceStep(wanted) > 0, "step for " + wanted);
        }
    }

    [TestMethod]
    public void A_chart_with_a_reversed_range_nothing_one_point_or_a_number_that_is_not_one_has_axes_that_can_be_drawn()
    {
        Models.Chart[] charts =
        [
            Chart(Models.ChartKind.Line),
            Chart(Models.ChartKind.Bar, []),
            Chart(Models.ChartKind.Line, [3]),
            Chart(Models.ChartKind.Line, new double?[] { null }),
            Chart(Models.ChartKind.Bar, [double.NaN, 1, double.PositiveInfinity]),
            Chart(Models.ChartKind.Area, [1e308, -1e308]),
            Chart(Models.ChartKind.Line, [1, 2, 3]) with { Y = new Models.Measure { Min = 10, Max = 0 } },
            Chart(Models.ChartKind.Bar, [1, 2]) with { Y = new Models.Measure { Min = double.NaN, Max = double.NaN } },
            Chart(Models.ChartKind.Line, [1, 2]) with { Zones = [new Models.Zone { From = 9, To = 2 }], References = [new Models.Reference { Value = double.NaN }] },
            Chart(Models.ChartKind.Bar, [1, 2], [3, 4]) with { Stacking = Models.Stacking.Values },
            Chart(Models.ChartKind.Line, [0, 1, 2]) with { Levels = ["a", "b"] },
        ];
        foreach (Models.Chart chart in charts)
        {
            AxisRules.Bounds bounds = AxisRules.Values(chart);
            Assert.IsTrue(double.IsFinite(bounds.Low) && double.IsFinite(bounds.High));
            Assert.IsTrue(AxisRules.Marks(bounds, 4).All(double.IsFinite));
            Assert.IsTrue(AxisRules.Count(chart) >= 0);
            foreach (double value in Hostile)
            {
                double place = AxisRules.Position(value, bounds.Low, bounds.High);
                Assert.IsTrue(place is >= 0 and <= 1);
            }
        }
    }

    [TestMethod]
    public void The_axes_of_a_scatter_with_no_points_one_point_or_numbers_that_are_not_numbers_are_numbers()
    {
        Models.Scatter[] scatters =
        [
            new(),
            new() { Series = [new Models.PointSeries { Name = "a" }] },
            new() { Series = [new Models.PointSeries { Name = "a", Points = [new Models.Point { X = 1, Y = 1 }] }] },
            new() { Series = [new Models.PointSeries { Name = "a", Points = [new Models.Point { X = double.NaN, Y = 1e308 }, new Models.Point { X = -1e308, Y = double.PositiveInfinity }] }], EqualAxes = true },
            new() { X = new Models.Measure { Min = 9, Max = 1 }, Y = new Models.Measure { Min = double.NaN } },
        ];
        foreach (Models.Scatter scatter in scatters)
        {
            (AxisRules.Bounds x, AxisRules.Bounds y) = AxisRules.Scatter(scatter);
            Assert.IsTrue(double.IsFinite(x.Low) && double.IsFinite(x.High) && double.IsFinite(y.Low) && double.IsFinite(y.High));
        }
    }

    [TestMethod]
    public void A_smooth_line_through_no_points_one_point_or_the_same_place_twice_has_pieces_that_are_numbers()
    {
        Assert.AreEqual(0, Curves.Monotone([]).Count);
        Assert.AreEqual(0, Curves.Monotone([new Pt(1, 1)]).Count);
        IReadOnlyList<Bezier> same = Curves.Monotone([new Pt(1, 1), new Pt(1, 5), new Pt(1, 2)]);
        Assert.IsTrue(same.All(piece => double.IsFinite(piece.First.X) && double.IsFinite(piece.Second.Y) && double.IsFinite(piece.To.Y)));
        Assert.AreEqual(0, Curves.Steps([]).Count);
        Assert.AreEqual(0, Curves.StepsCentred([]).Count);
        Assert.AreEqual(1, Curves.StepsCentred([new Pt(1, 1)]).Count);
    }

    [TestMethod]
    public void The_labels_along_an_axis_are_thinned_for_a_width_of_nothing_and_a_count_of_nothing()
    {
        Assert.AreEqual(0, AxisRules.Ticks(0, 5).Count);
        Assert.AreEqual(0, AxisRules.Ticks(5, 0).Count);
        Assert.AreEqual(0, AxisRules.Ticks(-3, 5).Count);
        Assert.AreEqual(1, AxisRules.Most(0, 30));
        Assert.AreEqual(1, AxisRules.Most(double.NaN, 30));
        Assert.AreEqual(1, AxisRules.Most(-5, double.NaN));
        Assert.IsTrue(AxisRules.Ticks(1_000_000, 3).Count <= 1_000_000);
    }

    [TestMethod]
    public void A_ring_cut_into_slices_is_drawn_in_any_surface_and_a_surface_too_small_has_no_ring()
    {
        foreach (double side in Hostile)
        {
            DonutRules.Ring ring = DonutRules.Of(side, side);
            Sound(ring.Outer, "outer of " + side);
            Sound(ring.Inner, "inner of " + side);
            Sound(ring.Middle, "middle of " + side);
            Sound(ring.Width, "width of " + side);
        }

        Assert.IsFalse(DonutRules.Of(1, 1).Draws);
        Assert.IsFalse(DonutRules.Of(4, 100).Draws);
        Assert.IsTrue(DonutRules.Of(176, 176).Draws);
        Assert.AreEqual(86.0, DonutRules.Of(176, 176).Outer, 1e-9);
        Assert.AreEqual(86.0 * 0.62, DonutRules.Of(176, 176).Inner, 1e-9);
    }

    [TestMethod]
    public void The_slices_of_a_donut_of_nothing_one_value_or_values_that_are_not_numbers_are_numbers()
    {
        Assert.AreEqual(0, DonutRules.Slices([]).Count);
        Assert.AreEqual(0, DonutRules.Slices([0, 0]).Sum(slice => slice.Sweep));
        Assert.AreEqual(360, DonutRules.Slices([7]).Sum(slice => slice.Sweep), 1e-9);
        IReadOnlyList<DonutRules.Slice> messy = DonutRules.Slices([double.NaN, -3, 2, double.PositiveInfinity, 2]);
        Assert.AreEqual(360, messy.Sum(slice => slice.Sweep), 1e-9);
        Assert.IsTrue(messy.All(slice => double.IsFinite(slice.Start) && double.IsFinite(slice.Sweep)));
        DonutRules.Ring ring = DonutRules.Of(1, 1);
        foreach (double x in Hostile)
        {
            Assert.AreEqual(-1, DonutRules.SliceAt([1, 2], ring, x, x, 6, mirrored: false));
        }
    }

    // ---- bars: meters, strips, columns --------------------------------------------------------------------

    [TestMethod]
    public void Parts_of_a_bar_never_add_up_to_more_than_the_bar_even_when_there_are_more_parts_than_pixels()
    {
        foreach (double length in new[] { 0.0, 1, 5, 200, 480, double.NaN, -3 })
        {
            foreach (int parts in new[] { 0, 1, 2, 30, 300, 5000 })
            {
                double[] shares = [.. Enumerable.Repeat(1.0 / Math.Max(parts, 1), parts)];
                IReadOnlyList<(int Index, double Start, double Size)> slots = BarRules.Slots(length, shares, 2);
                double bar = Guard.Length(length);
                foreach ((int _, double start, double size) in slots)
                {
                    Assert.IsTrue(start >= -1e-9 && size >= 0 && start + size <= bar + 1e-6, length + " / " + parts + ": " + start + " + " + size);
                }
            }
        }

        // 300 parts on 200 pixels: each is under a pixel, the last ends on the bar's end.
        IReadOnlyList<(int Index, double Start, double Size)> many = BarRules.Slots(200, [.. Enumerable.Repeat(1.0 / 300, 300)], 2);
        Assert.AreEqual(300, many.Count);
        Assert.AreEqual(200, many[^1].Start + many[^1].Size, 1e-6);
    }

    [TestMethod]
    public void A_part_with_a_share_is_drawn_at_least_a_pixel_wide_and_a_part_with_none_is_not_drawn()
    {
        IReadOnlyList<(int Index, double Start, double Size)> slots = BarRules.Slots(300, [0.001, 0, 0.5, double.NaN, 0.499], 2);
        Assert.AreEqual(3, slots.Count);
        Assert.AreEqual(0, slots[0].Index);
        Assert.IsTrue(slots[0].Size >= 1);
        Assert.AreEqual(2, slots[1].Index);
        Assert.AreEqual(4, slots[2].Index);
        Assert.AreEqual(0, BarRules.Slots(300, [], 2).Count);
        Assert.AreEqual(0, BarRules.Slots(300, [0, 0], 2).Count);
    }

    [TestMethod]
    public void Many_columns_in_a_narrow_surface_get_a_width_that_is_not_below_nothing()
    {
        foreach (double total in Hostile)
        {
            foreach (int count in new[] { -1, 0, 1, 2, 7, 31, 1000 })
            {
                (double width, double gap) = BarRules.Columns(total, count, 10);
                Sound(width, "width for " + total + " / " + count);
                Sound(gap, "gap for " + total + " / " + count);
                if (count > 0)
                {
                    Assert.IsTrue((width * count) + (gap * (count - 1)) <= Guard.Length(total) + 1e-6);
                }
            }
        }

        Assert.AreEqual(10.0, BarRules.Columns(800, 7, 10).Gap);
        Assert.IsTrue(BarRules.Columns(300, 30, 10).Width >= 1);
    }

    // ---- rings, gauges, ranges, the day clock ------------------------------------------------------------

    [TestMethod]
    public void A_ring_goes_round_for_every_value_and_every_goal()
    {
        foreach (double value in Hostile)
        {
            foreach (double goal in Hostile)
            {
                RingRules.Sweep? sweep = RingRules.SweepOf(value, goal);
                if (sweep is null)
                {
                    Assert.IsTrue(double.IsNaN(value));
                    continue;
                }

                Assert.IsTrue(sweep.Laps is >= 0 and <= RingRules.MostLaps, value + " of " + goal + " gave " + sweep.Laps + " laps");
                Assert.IsTrue(sweep.Part is >= 0 and <= 1, value + " of " + goal + " gave part " + sweep.Part);
            }
        }

        Assert.IsNull(RingRules.SweepOf(double.NaN, 100));
        Assert.IsNull(RingRules.SweepOf(null, 100));
    }

    [TestMethod]
    public void The_radii_of_a_day_clock_are_never_below_nothing_and_a_dial_too_small_is_not_drawn()
    {
        foreach (double half in Hostile)
        {
            ClockRules.Dial dial = ClockRules.DialOf(half, 15, 22);
            Sound(dial.Labels, "labels for " + half);
            Sound(dial.Moments, "moments for " + half);
            Sound(dial.Outer, "outer for " + half);
            Sound(dial.Inner, "inner for " + half);
            Sound(ClockRules.BarWidth(dial.Inner), "bar width for " + half);
        }

        Assert.IsFalse(ClockRules.Draws(ClockRules.DialOf(0.5, 15, 22)));
        Assert.IsFalse(ClockRules.Draws(ClockRules.DialOf(40, 15, 22)));
        Assert.IsTrue(ClockRules.Draws(ClockRules.DialOf(150, 15, 22)));
    }

    [TestMethod]
    public void A_range_given_the_wrong_way_round_holds_its_value_and_its_marker_inside_it()
    {
        Assert.AreEqual((1.0, 9.0), RangeRules.Ends(9, 1));
        Assert.AreEqual((0.0, 0.0), RangeRules.Ends(double.NaN, double.NaN));
        Assert.AreEqual(5, RangeRules.Value(5, 9, 1));
        Assert.AreEqual(9, RangeRules.Value(50, 9, 1));
        Assert.AreEqual(1, RangeRules.Value(double.NaN, 9, 1));

        // A marker on a line narrower than itself stands in the middle; on a wider one it stays inside both ends.
        Assert.AreEqual(5, RangeRules.MarkerAt(0.3, 10, 18));
        Assert.AreEqual(9, RangeRules.MarkerAt(0, 100, 18));
        Assert.AreEqual(91, RangeRules.MarkerAt(1, 100, 18));
        Assert.AreEqual(50, RangeRules.MarkerAt(0.5, 100, 18));
        foreach (double width in Hostile)
        {
            foreach (double marker in Hostile)
            {
                double at = RangeRules.MarkerAt(0.5, width, marker);
                Assert.IsTrue(double.IsFinite(at) && at >= 0 && at <= Guard.Length(width) + 1e-9, width + " / " + marker + " gave " + at);
            }
        }
    }

    [TestMethod]
    public void A_zone_band_on_a_line_starts_and_ends_inside_it()
    {
        foreach (double width in Hostile)
        {
            foreach (double low in new[] { -1, 0, 0.3, 0.9, 2, double.NaN })
            {
                foreach (double high in new[] { -1, 0, 0.3, 0.9, 2, double.NaN })
                {
                    (double start, double size) = RangeRules.Band(low, high, width, 8);
                    Assert.IsTrue(start >= 0 && size >= 0 && start + size <= Guard.Length(width) + 1e-9, width + " " + low + ".." + high);
                }
            }
        }
    }

    // ---- heat maps ----------------------------------------------------------------------------------------

    [TestMethod]
    public void A_calendar_narrower_than_its_cells_is_made_smaller_and_a_calendar_of_very_many_weeks_shows_the_latest()
    {
        foreach (double width in Hostile)
        {
            foreach (int weeks in new[] { -1, 0, 1, 26, 53, 260 })
            {
                double step = HeatRules.Step(width, 17.6, weeks);
                Assert.IsTrue(step is >= 1 and <= 20, width + " / " + weeks + " gave " + step);
                Sound(HeatRules.Square(step), "square for " + step);
            }
        }

        Assert.AreEqual(20, HeatRules.Step(2000, 17.6, 10));
        Assert.AreEqual(1, HeatRules.Step(100, 17.6, 200));
        Assert.AreEqual(2.0, HeatRules.Square(4));
        Assert.AreEqual(4.0, HeatRules.Square(6));
        Assert.AreEqual(0.5, HeatRules.Square(1));

        DateOnly first = new(2000, 1, 3);
        DateOnly last = new(2026, 11, 13);
        (DateOnly start, int weeks2) = HeatRules.Window(first, last, DayOfWeek.Monday);
        Assert.AreEqual(HeatRules.MostWeeks, weeks2);
        Assert.AreEqual(DayOfWeek.Monday, start.DayOfWeek);
        Assert.AreEqual(0, HeatRules.Cell(last, start).Week - (weeks2 - 1));
    }

    [TestMethod]
    public void A_calendar_whose_days_run_backwards_or_stand_at_the_ends_of_time_has_weeks_that_can_be_drawn()
    {
        foreach (DayOfWeek weekday in Enum.GetValues<DayOfWeek>())
        {
            (DateOnly start, int weeks) = HeatRules.Window(DateOnly.MinValue, DateOnly.MinValue, weekday);
            Assert.AreEqual(1, weeks);
            Assert.IsTrue(start <= DateOnly.MinValue.AddDays(6));
            Assert.AreEqual(1, HeatRules.Window(new DateOnly(2026, 5, 5), new DateOnly(2020, 1, 1), weekday).Weeks);
            (DateOnly _, int atEnd) = HeatRules.Window(DateOnly.MaxValue, DateOnly.MaxValue, weekday);
            Assert.AreEqual(1, atEnd);
            Assert.AreEqual(HeatRules.MostWeeks, HeatRules.Window(DateOnly.MinValue, DateOnly.MaxValue, weekday).Weeks);
            Assert.IsTrue(HeatRules.Months(DateOnly.MaxValue, 5, weekday).Count >= 1);
        }
    }

    [TestMethod]
    public void A_matrix_of_many_measures_in_a_narrow_surface_has_cells_that_can_still_be_pointed_at()
    {
        foreach (double width in Hostile)
        {
            foreach (int columns in new[] { 0, 1, 7, 40 })
            {
                double left = MatrixRules.Left(300, width);
                Sound(left, "left");
                Assert.IsTrue(left <= Guard.Length(width) / 2 + 1e-9);
                double cell = MatrixRules.Cell(width, left, columns, 44);
                Assert.IsTrue(cell is >= 6 and <= 44, width + " / " + columns + " gave " + cell);
            }
        }

        Assert.AreEqual(44.0, MatrixRules.Cell(2000, 100, 6, 44));
        Assert.IsFalse(MatrixRules.HoldsNumber(10));
        Assert.IsTrue(MatrixRules.HoldsNumber(44));
    }

    // ---- composition and flags ----------------------------------------------------------------------------

    [TestMethod]
    public void A_size_the_page_gives_is_held_between_nothing_and_a_size_that_can_be_laid_out()
    {
        Assert.IsTrue(double.IsNaN(ComposeRules.Extent(null, true, 2)));
        Assert.AreEqual(0.0, ComposeRules.Extent(-40, false, 1));
        Assert.AreEqual(ComposeRules.MostExtent, ComposeRules.Extent(int.MaxValue, true, 2));
        Assert.AreEqual(30.0, ComposeRules.Extent(20, true, 1.5));
        Assert.AreEqual(20.0, ComposeRules.Extent(20, false, 1.5));
        Assert.AreEqual(20.0, ComposeRules.Extent(20, true, double.NaN));
        Assert.AreEqual(0.0, ComposeRules.Steps(-3));
        Assert.AreEqual(256.0, ComposeRules.Steps(int.MaxValue));
        Assert.AreEqual(12.0, ComposeRules.GridGap(-2, cards: true));
        Assert.AreEqual(24.0, ComposeRules.GridGap(-2, cards: false));
    }

    [TestMethod]
    public void A_grid_a_carousel_and_a_row_of_parts_never_get_a_width_below_nothing_whatever_the_page_asks()
    {
        foreach (double width in Hostile)
        {
            foreach (int? count in new int?[] { null, int.MinValue, -1, 0, 1, 3, int.MaxValue })
            {
                foreach (int? least in new int?[] { null, int.MinValue, -5, 0, 1, 160, int.MaxValue })
                {
                    int columns = ComposeRules.Columns(width, count, least, 12, 5);
                    Assert.IsTrue(columns is >= 1 and <= 5, width + " " + count + " " + least);
                    Sound(ComposeRules.ColumnWidth(width, columns, 12), "column width");
                    (int shown, double? each) = ComposeRules.Carousel(width, least, 12, 5);
                    Assert.IsTrue(shown >= 1);
                    Assert.IsTrue(each is null || (double.IsFinite(each.Value) && each.Value >= 0), width + " " + least + " gave " + each);
                }
            }
        }
    }

    [TestMethod]
    public void A_flag_of_any_height_is_drawn_at_a_height_that_can_be_drawn()
    {
        foreach (double height in Hostile)
        {
            double stand = FlagRules.Stand(height);
            Assert.IsTrue(stand is >= 1 and <= 512, height + " gave " + stand);
        }

        Assert.AreEqual(16.0, FlagRules.Stand(16));
        Assert.AreEqual(16.0, FlagRules.Stand(double.NaN));
    }

    // ---- a drawing of very many places ----------------------------------------------------------------------

    [TestMethod]
    public void A_drawing_of_very_many_places_keeps_a_spread_of_them_with_the_first_and_the_last()
    {
        Assert.AreEqual(0, ThinRules.Places(0, 800).Count);
        Assert.AreEqual(0, ThinRules.Places(-4, 800).Count);
        Assert.AreEqual(12, ThinRules.Places(12, 800).Count);
        Assert.IsTrue(ThinRules.Places(12, 800).SequenceEqual(Enumerable.Range(0, 12)));

        IReadOnlyList<int> kept = ThinRules.Places(100_000, 4000);
        Assert.AreEqual(ThinRules.MostMarks, kept.Count);
        Assert.AreEqual(0, kept[0]);
        Assert.AreEqual(99_999, kept[^1]);
        Assert.IsTrue(kept.Zip(kept.Skip(1), (a, b) => b > a).All(ordered => ordered));

        // More places than pixels are not drawn either.
        Assert.AreEqual(300, ThinRules.Places(1000, 300).Count);

        // A fixed cap, as a scatter keeps: all of a few, a spread of very many with both ends.
        Assert.IsTrue(ThinRules.Spread(10, 1500).SequenceEqual(Enumerable.Range(0, 10)));
        Assert.AreEqual(300, ThinRules.Spread(5000, 300).Count);
        Assert.AreEqual(4999, ThinRules.Spread(5000, 300)[^1]);
        Assert.IsTrue(ThinRules.Spread(5000, 0).SequenceEqual([0, 4999]));
        Assert.AreEqual(0, ThinRules.Spread(0, 300).Count);

        // A surface of nothing still keeps the two ends.
        Assert.IsTrue(ThinRules.Places(50, 0).SequenceEqual([0, 49]));
        Assert.IsTrue(ThinRules.Places(50, double.NaN).SequenceEqual([0, 49]));
    }
    // ---- when a drawing is made again ---------------------------------------------------------------------------

    [TestMethod]
    public void A_drawing_is_made_again_when_its_height_changes_as_well_as_when_its_width_does()
    {
        Assert.IsTrue(PlotRules.Repaints(400, 180, 400, 260, painted: true, forced: false), "taller, same width");
        Assert.IsTrue(PlotRules.Repaints(400, 180, 520, 180, painted: true, forced: false), "wider, same height");
        Assert.IsFalse(PlotRules.Repaints(400, 180, 400.3, 180.2, painted: true, forced: false), "a change nobody can see");
        Assert.IsFalse(PlotRules.Repaints(400, 180, 400, 180, painted: true, forced: false));
        Assert.IsTrue(PlotRules.Repaints(400, 180, 400, 180, painted: true, forced: true), "asked for");
        Assert.IsTrue(PlotRules.Repaints(400, 180, 400, 180, painted: false, forced: false), "nothing painted yet");
        Assert.IsTrue(PlotRules.Repaints(-1, -1, 400, 180, painted: false, forced: false));
        Assert.IsTrue(PlotRules.Repaints(400, 180, 400, double.NaN, painted: true, forced: false), "a size that is not a number is painted again, not skipped");
    }

    [TestMethod]
    public void A_slice_of_a_donut_is_read_out_wherever_the_pointer_is_on_its_arc_and_not_only_near_its_middle()
    {
        DonutRules.Ring ring = DonutRules.Of(176, 176);
        double middle = ring.Middle;
        double[] halves = [1, 1];

        // On the ring at three oclock and at nine: the right half is the first slice (it runs clockwise from twelve) and the left half the second.
        Assert.AreEqual(0, DonutRules.SliceAt(halves, ring, ring.Cx + middle, ring.Cy, 6, mirrored: false));
        Assert.AreEqual(1, DonutRules.SliceAt(halves, ring, ring.Cx - middle, ring.Cy, 6, mirrored: false));

        // At the boundary of the two, six oclock, about 100 pixels from both slices middles: a pixel either side of it is each ones.
        Assert.AreEqual(0, DonutRules.SliceAt(halves, ring, ring.Cx + 1, ring.Cy + middle, 6, mirrored: false));
        Assert.AreEqual(1, DonutRules.SliceAt(halves, ring, ring.Cx - 1, ring.Cy + middle, 6, mirrored: false));
        Assert.AreEqual(1, DonutRules.SliceAt(halves, ring, ring.Cx - 1, ring.Cy - middle, 6, mirrored: false));
        Assert.AreEqual(0, DonutRules.SliceAt(halves, ring, ring.Cx + 1, ring.Cy - middle, 6, mirrored: false));

        // Flipped across (right to left): the right half is the second slice.
        Assert.AreEqual(1, DonutRules.SliceAt(halves, ring, ring.Cx + middle, ring.Cy, 6, mirrored: true));
        Assert.AreEqual(0, DonutRules.SliceAt(halves, ring, ring.Cx - middle, ring.Cy, 6, mirrored: true));

        // A big slice is read out all along it: a quarter, a half and a quarter.
        double[] parts = [1, 2, 1];
        Assert.AreEqual(0, DonutRules.SliceAt(parts, ring, ring.Cx + middle, ring.Cy - 5, 6, mirrored: false));
        Assert.AreEqual(1, DonutRules.SliceAt(parts, ring, ring.Cx, ring.Cy + middle, 6, mirrored: false));
        Assert.AreEqual(1, DonutRules.SliceAt(parts, ring, ring.Cx + (middle * 0.5), ring.Cy + (middle * 0.87), 6, mirrored: false));
        Assert.AreEqual(2, DonutRules.SliceAt(parts, ring, ring.Cx - middle, ring.Cy - 5, 6, mirrored: false));

        // Not on the ring: the hole, past the rim.
        Assert.AreEqual(-1, DonutRules.SliceAt(halves, ring, ring.Cx, ring.Cy, 6, mirrored: false));
        Assert.AreEqual(-1, DonutRules.SliceAt(halves, ring, ring.Cx + ring.Outer + 20, ring.Cy, 6, mirrored: false));
        Assert.AreEqual(0, DonutRules.SliceAt(halves, ring, ring.Cx + ring.Outer + 3, ring.Cy, 6, mirrored: false), "a few pixels past the rim still counts");

        // Slices with nothing in them are never read out.
        Assert.AreEqual(1, DonutRules.SliceAt([0, 5], ring, ring.Cx + middle, ring.Cy, 6, mirrored: false));
    }

    [TestMethod]
    public void A_ring_past_its_goal_tells_assistive_technology_a_value_inside_its_range_and_says_past_goal_in_words()
    {
        Assert.AreEqual((0.0, 100.0, 100.0), RingRules.Range(150, 100));
        Assert.AreEqual((0.0, 100.0, 83.0), RingRules.Range(83, 100));
        Assert.AreEqual((0.0, 100.0, 0.0), RingRules.Range(-4, 100));
        Assert.IsNull(RingRules.Range(null, 100));
        Assert.IsNull(RingRules.Range(double.NaN, 100));
        Assert.IsNull(RingRules.Range(50, 0));
        Assert.IsNull(RingRules.Range(50, double.PositiveInfinity));

        Assert.IsTrue(RingRules.PastGoal(150, 100));
        Assert.IsFalse(RingRules.PastGoal(100, 100));
        Assert.IsFalse(RingRules.PastGoal(null, 100));
        Assert.IsFalse(RingRules.PastGoal(5, 0));
        Assert.AreEqual("Steps, 150 of 100, past goal", RingRules.Said("Steps, 150 of 100", 150, 100));
        Assert.AreEqual("Steps, 83 of 100", RingRules.Said("Steps, 83 of 100", 83, 100));
        foreach (double value in new[] { -3, 0, 50, 100, 101, 1e9, double.PositiveInfinity })
        {
            (double min, double max, double held)? range = RingRules.Range(value, 100);
            if (range is { } found)
            {
                Assert.IsTrue(found.held >= found.min && found.held <= found.max, value + " held at " + found.held);
            }
        }
    }
}
