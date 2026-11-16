namespace Navree.Design;

/// <summary>One shape of a flag, in a box 60 across and 40 down.</summary>
public abstract record FlagShape;

/// <summary>A filled rectangle.</summary>
/// <param name="X">The left edge.</param>
/// <param name="Y">The top edge.</param>
/// <param name="Width">The width.</param>
/// <param name="Height">The height.</param>
/// <param name="Fill">The colour.</param>
public sealed record FlagRect(double X, double Y, double Width, double Height, Rgb Fill) : FlagShape;

/// <summary>A filled disc.</summary>
/// <param name="X">The middle across.</param>
/// <param name="Y">The middle down.</param>
/// <param name="Radius">The radius.</param>
/// <param name="Fill">The colour.</param>
public sealed record FlagDisc(double X, double Y, double Radius, Rgb Fill) : FlagShape;

/// <summary>A circle drawn with a line.</summary>
/// <param name="X">The middle across.</param>
/// <param name="Y">The middle down.</param>
/// <param name="Radius">The radius to the middle of the line.</param>
/// <param name="Width">The line's width.</param>
/// <param name="Stroke">The colour.</param>
public sealed record FlagRing(double X, double Y, double Radius, double Width, Rgb Stroke) : FlagShape;

/// <summary>Lines from a middle outwards, evenly round.</summary>
/// <param name="X">The middle across.</param>
/// <param name="Y">The middle down.</param>
/// <param name="Radius">How far out each reaches.</param>
/// <param name="Count">How many.</param>
/// <param name="Width">The lines' width.</param>
/// <param name="Stroke">The colour.</param>
public sealed record FlagSpokes(double X, double Y, double Radius, int Count, double Width, Rgb Stroke) : FlagShape;

/// <summary>A five-pointed star standing upright.</summary>
/// <param name="X">The middle across.</param>
/// <param name="Y">The middle down.</param>
/// <param name="Radius">The radius to the points.</param>
/// <param name="Fill">The colour.</param>
public sealed record FlagStar(double X, double Y, double Radius, Rgb Fill) : FlagShape;

/// <summary>A straight line.</summary>
/// <param name="X1">Where it starts across.</param>
/// <param name="Y1">Where it starts down.</param>
/// <param name="X2">Where it ends across.</param>
/// <param name="Y2">Where it ends down.</param>
/// <param name="Width">The line's width.</param>
/// <param name="Stroke">The colour.</param>
public sealed record FlagLine(double X1, double Y1, double X2, double Y2, double Width, Rgb Stroke) : FlagShape;

/// <summary>The flags of the countries the demos use, drawn from shapes: Windows has no flag emoji, and a flag is never "IN" in a box.</summary>
public static class FlagRules
{
    /// <summary>The box every flag is drawn in: its width.</summary>
    public const double Width = 60;

    /// <summary>The box every flag is drawn in: its height.</summary>
    public const double Height = 40;

    private static readonly Dictionary<string, IReadOnlyList<FlagShape>> Flags = Build();

    /// <summary>The countries that have a flag of their own.</summary>
    public static IReadOnlyCollection<string> Known => Flags.Keys;

    /// <summary>How high a flag stands: what is asked for, held to a size that can be drawn.</summary>
    /// <param name="height">The height asked for.</param>
    /// <returns>A height of at least one and at most 512.</returns>
    public static double Stand(double height) => Math.Clamp(Guard.Finite(height, 16), 1, 512);

    /// <summary>The shapes of a country's flag, back to front.</summary>
    /// <param name="country">The two-letter country code, in any case.</param>
    /// <returns>The shapes, or none when this build has no flag for the country (the neutral flag is drawn then).</returns>
    public static IReadOnlyList<FlagShape>? Of(string country) =>
        Flags.TryGetValue(country.ToUpperInvariant(), out IReadOnlyList<FlagShape>? found) ? found : null;

    /// <summary>The points of an upright five-pointed star, outer and inner alternating from the top.</summary>
    /// <param name="x">The middle across.</param>
    /// <param name="y">The middle down.</param>
    /// <param name="radius">The radius to the points.</param>
    /// <returns>Ten points.</returns>
    public static IReadOnlyList<Pt> StarPoints(double x, double y, double radius)
    {
        List<Pt> points = [];
        for (int at = 0; at < 10; at++)
        {
            double r = at % 2 == 0 ? radius : radius * 0.382;
            double angle = (-90 + (at * 36)) * Math.PI / 180;
            points.Add(new Pt(x + (r * Math.Cos(angle)), y + (r * Math.Sin(angle))));
        }

        return points;
    }

