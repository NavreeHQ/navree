using System.Globalization;
using System.Text.RegularExpressions;
using Navree.Protocol;

namespace Navree.Design;

/// <summary>A ring: one value against its goal, a second lap past it.</summary>
public static class RingRules
{
    /// <summary>How far round a value goes: the whole laps past the goal and the part of the lap it is on.</summary>
    /// <param name="Laps">The whole laps before the one being drawn.</param>
    /// <param name="Part">How far round that lap, 0 to 1.</param>
    public sealed record Sweep(int Laps, double Part)
    {
        /// <summary>Whether the value is past its goal: the arc goes round again.</summary>
        public bool Past => Laps >= 1;
    }

    /// <summary>The ring's width before the person's text size: the hero ring, one of several, a small one beside words.</summary>
    /// <param name="prominence">How much room the ring has.</param>
    /// <returns>The width.</returns>
    public static double Diameter(Models.Prominence prominence) =>
        prominence switch
        {
            Models.Prominence.High => 188,
            Models.Prominence.Low => 34,
            _ => 96,
        };

    /// <summary>The arc's width for a ring that wide; a small ring keeps a thicker arc for its size so it still reads as a ring.</summary>
    /// <param name="prominence">How much room the ring has.</param>
    /// <param name="diameter">The ring's width.</param>
    /// <returns>The arc's width.</returns>
    public static double Stroke(Models.Prominence prominence, double diameter) =>
        prominence switch
        {
            Models.Prominence.High => diameter * 0.115,
            Models.Prominence.Low => diameter * 0.16,
            _ => diameter * 0.12,
        };

    /// <summary>How much a ring grows with the person's text, at most: a small ring stands in a row of text and keeps its size.</summary>
    /// <param name="prominence">How much room the ring has.</param>
    /// <returns>The most the ring grows, as a multiple.</returns>
    public static double Growth(Models.Prominence prominence) =>
        prominence switch
        {
            Models.Prominence.High => 1.3,
            Models.Prominence.Low => 1,
            _ => 1.5,
        };

    /// <summary>Whether words go in the middle: a small ring says its value to a screen reader only.</summary>
    /// <param name="prominence">How much room the ring has.</param>
    /// <param name="centre">What the page puts in the middle.</param>
    /// <returns>Whether the middle has words.</returns>
    public static bool ShowsCentre(Models.Prominence prominence, Models.Centre centre) =>
        prominence != Models.Prominence.Low && centre != Models.Centre.Nothing;

    /// <summary>Whether the ring says its label, status and trend under it: a small ring has no room and its summary says them.</summary>
    /// <param name="prominence">How much room the ring has.</param>
    /// <returns>Whether the words stand under it.</returns>
    public static bool SaysWords(Models.Prominence prominence) => prominence != Models.Prominence.Low;

    /// <summary>The size of the number in a ring's middle: 0.3 of the ring.</summary>
    /// <param name="diameter">The ring's width.</param>
    /// <returns>The number's size.</returns>
    public static double CentreSize(double diameter) => diameter * 0.3;

    /// <summary>How far round a value goes; none for no value (an empty ring and a dash).</summary>
    /// <param name="value">The value, or none.</param>
    /// <param name="max">The goal.</param>
    /// <returns>The sweep, or none.</returns>
    public static Sweep? SweepOf(double? value, double max)
    {
        if (value is not double found)
        {
            return null;
        }

        if (max <= 0)
        {
            return new Sweep(0, found > 0 ? 1 : 0);
        }

        double share = Math.Max(found / max, 0);
        int laps = (int)Math.Floor(share);
        double part = share - laps;

        // Exactly at the goal is one whole lap, not a second one starting.
        return laps >= 1 && part == 0 ? new Sweep(laps - 1, 1) : new Sweep(laps, part);
    }

