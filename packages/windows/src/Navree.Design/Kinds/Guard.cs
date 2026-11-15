namespace Navree.Design;

/// <summary>
/// What a drawn kind does with a number it was given, from a page or from a size: a range that comes the wrong way round, a size that
/// is not a number or is below nothing is made into one that can be drawn, so no kind throws on what a server may send or on the size
/// it is given.
/// </summary>
public static class Guard
{
    /// <summary>A number that is a number: the fallback when it is not (NaN or infinite).</summary>
    /// <param name="value">The number.</param>
    /// <param name="fallback">What stands for it when it is not one.</param>
    /// <returns>The number or the fallback.</returns>
    public static double Finite(double value, double fallback = 0) => double.IsFinite(value) ? value : fallback;

    /// <summary>A length: not below nothing, and nothing when it is not a number.</summary>
    /// <param name="value">The length.</param>
    /// <returns>A length that can be a width, a height, a radius or a stroke.</returns>
    public static double Length(double value) => double.IsFinite(value) ? Math.Max(value, 0) : 0;

    /// <summary>A number held between two ends that may be given either way round; a value that is not a number is held at the lower end.</summary>
    /// <param name="value">The number.</param>
    /// <param name="one">One end.</param>
    /// <param name="other">The other end.</param>
    /// <returns>The number between the ends.</returns>
    public static double Clamp(double value, double one, double other)
    {
        if (double.IsNaN(one))
        {
            one = double.IsNaN(other) ? 0 : other;
        }

        if (double.IsNaN(other))
        {
            other = one;
        }

        double low = Math.Min(one, other);
        double high = Math.Max(one, other);
        return double.IsNaN(value) ? low : Math.Clamp(value, low, high);
    }

    /// <summary>A place held to the places there are.</summary>
    /// <param name="index">The place asked for.</param>
    /// <param name="count">How many places there are.</param>
    /// <returns>The nearest place that exists, or -1 when there is none.</returns>
    public static int Place(int index, int count) => count <= 0 ? -1 : Math.Clamp(index, 0, count - 1);

    /// <summary>A share of a whole, 0 to 1; none when it is not a number.</summary>
    /// <param name="value">The share.</param>
    /// <returns>The share.</returns>
    public static double Fraction(double value) => double.IsNaN(value) ? 0 : Math.Clamp(value, 0, 1);
}

/// <summary>How much of a text's room it takes: a text that is wider than its room is made smaller by the share that does not fit.</summary>
public static class TextFit
{
    /// <summary>The share to multiply a text's size by so that it fits a room.</summary>
    /// <param name="room">The room.</param>
    /// <param name="wide">How wide the text is at its size.</param>
    /// <param name="least">The least share: a text is never made smaller than this, whatever the room.</param>
    /// <returns>A share between <paramref name="least"/> and 1.</returns>
    public static double Shrink(double room, double wide, double least)
    {
        double floor = Math.Clamp(Guard.Finite(least, 0.5), 0.01, 1);
        if (!double.IsFinite(wide) || wide <= 0 || double.IsPositiveInfinity(room) || wide <= room)
        {
            return 1;
        }

        return double.IsNaN(room) ? floor : Math.Clamp(room / wide, floor, 1);
    }
}

/// <summary>Parts of a bar laid end to end, and columns side by side, so that they never run past the room they are given.</summary>
public static class BarRules
{
    /// <summary>The start and size of each part with a share along a bar this long, a hair between parts.</summary>
    /// <param name="length">How long the bar is.</param>
    /// <param name="shares">Each part's share of the whole.</param>
    /// <param name="hair">The gap between two parts.</param>
    /// <returns>The parts that are drawn, in order; they and their hairs never add up to more than the bar's length.</returns>
    public static IReadOnlyList<(int Index, double Start, double Size)> Slots(double length, IReadOnlyList<double> shares, double hair)
    {
        double bar = Guard.Length(length);
        List<int> drawn = [.. Enumerable.Range(0, shares.Count).Where(index => shares[index] > 0 && double.IsFinite(shares[index]))];
        if (drawn.Count == 0 || bar <= 0)
        {
            return [];
        }

        // A part is at least a pixel wide so it can be seen; where there are more parts than pixels, the hairs go first and then the parts
        // share what there is.
        double between = Guard.Length(hair);
        if (bar - (between * (drawn.Count - 1)) < drawn.Count)
        {
            between = 0;
        }

        double room = bar - (between * (drawn.Count - 1));
        double least = Math.Min(1, room / drawn.Count);
        double spare = room - (least * drawn.Count);
        double[] sizes = [.. drawn.Select(index => least + (spare * Math.Min(shares[index], 1)))];
        double total = sizes.Sum();
        double scale = total > room ? room / total : 1;
        List<(int Index, double Start, double Size)> slots = [];
        double at = 0;
        for (int part = 0; part < drawn.Count; part++)
        {
            double size = sizes[part] * scale;
            slots.Add((drawn[part], at, size));
            at += size + between;
        }

        return slots;
    }

