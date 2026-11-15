using System.Globalization;
using Navree.Protocol;

namespace Navree.Design.Tests;

/// <summary>The rules of the rings, gauges, the day clock, the heat maps, the composition kinds and the flags.</summary>
[TestClass]
public sealed class KindsDialTests
{
    private static readonly CultureInfo Invariant = CultureInfo.InvariantCulture;

    // ---- rings -----------------------------------------------------------------------------------------

    [TestMethod]
    public void A_ring_goes_round_by_its_share_and_a_second_lap_starts_only_past_the_goal()
    {
        RingRules.Sweep? part = RingRules.SweepOf(83, 100);
        Assert.IsNotNull(part);
        Assert.AreEqual(0, part.Laps);
        Assert.AreEqual(0.83, part.Part, 1e-9);
        Assert.IsFalse(part.Past);

        RingRules.Sweep? exact = RingRules.SweepOf(100, 100);
        Assert.AreEqual(new RingRules.Sweep(0, 1), exact);
        Assert.IsFalse(exact!.Past);

        RingRules.Sweep? over = RingRules.SweepOf(150, 100);
        Assert.AreEqual(1, over!.Laps);
        Assert.AreEqual(0.5, over.Part, 1e-9);
        Assert.IsTrue(over.Past);

        Assert.AreEqual(new RingRules.Sweep(1, 1), RingRules.SweepOf(200, 100));
        Assert.IsNull(RingRules.SweepOf(null, 100));
        Assert.AreEqual(new RingRules.Sweep(0, 1), RingRules.SweepOf(5, 0));
        Assert.AreEqual(new RingRules.Sweep(0, 0), RingRules.SweepOf(-5, 100));
    }

    [TestMethod]
    public void A_ring_has_the_size_its_prominence_gives_and_a_small_one_has_no_words_in_it()
    {
        Assert.AreEqual(188.0, RingRules.Diameter(Models.Prominence.High));
        Assert.AreEqual(96.0, RingRules.Diameter(Models.Prominence.Medium));
        Assert.AreEqual(34.0, RingRules.Diameter(Models.Prominence.Low));
        Assert.AreEqual(188 * 0.115, RingRules.Stroke(Models.Prominence.High, 188), 1e-9);
        Assert.IsTrue(RingRules.Stroke(Models.Prominence.Low, 34) / 34 > RingRules.Stroke(Models.Prominence.Medium, 96) / 96);
        Assert.AreEqual(1.0, RingRules.Growth(Models.Prominence.Low));
        Assert.IsFalse(RingRules.ShowsCentre(Models.Prominence.Low, Models.Centre.Value));
        Assert.IsFalse(RingRules.ShowsCentre(Models.Prominence.High, Models.Centre.Nothing));
        Assert.IsTrue(RingRules.ShowsCentre(Models.Prominence.Medium, Models.Centre.Icon));
        Assert.IsFalse(RingRules.SaysWords(Models.Prominence.Low));
    }

    [TestMethod]
    public void The_middle_of_a_ring_says_its_value_or_a_dash()
    {
        Assert.AreEqual("83", RingRules.Centre(new Models.Ring { Max = 100, Value = 83 }, Invariant));
        Assert.AreEqual("83 %", RingRules.Centre(new Models.Ring { Max = 100, Value = 83, Shown = "83 %" }, Invariant));
        Assert.AreEqual("–", RingRules.Centre(new Models.Ring { Max = 100 }, Invariant));
        Assert.AreEqual(56.4, RingRules.CentreSize(188), 1e-9);
    }

    [TestMethod]
    public void Rings_inside_each_other_keep_to_the_room_and_say_their_value_over_their_goal()
    {
        double stroke = RingsRules.Stroke(150, 3, Models.Prominence.Medium);
        Assert.AreEqual(150 * 0.5 / 2 / (3 + (2 * 0.12)), stroke, 1e-9);
        Assert.AreEqual(150 - stroke, RingsRules.RingDiameter(150, 0, stroke), 1e-9);
        Assert.IsTrue(RingsRules.RingDiameter(150, 2, stroke) > 0);
        Assert.AreEqual(150.0, RingsRules.Diameter(Models.Prominence.Medium, 3));
        Assert.AreEqual(170.0, RingsRules.Diameter(Models.Prominence.Medium, 5));
        Assert.AreEqual(30.0, RingsRules.Diameter(Models.Prominence.Low, 3));

        Models.RingValue calories = new() { Name = "Move", Max = 500, Value = 441, Unit = "kcal" };
        Assert.AreEqual("441/500 kcal", RingsRules.Value(calories, Invariant));
        Assert.AreEqual("–", RingsRules.Value(calories with { Value = null }, Invariant));
        Assert.AreEqual("441/500", RingsRules.Value(calories with { Unit = null }, Invariant));

        Assert.IsTrue(RingsRules.HasKey(new Models.Rings { Legend = true }));
        Assert.IsTrue(RingsRules.HasKey(new Models.Rings { Centre = Models.RingsCentre.Values }));
        Assert.IsFalse(RingsRules.HasKey(new Models.Rings { Legend = true, Prominence = Models.Prominence.Low }));
        Assert.IsFalse(RingsRules.HasKey(new Models.Rings()));
    }

