using System.Globalization;
using Navree.Protocol;

namespace Navree.Design.Tests;

/// <summary>The rules of the charts: colours, words for values, axes, shares, progress.</summary>
[TestClass]
public sealed class KindsChartTests
{
    private static readonly CultureInfo Invariant = CultureInfo.InvariantCulture;

    private static double?[] V(params double?[] values) => values;

    private static Models.Chart BarChart(params double?[][] series) =>
        new()
        {
            Kind = Models.ChartKind.Bar,
            Series = [.. series.Select((values, at) => new Models.Series { Label = "s" + at, Values = values })],
        };

    // ---- colour ----------------------------------------------------------------------------------------

    [TestMethod]
    public void A_colour_is_read_from_hex_with_or_without_the_hash_and_nothing_else_is_a_colour()
    {
        Assert.IsTrue(Rgb.TryParse("#FF8000", out Rgb orange));
        Assert.AreEqual((byte)255, orange.R);
        Assert.AreEqual((byte)128, orange.G);
        Assert.AreEqual((byte)0, orange.B);
        Assert.IsTrue(Rgb.TryParse("0F7B0F", out _));
        Assert.IsFalse(Rgb.TryParse("#FFF", out _));
        Assert.IsFalse(Rgb.TryParse("#GGGGGG", out _));
        Assert.IsFalse(Rgb.TryParse(null, out _));
    }

    [TestMethod]
    public void The_hues_a_chart_takes_in_order_are_eight_that_stand_apart()
    {
        Assert.AreEqual(8, HueRules.Series.Count);
        Rgb[] light = [.. HueRules.Series.Select(hue => HueRules.Of(hue).Light)];
        Assert.AreEqual(8, light.Distinct().Count());
        Assert.AreEqual(HueRules.Series[0], HueRules.SeriesHue(8));
        Assert.AreEqual(HueRules.Series[7], HueRules.SeriesHue(-1));
    }

    [TestMethod]
    public void A_status_tone_is_drawn_in_the_hue_the_rest_of_the_client_gives_it()
    {
        Assert.AreEqual(HueRules.Of(Models.Hue.Green), HueRules.Of(HueRules.OfTone(Models.Tone.Success)));
        Assert.AreEqual(HueRules.Of(Models.Hue.Orange), HueRules.Of(HueRules.OfTone(Models.Tone.Warning)));
        Assert.AreEqual(HueRules.Of(Models.Hue.Red), HueRules.Of(HueRules.OfTone(Models.Tone.Danger)));
        Assert.AreEqual(Models.Hue.Neutral, HueRules.OfTone(null));
    }

    [TestMethod]
    public void A_hue_this_client_does_not_know_is_drawn_in_the_colour_the_page_sent_for_the_theme()
    {
        Models.Colour future = new() { Hue = Models.Hue.Neutral, Light = "#112233", Dark = "#AABBCC" };
        Assert.AreEqual(Rgb.Of("#112233"), HueRules.Fallback(future, dark: false));
        Assert.AreEqual(Rgb.Of("#AABBCC"), HueRules.Fallback(future, dark: true));
        Assert.IsNull(HueRules.Fallback(new Models.Colour { Hue = Models.Hue.Green, Light = "#112233" }, dark: false));
        Assert.IsNull(HueRules.Fallback(new Models.Colour { Hue = Models.Hue.Neutral }, dark: false));
    }

    [TestMethod]
    public void Words_in_every_hue_read_on_a_light_ground_at_four_and_a_half_to_one()
    {
        Rgb white = new(1, 1, 1);
        foreach (Models.Hue hue in Enum.GetValues<Models.Hue>())
        {
            Rgb words = ContrastRules.ForWords(HueRules.Of(hue).Light, dark: false);
            Assert.IsTrue(ContrastRules.Contrast(words, white) >= ContrastRules.Words - 0.01, hue + " reads at " + ContrastRules.Contrast(words, white));
        }
    }

