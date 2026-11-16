using Navree.Protocol;

namespace Navree.Design.Tests;

/// <summary>How series are told apart in a contrast theme, and the ink that reads on a mark.</summary>
[TestClass]
public sealed class KindsThemeTests
{
    [TestMethod]
    public void The_first_eight_series_each_have_a_mark_of_their_own_and_a_contrast_theme_tells_them_apart_by_more_than_colour()
    {
        SeriesLook[] looks = [.. Enumerable.Range(0, 8).Select(SeriesLooks.Of)];
        Assert.AreEqual(8, looks.Select(look => look.Shape).Distinct().Count());
        Assert.AreEqual(2, looks.Select(look => look.Highlight).Distinct().Count(), "two colours only: the text and the highlight");
        Assert.IsTrue(looks.Any(look => look.Dash.Count == 0) && looks.Any(look => look.Dash.Count > 0));
        Assert.IsTrue(looks.Any(look => look.Filled) && looks.Any(look => !look.Filled));
        Assert.IsTrue(looks.Select(look => string.Join(",", look.Dash)).Distinct().Count() >= 4);

        // Two series next to each other differ in at least two ways, so one change of colour is never the only difference.
        for (int at = 0; at < 8; at++)
        {
            SeriesLook one = looks[at];
            SeriesLook other = looks[(at + 1) % 8];
            int differences =
                (one.Highlight != other.Highlight ? 1 : 0)
                + (one.Filled != other.Filled ? 1 : 0)
                + (string.Join(",", one.Dash) != string.Join(",", other.Dash) ? 1 : 0)
                + (one.Shape != other.Shape ? 1 : 0);
            Assert.IsTrue(differences >= 2, at + " and " + ((at + 1) % 8) + " differ in " + differences);
        }
    }

    [TestMethod]
    public void The_looks_go_round_again_after_eight_and_a_series_before_the_first_has_one_too()
    {
        Assert.AreEqual(SeriesLooks.Of(0), SeriesLooks.Of(8));
        Assert.AreEqual(SeriesLooks.Of(3).Shape, SeriesLooks.Of(11).Shape);
        Assert.AreEqual(SeriesLooks.Of(7).Shape, SeriesLooks.Of(-1).Shape);
        Assert.IsNotNull(SeriesLooks.Of(int.MinValue));
        Assert.IsNotNull(SeriesLooks.Of(int.MaxValue));
    }

    [TestMethod]
    public void A_mark_with_corners_has_them_round_its_middle_and_one_without_has_none()
    {
        Assert.AreEqual(4, SeriesLooks.Corners(MarkShape.Diamond, 10).Count);
        Assert.AreEqual(3, SeriesLooks.Corners(MarkShape.TriangleUp, 10).Count);
        Assert.AreEqual(3, SeriesLooks.Corners(MarkShape.TriangleDown, 10).Count);
        Assert.AreEqual(0, SeriesLooks.Corners(MarkShape.Circle, 10).Count);
        Assert.AreEqual(0, SeriesLooks.Corners(MarkShape.Ring, 10).Count);
        Assert.IsTrue(SeriesLooks.Corners(MarkShape.Diamond, 10).All(corner => Math.Abs(corner.X) <= 5 && Math.Abs(corner.Y) <= 5));
        Assert.IsTrue(SeriesLooks.Corners(MarkShape.Diamond, double.NaN).All(corner => corner == new Pt(0, 0)));
        Assert.IsTrue(SeriesLooks.Corners(MarkShape.TriangleUp, -4).All(corner => corner == new Pt(0, 0)));
    }

    [TestMethod]
    public void The_ink_on_a_mark_is_white_or_black_by_which_reads_better_and_never_a_colour_of_its_own()
    {
        Assert.AreEqual(new Rgb(1, 1, 1), ContrastRules.Ink(Rgb.Of("#005FB8")));
        Assert.AreEqual(new Rgb(0, 0, 0), ContrastRules.Ink(Rgb.Of("#FFC83D")));
        Assert.AreEqual(new Rgb(0, 0, 0), ContrastRules.Ink(new Rgb(1, 1, 1)));
        Assert.AreEqual(new Rgb(1, 1, 1), ContrastRules.Ink(new Rgb(0, 0, 0)));
        foreach (Models.Hue hue in Enum.GetValues<Models.Hue>())
        {
            foreach (bool dark in new[] { false, true })
            {
                Rgb ground = HueRules.Of(hue).For(dark);
                Rgb ink = ContrastRules.Ink(ground);
                Assert.IsTrue(ContrastRules.Contrast(ink, ground) >= 3, hue + " " + dark);
            }
        }
    }

    [TestMethod]
    public void A_cell_of_a_heat_map_is_read_in_the_ink_that_suits_what_the_person_sees_not_the_hue_alone()
    {
        Rgb card = new(1, 1, 1);
        Rgb accent = Rgb.Of("#005FB8");
        // A faint wash of a dark hue on a light card is light: black reads on it; the full hue is dark: white reads on it.
        Assert.AreEqual(new Rgb(0, 0, 0), ContrastRules.Ink(ContrastRules.Over(accent, card, 0.15)));
        Assert.AreEqual(new Rgb(1, 1, 1), ContrastRules.Ink(ContrastRules.Over(accent, card, 1)));
        Assert.AreEqual(card, ContrastRules.Over(accent, card, 0));
        Assert.AreEqual(accent, ContrastRules.Over(accent, card, 1));
        Assert.AreEqual(card, ContrastRules.Over(accent, card, double.NaN));
    }
}