    // ---- gauge -----------------------------------------------------------------------------------------

    [TestMethod]
    public void A_gauge_is_an_open_arc_that_starts_at_the_lower_left_and_runs_two_hundred_and_forty_degrees()
    {
        Assert.AreEqual(150.0, GaugeRules.Angle(0));
        Assert.AreEqual(270.0, GaugeRules.Angle(0.5));
        Assert.AreEqual(390.0, GaugeRules.Angle(1));
        Assert.AreEqual(52.0, GaugeRules.Diameter(aside: true));
        Assert.AreEqual(184.0, GaugeRules.Diameter(aside: false));
    }

    [TestMethod]
    public void A_gauges_words_split_into_the_number_to_write_large_and_what_follows_it()
    {
        Assert.AreEqual(("44.2", (string?)"mL/kg/min"), GaugeRules.Split("44.2 mL/kg/min"));
        Assert.AreEqual(("Good", (string?)null), GaugeRules.Split("Good"));
        Assert.AreEqual(("Excellent shape", (string?)null), GaugeRules.Split("Excellent shape"));
        Assert.AreEqual(("1,200", (string?)"steps"), GaugeRules.Split("1,200 steps"));
        Assert.AreEqual(("12", (string?)null), GaugeRules.Split("12 "));
    }

    // ---- the day clock ---------------------------------------------------------------------------------

    [TestMethod]
    public void Midnight_is_at_the_top_of_the_dial_and_six_in_the_morning_at_three_oclock()
    {
        Assert.AreEqual(-90.0, ClockRules.Angle(0));
        Assert.AreEqual(0.0, ClockRules.Angle(6));
        Assert.AreEqual(90.0, ClockRules.Angle(12));
        CollectionAssert.AreEqual(new[] { 0, 6, 12, 18 }, ClockRules.Quarters.ToArray());
    }

    [TestMethod]
    public void An_hours_mark_reaches_by_its_level_else_by_its_value_against_the_most_else_not_at_all()
    {
        Assert.AreEqual(0.5, ClockRules.Reach(new Models.Hour { Level = 0.5 }, null));
        Assert.AreEqual(1.0, ClockRules.Reach(new Models.Hour { Level = 3 }, null));
        Assert.AreEqual(0.5, ClockRules.Reach(new Models.Hour { Value = 5 }, 10));
        Assert.AreEqual(0.0, ClockRules.Reach(new Models.Hour { Value = 5 }, 0));
        Assert.IsNull(ClockRules.Reach(new Models.Hour(), 10));
        Assert.IsNull(ClockRules.Reach(new Models.Hour { Value = 5 }, null));

        Models.DayClock own = new() { Max = 60, Hours = [new Models.Hour { Value = 10 }] };
        Assert.AreEqual(60.0, ClockRules.Most(own));
        Models.DayClock busiest = new() { Hours = [new Models.Hour { Value = 10 }, new Models.Hour { Value = 25 }, new Models.Hour()] };
        Assert.AreEqual(25.0, ClockRules.Most(busiest));
        Assert.IsNull(ClockRules.Most(new Models.DayClock()));
    }