    [TestMethod]
    public void Words_on_a_dark_ground_keep_the_colour_they_have()
    {
        Rgb orange = HueRules.Of(Models.Hue.Orange).Dark;
        Assert.AreEqual(orange, ContrastRules.ForWords(orange, dark: true));
        Assert.AreEqual(1.0, ContrastRules.Luminance(new Rgb(1, 1, 1)), 1e-9);
        Assert.AreEqual(21.0, ContrastRules.Contrast(new Rgb(0, 0, 0), new Rgb(1, 1, 1)), 1e-9);
    }

    // ---- words for values ------------------------------------------------------------------------------

    [TestMethod]
    public void Numbers_are_written_with_the_decimals_they_need_and_no_more()
    {
        Assert.AreEqual("7", NumberWords.Amount(7.0, Invariant));
        Assert.AreEqual("1,234.6", NumberWords.Amount(1234.56, Invariant));
        Assert.AreEqual("100,000", NumberWords.Amount(100000, Invariant));
        Assert.AreEqual("12%", NumberWords.WithUnit("12", "%"));
        Assert.AreEqual("12 kg", NumberWords.WithUnit("12", "kg"));
        Assert.AreEqual("12", NumberWords.WithUnit("12", null));
    }

    [TestMethod]
    public void Rupees_are_grouped_as_India_groups_them_and_other_numbers_in_threes()
    {
        Assert.AreEqual("31,55,138", ValueWords.IndianGroups("3155138"));
        Assert.AreEqual("1,00,000", ValueWords.IndianGroups("100000"));
        Assert.AreEqual("999", ValueWords.IndianGroups("999"));
        Assert.AreEqual("1,234,567", ValueWords.WesternGroups("1234567"));

        ValueStyle rupees = ValueWords.StyleOf("₹31,55,138.50");
        Assert.AreEqual("₹", rupees.Prefix);
        Assert.IsTrue(rupees.Indian);
        Assert.AreEqual("₹31,55,138.5", ValueWords.Text(3155138.5, rupees));
        Assert.AreEqual("−₹1,00,000", ValueWords.Text(-100000, rupees));
    }

    [TestMethod]
    public void A_chart_writes_its_axis_in_the_pages_own_words_for_a_value_else_in_its_unit()
    {
        Models.Chart withWords = new()
        {
            Series = [new Models.Series { Label = "Sales", Values = [1000, 2000], Shown = ["₹1,000", "₹2,000"] }],
        };
        ValueStyle words = ValueWords.Style(withWords);
        Assert.AreEqual("₹", words.Prefix);

        Models.Chart withUnit = new() { Series = [new Models.Series { Label = "Weight", Values = [60], Unit = "kg" }] };
        Assert.AreEqual(" kg", ValueWords.Style(withUnit).Suffix);
        Assert.AreEqual("%", ValueWords.StyleOfUnit("%").Suffix);
        Assert.IsTrue(ValueWords.StyleOfUnit("INR").Indian);
        Assert.AreEqual(new ValueStyle(), ValueWords.StyleOfUnit(null));
    }

    [TestMethod]
    public void Words_that_write_one_value_in_two_numbers_give_the_axis_no_unit_of_their_own()
    {
        Assert.IsTrue(ValueWords.Compound("7 h 6 min"));
        Assert.IsTrue(ValueWords.Compound("7:15"));
        Assert.IsFalse(ValueWords.Compound("1,994"));
        Assert.IsFalse(ValueWords.Compound("12.5 kg"));

        Models.Chart duration = new()
        {
            Series = [new Models.Series { Label = "Sleep", Values = [7.1], Shown = ["7 h 6 min"], Unit = "h" }],
        };
        Assert.AreEqual(" h", ValueWords.Style(duration).Suffix);
    }

