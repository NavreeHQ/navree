using System.Globalization;

namespace Navree.Design.Tests;

/// <summary>What the client writes itself (a day, a time of day, a number) follows the locale it was started with.</summary>
[TestClass]
public sealed class KindsLocaleTests
{
    private static readonly CultureInfo India = new("en-IN");
    private static readonly CultureInfo America = new("en-US");
    private static readonly CultureInfo Hindi = new("hi-IN");

    [TestMethod]
    public void A_day_is_written_the_way_the_locale_writes_it()
    {
        Assert.AreEqual("Friday, 13 November 2026", PagerRules.Words("2026-11-13", India));
        Assert.AreEqual("Friday, November 13, 2026", PagerRules.Words("2026-11-13", America));
        string hindi = PagerRules.Words("2026-11-13", Hindi);
        Assert.IsFalse(hindi.Contains("Friday", StringComparison.Ordinal), hindi);
        Assert.IsTrue(hindi.Contains("2026", StringComparison.Ordinal), hindi);
    }

    [TestMethod]
    public void A_time_of_day_is_written_as_the_locales_clock_writes_it()
    {
        double quarterPastSeven = 7.25;
        Assert.AreEqual("7:15 am", ClockRules.TimeLabel(quarterPastSeven, ClockRules.Uses24(India), India));
        Assert.AreEqual("7:15 AM", ClockRules.TimeLabel(quarterPastSeven, ClockRules.Uses24(America), America));
        Assert.AreEqual("6 pm", ClockRules.QuarterLabel(18, ClockRules.Uses24(India), India));
        string hindi = ClockRules.TimeLabel(quarterPastSeven, ClockRules.Uses24(Hindi), Hindi);
        Assert.IsTrue(hindi.StartsWith("7:15", StringComparison.Ordinal) || hindi.StartsWith("07:15", StringComparison.Ordinal), hindi);
    }

    [TestMethod]
    public void Numbers_are_grouped_as_the_locale_groups_them()
    {
        Assert.AreEqual("1,00,000", NumberWords.Amount(100000, India));
        Assert.AreEqual("100,000", NumberWords.Amount(100000, America));
    }

    [TestMethod]
    public void What_the_client_writes_without_being_told_a_culture_follows_the_one_it_chose()
    {
        CultureInfo? before = ClientCulture.Chosen;
        try
        {
            ClientCulture.Chosen = India;
            Assert.AreEqual("Friday, 13 November 2026", PagerRules.Words("2026-11-13"));
            Assert.AreEqual("1,00,000", NumberWords.Amount(100000));
            ClientCulture.Chosen = America;
            Assert.AreEqual("Friday, November 13, 2026", PagerRules.Words("2026-11-13"));
        }
        finally
        {
            ClientCulture.Chosen = before;
        }
    }

    [TestMethod]
    public void A_locale_this_machine_does_not_know_is_no_culture_at_all()
    {
        Assert.IsNull(ClientCulture.Named(null));
        Assert.IsNull(ClientCulture.Named(" "));
        Assert.IsNull(ClientCulture.Named("not a locale at all"));
        Assert.AreEqual("en-IN", ClientCulture.Named("en-IN")?.Name);
    }

    [TestMethod]
    public void A_grid_of_figures_stands_further_apart_than_a_grid_of_cards()
    {
        Assert.AreEqual(12.0, ComposeRules.GridGap(0, cards: true));
        Assert.AreEqual(12.0, ComposeRules.GridGap(3, cards: true));
        Assert.AreEqual(24.0, ComposeRules.GridGap(3, cards: false));
        Assert.AreEqual(32.0, ComposeRules.GridGap(8, cards: false));
    }
}
