using System.Globalization;
using Navree.Protocol;

namespace Navree.Design;

/// <summary>The day a screen is about and the days a chart names, written <c>YYYY-MM-DD</c>.</summary>
public static class PagerRules
{
    /// <summary>A day as the page writes it.</summary>
    /// <param name="text">The text.</param>
    /// <returns>The day, or none.</returns>
    public static DateOnly? Day(string? text) =>
        DateOnly.TryParseExact(text, "yyyy-MM-dd", CultureInfo.InvariantCulture, DateTimeStyles.None, out DateOnly day) ? day : null;

    /// <summary>The day as the core wants it in an event's text.</summary>
    /// <param name="day">The day.</param>
    /// <returns>The text.</returns>
    public static string Text(DateOnly day) => day.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture);

    /// <summary>Whether a day may be chosen: inside the first and last days where they are given.</summary>
    /// <param name="day">The day's text.</param>
    /// <param name="first">The first day, or none.</param>
    /// <param name="last">The last day, or none.</param>
    /// <returns>Whether it may.</returns>
    public static bool Choosable(string day, string? first, string? last)
    {
        if (Day(day) is not DateOnly chosen)
        {
            return false;
        }

        if (Day(first) is DateOnly low && chosen < low)
        {
            return false;
        }

        return !(Day(last) is DateOnly high && chosen > high);
    }

    /// <summary>The day as it is written for a person, in their culture: "Friday, 13 November 2026".</summary>
    /// <param name="day">The day's text.</param>
    /// <param name="culture">The culture.</param>
    /// <returns>The words, or the text itself when it is not a day.</returns>
    public static string Words(string day, CultureInfo? culture = null)
    {
        CultureInfo use = culture ?? ClientCulture.Current;
        return Day(day) is DateOnly found ? found.ToString(LongPattern(use), use) : day;
    }

    /// <summary>
    /// The culture's long date, except that a date written day first has no comma before its year ("Friday, 13 November 2026"; some
    /// machines' data for English (India) put one there).
    /// </summary>
    /// <param name="culture">The culture.</param>
    /// <returns>The pattern.</returns>
    public static string LongPattern(CultureInfo culture)
    {
        string pattern = culture.DateTimeFormat.LongDatePattern;
        string bare = pattern.Replace("dddd", string.Empty, StringComparison.Ordinal);
        int dayAt = bare.IndexOf('d', StringComparison.Ordinal);
        int monthAt = bare.IndexOf("MMMM", StringComparison.Ordinal);
        return dayAt >= 0 && monthAt > dayAt
            ? System.Text.RegularExpressions.Regex.Replace(pattern, @"MMMM,(\s*)y", "MMMM$1y")
            : pattern;
    }

    /// <summary>What the pager says of its day to a screen reader: its name and its date.</summary>
    /// <param name="pager">The pager.</param>
    /// <param name="culture">The culture.</param>
    /// <returns>The words.</returns>
    public static string Say(Models.DayPager pager, CultureInfo? culture = null) => pager.Day + ", " + Words(pager.Date, culture);
}

/// <summary>The calendar of days: a week a column, a day a row.</summary>
public static class HeatRules
{
    /// <summary>How strong each of the four shades is, as an opacity of the hue.</summary>
    public static IReadOnlyList<double> Opacities { get; } = [0.22, 0.45, 0.7, 1.0];

    /// <summary>
    /// The shade of a day, 0 to 3, or none for a day with no value (no data, not a day of none). The steps are where the second, third
    /// and fourth shade begin; without them, four equal parts from 0 to the highest.
    /// </summary>
    /// <param name="value">The day's value, or none.</param>
    /// <param name="steps">The page's steps.</param>
    /// <param name="highest">The highest value.</param>
    /// <returns>The shade, or none.</returns>
    public static int? Shade(double? value, IReadOnlyList<double> steps, double highest)
    {
        if (value is not double found)
        {
            return null;
        }

        double[] bounds = Bounds(steps, highest);
        return bounds.Count(bound => found >= bound);
    }

    /// <summary>Where the second, third and fourth shade begin.</summary>
    /// <param name="steps">The page's steps.</param>
    /// <param name="highest">The highest value.</param>
    /// <returns>Three numbers.</returns>
    public static double[] Bounds(IReadOnlyList<double> steps, double highest) =>
        steps.Count >= 3 ? [steps[0], steps[1], steps[2]] : [highest / 4, highest / 2, highest * 3 / 4];