    /// <summary>The width of each column of a row of columns and the gap between them, for a surface this wide: the gap gives way before a column does.</summary>
    /// <param name="total">The surface's width.</param>
    /// <param name="count">How many columns.</param>
    /// <param name="gap">The gap that is wanted.</param>
    /// <returns>The width of a column and the gap.</returns>
    public static (double Width, double Gap) Columns(double total, int count, double gap)
    {
        double room = Guard.Length(total);
        if (count <= 0)
        {
            return (0, 0);
        }

        double between = count == 1 ? 0 : Math.Min(Guard.Length(gap), room / (2.0 * count));
        return (Math.Max((room - (between * (count - 1))) / count, 0), between);
    }
}

/// <summary>A ring cut into slices (a donut): where it stands in its surface, and which slice a point is on.</summary>
public static class DonutRules
{
    /// <summary>Where the ring stands.</summary>
    /// <param name="Cx">The middle, across.</param>
    /// <param name="Cy">The middle, down.</param>
    /// <param name="Outer">The outer radius.</param>
    /// <param name="Inner">The inner radius.</param>
    public readonly record struct Ring(double Cx, double Cy, double Outer, double Inner)
    {
        /// <summary>The radius of the middle of the arc.</summary>
        public double Middle => (Outer + Inner) / 2;

        /// <summary>How thick the arc is.</summary>
        public double Width => Outer - Inner;

        /// <summary>Whether there is room for it: a ring thinner than a pixel is not drawn.</summary>
        public bool Draws => Width >= 1;
    }

    /// <summary>One slice: where it starts and how far it goes, in degrees from three o'clock, clockwise.</summary>
    /// <param name="Start">Where it starts.</param>
    /// <param name="Sweep">How far it goes.</param>
    public readonly record struct Slice(double Start, double Sweep);

    /// <summary>The share of the outer radius that is the hole.</summary>
    public const double HoleShare = 0.62;

    /// <summary>Where the ring stands in a surface this big.</summary>
    /// <param name="width">The surface's width.</param>
    /// <param name="height">The surface's height.</param>
    /// <returns>The ring.</returns>
    public static Ring Of(double width, double height)
    {
        double w = Guard.Length(width);
        double h = Guard.Length(height);
        double outer = Math.Max((Math.Min(w, h) / 2) - 2, 0);
        return new Ring(w / 2, h / 2, outer, outer * HoleShare);
    }

    /// <summary>The slices of a set of values, from twelve o'clock; a value that is not above nothing has no slice (it gets a sweep of none).</summary>
    /// <param name="values">The values.</param>
    /// <returns>A slice for each value.</returns>
    public static IReadOnlyList<Slice> Slices(IReadOnlyList<double> values)
    {
        double total = values.Where(value => double.IsFinite(value) && value > 0).Sum();
        List<Slice> slices = [];
        double at = -90;
        foreach (double value in values)
        {
            double sweep = total > 0 && double.IsFinite(value) && value > 0 ? 360 * value / total : 0;
            slices.Add(new Slice(at, sweep));
            at += sweep;
        }

        return slices;
    }

    /// <summary>The slice a point is on: the point's angle from the middle against each slice's sweep, so a slice is read out wherever along its arc the pointer is.</summary>
    /// <param name="values">The values.</param>
    /// <param name="ring">The ring.</param>
    /// <param name="x">The point, across, as the ring is drawn.</param>
    /// <param name="y">The point, down.</param>
    /// <param name="slack">How far in from the hole or out from the rim still counts as on the ring.</param>
    /// <param name="mirrored">Whether the ring is drawn flipped across (right to left).</param>
    /// <returns>The place of the slice, or -1 when the point is not on the ring.</returns>
    public static int SliceAt(IReadOnlyList<double> values, Ring ring, double x, double y, double slack, bool mirrored)
    {
        if (!ring.Draws || !double.IsFinite(x) || !double.IsFinite(y))
        {
            return -1;
        }

        double dx = mirrored ? ring.Cx - x : x - ring.Cx;
        double dy = y - ring.Cy;
        double distance = Math.Sqrt((dx * dx) + (dy * dy));
        if (distance < ring.Inner - slack || distance > ring.Outer + slack)
        {
            return -1;
        }

        double angle = ((((Math.Atan2(dy, dx) * 180 / Math.PI) + 90) % 360) + 360) % 360;
        IReadOnlyList<Slice> slices = Slices(values);
        int last = -1;
        for (int at = 0; at < slices.Count; at++)
        {
            if (slices[at].Sweep <= 0)
            {
                continue;
            }

            last = at;
            double from = slices[at].Start + 90;
            if (angle >= from && angle < from + slices[at].Sweep)
            {
                return at;
            }
        }

        // The very end of the last slice is the start of the first.
        return last;
    }
}

