using System.Globalization;
using System.Text;
using Navree.Protocol;

namespace Navree.Design;

/// <summary>Numbers in the person's way of writing them.</summary>
public static class NumberWords
{
    /// <summary>A number with the decimals it needs and no more (one at most), grouped as the culture groups ("1,00,000" in India).</summary>
    /// <param name="value">The number.</param>
    /// <param name="culture">The culture; the person's own when left out.</param>
    /// <returns>The words.</returns>
    public static string Amount(double value, CultureInfo? culture = null)
    {
        culture ??= ClientCulture.Current;
        double rounded = Math.Round(value * 10, MidpointRounding.AwayFromZero) / 10;
        return rounded == Math.Round(rounded)
            ? ((long)rounded).ToString("N0", culture)
            : rounded.ToString("#,0.0", culture);
    }

    /// <summary>An amount with its unit: a space between them, except before a percent sign.</summary>
    /// <param name="amount">The amount's words.</param>
    /// <param name="unit">The unit, or none.</param>
    /// <returns>The words.</returns>
    public static string WithUnit(string amount, string? unit) =>
        string.IsNullOrEmpty(unit) ? amount : unit == "%" ? amount + unit : amount + " " + unit;
}

/// <summary>How a chart's values are written on its axis: the page's own way, as its words for each value show it.</summary>
/// <param name="Prefix">What comes before the digits (a sign of money).</param>
/// <param name="Suffix">What comes after.</param>
/// <param name="Indian">Whether the digits are grouped as India groups them: three, then twos.</param>
public sealed record ValueStyle(string Prefix = "", string Suffix = "", bool Indian = false);

/// <summary>Writing a chart's values as the page writes them.</summary>
public static class ValueWords
{
    /// <summary>The style of a chart's values: from the page's words for them, else from the unit its axis or its series name.</summary>
    /// <param name="chart">The chart.</param>
    /// <returns>The style.</returns>
    public static ValueStyle Style(Models.Chart chart)
    {
        string? shown = chart.Series.SelectMany(series => series.Shown).FirstOrDefault(words => words.Any(char.IsDigit));
        if (shown is not null && !Compound(shown))
        {
            return StyleOf(shown);
        }

        string? unit = chart.Y?.Unit ?? chart.Series.Select(series => series.Unit).FirstOrDefault(found => found is not null);
        return StyleOfUnit(unit);
    }

    /// <summary>The style a unit gives: rupees are written with the sign before and Indian grouping, a percent sign follows its number.</summary>
    /// <param name="unit">The unit, or none.</param>
    /// <returns>The style.</returns>
    public static ValueStyle StyleOfUnit(string? unit)
    {
        if (string.IsNullOrEmpty(unit))
        {
            return new ValueStyle();
        }

        if (unit is "₹" or "INR")
        {
            return new ValueStyle("₹", string.Empty, Indian: true);
        }

        return new ValueStyle(string.Empty, unit == "%" ? "%" : " " + unit);
    }

    /// <summary>Whether words write one value in more than one number, as a duration (<c>7 h 6 min</c>) or a time of day does.</summary>
    /// <param name="words">The words.</param>
    /// <returns>Whether there is more than one number.</returns>
    public static bool Compound(string words)
    {
        int numbers = 0;
        bool inNumber = false;
        char previous = ' ';
        foreach (char character in words)
        {
            if (char.IsDigit(character))
            {
                if (!inNumber)
                {
                    numbers++;
                }

                inNumber = true;
            }
            else if (inNumber && (character is ',' or '.') && char.IsDigit(previous))
            {
                // A separator inside a number: the number goes on if a digit follows.
            }
            else
            {
                inNumber = false;
            }

            previous = character;
        }

        return numbers > 1;
    }

    /// <summary>The style of one value as the page wrote it.</summary>
    /// <param name="words">The words.</param>
    /// <returns>The style.</returns>
    public static ValueStyle StyleOf(string words)
    {
        int first = words.ToList().FindIndex(char.IsDigit);
        int last = words.ToList().FindLastIndex(char.IsDigit);
        if (first < 0 || last < 0)
        {
            return new ValueStyle();
        }

        string prefix = words[..first];
        if (prefix.EndsWith('-') || prefix.EndsWith('−'))
        {
            prefix = prefix[..^1];
        }

        string suffix = words[(last + 1)..];
        string number = words[first..(last + 1)];
        string whole = number.Split('.')[0];
        int[] groups = [.. whole.Split(',').Select(group => group.Length)];

        // Indian grouping: the last group of three, those before it of two.
        bool indian = groups.Length > 2
            ? groups[1..^1].All(size => size == 2) && groups[^1] == 3
            : prefix.Contains('₹');
        return new ValueStyle(prefix, suffix, indian);
    }