    private static Dictionary<string, IReadOnlyList<FlagShape>> Build()
    {
        Dictionary<string, IReadOnlyList<FlagShape>> flags = new(StringComparer.Ordinal)
        {
            ["IN"] = India(),
            ["US"] = UnitedStates(),
            ["GB"] = UnitedKingdom(),
            ["AE"] =
            [
                new FlagRect(0, 0, 60, 40, Rgb.Of("#FFFFFF")),
                new FlagRect(15, 0, 45, 40.0 / 3, Rgb.Of("#00732F")),
                new FlagRect(15, 80.0 / 3, 45, 40.0 / 3, Rgb.Of("#000000")),
                new FlagRect(0, 0, 15, 40, Rgb.Of("#FF0000")),
            ],
            ["JP"] =
            [
                new FlagRect(0, 0, 60, 40, Rgb.Of("#FFFFFF")),
                new FlagDisc(30, 20, 12, Rgb.Of("#BC002D")),
            ],
            ["BD"] =
            [
                new FlagRect(0, 0, 60, 40, Rgb.Of("#006A4E")),
                new FlagDisc(27, 20, 12, Rgb.Of("#F42A41")),
            ],
            ["FR"] = Vertical("#0055A4", "#FFFFFF", "#EF4135"),
            ["IT"] = Vertical("#009246", "#FFFFFF", "#CE2B37"),
            ["IE"] = Vertical("#169B62", "#FFFFFF", "#FF883E"),
            ["DE"] = Horizontal("#000000", "#DD0000", "#FFCE00"),
            ["NL"] = Horizontal("#AE1C28", "#FFFFFF", "#21468B"),
            ["RU"] = Horizontal("#FFFFFF", "#0039A6", "#D52B1E"),
            ["AT"] = Horizontal("#ED2939", "#FFFFFF", "#ED2939"),
            ["PL"] = Horizontal("#FFFFFF", "#DC143C"),
            ["UA"] = Horizontal("#0057B7", "#FFD700"),
            ["ID"] = Horizontal("#FF0000", "#FFFFFF"),
        };
        return flags;
    }

    private static List<FlagShape> Horizontal(params string[] colours)
    {
        List<FlagShape> shapes = [];
        double each = Height / colours.Length;
        for (int at = 0; at < colours.Length; at++)
        {
            shapes.Add(new FlagRect(0, at * each, Width, each, Rgb.Of(colours[at])));
        }

        return shapes;
    }

    private static List<FlagShape> Vertical(params string[] colours)
    {
        List<FlagShape> shapes = [];
        double each = Width / colours.Length;
        for (int at = 0; at < colours.Length; at++)
        {
            shapes.Add(new FlagRect(at * each, 0, each, Height, Rgb.Of(colours[at])));
        }

        return shapes;
    }

    private static List<FlagShape> India()
    {
        Rgb navy = Rgb.Of("#000080");
        List<FlagShape> shapes = Horizontal("#FF9933", "#FFFFFF", "#138808");
        shapes.Add(new FlagRing(30, 20, 5.2, 0.8, navy));
        shapes.Add(new FlagSpokes(30, 20, 5.2, 24, 0.4, navy));
        shapes.Add(new FlagDisc(30, 20, 1, navy));
        return shapes;
    }

    private static List<FlagShape> UnitedStates()
    {
        Rgb red = Rgb.Of("#B22234");
        Rgb white = Rgb.Of("#FFFFFF");
        List<FlagShape> shapes = [];
        double stripe = Height / 13;
        for (int at = 0; at < 13; at++)
        {
            shapes.Add(new FlagRect(0, at * stripe, Width, stripe, at % 2 == 0 ? red : white));
        }

        shapes.Add(new FlagRect(0, 0, 25, stripe * 7, Rgb.Of("#3C3B6E")));
        for (int row = 0; row < 9; row++)
        {
            int count = row % 2 == 0 ? 6 : 5;
            double offset = row % 2 == 0 ? 2.1 : 4.2;
            for (int at = 0; at < count; at++)
            {
                shapes.Add(new FlagStar(offset + (at * 4.2), 1.5 + (row * 2.5), 1.15, white));
            }
        }

        return shapes;
    }

    private static List<FlagShape> UnitedKingdom()
    {
        Rgb blue = Rgb.Of("#012169");
        Rgb white = Rgb.Of("#FFFFFF");
        Rgb red = Rgb.Of("#C8102E");
        return
        [
            new FlagRect(0, 0, 60, 40, blue),
            new FlagLine(0, 0, 60, 40, 7, white),
            new FlagLine(60, 0, 0, 40, 7, white),
            new FlagLine(0, 0, 60, 40, 2.6, red),
            new FlagLine(60, 0, 0, 40, 2.6, red),
            new FlagLine(30, 0, 30, 40, 12, white),
            new FlagLine(0, 20, 60, 20, 12, white),
            new FlagLine(30, 0, 30, 40, 7, red),
            new FlagLine(0, 20, 60, 20, 7, red),
        ];
    }
}
