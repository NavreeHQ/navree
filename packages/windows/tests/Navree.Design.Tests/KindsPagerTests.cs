namespace Navree.Design.Tests;

/// <summary>The days a calendar control is given and the day it gives back: the person's own days, in every time zone.</summary>
[TestClass]
public sealed class KindsPagerTests
{
    private static TimeZoneInfo Zone(string name, TimeSpan offset) => TimeZoneInfo.CreateCustomTimeZone(name, offset, name, name);

    private static readonly TimeZoneInfo West = Zone("minus-five", TimeSpan.FromHours(-5));
    private static readonly TimeZoneInfo India = Zone("plus-five-thirty", new TimeSpan(5, 30, 0));
    private static readonly TimeZoneInfo Far = Zone("plus-fourteen", TimeSpan.FromHours(14));
    private static readonly TimeZoneInfo Baker = Zone("minus-twelve", TimeSpan.FromHours(-12));

    [TestMethod]
    public void A_day_given_to_a_calendar_is_that_day_at_minus_five_and_at_plus_five_thirty()
    {
        DateOnly day = new(2026, 11, 13);
        Assert.AreEqual(day, PagerRules.Midnight(day, West).Date.ToDateOnly());
        Assert.AreEqual(day, PagerRules.Midnight(day, India).Date.ToDateOnly());
        Assert.AreEqual(TimeSpan.FromHours(-5), PagerRules.Midnight(day, West).Offset);
        Assert.AreEqual(new TimeSpan(5, 30, 0), PagerRules.Midnight(day, India).Offset);

        // The instant is the person's midnight: five hours after midnight UTC at UTC-5, and the evening before it at UTC+5:30.
        Assert.AreEqual(new DateTimeOffset(2026, 11, 13, 5, 0, 0, TimeSpan.Zero), PagerRules.Midnight(day, West));
        Assert.AreEqual(new DateTimeOffset(2026, 11, 12, 18, 30, 0, TimeSpan.Zero), PagerRules.Midnight(day, India));
    }

    [TestMethod]
    public void The_day_a_calendar_gives_back_is_the_persons_day_whatever_offset_it_comes_with()
    {
        DateOnly day = new(2026, 11, 13);
        foreach (TimeZoneInfo zone in new[] { West, India, Far, Baker, TimeZoneInfo.Utc })
        {
            DateTimeOffset given = PagerRules.Midnight(day, zone);
            Assert.AreEqual(day, PagerRules.DayOf(given, zone), zone.Id);

            // The same instant written with another offset is still that day.
            Assert.AreEqual(day, PagerRules.DayOf(given.ToUniversalTime(), zone), zone.Id);
            Assert.AreEqual(day, PagerRules.DayOf(given.ToOffset(TimeSpan.FromHours(9)), zone), zone.Id);
        }
    }

    [TestMethod]
    public void Midnight_in_utc_is_the_day_before_for_a_person_west_of_it_which_is_the_fault_the_rule_removes()
    {
        DateOnly day = new(2026, 11, 13);
        DateTimeOffset utcMidnight = new(2026, 11, 13, 0, 0, 0, TimeSpan.Zero);
        Assert.AreEqual(new DateOnly(2026, 11, 12), PagerRules.DayOf(utcMidnight, West));
        Assert.AreEqual(day, PagerRules.DayOf(utcMidnight, India));
        Assert.AreNotEqual(utcMidnight, PagerRules.Midnight(day, West));
    }

    [TestMethod]
    public void The_first_and_the_last_day_of_the_calendar_are_the_first_and_the_last_day_the_page_names_in_every_zone_there_is()
    {
        DateOnly first = new(2026, 3, 8);
        DateOnly last = new(2026, 11, 1);
        foreach (TimeZoneInfo zone in TimeZoneInfo.GetSystemTimeZones())
        {
            Assert.AreEqual(first, PagerRules.DayOf(PagerRules.Midnight(first, zone), zone), zone.Id);
            Assert.AreEqual(last, PagerRules.DayOf(PagerRules.Midnight(last, zone), zone), zone.Id);
            Assert.IsTrue(PagerRules.Midnight(first, zone) < PagerRules.Midnight(last, zone), zone.Id);
        }
    }

    [TestMethod]
    public void A_day_at_the_ends_of_the_calendar_of_time_does_not_throw_in_any_zone()
    {
        foreach (TimeZoneInfo zone in new[] { West, India, Far, Baker, TimeZoneInfo.Utc })
        {
            Assert.AreEqual(DateOnly.MinValue, PagerRules.DayOf(PagerRules.Midnight(DateOnly.MinValue, TimeZoneInfo.Utc), TimeZoneInfo.Utc));
            Assert.IsNotNull(PagerRules.Midnight(new DateOnly(1, 1, 2), zone));
            Assert.IsNotNull(PagerRules.Midnight(new DateOnly(9999, 12, 30), zone));
        }
    }
}

internal static class DateExtensions
{
    internal static DateOnly ToDateOnly(this DateTime value) => DateOnly.FromDateTime(value);
}
