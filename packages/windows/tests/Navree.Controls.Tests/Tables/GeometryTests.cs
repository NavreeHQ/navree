using Navree.Design.Tables;

namespace Navree.Controls.Tests.Tables;

[TestClass]
public sealed class PixelsTests
{
    [TestMethod]
    [DataRow(39.3, 1.25, 40.0)]
    [DataRow(40.0, 1.25, 40.0)]
    [DataRow(37.0, 1.75, 37.142857142857146)]
    [DataRow(40.0000000001, 1.0, 40.0)]
    [DataRow(40.0, 1.0, 40.0)]
    [DataRow(-3.0, 1.5, 0.0)]
    [DataRow(0.0, 2.0, 0.0)]
    public void A_size_is_made_whole_in_physical_pixels(double epx, double scale, double expected) =>
        Same.Number(expected, Pixels.RoundUp(epx, scale));

    [TestMethod]
    public void A_scale_of_nothing_is_refused() =>
        Assert.ThrowsExactly<ArgumentOutOfRangeException>(() => Pixels.RoundUp(10, 0));
}

[TestClass]
public sealed class RowGeometryTests
{
    [TestMethod]
    [DataRow(Density.Standard, RowShape.OneLine, false, 20.0, 0.0, 1.0, 40.0)]
    [DataRow(Density.Compact, RowShape.OneLine, true, 20.0, 0.0, 1.0, 40.0)]
    [DataRow(Density.Compact, RowShape.OneLine, false, 20.0, 0.0, 1.0, 32.0)]
    [DataRow(Density.Standard, RowShape.OneLine, false, 45.0, 0.0, 1.0, 62.0)]
    [DataRow(Density.Comfortable, RowShape.TwoLine, false, 20.0, 16.0, 1.0, 64.0)]
    [DataRow(Density.Standard, RowShape.TwoLine, false, 30.0, 24.0, 1.0, 71.0)]
    [DataRow(Density.Standard, RowShape.TwoLine, false, 30.0, 24.0, 1.25, 71.2)]
    [DataRow(Density.Compact, RowShape.OneLine, false, 30.0, 0.0, 1.5, 39.333333333)]
    [DataRow(Density.Comfortable, RowShape.OneLine, false, 20.0, 0.0, 2.0, 48.0)]
    public void A_row_is_as_tall_as_its_density_and_its_text_ask(
        Density density, RowShape shape, bool controls, double line, double second, double scale, double expected) =>
        Same.Number(expected, RowGeometry.RowHeight(density, shape, controls, line, second, scale));

    [TestMethod]
    public void The_header_is_a_one_line_row()
    {
        Same.Number(40, RowGeometry.HeaderHeight(Density.Standard, false, 20, 1.0));
        Same.Number(40, RowGeometry.HeaderHeight(Density.Compact, true, 20, 1.0));
    }

    [TestMethod]
    public void A_row_with_no_text_height_is_refused()
    {
        Assert.ThrowsExactly<ArgumentOutOfRangeException>(() => RowGeometry.RowHeight(Density.Standard, RowShape.OneLine, false, 0, 0, 1));
        Assert.ThrowsExactly<ArgumentOutOfRangeException>(() => RowGeometry.RowHeight(Density.Standard, RowShape.OneLine, false, 20, 0, 0));
    }

    [TestMethod]
    [DataRow(40.0, 200_000)]
    [DataRow(64.0, 200_000)]
    [DataRow(80.0, 200_000)]
    [DataRow(96.0, 166_666)]
    [DataRow(62.0, 200_000)]
    public void The_rows_held_are_bounded_by_the_height_they_make(double rowHeight, int expected) =>
        Assert.AreEqual(expected, RowGeometry.MaxHeldRows(rowHeight));

    [TestMethod]
    [DataRow(40.0, 1.0, 200_000)]
    [DataRow(40.0, 1.25, 200_000)]
    [DataRow(40.0, 2.5, 160_000)]
    [DataRow(64.0, 2.5, 100_000)]
    [DataRow(64.0, 3.0, 83_333)]
    [DataRow(80.0, 2.5, 80_000)]
    [DataRow(96.0, 1.0, 166_666)]
    [DataRow(62.0, 2.25, 114_695)]
    public void The_rows_held_are_bounded_by_the_height_they_make_in_physical_pixels(double rowHeight, double scale, int expected)
    {
        Assert.AreEqual(expected, RowGeometry.MaxHeldRows(rowHeight, scale));
        Assert.IsTrue(expected * rowHeight * scale <= RowGeometry.MaxExtent, "the whole table stays within the pixels a single-precision position holds");
    }