    /// <summary>What the middle of a ring says: its value without the unit, or a dash when it has none.</summary>
    /// <param name="ring">The ring.</param>
    /// <param name="culture">The culture of the numbers.</param>
    /// <returns>The words.</returns>
    public static string Centre(Models.Ring ring, CultureInfo? culture = null) =>
        ring.Value is double value ? ring.Shown ?? NumberWords.Amount(value, culture) : "\u2013";
}

/// <summary>Concentric rings, the first outside, as the system's activity rings.</summary>
public static class RingsRules
{
    /// <summary>At most six are drawn, as the protocol says.</summary>
    public const int Most = 6;

    /// <summary>The gap between two rings, as a share of the arc's width.</summary>
    public const double GapShare = 0.12;

    /// <summary>The width of the whole before the person's text size.</summary>
    /// <param name="prominence">How much room the rings have.</param>
    /// <param name="count">How many rings.</param>
    /// <returns>The width.</returns>
    public static double Diameter(Models.Prominence prominence, int count) =>
        prominence switch
        {
            Models.Prominence.Low => 30,
            Models.Prominence.High => 188,
            _ => 150 + (10 * Math.Max(count - 3, 0)),
        };

    /// <summary>The width of each arc for a whole this wide: the rings and their gaps take a little under half the radius.</summary>
    /// <param name="diameter">The whole's width.</param>
    /// <param name="count">How many rings.</param>
    /// <param name="prominence">How much room the rings have.</param>
    /// <returns>The arc's width.</returns>
    public static double Stroke(double diameter, int count, Models.Prominence prominence)
    {
        double n = Math.Min(Math.Max(count, 1), Most);
        double share = prominence == Models.Prominence.Low ? 0.82 : 0.5;
        return diameter * share / 2 / (n + ((n - 1) * GapShare));
    }

    /// <summary>The diameter of a ring (0 the outside one), at the middle of its arc.</summary>
    /// <param name="diameter">The whole's width.</param>
    /// <param name="index">Which ring.</param>
    /// <param name="stroke">The arc's width.</param>
    /// <returns>The diameter.</returns>
    public static double RingDiameter(double diameter, int index, double stroke) =>
        diameter - stroke - (index * 2 * stroke * (1 + GapShare));

    /// <summary>The words a ring's key says of its value: the value over its goal with the unit ("441/500 kcal").</summary>
    /// <param name="ring">The ring.</param>
    /// <param name="culture">The culture of the numbers.</param>
    /// <returns>The words.</returns>
    public static string Value(Models.RingValue ring, CultureInfo? culture = null)
    {
        if (ring.Value is not double value)
        {
            return "\u2013";
        }

        string shown = ring.Shown ?? NumberWords.Amount(value, culture);
        string goal = NumberWords.Amount(ring.Max, culture);
        return ring.Unit is string unit ? shown + "/" + goal + " " + unit : shown + "/" + goal;
    }

    /// <summary>Whether the rings stand beside a key of their values.</summary>
    /// <param name="rings">The rings.</param>
    /// <returns>Whether there is a key.</returns>
    public static bool HasKey(Models.Rings rings) =>
        rings.Prominence != Models.Prominence.Low && (rings.Legend || rings.Centre == Models.RingsCentre.Values);
}

/// <summary>A gauge: a value on an open arc.</summary>
public static class GaugeRules
{
    /// <summary>The arc runs through this many degrees, open at the foot, as the system's circular gauge is.</summary>
    public const double Sweep = 240;

    /// <summary>The angle of the arc's start, in degrees from three o'clock going clockwise: the lower left.</summary>
    public const double Start = 150;

    /// <summary>The share of its square an open arc needs in height: the foot holds the scale's end labels.</summary>
    public const double HeightShare = 0.84;

    /// <summary>The angle a place along the scale stands at.</summary>
    /// <param name="position">0 to 1.</param>
    /// <returns>The angle, in degrees.</returns>
    public static double Angle(double position) => Start + (Sweep * position);