    [TestMethod]
    public void The_clock_writes_the_quarters_and_moments_the_way_the_persons_clock_does()
    {
        Assert.AreEqual("12 AM", ClockRules.QuarterLabel(0, uses24: false, Invariant));
        Assert.AreEqual("6 PM", ClockRules.QuarterLabel(18, uses24: false, Invariant));
        Assert.AreEqual("18", ClockRules.QuarterLabel(18, uses24: true, Invariant));
        Assert.AreEqual("00", ClockRules.QuarterLabel(24, uses24: true, Invariant));
        double sevenOhSeven = 7 + (7 / 60.0);
        Assert.AreEqual("7:07 AM", ClockRules.TimeLabel(sevenOhSeven, uses24: false, Invariant));
        Assert.AreEqual("07:07", ClockRules.TimeLabel(sevenOhSeven, uses24: true, Invariant));
        Assert.AreEqual("12:00 AM", ClockRules.TimeLabel(24, uses24: false, Invariant));
        Assert.IsTrue(ClockRules.Uses24(new CultureInfo("de-DE")));
        Assert.IsFalse(ClockRules.Uses24(new CultureInfo("en-US")));

        Assert.IsTrue(ClockRules.HoldsTime("Bedtime 10:30 pm"));
        Assert.IsFalse(ClockRules.HoldsTime("Bedtime"));
        Assert.AreEqual("Bedtime 10:30 pm", ClockRules.MomentEntry("Bedtime 10:30 pm", 22.5, false, Invariant));
        Assert.AreEqual("Wake, 7:30 AM", ClockRules.MomentEntry("Wake", 7.5, false, Invariant));
    }

    [TestMethod]
    public void Moments_that_would_sit_on_one_another_are_moved_apart_about_their_middle_and_the_rest_stay()
    {
        IReadOnlyList<double> placed = ClockRules.Spread([3, 3.1, 12], 0.5);
        Assert.IsTrue(placed[1] - placed[0] >= 0.5 - 1e-9, "apart by " + (placed[1] - placed[0]));
        Assert.AreEqual(3.05, (placed[0] + placed[1]) / 2, 1e-6);
        Assert.AreEqual(12.0, placed[2], 1e-9);

        IReadOnlyList<double> far = ClockRules.Spread([1, 10], 1);
        Assert.AreEqual(1.0, far[0], 1e-9);
        Assert.AreEqual(10.0, far[1], 1e-9);

        // The order is kept, and each moment's answer is where it was given.
        IReadOnlyList<double> unordered = ClockRules.Spread([9, 2], 0.2);
        Assert.IsTrue(unordered[0] > unordered[1]);
    }

    [TestMethod]
    public void The_dial_puts_the_quarters_at_the_edge_the_moments_inside_them_and_the_hours_inside_those()
    {
        ClockRules.Dial dial = ClockRules.DialOf(150, 15, 22);
        Assert.AreEqual(142.5, dial.Labels, 1e-9);
        Assert.AreEqual(122.0, dial.Moments, 1e-9);
        Assert.AreEqual(108.0, dial.Outer, 1e-9);
        Assert.AreEqual(108 * 0.46, dial.Inner, 1e-9);
        Assert.IsTrue(ClockRules.BarWidth(dial.Inner) > 0);
        Assert.IsTrue(ClockRules.BarWidth(dial.Inner) < 2 * Math.PI * dial.Inner / 24);
    }

    // ---- heat maps -------------------------------------------------------------------------------------

    [TestMethod]
    public void A_day_is_pale_or_deep_by_the_pages_steps_and_a_day_with_no_reading_has_no_shade()
    {
        Assert.AreEqual(0, HeatRules.Shade(0, [], 100));
        Assert.AreEqual(1, HeatRules.Shade(30, [], 100));
        Assert.AreEqual(3, HeatRules.Shade(80, [], 100));
        Assert.AreEqual(2, HeatRules.Shade(25, [10, 20, 30], 100));
        Assert.IsNull(HeatRules.Shade(null, [10, 20, 30], 100));
        Assert.AreEqual(4, HeatRules.Opacities.Count);
        Assert.IsTrue(HeatRules.Opacities[0] < HeatRules.Opacities[3]);
    }

    [TestMethod]
    public void The_calendar_starts_on_the_persons_first_weekday_and_a_day_has_its_week_and_its_row()
    {
        DateOnly friday = new(2026, 11, 13);
        Assert.AreEqual(DayOfWeek.Friday, friday.DayOfWeek);
        Assert.AreEqual(new DateOnly(2026, 11, 8), HeatRules.Start(friday, DayOfWeek.Sunday));
        Assert.AreEqual(new DateOnly(2026, 11, 9), HeatRules.Start(friday, DayOfWeek.Monday));
        Assert.AreEqual(friday, HeatRules.Start(friday, DayOfWeek.Friday));

        DateOnly start = new(2026, 11, 8);
        Assert.AreEqual((0, 5), HeatRules.Cell(friday, start));
        Assert.AreEqual((1, 0), HeatRules.Cell(new DateOnly(2026, 11, 15), start));
        Assert.AreEqual((-1, 6), HeatRules.Cell(new DateOnly(2026, 11, 7), start));
        Assert.AreEqual(2, HeatRules.Weeks(new DateOnly(2026, 11, 8), new DateOnly(2026, 11, 21), DayOfWeek.Sunday));
        Assert.AreEqual(1, HeatRules.Weeks(friday, friday, DayOfWeek.Sunday));
    }