    [TestMethod]
    public void One_lakh_rows_are_held_at_every_scale_a_window_has_with_the_rows_of_every_density_and_text_size_up_to_two_hundred_twenty_five_percent()
    {
        foreach (double scale in new[] { 1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5 })
        {
            foreach (double height in new[] { 32.0, 40.0, 48.0, 54.0, 62.0, 64.0 })
            {
                Assert.IsTrue(RowGeometry.MaxHeldRows(height, scale) >= 100_000, $"a row of {height} at {scale}");
            }
        }
    }

    [TestMethod]
    public void A_row_of_no_height_or_a_display_of_no_scale_holds_nothing_sensible()
    {
        Assert.ThrowsExactly<ArgumentOutOfRangeException>(() => RowGeometry.MaxHeldRows(0));
        Assert.ThrowsExactly<ArgumentOutOfRangeException>(() => RowGeometry.MaxHeldRows(40, 0));
    }
}

[TestClass]
public sealed class GeometryTests
{
    private static Geometry WithOneOpen() => new(40, [(3, 120)]);

    [TestMethod]
    public void A_row_stands_below_the_rows_before_it_and_the_detail_of_the_open_ones()
    {
        Geometry g = WithOneOpen();
        Same.Number(0, g.RowTop(0));
        Same.Number(120, g.RowTop(3));
        Same.Number(280, g.RowTop(4));
        Same.Number(520, g.RowTop(10));
        Same.Number(160, g.HeightAt(3));
        Same.Number(40, g.HeightAt(4));
    }

    [TestMethod]
    public void The_extent_counts_only_the_open_rows_within_it()
    {
        Geometry g = WithOneOpen();
        Same.Number(4120, g.Extent(100));
        Same.Number(120, g.Extent(3));
        Same.Number(280, g.Extent(4));
    }

    [TestMethod]
    public void A_position_is_found_in_its_row()
    {
        Geometry g = WithOneOpen();
        Assert.AreEqual((3, 10.0), g.IndexAt(130, 100));
        Assert.AreEqual((4, 0.0), g.IndexAt(280, 100));
        Assert.AreEqual((2, 20.0), g.IndexAt(100, 100));
        Assert.AreEqual((0, 0.0), g.IndexAt(-5, 100));
        Assert.AreEqual((99, 40.0), g.IndexAt(4120, 100));
        Assert.AreEqual((-1, 0.0), g.IndexAt(5, 0));
    }

    [TestMethod]
    public void The_rows_to_make_for_a_viewport_are_those_it_touches_and_a_margin()
    {
        Assert.AreEqual((2, 4), WithOneOpen().RealizedRange(150, 100, 10));
        Geometry plain = new(40, []);
        Assert.AreEqual((0, 20), plain.RealizedRange(0, 800, 100_000));
        Assert.AreEqual((99_979, 99_999), plain.RealizedRange(3_999_200, 800, 100_000));
        Assert.AreEqual((0, -1), plain.RealizedRange(0, 800, 0));
        Same.Number(4_000_000, plain.Extent(100_000));
    }

    [TestMethod]
    public void A_viewport_of_no_end_or_no_number_never_makes_every_row()
    {
        Geometry plain = new(40, []);
        (int first, int last) = plain.RealizedRange(0, double.PositiveInfinity, 200_000);
        Assert.AreEqual(0, first);
        Assert.AreEqual(Geometry.MostRealized - 1, last);
        (first, last) = plain.RealizedRange(120_000, double.PositiveInfinity, 200_000);
        Assert.IsTrue(last - first + 1 <= Geometry.MostRealized, "a viewport that starts down the table is held to the same number");
        Assert.AreEqual((0, -1), plain.RealizedRange(0, double.NaN, 200_000));
        Assert.AreEqual((0, 20), plain.RealizedRange(0, 800, 100_000), "an ordinary viewport is not held back");
    }

    [TestMethod]
    public void Open_rows_out_of_order_are_refused() =>
        Assert.ThrowsExactly<ArgumentException>(() => new Geometry(40, [(5, 10), (3, 10)]));
}