    /// <summary>The base width of a gauge before the person's text size: one that leads its card, and one beside a figure's words.</summary>
    /// <param name="aside">Whether it stands beside words.</param>
    /// <returns>The width.</returns>
    public static double Diameter(bool aside) => aside ? 52 : 184;

    /// <summary>A gauge's words as the number to write large and what follows it; words that do not start with a number are written whole.</summary>
    /// <param name="shown">The words.</param>
    /// <returns>The number and the rest.</returns>
    public static (string Number, string? Unit) Split(string shown)
    {
        int space = shown.IndexOf(' ', StringComparison.Ordinal);
        if (space <= 0)
        {
            return (shown, null);
        }

        string head = shown[..space];
        bool number = head.Any(char.IsDigit) && head.All(character => char.IsDigit(character) || ",.-\u2212+".Contains(character, StringComparison.Ordinal));
        if (!number)
        {
            return (shown, null);
        }

        string rest = shown[(space + 1)..].Trim();
        return (head, rest.Length == 0 ? null : rest);
    }
}

/// <summary>The day clock: 24 hours round a dial, midnight at the top.</summary>
public static class ClockRules
{
    /// <summary>The hours named round the dial: the four quarters, midnight at the top.</summary>
    public static IReadOnlyList<int> Quarters { get; } = [0, 6, 12, 18];

    /// <summary>The base width of a dial before the person's text size.</summary>
    public const double Diameter = 300;

    /// <summary>The radii of a dial from its middle to its edge.</summary>
    /// <param name="Labels">The middle of the quarter labels, outside everything.</param>
    /// <param name="Moments">The line the day's moments stand on.</param>
    /// <param name="Outer">Where the hours' bars end at their longest.</param>
    /// <param name="Inner">Where they start.</param>
    public sealed record Dial(double Labels, double Moments, double Outer, double Inner);

    /// <summary>The angle in degrees (0 at three o'clock, clockwise) of a time of day in hours.</summary>
    /// <param name="hours">The hours past midnight.</param>
    /// <returns>The angle.</returns>
    public static double Angle(double hours) => -90 + (hours * 15);

    /// <summary>How far out an hour's mark reaches, 0 to 1: its level, else its value against the most, else nothing.</summary>
    /// <param name="hour">The hour.</param>
    /// <param name="most">The most an hour holds.</param>
    /// <returns>The reach, or none for an hour with no reading.</returns>
    public static double? Reach(Models.Hour hour, double? most)
    {
        if (hour.Level is double level)
        {
            return Math.Clamp(level, 0, 1);
        }

        if (hour.Value is not double value || most is not double top)
        {
            return null;
        }

        return top <= 0 ? 0 : Math.Clamp(value / top, 0, 1);
    }

    /// <summary>The most an hour holds: the clock's own maximum, else the busiest hour.</summary>
    /// <param name="clock">The clock.</param>
    /// <returns>The most, or none.</returns>
    public static double? Most(Models.DayClock clock)
    {
        if (clock.Max is double max)
        {
            return max;
        }

        double[] values = [.. clock.Hours.Where(hour => hour.Value.HasValue).Select(hour => hour.Value!.Value)];
        return values.Length == 0 ? null : values.Max();
    }

    /// <summary>Whether the culture writes the time on a 24 hour clock.</summary>
    /// <param name="culture">The culture.</param>
    /// <returns>Whether it does.</returns>
    public static bool Uses24(CultureInfo culture) => culture.DateTimeFormat.ShortTimePattern.Contains('H', StringComparison.Ordinal);

    /// <summary>An hour as the person's clock writes it round the dial: "12 am" and "6 pm" on a 12 hour clock, "00" and "18" on a 24 hour one.</summary>
    /// <param name="hour">The hour.</param>
    /// <param name="uses24">Whether the clock has 24 hours.</param>
    /// <param name="culture">The culture.</param>
    /// <returns>The words.</returns>
    public static string QuarterLabel(int hour, bool uses24, CultureInfo culture)
    {
        DateTime time = new(2000, 1, 1, ((hour % 24) + 24) % 24, 0, 0, DateTimeKind.Unspecified);
        return uses24 ? time.ToString("HH", culture) : time.ToString("h tt", culture);
    }