    /// <summary>The first day shown: the start of the week the first day is in, by the person's first weekday.</summary>
    /// <param name="first">The first day.</param>
    /// <param name="firstWeekday">The first day of the week.</param>
    /// <returns>The start of that week.</returns>
    public static DateOnly Start(DateOnly first, DayOfWeek firstWeekday)
    {
        int back = ((int)first.DayOfWeek - (int)firstWeekday + 7) % 7;
        return first.AddDays(-back);
    }

    /// <summary>The column (week) and row (day of the week) of a date.</summary>
    /// <param name="date">The date.</param>
    /// <param name="start">The calendar's first day.</param>
    /// <returns>The week and the day.</returns>
    public static (int Week, int Day) Cell(DateOnly date, DateOnly start)
    {
        int days = date.DayNumber - start.DayNumber;
        int week = days >= 0 ? days / 7 : (days - 6) / 7;
        return (week, ((days % 7) + 7) % 7);
    }

    /// <summary>How many weeks the calendar spans.</summary>
    /// <param name="first">The first day.</param>
    /// <param name="last">The last day.</param>
    /// <param name="firstWeekday">The first day of the week.</param>
    /// <returns>The count, at least one.</returns>
    public static int Weeks(DateOnly first, DateOnly last, DayOfWeek firstWeekday) =>
        Math.Max(Cell(last, Start(first, firstWeekday)).Week + 1, 1);

    /// <summary>The week each month's name stands over: the first week whose first day starts that month, where the name would not crowd the one before.</summary>
    /// <param name="first">The first day.</param>
    /// <param name="weeks">How many weeks.</param>
    /// <param name="firstWeekday">The first day of the week.</param>
    /// <param name="room">The least number of weeks between two names.</param>
    /// <returns>The week and the month (1 to 12).</returns>
    public static IReadOnlyList<(int Week, int Month)> Months(DateOnly first, int weeks, DayOfWeek firstWeekday, int room = 3)
    {
        DateOnly start = Start(first, firstWeekday);
        List<(int Week, int Month)> found = [];
        int last = -1;
        for (int week = 0; week < weeks; week++)
        {
            DateOnly day = start.AddDays(week * 7);
            bool starts = week == 0 || day.Day <= 7;
            if (day.Month != last && starts)
            {
                if (found.Count > 0 && week - found[^1].Week < room)
                {
                    found.RemoveAt(found.Count - 1);
                }

                found.Add((week, day.Month));
                last = day.Month;
            }
        }

        return found;
    }
}

/// <summary>Every measure against every other.</summary>
public static class MatrixRules
{
    /// <summary>The relation of two measures, either way round.</summary>
    /// <param name="relations">The relations.</param>
    /// <param name="a">One measure.</param>
    /// <param name="b">The other.</param>
    /// <returns>The relation, or none.</returns>
    public static Models.Relation? Relation(IReadOnlyList<Models.Relation> relations, int a, int b) =>
        relations.FirstOrDefault(relation => (relation.A == a && relation.B == b) || (relation.A == b && relation.B == a));

    /// <summary>Whether a cell is drawn: with the lower half only, those under the diagonal.</summary>
    /// <param name="row">The row.</param>
    /// <param name="column">The column.</param>
    /// <param name="lower">Whether only the lower half is drawn.</param>
    /// <returns>Whether it is drawn.</returns>
    public static bool Drawn(int row, int column, bool lower) => lower ? column < row : row != column;

    /// <summary>How strong a relation is, 0 to 1 against the peak (1 when none is given); one that is not significant, or never tested, is none.</summary>
    /// <param name="relation">The relation, or none.</param>
    /// <param name="peak">The peak, or none.</param>
    /// <returns>The strength.</returns>
    public static double Strength(Models.Relation? relation, double? peak)
    {
        if (relation is null || relation.Value is not double value || !relation.Significant)
        {
            return 0;
        }

        double top = (peak ?? 1) > 0 ? peak ?? 1 : 1;
        return Math.Min(Math.Abs(value) / top, 1);
    }
}
