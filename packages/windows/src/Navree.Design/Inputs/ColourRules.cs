namespace Navree.Design.Inputs;

/// <summary>
/// A colour field's value and the words said about it. The protocol sends and takes a colour as <c>#rrggbb</c>; what the person is shown is the
/// same value as a swatch and as words, so the colour is never told by colour alone.
/// </summary>
public static class ColourRules
{
    /// <summary>What is shown and said when the field holds no colour yet.</summary>
    public const string NoneWords = "Not chosen";

    /// <summary>Reads a colour written with or without the <c>#</c>, with three or six digits, in either case.</summary>
    /// <param name="text">The text.</param>
    /// <param name="red">The red part.</param>
    /// <param name="green">The green part.</param>
    /// <param name="blue">The blue part.</param>
    /// <returns>True when the text is a colour.</returns>
    public static bool TryRead(string? text, out byte red, out byte green, out byte blue)
    {
        red = green = blue = 0;
        string digits = (text ?? string.Empty).Trim();
        if (digits.StartsWith('#'))
        {
            digits = digits[1..];
        }

        if (digits.Length == 3)
        {
            digits = string.Concat(digits.Select(digit => new string(digit, 2)));
        }

        if (digits.Length != 6 || !digits.All(Uri.IsHexDigit))
        {
            return false;
        }

        red = Convert.ToByte(digits[..2], 16);
        green = Convert.ToByte(digits[2..4], 16);
        blue = Convert.ToByte(digits[4..], 16);
        return true;
    }

    /// <summary>A colour as the protocol sends it: <c>#rrggbb</c>, lower case.</summary>
    /// <param name="red">The red part.</param>
    /// <param name="green">The green part.</param>
    /// <param name="blue">The blue part.</param>
    /// <returns>The value.</returns>
    public static string Hex(byte red, byte green, byte blue) =>
        "#" + red.ToString("x2", System.Globalization.CultureInfo.InvariantCulture)
        + green.ToString("x2", System.Globalization.CultureInfo.InvariantCulture)
        + blue.ToString("x2", System.Globalization.CultureInfo.InvariantCulture);

    /// <summary>A text that reads as a colour, written as the protocol sends it; null when the text is not a colour.</summary>
    /// <param name="text">The text.</param>
    /// <returns>The value, or null.</returns>
    public static string? Normalise(string? text) => TryRead(text, out byte red, out byte green, out byte blue) ? Hex(red, green, blue) : null;

    /// <summary>What a colour is called in a word or two: <c>dark green</c>, <c>light blue</c>, <c>grey</c>.</summary>
    /// <param name="red">The red part.</param>
    /// <param name="green">The green part.</param>
    /// <param name="blue">The blue part.</param>
    /// <returns>The words.</returns>
    public static string Family(byte red, byte green, byte blue)
    {
        double r = red / 255.0;
        double g = green / 255.0;
        double b = blue / 255.0;
        double high = Math.Max(r, Math.Max(g, b));
        double low = Math.Min(r, Math.Min(g, b));
        double light = (high + low) / 2;
        double spread = high - low;
        double saturation = spread == 0 ? 0 : spread / (1 - Math.Abs((2 * light) - 1));
        if (light < 0.1)
        {
            return "black";
        }

        if (light > 0.93)
        {
            return "white";
        }

        if (saturation < 0.12)
        {
            return light < 0.35 ? "dark grey" : light > 0.7 ? "light grey" : "grey";
        }

        double hue;
        if (high == r)
        {
            hue = 60 * (((g - b) / spread) % 6);
        }
        else if (high == g)
        {
            hue = 60 * (((b - r) / spread) + 2);
        }
        else
        {
            hue = 60 * (((r - g) / spread) + 4);
        }

        if (hue < 0)
        {
            hue += 360;
        }

        string family = hue switch
        {
            < 15 or >= 345 => "red",
            < 40 => light < 0.4 ? "brown" : "orange",
            < 70 => "yellow",
            < 165 => "green",
            < 195 => "teal",
            < 255 => "blue",
            < 290 => "purple",
            _ => "pink",
        };
        return family == "brown" ? family : light < 0.3 ? "dark " + family : light > 0.75 ? "light " + family : family;
    }

    /// <summary>The value as it is written on the field: <c>#16a34a</c>, or <see cref="NoneWords"/> when the field holds no colour.</summary>
    /// <param name="value">What the field holds.</param>
    /// <returns>The words.</returns>
    public static string Shown(string? value) => Normalise(value) ?? NoneWords;

    /// <summary>The colour in words: <c>dark green, #16a34a</c>; <see cref="NoneWords"/> when none is chosen.</summary>
    /// <param name="value">What the field holds.</param>
    /// <returns>The words.</returns>
    public static string Words(string? value) =>
        TryRead(value, out byte red, out byte green, out byte blue) ? Family(red, green, blue) + ", " + Hex(red, green, blue) : NoneWords;

    /// <summary>
    /// What a screen reader says of the field: its label, the colour's name, and the value one character at a time (a value read as
    /// a word is said as nonsense): <c>Label colour, green, hex 1 6 a 3 4 a</c>.
    /// </summary>
    /// <param name="label">The field's label.</param>
    /// <param name="value">What the field holds.</param>
    /// <returns>The words.</returns>
    public static string Spoken(string label, string? value)
    {
        if (!TryRead(value, out byte red, out byte green, out byte blue))
        {
            return label + ", " + NoneWords.ToLowerInvariant();
        }

        string spelled = string.Join(' ', Hex(red, green, blue)[1..].ToCharArray());
        return label + ", " + Family(red, green, blue) + ", hex " + spelled;
    }
}
