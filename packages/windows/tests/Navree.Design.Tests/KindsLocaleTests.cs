using System.Globalization;

namespace Navree.Design.Tests;

/// <summary>
/// What the client writes itself (a day, a time of day, a number) follows the locale it was started with. The cultures here carry the patterns the
/// tests assume (a machine's own data for a region differs: Windows' English (India) has no weekday in its long date, a 24 hour clock, another
/// grouping), so the tests say what the client does with a culture and not what a machine knows of a region.
/// </summary>
[TestClass]
public sealed class KindsLocaleTests
{
    private static CultureInfo Culture(string longDate, string shortTime, string am, string pm, params int[] groups)
    {
        CultureInfo culture = (CultureInfo)CultureInfo.InvariantCulture.Clone();
        culture.DateTimeFormat.LongDatePattern = longDate;
        culture.DateTimeFormat.ShortTimePattern = shortTime;
        culture.DateTimeFormat.AMDesignator = am;
        culture.DateTimeFormat.PMDesignator = pm;
        culture.NumberFormat.NumberGroupSizes = groups;
        culture.NumberFormat.NumberGroupSeparator = ",";
        culture.NumberFormat.NumberDecimalSeparator = ".";
        return culture;
    }

    // Day first, the twelve hour clock in lower case, groups of three and then two.
    private static readonly CultureInfo India = Culture("dddd, d MMMM yyyy", "h:mm tt", "am", "pm", 3, 2);

    // The same, with the comma some machines' data put before the year.
    private static readonly CultureInfo IndiaWithComma = Culture("dddd, d MMMM, yyyy", "h:mm tt", "am", "pm", 3, 2);

    // Month first, the twelve hour clock in capitals, groups of three.
    private static readonly CultureInfo America = Culture("dddd, MMMM d, yyyy", "h:mm tt", "AM", "PM", 3);

    // The twenty-four hour clock.
    private static readonly CultureInfo Europe = Culture("dddd d MMMM yyyy", "HH:mm", string.Empty, string.Empty, 3);

    [TestMethod]
    public void A_day_is_written_the_way_the_locale_writes_it()
    {
        Assert.AreEqual("Friday, 13 November 2026", PagerRules.Words("2026-11-13", India));
        Assert.AreEqual("Friday, November 13, 2026", PagerRules.Words("2026-11-13", America));
        Assert.AreEqual("Friday 13 November 2026", PagerRules.Words("2026-11-13", Europe));
    }

    [TestMethod]
    public void A_day_written_day_first_has_no_comma_before_its_year_and_a_day_written_month_first_keeps_it()
    {
        Assert.AreEqual("Friday, 13 November 2026", PagerRules.Words("2026-11-13", IndiaWithComma));
        Assert.AreEqual("dddd, d MMMM yyyy", PagerRules.LongPattern(IndiaWithComma));
        Assert.AreEqual("dddd, MMMM d, yyyy", PagerRules.LongPattern(America));
    }

    [TestMethod]
    public void Text_that_is_not_a_day_is_given_back_as_it_came()
    {
        Assert.AreEqual("last Tuesday", PagerRules.Words("last Tuesday", India));
    }

    [TestMethod]
    public void A_time_of_day_is_written_as_the_locales_clock_writes_it()
    {
        double quarterPastSeven = 7.25;
        Assert.IsFalse(ClockRules.Uses24(India));
        Assert.IsTrue(ClockRules.Uses24(Europe));
        Assert.AreEqual("7:15 am", ClockRules.TimeLabel(quarterPastSeven, ClockRules.Uses24(India), India));
        Assert.AreEqual("7:15 AM", ClockRules.TimeLabel(quarterPastSeven, ClockRules.Uses24(America), America));
        Assert.AreEqual("07:15", ClockRules.TimeLabel(quarterPastSeven, ClockRules.Uses24(Europe), Europe));
        Assert.AreEqual("6 pm", ClockRules.QuarterLabel(18, ClockRules.Uses24(India), India));
        Assert.AreEqual("18", ClockRules.QuarterLabel(18, ClockRules.Uses24(Europe), Europe));
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