    [TestMethod]
    public void A_screen_reader_is_not_told_a_parts_name_twice()
    {
        Assert.AreEqual("79 of 100. Good.", SpeechRules.Value("Health score: 79 of 100. Good.", "Health score"));
        Assert.AreEqual("Recoveryish 3", SpeechRules.Value("Recoveryish 3", "Recovery"));
        Assert.AreEqual("Sleep", SpeechRules.Value("Sleep", "Sleep"));
        Assert.AreEqual("Subtotal, ₹428", SpeechRules.Fact("Subtotal", " ₹428 "));
        Assert.AreEqual("Subtotal", SpeechRules.Fact("Subtotal", " "));
        Assert.AreEqual("UPI · 62%", KeyRules.Entry("UPI", "62%"));
        Assert.AreEqual("UPI", KeyRules.Entry("UPI", null));
    }

    // ---- axes ------------------------------------------------------------------------------------------

    [TestMethod]
    public void Two_ends_are_rounded_out_to_numbers_read_at_a_glance()
    {
        AxisRules.Bounds ends = AxisRules.Nice(0, 97);
        Assert.AreEqual(0.0, ends.Low);
        Assert.AreEqual(100.0, ends.High);
        Assert.AreEqual(2.5, AxisRules.NiceStep(2.4));
        Assert.AreEqual(50.0, AxisRules.NiceStep(41));
        Assert.AreEqual(new AxisRules.Bounds(3, 4), AxisRules.Nice(3, 3));
    }

    [TestMethod]
    public void A_bar_chart_stands_on_zero_and_a_stack_is_as_high_as_its_tallest_sum()
    {
        AxisRules.Bounds bars = AxisRules.Values(BarChart(V(41, 38)));
        Assert.AreEqual(0.0, bars.Low);
        Assert.AreEqual(50.0, bars.High);

        Models.Chart stacked = BarChart(V(10, 20), V(5, 5)) with { Stacking = Models.Stacking.Values };
        Assert.AreEqual(new AxisRules.Bounds(0, 30), AxisRules.Values(stacked));

        Models.Chart shares = BarChart(V(10, 20), V(5, 5)) with { Stacking = Models.Stacking.Shares };
        Assert.AreEqual(new AxisRules.Bounds(0, 1), AxisRules.Values(shares));

        Models.Chart lanes = BarChart(V(0, 3, 2)) with { Levels = ["Awake", "REM", "Light", "Deep"] };
        Assert.AreEqual(new AxisRules.Bounds(0, 3), AxisRules.Values(lanes));
    }

    [TestMethod]
    public void A_measure_that_gives_its_own_ends_is_taken_at_its_word_and_references_and_zones_are_in_reach()
    {
        Models.Chart own = BarChart(V(5)) with { Y = new Models.Measure { Min = -10, Max = 10 } };
        Assert.AreEqual(new AxisRules.Bounds(-10, 10), AxisRules.Values(own));

        Models.Chart reference = BarChart(V(5)) with { References = [new Models.Reference { Value = 90 }] };
        Assert.IsTrue(AxisRules.Values(reference).High >= 90);

        Models.Chart zone = BarChart(V(5)) with { Zones = [new Models.Zone { From = 0, To = 120 }] };
        Assert.IsTrue(AxisRules.Values(zone).High >= 120);
    }

    [TestMethod]
    public void The_marks_of_a_value_axis_are_the_nice_numbers_between_its_ends()
    {
        CollectionAssert.AreEqual(new double[] { 0, 10, 20, 30, 40, 50 }, AxisRules.Marks(new AxisRules.Bounds(0, 50), 5).ToArray());
        CollectionAssert.AreEqual(new double[] { 0 }, AxisRules.Marks(new AxisRules.Bounds(0, 0), 5).ToArray());
    }