    /// <summary>A value on the axis, in the style: whole numbers grouped, a fraction to one place.</summary>
    /// <param name="value">The value.</param>
    /// <param name="style">The style.</param>
    /// <returns>The words.</returns>
    public static string Text(double value, ValueStyle style)
    {
        bool negative = value < 0;
        double magnitude = Math.Abs(value);
        double whole = Math.Truncate(magnitude);
        double fraction = magnitude - whole;
        string digits = ((long)whole).ToString(CultureInfo.InvariantCulture);
        digits = style.Indian ? IndianGroups(digits) : WesternGroups(digits);
        if (fraction >= 0.05)
        {
            digits += "." + (((int)Math.Round(fraction * 10)) % 10).ToString(CultureInfo.InvariantCulture);
        }

        return (negative ? "−" : string.Empty) + style.Prefix + digits + style.Suffix;
    }

    /// <summary>Digits grouped in threes.</summary>
    /// <param name="digits">The digits.</param>
    /// <returns>The grouped digits.</returns>
    public static string WesternGroups(string digits)
    {
        List<string> groups = [];
        string rest = digits;
        while (rest.Length > 3)
        {
            groups.Insert(0, rest[^3..]);
            rest = rest[..^3];
        }

        groups.Insert(0, rest);
        return string.Join(',', groups);
    }

    /// <summary>Digits grouped as India groups them: three, then twos.</summary>
    /// <param name="digits">The digits.</param>
    /// <returns>The grouped digits.</returns>
    public static string IndianGroups(string digits)
    {
        if (digits.Length <= 3)
        {
            return digits;
        }

        List<string> groups = [digits[^3..]];
        string rest = digits[..^3];
        while (rest.Length > 2)
        {
            groups.Insert(0, rest[^2..]);
            rest = rest[..^2];
        }

        if (rest.Length > 0)
        {
            groups.Insert(0, rest);
        }

        return string.Join(',', groups);
    }
}

/// <summary>What Narrator says of a part: one stop for one thing, each word said once.</summary>
public static class SpeechRules
{
    /// <summary>The value a part says after its name, without its name again: a ring named <c>Health score</c> whose summary is <c>Health score: 79 of 100.</c> says <c>79 of 100.</c> after it.</summary>
    /// <param name="value">The words.</param>
    /// <param name="label">The part's name.</param>
    /// <returns>The words without the name.</returns>
    public static string Value(string value, string? label)
    {
        string name = label?.Trim() ?? string.Empty;
        if (name.Length == 0)
        {
            return value;
        }

        return Dropping(name, value) ?? value;
    }

    /// <summary>The words with <paramref name="lead"/> and what joins it to the rest taken off the start, ignoring case; none when they do not start so or nothing would be left.</summary>
    /// <param name="lead">The words to take off.</param>
    /// <param name="text">The text.</param>
    /// <returns>The rest, or none.</returns>
    public static string? Dropping(string lead, string text)
    {
        if (!text.StartsWith(lead, StringComparison.OrdinalIgnoreCase))
        {
            return null;
        }

        string rest = text[lead.Length..];
        const string joins = ":,.-–— ";
        if (rest.Length > 0 && !joins.Contains(rest[0]))
        {
            // Only a word's end counts: "Recovery" does not lead "Recoveryish".
            return null;
        }

        rest = rest.AsSpan().TrimStart(joins).ToString();
        return rest.Length == 0 ? null : rest;
    }

    /// <summary>A label and its value, said as one stop.</summary>
    /// <param name="label">The label.</param>
    /// <param name="value">The value.</param>
    /// <returns>The words.</returns>
    public static string Fact(string label, string value)
    {
        string trimmed = value.Trim();
        return trimmed.Length == 0 ? label : label + ", " + trimmed;
    }
}

/// <summary>The key of a chart: each kind's name and what the page says of it.</summary>
public static class KeyRules
{
    /// <summary>An entry: the name, and the page's own words of its share or total when it has them.</summary>
    /// <param name="name">The name.</param>
    /// <param name="detail">The words, or none.</param>
    /// <returns>The entry's words.</returns>
    public static string Entry(string name, string? detail) =>
        string.IsNullOrEmpty(detail) ? name : name + " · " + detail;
}

/// <summary>Which way a trend goes and how it is said.</summary>
public static class TrendRules
{
    /// <summary>The system icon font's arrow for a direction.</summary>
    /// <param name="direction">The direction.</param>
    /// <returns>The glyph.</returns>
    public static string Glyph(Models.Direction direction) =>
        direction switch
        {
            Models.Direction.Up => "",
            Models.Direction.Down => "",
            _ => "",
        };