    /// <summary>A time of day in hours past midnight as the person's clock writes it: "7:07 am" on a 12 hour clock, "07:07" on a 24 hour one.</summary>
    /// <param name="at">The hours past midnight.</param>
    /// <param name="uses24">Whether the clock has 24 hours.</param>
    /// <param name="culture">The culture.</param>
    /// <returns>The words.</returns>
    public static string TimeLabel(double at, bool uses24, CultureInfo culture)
    {
        int minutes = ((int)Math.Round((((at % 24) + 24) % 24) * 60, MidpointRounding.AwayFromZero)) % (24 * 60);
        DateTime time = new(2000, 1, 1, minutes / 60, minutes % 60, 0, DateTimeKind.Unspecified);
        return uses24 ? time.ToString("HH:mm", culture) : time.ToString("h:mm tt", culture);
    }

    /// <summary>
    /// Where the day's moments stand on the dial: at their hour, but no two closer than <paramref name="gap"/> hours, so marks that
    /// would sit on one another stand side by side. The order stays, and moments far apart do not move.
    /// </summary>
    /// <param name="ats">The moments' hours.</param>
    /// <param name="gap">The least gap, in hours.</param>
    /// <returns>Where each stands, in the order given.</returns>
    public static IReadOnlyList<double> Spread(IReadOnlyList<double> ats, double gap)
    {
        int[] order = [.. Enumerable.Range(0, ats.Count).OrderBy(index => ats[index])];
        double[] placed = [.. order.Select(index => ats[index])];
        for (int round = 0; round < 40; round++)
        {
            for (int at = 1; at < placed.Length; at++)
            {
                if (placed[at] - placed[at - 1] < gap)
                {
                    double middle = (placed[at] + placed[at - 1]) / 2;
                    placed[at - 1] = middle - (gap / 2);
                    placed[at] = middle + (gap / 2);
                }
            }
        }

        double[] result = new double[ats.Count];
        for (int rank = 0; rank < order.Length; rank++)
        {
            result[order[rank]] = placed[rank];
        }

        return result;
    }

    /// <summary>The radii of a dial: the quarters' names at the edge, the line of the moments inside them, the hours' bars inside that, and the hole in the middle for the day's total.</summary>
    /// <param name="half">From the middle to the edge.</param>
    /// <param name="label">The height of a quarter's name.</param>
    /// <param name="mark">The width of a moment's mark.</param>
    /// <returns>The radii.</returns>
    public static Dial DialOf(double half, double label, double mark)
    {
        double labels = half - (label / 2);
        double moments = labels - (label / 2) - 2 - (mark / 2);
        double outer = moments - (mark / 2) - 3;
        return new Dial(labels, moments, outer, outer * 0.46);
    }

    /// <summary>The width of an hour's bar at the start of the bars: most of the room between neighbours.</summary>
    /// <param name="inner">The inner radius.</param>
    /// <returns>The width.</returns>
    public static double BarWidth(double inner) => 2 * Math.PI * inner / 24 * 0.6;

    /// <summary>Whether a moment's name already says its time (an older page wrote it in).</summary>
    /// <param name="name">The name.</param>
    /// <returns>Whether it holds a time.</returns>
    public static bool HoldsTime(string name) => Regex.IsMatch(name, @"\d:\d\d");

    /// <summary>What a moment says in the clock's key: its name and its time in the person's clock.</summary>
    /// <param name="name">The name.</param>
    /// <param name="at">The hour.</param>
    /// <param name="uses24">Whether the clock has 24 hours.</param>
    /// <param name="culture">The culture.</param>
    /// <returns>The words.</returns>
    public static string MomentEntry(string name, double at, bool uses24, CultureInfo culture) =>
        HoldsTime(name) ? name : name + ", " + TimeLabel(at, uses24, culture);
}