    [TestMethod]
    public void A_months_name_stands_over_the_week_it_starts_in_unless_it_would_crowd_the_one_before()
    {
        IReadOnlyList<(int Week, int Month)> months = HeatRules.Months(new DateOnly(2026, 9, 27), 10, DayOfWeek.Sunday);
        CollectionAssert.AreEqual(new (int, int)[] { (1, 10), (5, 11) }, months.ToArray());

        IReadOnlyList<(int Week, int Month)> wide = HeatRules.Months(new DateOnly(2026, 1, 4), 12, DayOfWeek.Sunday);
        Assert.AreEqual((0, 1), wide[0]);
        Assert.AreEqual(2, wide[1].Month);
    }

    [TestMethod]
    public void Days_are_read_as_the_page_writes_them_and_a_chosen_day_stays_inside_the_days_there_are()
    {
        Assert.AreEqual(new DateOnly(2026, 11, 13), PagerRules.Day("2026-11-13"));
        Assert.IsNull(PagerRules.Day("13/11/2026"));
        Assert.IsNull(PagerRules.Day(null));
        Assert.AreEqual("2026-01-02", PagerRules.Text(new DateOnly(2026, 1, 2)));
        Assert.IsTrue(PagerRules.Choosable("2026-06-01", "2026-01-02", "2026-11-13"));
        Assert.IsFalse(PagerRules.Choosable("2025-12-31", "2026-01-02", "2026-11-13"));
        Assert.IsFalse(PagerRules.Choosable("2026-11-14", "2026-01-02", "2026-11-13"));
        Assert.IsTrue(PagerRules.Choosable("2030-01-01", null, null));
        Assert.IsFalse(PagerRules.Choosable("not a day", null, null));
        Assert.AreEqual("Friday, 13 November 2026", PagerRules.Words("2026-11-13", Invariant));
        Assert.AreEqual("soon", PagerRules.Words("soon", Invariant));
        Models.DayPager pager = new() { Day = "Today", Date = "2026-11-13" };
        Assert.AreEqual("Today, Friday, 13 November 2026", PagerRules.Say(pager, Invariant));
    }

    [TestMethod]
    public void Two_measures_are_related_either_way_round_and_a_chance_pair_has_no_strength()
    {
        Models.Relation[] relations =
        [
            new() { A = 0, B = 1, Value = -0.5 },
            new() { A = 2, B = 0, Value = 0.9, Significant = false },
        ];
        Assert.AreEqual(-0.5, MatrixRules.Relation(relations, 1, 0)?.Value);
        Assert.AreEqual(0.9, MatrixRules.Relation(relations, 0, 2)?.Value);
        Assert.IsNull(MatrixRules.Relation(relations, 1, 2));

        Assert.AreEqual(0.5, MatrixRules.Strength(relations[0], null), 1e-9);
        Assert.AreEqual(0.625, MatrixRules.Strength(relations[0], 0.8), 1e-9);
        Assert.AreEqual(0.0, MatrixRules.Strength(relations[1], null));
        Assert.AreEqual(0.0, MatrixRules.Strength(null, null));
        Assert.AreEqual(0.0, MatrixRules.Strength(new Models.Relation { A = 0, B = 1 }, null));

        Assert.IsTrue(MatrixRules.Drawn(1, 0, lower: true));
        Assert.IsFalse(MatrixRules.Drawn(1, 1, lower: true));
        Assert.IsFalse(MatrixRules.Drawn(0, 1, lower: true));
        Assert.IsTrue(MatrixRules.Drawn(0, 1, lower: false));
        Assert.IsFalse(MatrixRules.Drawn(1, 1, lower: false));
    }

    // ---- composition -----------------------------------------------------------------------------------