    /// <summary>What a trend says to a screen reader when it has no words of its own.</summary>
    /// <param name="direction">The direction.</param>
    /// <returns>The word.</returns>
    public static string Word(Models.Direction direction) =>
        direction switch
        {
            Models.Direction.Up => "Up",
            Models.Direction.Down => "Down",
            _ => "Steady",
        };
}

/// <summary>The words of a conversation's messages.</summary>
public static class MessageRules
{
    /// <summary>What an internal note says it is.</summary>
    public const string NoteWords = "Internal note, not sent to the customer";

    /// <summary>The words and icon glyph for how far a message got; none for a delivery this client does not know.</summary>
    /// <param name="delivery">The delivery.</param>
    /// <returns>The words and the glyph, or none.</returns>
    public static (string Text, string Glyph)? Delivery(Models.Delivery delivery) =>
        delivery switch
        {
            Models.Delivery.Sending => ("Sending", ""),
            Models.Delivery.Sent => ("Sent", ""),
            Models.Delivery.Delivered => ("Delivered", ""),
            Models.Delivery.Read => ("Read", ""),
            Models.Delivery.Failed => ("Not sent", ""),
            _ => null,
        };
}

/// <summary>Where a flag emoji stands in a text: Windows draws no flag for a pair of regional indicator letters, so the client draws it.</summary>
public static class FlagText
{
    /// <summary>A stretch of a text: words, or a flag by its two-letter country code.</summary>
    /// <param name="Text">The words, when it is words.</param>
    /// <param name="Country">The country code, when it is a flag.</param>
    public sealed record Piece(string Text, string? Country);

    /// <summary>Whether a text holds a flag.</summary>
    /// <param name="text">The text.</param>
    /// <returns>Whether it does.</returns>
    public static bool Holds(string text)
    {
        for (int at = 0; at < text.Length; at++)
        {
            if (CodeAt(text, at) is not null)
            {
                return true;
            }
        }

        return false;
    }

    /// <summary>Splits a text at its flags.</summary>
    /// <param name="text">The text.</param>
    /// <returns>The pieces, in order; words keep their spacing.</returns>
    public static IReadOnlyList<Piece> Split(string text)
    {
        List<Piece> pieces = [];
        StringBuilder words = new();
        int at = 0;
        while (at < text.Length)
        {
            string? code = CodeAt(text, at);
            if (code is null)
            {
                words.Append(text[at]);
                at++;
                continue;
            }

            if (words.Length > 0)
            {
                pieces.Add(new Piece(words.ToString(), null));
                words.Clear();
            }

            pieces.Add(new Piece(string.Empty, code));
            at += 4;
        }

        if (words.Length > 0)
        {
            pieces.Add(new Piece(words.ToString(), null));
        }

        return pieces;
    }

    // Two regional indicator symbols, each a surrogate pair (U+1F1E6 to U+1F1FF), make the country's code.
    private static string? CodeAt(string text, int at)
    {
        if (at + 3 >= text.Length)
        {
            return null;
        }

        int? first = Indicator(text, at);
        int? second = Indicator(text, at + 2);
        return first is int a && second is int b ? $"{(char)('A' + a)}{(char)('A' + b)}" : null;
    }

    private static int? Indicator(string text, int at)
    {
        if (!char.IsHighSurrogate(text[at]) || !char.IsLowSurrogate(text[at + 1]))
        {
            return null;
        }

        int point = char.ConvertToUtf32(text[at], text[at + 1]);
        return point is >= 0x1F1E6 and <= 0x1F1FF ? point - 0x1F1E6 : null;
    }
}

/// <summary>More about a part, on request.</summary>
public static class HintRules
{
    /// <summary>The name of the button that opens a hint: "About the health score".</summary>
    /// <param name="hint">The hint.</param>
    /// <returns>The name.</returns>
    public static string ActionName(Models.Hint hint)
    {
        string about = hint.About.Trim();
        return about.Length == 0 ? "About" : "About " + about;
    }

    /// <summary>The words a chart's own title says: not when the screen has just said the same words over it.</summary>
    /// <param name="title">The chart's title.</param>
    /// <param name="wordsAbove">The words of the texts just above it.</param>
    /// <returns>Whether the chart says its own title.</returns>
    public static bool ShowsTitle(string? title, IEnumerable<string> wordsAbove) =>
        !string.IsNullOrEmpty(title) && !wordsAbove.Any(words => string.Equals(words, title, StringComparison.OrdinalIgnoreCase));
}