    [TestMethod]
    public void A_scatter_pads_its_ends_and_with_equal_axes_uses_one_range_for_both()
    {
        Models.Scatter plot = new()
        {
            Series = [new Models.PointSeries { Name = "Days", Points = [new Models.Point { X = 1, Y = 10 }, new Models.Point { X = 5, Y = 90 }] }],
        };
        (AxisRules.Bounds x, AxisRules.Bounds y) = AxisRules.Scatter(plot);
        Assert.IsTrue(x.Low <= 1 && x.High >= 5);
        Assert.IsTrue(y.Low <= 10 && y.High >= 90);

        (AxisRules.Bounds ex, AxisRules.Bounds ey) = AxisRules.Scatter(plot with { EqualAxes = true });
        Assert.AreEqual(ex, ey);
        Assert.IsTrue(ex.Low <= 1 && ex.High >= 90);
    }

    [TestMethod]
    public void Labels_along_an_axis_are_thinned_to_a_whole_step_from_the_first()
    {
        CollectionAssert.AreEqual(new[] { 0, 1, 2, 3, 4, 5, 6 }, AxisRules.Ticks(7, 12).ToArray());
        CollectionAssert.AreEqual(new[] { 0, 6, 12, 18, 24 }, AxisRules.Ticks(30, 5).ToArray());
        Assert.AreEqual(0, AxisRules.Ticks(0, 5).Count);
        Assert.AreEqual(0, AxisRules.Ticks(5, 0).Count);
        Assert.AreEqual(5, AxisRules.Most(300, 30));
        Assert.AreEqual(1, AxisRules.Most(0, 30));
        Assert.AreEqual(1, AxisRules.Most(double.NaN, 30));
    }

    [TestMethod]
    public void A_gap_in_a_series_breaks_the_line_and_a_run_of_one_level_is_one_stretch()
    {
        CollectionAssert.AreEqual(new[] { 0, 1, 1, 1, 1, 2, 2 }, AxisRules.Runs([1, null, null, 2, 3, null, 4]).ToArray());
        (int Level, int From, int To)[] stretches = [.. AxisRules.Stretches([1, 1, 2, 2, 2, null, 1])];
        CollectionAssert.AreEqual(new (int, int, int)[] { (1, 0, 1), (2, 2, 4), (1, 6, 6) }, stretches);
    }

    [TestMethod]
    public void A_chart_has_as_many_places_as_its_labels_or_its_longest_series()
    {
        Assert.AreEqual(3, AxisRules.Count(BarChart(V(1, 2, 3)) with { Labels = ["a", "b"] }));
        Assert.AreEqual(4, AxisRules.Count(BarChart(V(1, 2)) with { Labels = ["a", "b", "c", "d"] }));
        Assert.AreEqual(0, AxisRules.Count(new Models.Chart()));
        Assert.AreEqual(0.5, AxisRules.Position(5, 0, 10));
        Assert.AreEqual(1.0, AxisRules.Position(50, 0, 10));
        Assert.AreEqual(0.0, AxisRules.Position(5, 10, 10));
    }

    // ---- lines -----------------------------------------------------------------------------------------

    [TestMethod]
    public void A_smooth_line_through_a_peak_never_goes_past_it()
    {
        Pt[] points = [new(0, 0), new(1, 1), new(2, 0), new(3, 0.5)];
        IReadOnlyList<Bezier> pieces = Curves.Monotone(points);
        Assert.AreEqual(3, pieces.Count);
        foreach (Bezier piece in pieces)
        {
            Assert.IsTrue(piece.First.Y >= -1e-9 && piece.First.Y <= 1 + 1e-9, "first " + piece.First);
            Assert.IsTrue(piece.Second.Y >= -1e-9 && piece.Second.Y <= 1 + 1e-9, "second " + piece.Second);
        }

        Assert.AreEqual(points[1], pieces[0].To);
        Assert.AreEqual(0, Curves.Monotone([new Pt(0, 0)]).Count);
    }

    [TestMethod]
    public void A_step_line_goes_across_and_then_up_or_down()
    {
        IReadOnlyList<Pt> corners = Curves.Steps([new Pt(0, 0), new Pt(1, 2), new Pt(2, 1)]);
        CollectionAssert.AreEqual(new[] { new Pt(0, 0), new Pt(1, 0), new Pt(1, 2), new Pt(2, 2), new Pt(2, 1) }, corners.ToArray());
    }

