namespace Navree.Design.Tables;

/// <summary>How tall a table's rows are.</summary>
public enum Density
{
    /// <summary>The shortest rows.</summary>
    Compact,

    /// <summary>The usual rows.</summary>
    Standard,

    /// <summary>The tallest rows.</summary>
    Comfortable,
}

/// <summary>How many lines of words a table's rows hold: the same for every row of a table.</summary>
public enum RowShape
{
    /// <summary>One line in each cell.</summary>
    OneLine,

    /// <summary>The name cell holds a second line under the name.</summary>
    TwoLine,
}

/// <summary>The heights of rows, and what follows from them.</summary>
public static class RowGeometry
{
    /// <summary>
    /// The most a table may stand tall, in physical pixels: past it a single-precision position no longer holds every whole pixel, and
    /// layout rounding works in physical pixels, so a row past it would stand on the wrong one.
    /// </summary>
    public const double MaxExtent = 16_000_000;

    /// <summary>The most rows a table holds, whatever their height.</summary>
    public const int HeldRowsCeiling = 200_000;

    private static readonly double[] OneLineMinimum = [32, 40, 48];
    private static readonly double[] TwoLineMinimum = [48, 56, 64];
    private static readonly double[] Padding = [4, 8, 12];

    /// <summary>The height of a row, including its one-pixel divider, made whole in physical pixels.</summary>
    /// <param name="density">The table's density.</param>
    /// <param name="shape">One line or two.</param>
    /// <param name="hasControls">Whether a column holds controls, which need room round them.</param>
    /// <param name="lineHeight">The height of one line of body text, as measured at the person's text size.</param>
    /// <param name="secondLineHeight">The height of the second line, when the shape has one.</param>
    /// <param name="rasterizationScale">Physical pixels for each effective pixel.</param>
    /// <returns>The row's height in effective pixels.</returns>
    public static double RowHeight(
        Density density,
        RowShape shape,
        bool hasControls,
        double lineHeight,
        double secondLineHeight,
        double rasterizationScale)
    {
        if (!(lineHeight > 0))
        {
            throw new ArgumentOutOfRangeException(nameof(lineHeight), lineHeight, "A line has a height.");
        }

        if (!(rasterizationScale > 0))
        {
            throw new ArgumentOutOfRangeException(nameof(rasterizationScale), rasterizationScale, "The scale must be more than zero.");
        }

        int at = (int)density;
        double minimum = shape == RowShape.TwoLine ? TwoLineMinimum[at] : OneLineMinimum[at];
        if (hasControls && shape == RowShape.OneLine && density == Density.Compact)
        {
            minimum = 40;
        }

        double content = lineHeight + (shape == RowShape.TwoLine ? Math.Max(0, secondLineHeight) : 0);
        double height = Math.Max(minimum, content + (2 * Padding[at]) + 1);
        return Pixels.RoundUp(height, rasterizationScale);
    }

    /// <summary>The height of the header row: a one-line row of the density.</summary>
    /// <param name="density">The table's density.</param>
    /// <param name="hasControls">Whether a column holds controls.</param>
    /// <param name="lineHeight">The height of one line of text.</param>
    /// <param name="rasterizationScale">Physical pixels for each effective pixel.</param>
    /// <returns>The header's height in effective pixels.</returns>
    public static double HeaderHeight(Density density, bool hasControls, double lineHeight, double rasterizationScale) =>
        RowHeight(density, RowShape.OneLine, hasControls, lineHeight, 0, rasterizationScale);

    /// <summary>The most rows a table holds at a row height on a display of a scale.</summary>
    /// <param name="rowHeight">The height of a row, in effective pixels.</param>
    /// <param name="rasterizationScale">Physical pixels for each effective pixel: the extent is held to <see cref="MaxExtent"/> in physical pixels.</param>
    /// <returns>The smaller of the ceiling and what fits in the greatest extent.</returns>
    public static int MaxHeldRows(double rowHeight, double rasterizationScale = 1.0)
    {
        if (!(rowHeight > 0))
        {
            throw new ArgumentOutOfRangeException(nameof(rowHeight), rowHeight, "A row has a height.");
        }

        if (!(rasterizationScale > 0))
        {
            throw new ArgumentOutOfRangeException(nameof(rasterizationScale), rasterizationScale, "The scale must be more than zero.");
        }

        return (int)Math.Min(HeldRowsCeiling, Math.Floor(MaxExtent / (rowHeight * rasterizationScale)));
    }
}

/// <summary>
/// Where each row of a table stands: every row has one height and a few rows that are open have
/// more. A snapshot; make a new one when the open rows change.
/// </summary>
public sealed class Geometry
{
    private readonly int[] openIndex;
    private readonly double[] openExtra;
    private readonly double[] before;
    private readonly double[] openEnd;

    /// <summary>Makes a snapshot.</summary>
    /// <param name="rowHeight">The height of a closed row, more than zero.</param>
    /// <param name="opens">The open rows with the height each adds, in ascending order of index, one entry for each index.</param>
    public Geometry(double rowHeight, IReadOnlyList<(int Index, double Extra)> opens)
    {
        ArgumentNullException.ThrowIfNull(opens);
        if (!(rowHeight > 0))
        {
            throw new ArgumentOutOfRangeException(nameof(rowHeight), rowHeight, "A row has a height.");
        }

        RowHeight = rowHeight;
        openIndex = new int[opens.Count];
        openExtra = new double[opens.Count];
        before = new double[opens.Count + 1];
        openEnd = new double[opens.Count];
        for (int i = 0; i < opens.Count; i++)
        {
            (int index, double extra) = opens[i];
            if (index < 0 || extra < 0 || double.IsNaN(extra) || (i > 0 && index <= openIndex[i - 1]))
            {
                throw new ArgumentException("The open rows must be in ascending order, once each, with an extra of zero or more.", nameof(opens));
            }

            openIndex[i] = index;
            openExtra[i] = extra;
            before[i + 1] = before[i] + extra;
            openEnd[i] = (index * rowHeight) + before[i] + rowHeight + extra;
        }
    }

