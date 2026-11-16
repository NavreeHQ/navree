namespace Navree.Design.Tables;

/// <summary>Where a column stands relative to the scroll.</summary>
public enum ColumnGroup
{
    /// <summary>Kept at the start edge while the rest scrolls.</summary>
    FrozenStart,

    /// <summary>Scrolls with the rows.</summary>
    Scroll,

    /// <summary>Kept at the end edge while the rest scrolls.</summary>
    FrozenEnd,
}

/// <summary>A column as the layout is given it.</summary>
/// <param name="Key">The column's identity.</param>
/// <param name="Kind">What it holds.</param>
/// <param name="Width">Its width before any room is shared out.</param>
/// <param name="Flexible">Whether it takes the room the others leave.</param>
/// <param name="Sized">Whether the person gave it its width: a width a person gave is not taken back to make room.</param>
public readonly record struct ColumnSpec(string Key, ColumnKind Kind, double Width, bool Flexible, bool Sized = false);

/// <summary>A column in its place.</summary>
/// <param name="Key">The column's identity.</param>
/// <param name="Width">Its width.</param>
/// <param name="Group">Whether it is frozen at the start, scrolls, or is frozen at the end.</param>
/// <param name="X">Its offset from the row's inline start; the gutter comes first.</param>
public readonly record struct PlacedColumn(string Key, double Width, ColumnGroup Group, double X);

/// <summary>Where every column of a table stands, and how wide the row is.</summary>
/// <param name="Columns">The columns in display order, the end columns last.</param>
/// <param name="Gutter">The width of the check and chevron cells.</param>
/// <param name="Fs">The width kept at the start: the gutter and the frozen columns.</param>
/// <param name="Fe">The width kept at the end: the end columns and the padding after them; zero when none.</param>
/// <param name="ScrollWidth">The width of the columns that scroll.</param>
/// <param name="Viewport">The width of the viewport.</param>
/// <param name="RowWidth">The width of a row: the larger of the sum of the parts and the viewport.</param>
/// <param name="FrozenStartApplied">How many frozen columns the room allowed.</param>
/// <param name="EndFrozen">Whether the end columns are frozen.</param>
public sealed record ColumnLayout(
    IReadOnlyList<PlacedColumn> Columns,
    double Gutter,
    double Fs,
    double Fe,
    double ScrollWidth,
    double Viewport,
    double RowWidth,
    int FrozenStartApplied,
    bool EndFrozen);

/// <summary>How much of a viewport columns may keep to themselves.</summary>
public static class FrozenRegions
{
    /// <summary>The padding after the end columns, which keeps them clear of the scroll bar.</summary>
    public const double EndPadding = 16;

    /// <summary>
    /// What fits in half the viewport: the gutter always, the end columns if they fit, then the
    /// frozen columns one at a time in order while they fit.
    /// </summary>
    /// <param name="viewport">The width of the viewport.</param>
    /// <param name="gutter">The width of the gutter.</param>
    /// <param name="endWidth">The width of the end columns with their padding; zero when none.</param>
    /// <param name="frozenWishWidths">The widths of the columns the person wants frozen, in order.</param>
    /// <returns>How many frozen columns fit, and whether the end columns are frozen.</returns>
    public static (int Start, bool End) Allocate(double viewport, double gutter, double endWidth, IReadOnlyList<double> frozenWishWidths)
    {
        ArgumentNullException.ThrowIfNull(frozenWishWidths);
        double budget = viewport / 2;
        double used = gutter;
        bool end = false;
        if (endWidth > 0 && used + endWidth <= budget)
        {
            end = true;
            used += endWidth;
        }

        int start = 0;
        foreach (double width in frozenWishWidths)
        {
            if (used + width > budget)
            {
                break;
            }

            used += width;
            start++;
        }

        return (start, end);
    }
}

/// <summary>Places the columns of a table.</summary>
public static class ColumnLayoutBuilder
{
    // Takes up to a number of pixels from the columns of words, numbers and states the person gave no width to, never below their least,
    // each giving in proportion to what it can give, in whole pixels; returns what was taken.
    private static double TakeFromMiddle(IReadOnlyList<ColumnSpec> shown, double[] widths, double wanted)
    {
        List<int> givers = [];
        List<double> room = [];
        double total = 0;
        for (int i = 0; i < shown.Count; i++)
        {
            ColumnSpec column = shown[i];
            if (column.Flexible || column.Sized || column.Kind is not (ColumnKind.Words or ColumnKind.Number or ColumnKind.Status))
            {
                continue;
            }

            double spare = Math.Max(0, Math.Floor(widths[i]) - ColumnSizing.LeastWidth(column.Kind));
            if (spare > 0)
            {
                givers.Add(i);
                room.Add(spare);
                total += spare;
            }
        }

        if (total <= 0)
        {
            return 0;
        }

        double take = Math.Min(Math.Ceiling(wanted), total);
        double[] given = new double[givers.Count];
        double taken = 0;
        for (int g = 0; g < givers.Count; g++)
        {
            given[g] = Math.Floor(take * room[g] / total);
            taken += given[g];
        }

        // The pixels the shares left over are taken one at a time from the first that can still give.
        for (int g = 0; taken < take && g < givers.Count; g = (g + 1) % givers.Count)
        {
            if (given[g] < room[g])
            {
                given[g]++;
                taken++;
            }
        }

        for (int g = 0; g < givers.Count; g++)
        {
            widths[givers[g]] -= given[g];
        }

        return taken;
    }

