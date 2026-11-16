using System.Globalization;

namespace Navree.Design.Pages;

/// <summary>
/// The one form a field's value is compared in, for the fields whose control does not hold the core's own words: a time picker holds a time, not
/// <c>"09:00:00"</c>, and two ways of writing one time are one value. A value the control cannot hold (words that are no time, no day, no
/// number) is none, written as the empty text, and the control then shows nothing: it never throws and never keeps what it showed before.
/// </summary>
public static class ShownForms
{
    /// <summary>The time a field holds, when its words say one between 00:00 and 23:59: <c>09:05</c>, <c>9:05:30</c>, or a moment with a time in it.</summary>
    /// <param name="text">What the core holds.</param>
    /// <returns>The time of day, or null for words that hold none (a day with no time, a bare number, a time of 24 hours or more).</returns>
    public static TimeSpan? TimeOf(string? text)
    {
        if (string.IsNullOrWhiteSpace(text) || !text.Contains(':', StringComparison.Ordinal))
        {
            return null;
        }

        if (TimeSpan.TryParse(text, CultureInfo.InvariantCulture, out TimeSpan span))
        {
            return span >= TimeSpan.Zero && span < TimeSpan.FromDays(1) ? span : null;
        }

        return DateTimeOffset.TryParse(text, CultureInfo.InvariantCulture, DateTimeStyles.AssumeLocal, out DateTimeOffset moment) ? moment.TimeOfDay : null;
    }

    /// <summary>A time as it is compared and sent: hours and minutes, <c>09:05</c>; empty for none.</summary>
    /// <param name="text">What the core holds.</param>
    /// <returns>The form.</returns>
    public static string Time(string? text) => TimeOf(text) is TimeSpan time ? Time(time) : string.Empty;

    /// <summary>A time as it is compared and sent: hours and minutes.</summary>
    /// <param name="time">The time of day.</param>
    /// <returns>The form.</returns>
    public static string Time(TimeSpan time) => time.ToString(@"hh\:mm", CultureInfo.InvariantCulture);

    /// <summary>The day a field holds, when its words say one.</summary>
    /// <param name="text">What the core holds.</param>
    /// <returns>The day, or null for words that hold none.</returns>
    public static DateTimeOffset? DayOf(string? text) =>
        !string.IsNullOrWhiteSpace(text) && DateTimeOffset.TryParse(text, CultureInfo.InvariantCulture, DateTimeStyles.AssumeLocal, out DateTimeOffset day) ? day : null;

    /// <summary>A day as it is compared and sent: <c>2026-11-16</c>; empty for none.</summary>
    /// <param name="text">What the core holds.</param>
    /// <returns>The form.</returns>
    public static string Day(string? text) => DayOf(text) is DateTimeOffset day ? Day(day) : string.Empty;

    /// <summary>A day as it is compared and sent.</summary>
    /// <param name="day">The day.</param>
    /// <returns>The form.</returns>
    public static string Day(DateTimeOffset day) => day.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture);

    /// <summary>The number a field holds, when its words say one.</summary>
    /// <param name="text">What the core holds.</param>
    /// <returns>The number, or null for words that hold none.</returns>
    public static double? NumberOf(string? text) =>
        double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out double number) && double.IsFinite(number) ? number : null;

    /// <summary>A number as it is compared: one way of writing it, so <c>5</c> and <c>5.0</c> are one value; empty for none.</summary>
    /// <param name="text">What the core holds.</param>
    /// <returns>The form.</returns>
    public static string Number(string? text) => NumberOf(text) is double number ? number.ToString("R", CultureInfo.InvariantCulture) : string.Empty;

    /// <summary>Several chosen values as they are compared: the same values in any order are one value.</summary>
    /// <param name="values">The values.</param>
    /// <returns>The form.</returns>
    public static string Many(IEnumerable<string> values)
    {
        ArgumentNullException.ThrowIfNull(values);
        return System.Text.Json.JsonSerializer.Serialize(values.Distinct(StringComparer.Ordinal).Order(StringComparer.Ordinal).ToArray());
    }
}