    /// <summary>The height of a closed row.</summary>
    public double RowHeight { get; }

    /// <summary>How many rows are open.</summary>
    public int OpenCount => openIndex.Length;

    /// <summary>The distance from the top of the table to the top of a row.</summary>
    /// <param name="index">The row's index, zero or more.</param>
    /// <returns>The top of the row.</returns>
    public double RowTop(int index)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(index);
        return (index * RowHeight) + before[OpensBefore(index)];
    }

    /// <summary>The height of a row: its own and, when it is open, what its detail adds.</summary>
    /// <param name="index">The row's index.</param>
    /// <returns>The row's height.</returns>
    public double HeightAt(int index)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(index);
        int at = Array.BinarySearch(openIndex, index);
        return at >= 0 ? RowHeight + openExtra[at] : RowHeight;
    }

    /// <summary>The height of the table when it holds <paramref name="count"/> rows.</summary>
    /// <param name="count">How many rows.</param>
    /// <returns>The height of all of them.</returns>
    public double Extent(int count) => RowTop(count);

    /// <summary>The row that holds a vertical position.</summary>
    /// <param name="y">The position, from the top of the table.</param>
    /// <param name="count">How many rows the table holds.</param>
    /// <returns>
    /// The row and how far down inside it the position is. Above the table gives the first row at
    /// zero; at or past the end the last row at its full height; no rows gives index minus one.
    /// </returns>
    public (int Index, double Within) IndexAt(double y, int count)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(count);
        if (count == 0)
        {
            return (-1, 0);
        }

        if (!(y > 0))
        {
            return (0, 0);
        }

        if (y >= Extent(count))
        {
            return (count - 1, HeightAt(count - 1));
        }

        // How many open rows end at or above the position.
        int low = 0;
        int high = openEnd.Length;
        while (low < high)
        {
            int middle = (low + high) / 2;
            if (openEnd[middle] <= y)
            {
                low = middle + 1;
            }
            else
            {
                high = middle;
            }
        }

        int endedAbove = low;
        int candidate = (int)Math.Floor((y - before[endedAbove]) / RowHeight);
        if (endedAbove < openIndex.Length && candidate >= openIndex[endedAbove])
        {
            int open = openIndex[endedAbove];
            return (open, y - ((open * RowHeight) + before[endedAbove]));
        }

        return (candidate, y - before[endedAbove] - (candidate * RowHeight));
    }

    /// <summary>The most rows a pass makes, whatever the viewport says: a viewport of no end (a repeater with nothing to measure against) must not make every row.</summary>
    public const int MostRealized = 400;

    /// <summary>The rows to have made for a viewport: those it touches and a margin of rows each side, never more than <see cref="MostRealized"/>.</summary>
    /// <param name="top">The top of the viewport.</param>
    /// <param name="height">The height of the viewport.</param>
    /// <param name="count">How many rows the table holds.</param>
    /// <param name="margin">How many rows to add each side.</param>
    /// <returns>The first and the last row; the last is one less than the first when there are none.</returns>
    public (int First, int Last) RealizedRange(double top, double height, int count, int margin = 1)
    {
        if (count <= 0 || !(height > 0))
        {
            return (0, -1);
        }

        double start = Math.Max(0, top);
        int first = Math.Max(0, IndexAt(start, count).Index - margin);
        int last = Math.Min(count - 1, IndexAt(start + height - 1e-6, count).Index + margin);
        return (first, Math.Min(last, first + MostRealized - 1));
    }

    // How many open rows have an index below the given one.
    private int OpensBefore(int index)
    {
        int at = Array.BinarySearch(openIndex, index);
        return at >= 0 ? at : ~at;
    }
}

/// <summary>Which rows a pass of the rows layout makes: those the viewport touches and the rows kept made wherever they stand.</summary>
public static class RealizationPlan
{
    /// <summary>The places of the rows to make, in order.</summary>
    /// <param name="geometry">Where each row stands.</param>
    /// <param name="top">The top of the realization rectangle.</param>
    /// <param name="height">The height of the realization rectangle; of no end or no number it still makes no more than <see cref="Geometry.MostRealized"/> rows.</param>
    /// <param name="count">How many rows the table holds.</param>
    /// <param name="pinned">The places of the rows kept made wherever they stand.</param>
    /// <param name="margin">How many rows to add each side of the viewport.</param>
    /// <returns>The rows the viewport touches, then the pinned rows that exist and are not among them, each once.</returns>
    public static IReadOnlyList<int> Indices(Geometry geometry, double top, double height, int count, IReadOnlyList<int> pinned, int margin = 1)
    {
        ArgumentNullException.ThrowIfNull(geometry);
        ArgumentNullException.ThrowIfNull(pinned);
        List<int> made = [];
        (int first, int last) = geometry.RealizedRange(top, height, count, margin);
        for (int index = first; index <= last; index++)
        {
            made.Add(index);
        }

        foreach (int place in pinned)
        {
            if (place >= 0 && place < count && (place < first || place > last) && !made.Contains(place))
            {
                made.Add(place);
            }
        }

        return made;
    }
}
