using System.Globalization;
using Navree.Protocol;

namespace Navree.Design;

/// <summary>A colour as red, green and blue between 0 and 1, in sRGB.</summary>
/// <param name="Red">The red part.</param>
/// <param name="Green">The green part.</param>
/// <param name="Blue">The blue part.</param>
public readonly record struct Rgb(double Red, double Green, double Blue)
{
    /// <summary>Reads <c>#RRGGBB</c> or <c>RRGGBB</c>.</summary>
    /// <param name="hex">The text.</param>
    /// <param name="colour">The colour, when the text is one.</param>
    /// <returns>Whether the text was a colour.</returns>
    public static bool TryParse(string? hex, out Rgb colour)
    {
        colour = default;
        string digits = hex is not null && hex.StartsWith('#') ? hex[1..] : hex ?? string.Empty;
        if (digits.Length != 6 || !int.TryParse(digits, NumberStyles.AllowHexSpecifier, CultureInfo.InvariantCulture, out int value))
        {
            return false;
        }

        colour = new Rgb(((value >> 16) & 0xFF) / 255.0, ((value >> 8) & 0xFF) / 255.0, (value & 0xFF) / 255.0);
        return true;
    }

    /// <summary>A colour from <c>#RRGGBB</c>; the colour must be a good one.</summary>
    /// <param name="hex">The text.</param>
    /// <returns>The colour.</returns>
    public static Rgb Of(string hex) =>
        TryParse(hex, out Rgb colour) ? colour : throw new FormatException("not a colour: " + hex);

    /// <summary>The red part as a byte.</summary>
    public byte R => ToByte(Red);

    /// <summary>The green part as a byte.</summary>
    public byte G => ToByte(Green);

    /// <summary>The blue part as a byte.</summary>
    public byte B => ToByte(Blue);

    private static byte ToByte(double part) => (byte)Math.Clamp((int)Math.Round(part * 255), 0, 255);
}

/// <summary>What a hue is drawn in: one colour for the light theme and one for the dark.</summary>
/// <param name="Light">On a light ground.</param>
/// <param name="Dark">On a dark ground.</param>
/// <param name="FromTheme">Whether the system's own colour for it is used when there is one: the accent the person chose, and the secondary text colour.</param>
public sealed record Swatch(Rgb Light, Rgb Dark, bool FromTheme = false)
{
    /// <summary>The colour for a ground.</summary>
    /// <param name="dark">Whether the ground is dark.</param>
    /// <returns>The colour.</returns>
    public Rgb For(bool dark) => dark ? Dark : Light;
}

/// <summary>The colours of the protocol's hues. Windows has names for the accent and for status, not for the rest, so the rest are fixed here.</summary>
public static class HueRules
{
    private static readonly Swatch Neutral = new(Rgb.Of("#5C6773"), Rgb.Of("#A0AAB5"), FromTheme: true);
    private static readonly Swatch Accent = new(Rgb.Of("#005FB8"), Rgb.Of("#60CDFF"), FromTheme: true);
    private static readonly Swatch Green = new(Rgb.Of("#0F7B0F"), Rgb.Of("#6CCB5F"));
    private static readonly Swatch Orange = new(Rgb.Of("#D2560F"), Rgb.Of("#FF9A4D"));
    private static readonly Swatch Red = new(Rgb.Of("#C42B1C"), Rgb.Of("#FF99A4"));
    private static readonly Swatch Blue = new(Rgb.Of("#0F6CBD"), Rgb.Of("#5FB0F2"));
    private static readonly Swatch Amber = new(Rgb.Of("#E5A100"), Rgb.Of("#FFC83D"));
    private static readonly Swatch Rose = new(Rgb.Of("#C9306F"), Rgb.Of("#FF7DA8"));
    private static readonly Swatch Violet = new(Rgb.Of("#6B4FBB"), Rgb.Of("#B79BFF"));
    private static readonly Swatch Cyan = new(Rgb.Of("#00899A"), Rgb.Of("#40D0E0"));
    private static readonly Swatch Slate = new(Rgb.Of("#5C6773"), Rgb.Of("#A0AAB5"));

    /// <summary>The hues a chart's series take, in order, when the page names none.</summary>
    public static IReadOnlyList<Models.Hue> Series { get; } =
        [Models.Hue.Accent, Models.Hue.Orange, Models.Hue.Green, Models.Hue.Violet, Models.Hue.Rose, Models.Hue.Cyan, Models.Hue.Amber, Models.Hue.Red];

    /// <summary>The hue of the series at a place, wrapping round.</summary>
    /// <param name="index">The place.</param>
    /// <returns>The hue.</returns>
    public static Models.Hue SeriesHue(int index) => Series[((index % Series.Count) + Series.Count) % Series.Count];

    /// <summary>What a hue is drawn in.</summary>
    /// <param name="hue">The hue.</param>
    /// <returns>Its swatch.</returns>
    public static Swatch Of(Models.Hue hue) =>
        hue switch
        {
            Models.Hue.Accent => Accent,
            Models.Hue.Success or Models.Hue.Green => Green,
            Models.Hue.Warning or Models.Hue.Orange => Orange,
            Models.Hue.Danger or Models.Hue.Red => Red,
            Models.Hue.Info or Models.Hue.Blue => Blue,
            Models.Hue.Amber => Amber,
            Models.Hue.Rose => Rose,
            Models.Hue.Violet => Violet,
            Models.Hue.Cyan => Cyan,
            Models.Hue.Slate => Slate,
            _ => Neutral,
        };