    /// <summary>Works out the layout of a table's columns for a viewport.</summary>
    /// <param name="shown">The columns that scroll or are frozen at the start, in order.</param>
    /// <param name="frozenStartWished">How many of them the person wants frozen.</param>
    /// <param name="endColumns">The columns kept at the end, in order.</param>
    /// <param name="gutter">The width of the check and chevron cells.</param>
    /// <param name="viewport">The width of the viewport.</param>
    /// <returns>The layout.</returns>
    public static ColumnLayout Build(
        IReadOnlyList<ColumnSpec> shown,
        int frozenStartWished,
        IReadOnlyList<ColumnSpec> endColumns,
        double gutter,
        double viewport)
    {
        ArgumentNullException.ThrowIfNull(shown);
        ArgumentNullException.ThrowIfNull(endColumns);
        double endWidth = endColumns.Count == 0 ? 0 : endColumns.Sum(column => column.Width) + FrozenRegions.EndPadding;
        int wished = Math.Clamp(frozenStartWished, 0, shown.Count);
        double[] wishWidths = shown.Take(wished).Select(column => column.Width).ToArray();
        (int applied, bool endFrozen) = FrozenRegions.Allocate(viewport, gutter, endWidth, wishWidths);

        // Every column in order, with the group it is in. End columns that are not frozen scroll, after the rest.
        List<(ColumnSpec Spec, ColumnGroup Group)> all = [];
        for (int i = 0; i < shown.Count; i++)
        {
            all.Add((shown[i], i < applied ? ColumnGroup.FrozenStart : ColumnGroup.Scroll));
        }

        foreach (ColumnSpec spec in endColumns)
        {
            all.Add((spec, endFrozen ? ColumnGroup.FrozenEnd : ColumnGroup.Scroll));
        }

        double[] widths = all.Select(entry => entry.Spec.Width).ToArray();
        double Sum(ColumnGroup group) => all.Select((entry, i) => entry.Group == group ? widths[i] : 0).Sum();

        double fs = gutter + Sum(ColumnGroup.FrozenStart);
        double fe = endFrozen ? endWidth : 0;
        double scrollWidth = Sum(ColumnGroup.Scroll);
        double sum = fs + scrollWidth + fe;

        // A table too wide for its viewport finds the room in this order, and scrolls only when none is left: the name gives what it
        // has above what its words need; then the other columns of words, numbers and states give, down to their least, each in
        // proportion to what it can give; and only then the name gives below what its words need, down to the least a person may make it.
        // A column the person gave a width, a column of controls and a picture keep theirs.
        if (sum > viewport)
        {
            int title = -1;
            for (int i = 0; i < shown.Count; i++)
            {
                if (shown[i].Flexible)
                {
                    title = i;
                    break;
                }
            }

            double excess = sum - viewport;
            if (title >= 0)
            {
                double give = Math.Min(excess, Math.Max(0, widths[title] - ColumnSet.TitleNeed));
                widths[title] -= give;
                excess -= give;
            }

            if (excess > 0)
            {
                excess -= TakeFromMiddle(shown, widths, excess);
            }

            if (excess > 0 && title >= 0)
            {
                double least = ColumnSizing.ResizeRange(shown[title].Kind).Min;
                widths[title] -= Math.Min(excess, Math.Max(0, widths[title] - least));
            }

            fs = gutter + Sum(ColumnGroup.FrozenStart);
            scrollWidth = Sum(ColumnGroup.Scroll);
            sum = fs + scrollWidth + fe;
        }

        // Room left over goes to the flexible column, else to the last words column, within its range.
        if (sum < viewport)
        {
            double slack = viewport - sum;
            int flexible = -1;
            for (int i = 0; i < shown.Count; i++)
            {
                if (shown[i].Flexible)
                {
                    flexible = i;
                    break;
                }
            }

            if (flexible >= 0)
            {
                widths[flexible] += slack;
            }
            else
            {
                for (int i = shown.Count - 1; i >= 0; i--)
                {
                    if (shown[i].Kind == ColumnKind.Words)
                    {
                        double room = Math.Max(0, ColumnSizing.ResizeRange(ColumnKind.Words).Max - widths[i]);
                        widths[i] += Math.Min(slack, room);
                        break;
                    }
                }
            }

            fs = gutter + Sum(ColumnGroup.FrozenStart);
            scrollWidth = Sum(ColumnGroup.Scroll);
            sum = fs + scrollWidth + fe;
        }

        double rowWidth = Math.Max(sum, viewport);
        double frozenX = gutter;
        double scrollX = fs;
        double endX = rowWidth - fe;
        List<PlacedColumn> placed = new(all.Count);
        for (int i = 0; i < all.Count; i++)
        {
            (ColumnSpec spec, ColumnGroup group) = all[i];
            double x;
            switch (group)
            {
                case ColumnGroup.FrozenStart:
                    x = frozenX;
                    frozenX += widths[i];
                    break;
                case ColumnGroup.FrozenEnd:
                    x = endX;
                    endX += widths[i];
                    break;
                default:
                    x = scrollX;
                    scrollX += widths[i];
                    break;
            }

            placed.Add(new PlacedColumn(spec.Key, widths[i], group, x));
        }

        return new ColumnLayout(placed, gutter, fs, fe, scrollWidth, viewport, rowWidth, applied, endFrozen);
    }
}