    [TestMethod]
    public void A_carousel_shows_as_many_parts_as_fit_and_lets_the_next_one_peek_when_more_are_waiting()
    {
        Assert.AreEqual(4, ComposeRules.Fit(1000, 200, 12));
        Assert.AreEqual(1, ComposeRules.Fit(50, 200, 12));
        Assert.AreEqual(192.0, ComposeRules.ColumnWidth(600, 3, 12));

        (int shown, double? width) = ComposeRules.Carousel(600, 160, 12, 3);
        Assert.AreEqual(3, shown);
        Assert.AreEqual(192.0, width);
        Assert.IsFalse(ComposeRules.Scrolls(shown, 3));

        (int shownMore, double? widthMore) = ComposeRules.Carousel(600, 160, 12, 10);
        Assert.AreEqual(3, shownMore);
        Assert.AreEqual(180.0, widthMore);
        Assert.IsTrue(ComposeRules.Scrolls(shownMore, 10));

        (int own, double? ownWidth) = ComposeRules.Carousel(600, null, 12, 7);
        Assert.AreEqual(7, own);
        Assert.IsNull(ownWidth);
    }

    [TestMethod]
    public void A_grid_has_as_many_columns_as_fit_the_least_width_no_more_than_its_count_and_no_more_than_its_parts()
    {
        Assert.AreEqual(4, ComposeRules.Columns(1000, null, 200, 12, 9));
        Assert.AreEqual(3, ComposeRules.Columns(1000, 3, 200, 12, 9));
        Assert.AreEqual(2, ComposeRules.Columns(1000, 3, 200, 12, 2));
        Assert.AreEqual(1, ComposeRules.Columns(150, null, 200, 12, 9));
        Assert.AreEqual(5, ComposeRules.Columns(100, 5, null, 12, 9));
        Assert.AreEqual(6, ComposeRules.Columns(1000, null, null, 12, 9));
        Assert.AreEqual(1, ComposeRules.Columns(1000, null, null, 12, 0));
    }

    [TestMethod]
    public void One_short_word_or_figure_is_kept_whole_and_longer_words_may_wrap()
    {
        Assert.IsTrue(ComposeRules.KeepsWhole("Movement"));
        Assert.IsTrue(ComposeRules.KeepsWhole("\u20B91,24,050"));
        Assert.IsFalse(ComposeRules.KeepsWhole("25% of score"));
        Assert.IsFalse(ComposeRules.KeepsWhole("Antidisestablishmentarianism"));
        Assert.IsFalse(ComposeRules.KeepsWhole("  "));
    }

    [TestMethod]
    public void A_part_comes_in_as_it_says_and_only_fades_for_a_person_who_asked_for_less_motion()
    {
        Assert.AreEqual(ComposeRules.Arrival.None, ComposeRules.ArrivalOf(null, false));
        Assert.AreEqual(ComposeRules.Arrival.Fades, ComposeRules.ArrivalOf(Models.Appear.Fade, false));
        Assert.AreEqual(ComposeRules.Arrival.Rises, ComposeRules.ArrivalOf(Models.Appear.Rise, false));
        Assert.AreEqual(ComposeRules.Arrival.Grows, ComposeRules.ArrivalOf(Models.Appear.Scale, false));
        Assert.AreEqual(ComposeRules.Arrival.Fades, ComposeRules.ArrivalOf(Models.Appear.Rise, true));
        Assert.AreEqual(ComposeRules.Arrival.Fades, ComposeRules.ArrivalOf(Models.Appear.Scale, true));
    }

    [TestMethod]
    public void A_shape_has_the_corner_its_form_gives_and_a_tint_is_a_little_stronger_on_a_dark_ground()
    {
        Assert.AreEqual(0.0, ComposeRules.Corner(Models.ShapeForm.Rect, 100, 40));
        Assert.AreEqual(20.0, ComposeRules.Corner(Models.ShapeForm.Capsule, 100, 40));
        Assert.AreEqual(20.0, ComposeRules.Corner(Models.ShapeForm.Circle, 40, 40));
        Assert.AreEqual(8.0, ComposeRules.Corner(Models.ShapeForm.Rounded, 100, 40));
        Assert.AreEqual(8.0, ComposeRules.Corner(Models.ShapeForm.Rounded, 400, 400));
        Assert.AreEqual(2.0, ComposeRules.Corner(Models.ShapeForm.Rounded, 8, 8));
        Assert.IsTrue(ComposeRules.Tint(dark: true).Ground > ComposeRules.Tint(dark: false).Ground);
        Assert.AreEqual(12.0, ComposeRules.Outset(16));
        Assert.AreEqual(0.0, ComposeRules.Outset(44));
    }