    /// <summary>The hue a tone stands for.</summary>
    /// <param name="tone">The tone, or none.</param>
    /// <returns>The hue.</returns>
    public static Models.Hue OfTone(Models.Tone? tone) =>
        tone switch
        {
            Models.Tone.Success => Models.Hue.Success,
            Models.Tone.Warning => Models.Hue.Warning,
            Models.Tone.Danger => Models.Hue.Danger,
            Models.Tone.Info => Models.Hue.Info,
            Models.Tone.Accent => Models.Hue.Accent,
            _ => Models.Hue.Neutral,
        };

    /// <summary>The theme's own colour for a colour whose hue this client read as neutral (a hue newer than this client), when the page sent one.</summary>
    /// <param name="colour">The colour.</param>
    /// <param name="dark">Whether the ground is dark.</param>
    /// <returns>The colour, or none.</returns>
    public static Rgb? Fallback(Models.Colour colour, bool dark)
    {
        if (colour.Hue != Models.Hue.Neutral && colour.Hue != Models.Hue.Unknown)
        {
            return null;
        }

        string? hex = dark ? colour.Dark ?? colour.Light : colour.Light ?? colour.Dark;
        return Rgb.TryParse(hex, out Rgb found) ? found : null;
    }

    /// <summary>The hue of a colour of the page, or <paramref name="fallback"/> when it names none.</summary>
    /// <param name="colour">The page's colour.</param>
    /// <param name="fallback">The hue to take when there is none.</param>
    /// <returns>The hue.</returns>
    public static Models.Hue HueOf(Models.Colour? colour, Models.Hue fallback) => colour?.Hue ?? fallback;
}

/// <summary>Words in a colour that means something, dark enough to read on a light ground: the least darkening that gives 4.5 to 1.</summary>
public static class ContrastRules
{
    /// <summary>The contrast words need against their ground.</summary>
    public const double Words = 4.5;

    private static readonly Rgb[] LightGrounds = [new Rgb(1, 1, 1), Rgb.Of("#F3F3F3")];

    /// <summary>A colour's relative luminance, as WCAG works it out.</summary>
    /// <param name="colour">The colour.</param>
    /// <returns>Its luminance.</returns>
    public static double Luminance(Rgb colour)
    {
        static double Linear(double channel) => channel <= 0.04045 ? channel / 12.92 : Math.Pow((channel + 0.055) / 1.055, 2.4);
        return (0.2126 * Linear(colour.Red)) + (0.7152 * Linear(colour.Green)) + (0.0722 * Linear(colour.Blue));
    }

    /// <summary>How far apart two colours stand, 1 to 21.</summary>
    /// <param name="a">One colour.</param>
    /// <param name="b">The other.</param>
    /// <returns>The ratio.</returns>
    public static double Contrast(Rgb a, Rgb b)
    {
        double x = Luminance(a);
        double y = Luminance(b);
        return (Math.Max(x, y) + 0.05) / (Math.Min(x, y) + 0.05);
    }

    /// <summary>A colour taken part of the way to black.</summary>
    /// <param name="colour">The colour.</param>
    /// <param name="amount">0 to 1.</param>
    /// <returns>The darker colour.</returns>
    public static Rgb Darkened(Rgb colour, double amount)
    {
        double keep = 1 - Math.Clamp(amount, 0, 1);
        return new Rgb(colour.Red * keep, colour.Green * keep, colour.Blue * keep);
    }

    /// <summary>The least darkening, in hundredths, that gives the colour the contrast against both light grounds.</summary>
    /// <param name="colour">The colour.</param>
    /// <returns>The amount, 0 to 1.</returns>
    public static double Shade(Rgb colour)
    {
        for (int step = 0; step <= 100; step++)
        {
            double amount = step / 100.0;
            Rgb shaded = Darkened(colour, amount);
            if (LightGrounds.All(ground => Contrast(shaded, ground) >= Words))
            {
                return amount;
            }
        }

        return 1;
    }

    /// <summary>The ink that reads on a ground: white or black, whichever stands further from it.</summary>
    /// <param name="ground">The ground's colour.</param>
    /// <returns>White or black.</returns>
    public static Rgb Ink(Rgb ground) =>
        Contrast(ground, new Rgb(1, 1, 1)) >= Contrast(ground, new Rgb(0, 0, 0)) ? new Rgb(1, 1, 1) : new Rgb(0, 0, 0);

    /// <summary>A colour laid over a ground at an opacity: what a person sees.</summary>
    /// <param name="colour">The colour on top.</param>
    /// <param name="ground">The colour under it.</param>
    /// <param name="opacity">How much of the top colour shows, 0 to 1.</param>
    /// <returns>The blended colour.</returns>
    public static Rgb Over(Rgb colour, Rgb ground, double opacity)
    {
        double t = Math.Clamp(Guard.Finite(opacity), 0, 1);
        return new Rgb(
            ground.Red + ((colour.Red - ground.Red) * t),
            ground.Green + ((colour.Green - ground.Green) * t),
            ground.Blue + ((colour.Blue - ground.Blue) * t));
    }

    /// <summary>The colour words take: darkened as little as reads on a light ground, as it is on a dark one.</summary>
    /// <param name="colour">The hue's colour for the ground.</param>
    /// <param name="dark">Whether the ground is dark.</param>
    /// <returns>The colour for words.</returns>
    public static Rgb ForWords(Rgb colour, bool dark) => dark ? colour : Darkened(colour, Shade(colour));
}