/// <summary>A value held against a range that may be given either way round: the line of a range and the value a screen reader is told.</summary>
public static class RangeRules
{
    /// <summary>The two ends of a range in order, whichever way the page gave them.</summary>
    /// <param name="min">One end.</param>
    /// <param name="max">The other end.</param>
    /// <returns>The lower and the higher end.</returns>
    public static (double Min, double Max) Ends(double min, double max)
    {
        double one = Guard.Finite(min);
        double other = Guard.Finite(max, one);
        return (Math.Min(one, other), Math.Max(one, other));
    }

    /// <summary>A value held inside the range.</summary>
    /// <param name="value">The value.</param>
    /// <param name="min">One end.</param>
    /// <param name="max">The other end.</param>
    /// <returns>The value, or the nearer end when it is outside.</returns>
    public static double Value(double value, double min, double max)
    {
        (double low, double high) = Ends(min, max);
        return Guard.Clamp(value, low, high);
    }

    /// <summary>Where the middle of a marker stands on a line this wide: it never goes past the line's ends, and on a line narrower than the marker it stands in the middle.</summary>
    /// <param name="position">Where the value is on the line, 0 to 1.</param>
    /// <param name="width">The line's width.</param>
    /// <param name="marker">The marker's width.</param>
    /// <returns>The place of the marker's middle.</returns>
    public static double MarkerAt(double position, double width, double marker)
    {
        double line = Guard.Length(width);
        double half = Guard.Length(marker) / 2;
        if (line <= half * 2)
        {
            return line / 2;
        }

        return Math.Clamp(Guard.Fraction(position) * line, half, line - half);
    }

    /// <summary>Where a zone's band starts and how long it is on a line this wide: at least as long as <paramref name="least"/> where there is room, and always inside the line.</summary>
    /// <param name="low">Where the zone starts, 0 to 1.</param>
    /// <param name="high">Where it ends, 0 to 1.</param>
    /// <param name="width">The line's width.</param>
    /// <param name="least">The shortest a band is drawn.</param>
    /// <returns>Where it starts and how long it is.</returns>
    public static (double Start, double Size) Band(double low, double high, double width, double least)
    {
        double line = Guard.Length(width);
        double size = Math.Min(Math.Max((Guard.Fraction(high) - Guard.Fraction(low)) * line, Guard.Length(least)), line);
        double start = Math.Clamp(Guard.Fraction(low) * line, 0, line - size);
        return (start, size);
    }
}

/// <summary>How much of a drawing with very many places is drawn: more places than a person can tell apart are thinned.</summary>
public static class ThinRules
{
    /// <summary>
    /// The most marks (places of a chart, points of a scatter) one drawing makes. Every mark is an element on the screen and a place
    /// that is read out, and the pointer looks through them all at each move; past about this many a mark is narrower than a pixel
    /// and a person cannot tell it from the next one, so the drawing keeps a spread of them instead.
    /// </summary>
    public const int MostMarks = 1500;

    /// <summary>The places of a run of <paramref name="count"/> that are drawn: all of them up to the cap, else an even spread that keeps the first and the last.</summary>
    /// <param name="count">How many places there are.</param>
    /// <param name="width">How wide the drawing is, in pixels: more places than pixels are not drawn either.</param>
    /// <returns>The places, in order.</returns>
    public static IReadOnlyList<int> Places(int count, double width)
    {
        if (count <= 0)
        {
            return [];
        }

        int most = (int)Math.Min(MostMarks, Math.Max(Math.Floor(Guard.Length(width)), 2));
        if (count <= most)
        {
            return [.. Enumerable.Range(0, count)];
        }

        List<int> kept = [];
        for (int at = 0; at < most; at++)
        {
            int place = (int)Math.Round((double)at * (count - 1) / (most - 1));
            if (kept.Count == 0 || kept[^1] != place)
            {
                kept.Add(place);
            }
        }

        return kept;
    }
}