    // ---- flags -----------------------------------------------------------------------------------------

    [TestMethod]
    public void A_pair_of_regional_indicator_letters_is_a_flag_and_the_words_round_it_are_kept()
    {
        string india = "\U0001F1EE\U0001F1F3";
        Assert.IsTrue(FlagText.Holds(india + " India"));
        Assert.IsFalse(FlagText.Holds("India"));
        Assert.IsFalse(FlagText.Holds("\U0001F1EE"));

        IReadOnlyList<FlagText.Piece> pieces = FlagText.Split("Ship to " + india + " India, " + "\U0001F1FA\U0001F1F8");
        Assert.AreEqual(4, pieces.Count);
        Assert.AreEqual("Ship to ", pieces[0].Text);
        Assert.AreEqual("IN", pieces[1].Country);
        Assert.AreEqual(" India, ", pieces[2].Text);
        Assert.AreEqual("US", pieces[3].Country);
        Assert.AreEqual(1, FlagText.Split("plain").Count);
    }

    [TestMethod]
    public void The_flag_of_India_is_three_bands_and_a_wheel_of_twenty_four_spokes()
    {
        IReadOnlyList<FlagShape>? india = FlagRules.Of("in");
        Assert.IsNotNull(india);
        FlagRect[] bands = [.. india.OfType<FlagRect>()];
        CollectionAssert.AreEqual(new[] { Rgb.Of("#FF9933"), Rgb.Of("#FFFFFF"), Rgb.Of("#138808") }, bands.Select(band => band.Fill).ToArray());
        FlagSpokes spokes = india.OfType<FlagSpokes>().Single();
        Assert.AreEqual(24, spokes.Count);
        Assert.AreEqual(30.0, spokes.X);
        Assert.AreEqual(20.0, spokes.Y);
        Assert.AreEqual(1, india.OfType<FlagRing>().Count());
    }

    [TestMethod]
    public void The_flag_of_the_United_States_has_thirteen_stripes_and_fifty_stars()
    {
        IReadOnlyList<FlagShape>? flag = FlagRules.Of("US");
        Assert.IsNotNull(flag);
        Assert.AreEqual(14, flag.OfType<FlagRect>().Count());
        Assert.AreEqual(50, flag.OfType<FlagStar>().Count());
    }

    [TestMethod]
    public void Every_flag_stays_inside_its_box_and_a_country_with_no_flag_of_its_own_is_left_to_the_neutral_one()
    {
        Assert.IsTrue(FlagRules.Known.Count >= 15);
        Assert.IsNull(FlagRules.Of("ZZ"));
        foreach (string country in FlagRules.Known)
        {
            IReadOnlyList<FlagShape>? shapes = FlagRules.Of(country);
            Assert.IsNotNull(shapes, country);
            Assert.IsTrue(shapes.Count > 0, country);
            foreach (FlagShape shape in shapes)
            {
                switch (shape)
                {
                    case FlagRect rect:
                        Assert.IsTrue(rect.X >= -1e-9 && rect.Y >= -1e-9 && rect.X + rect.Width <= FlagRules.Width + 1e-9 && rect.Y + rect.Height <= FlagRules.Height + 1e-9, country + " rectangle");
                        break;
                    case FlagDisc disc:
                        Assert.IsTrue(disc.X - disc.Radius >= 0 && disc.X + disc.Radius <= FlagRules.Width && disc.Y - disc.Radius >= 0 && disc.Y + disc.Radius <= FlagRules.Height, country + " disc");
                        break;
                    case FlagStar star:
                        Assert.IsTrue(star.X - star.Radius >= 0 && star.X + star.Radius <= FlagRules.Width && star.Y - star.Radius >= 0 && star.Y + star.Radius <= FlagRules.Height, country + " star");
                        break;
                }
            }
        }
    }

    [TestMethod]
    public void A_star_has_ten_points_and_stands_upright()
    {
        IReadOnlyList<Pt> star = FlagRules.StarPoints(10, 10, 5);
        Assert.AreEqual(10, star.Count);
        Assert.AreEqual(10.0, star[0].X, 1e-9);
        Assert.AreEqual(5.0, star[0].Y, 1e-9);
        Assert.IsTrue(star[1].Y > star[0].Y);
    }
}