    // ---- zones, shares, progress -----------------------------------------------------------------------

    [TestMethod]
    public void The_zone_a_value_is_in_is_the_last_one_that_holds_it()
    {
        Models.Zone[] zones =
        [
            new() { From = 0, To = 50 },
            new() { From = 50, To = 80 },
            new() { From = 80, To = 100 },
        ];
        Assert.AreEqual(0, ZoneRules.IndexOf(20, zones));
        Assert.AreEqual(1, ZoneRules.IndexOf(50, zones));
        Assert.AreEqual(2, ZoneRules.IndexOf(100, zones));
        Assert.IsNull(ZoneRules.IndexOf(101, zones));
        Assert.IsNull(ZoneRules.IndexOf(null, zones));
        Assert.AreEqual(0, ZoneRules.IndexOf(5, [new Models.Zone { From = 10, To = 0 }]));
    }

    [TestMethod]
    public void Shares_add_up_to_the_whole_and_a_part_that_has_some_is_never_none()
    {
        CollectionAssert.AreEqual(new[] { 0.25, 0.75, 0.0 }, ShareRules.Shares([1, 3, null]).ToArray());
        CollectionAssert.AreEqual(new[] { 0.0, 0.0 }, ShareRules.Shares([null, -4]).ToArray());
        Assert.AreEqual(1, ShareRules.Percent(0.004));
        Assert.AreEqual(0, ShareRules.Percent(0));
        Assert.AreEqual(62, ShareRules.Percent(0.62));
        Assert.IsTrue(ShareRules.SaysInside(0.5, 200, 30));
        Assert.IsFalse(ShareRules.SaysInside(0.1, 200, 30));
    }

    [TestMethod]
    public void A_strip_counts_its_cells_by_the_kind_each_has_and_skips_a_cell_with_none()
    {
        Models.StripCell[] cells =
        [
            new() { Label = "a", Category = 0 },
            new() { Label = "b", Category = 0 },
            new() { Label = "c", Category = 1 },
            new() { Label = "d" },
            new() { Label = "e", Category = 7 },
        ];
        CollectionAssert.AreEqual(new[] { 2.0 / 3, 1.0 / 3 }, ShareRules.CategoryShares(cells, 2).ToArray());
    }

    [TestMethod]
    public void Progress_counts_in_thousandths_and_takes_the_colour_of_its_measure_before_its_tone()
    {
        Assert.AreEqual(0.62, ProgressRules.Fraction(620));
        Assert.AreEqual(1.0, ProgressRules.Fraction(1500));
        Assert.AreEqual(0.0, ProgressRules.Fraction(-5));
        Assert.IsNull(ProgressRules.Fraction(null));

        Assert.AreEqual(Models.Hue.Violet, ProgressRules.Hue(new Models.Progress { Colour = new Models.Colour { Hue = Models.Hue.Violet }, Tone = Models.Tone.Danger }));
        Assert.AreEqual(Models.Hue.Warning, ProgressRules.Hue(new Models.Progress { Tone = Models.Tone.Warning }));
        Assert.AreEqual(Models.Hue.Accent, ProgressRules.Hue(new Models.Progress()));
    }

    [TestMethod]
    public void A_trend_has_its_arrow_and_a_word_for_a_reader_that_cannot_see_it()
    {
        Assert.AreNotEqual(TrendRules.Glyph(Models.Direction.Up), TrendRules.Glyph(Models.Direction.Down));
        Assert.AreEqual("Up", TrendRules.Word(Models.Direction.Up));
        Assert.AreEqual("Steady", TrendRules.Word(Models.Direction.Flat));
        Assert.IsNull(MessageRules.Delivery(Models.Delivery.Unknown));
        Assert.AreEqual("Not sent", MessageRules.Delivery(Models.Delivery.Failed)?.Text);
    }
}
