namespace Navree.Design.Tables;

/// <summary>A column as the choice of shown columns reads it.</summary>
/// <param name="Key">The column's identity.</param>
/// <param name="Kind">What it holds.</param>
/// <param name="Role">What it is to its row.</param>
/// <param name="Secondary">Whether the server says a narrow window may leave it out.</param>
public readonly record struct ColInfo(string Key, ColumnKind Kind, ColumnRole Role, bool Secondary);

/// <summary>Which columns a table shows.</summary>
public static class ColumnSet
{
    /// <summary>The width a name column is counted at when the room is shared out: it takes what is left, down to its least, so it is not counted whole.</summary>
    public const double TitleComfort = 160;

    /// <summary>The most columns a table shows.</summary>
    public const int MostShown = 48;

    /// <summary>
    /// The columns to show: all but the hidden ones and the subtitle, and when the room cannot hold them at the widths they ask for,
    /// the secondary columns go first, the last one first, until the rest fit or no secondary column is left to go.
    /// </summary>
    /// <param name="inPersonsOrder">All the columns, in the person's order.</param>
    /// <param name="hidden">The keys the person hid.</param>
    /// <param name="forcedShown">The keys of secondary columns the person showed: these do not go.</param>
    /// <param name="widths">The width each column asks for, in the same order.</param>
    /// <param name="room">The width the columns share (the table's, less the gutter).</param>
    /// <param name="cap">The most columns to show.</param>
    /// <returns>The indices of the columns shown, in order.</returns>
    public static IReadOnlyList<int> Shown(
        IReadOnlyList<ColInfo> inPersonsOrder,
        IReadOnlySet<string> hidden,
        IReadOnlySet<string> forcedShown,
        IReadOnlyList<double> widths,
        double room,
        int cap = MostShown)
    {
        ArgumentNullException.ThrowIfNull(inPersonsOrder);
        ArgumentNullException.ThrowIfNull(hidden);
        ArgumentNullException.ThrowIfNull(forcedShown);
        ArgumentNullException.ThrowIfNull(widths);
        if (widths.Count != inPersonsOrder.Count)
        {
            throw new ArgumentException("Each column has a width.", nameof(widths));
        }

        List<int> shown = [];
        for (int i = 0; i < inPersonsOrder.Count && shown.Count < cap; i++)
        {
            ColInfo column = inPersonsOrder[i];
            if (column.Role != ColumnRole.Subtitle && !hidden.Contains(column.Key))
            {
                shown.Add(i);
            }
        }

        double Asked() => shown.Sum(i => inPersonsOrder[i].Kind == ColumnKind.Title ? Math.Min(widths[i], TitleComfort) : widths[i]);
        for (int at = shown.Count - 1; at >= 0 && Asked() > room; at--)
        {
            ColInfo column = inPersonsOrder[shown[at]];
            if (column.Secondary && !forcedShown.Contains(column.Key))
            {
                shown.RemoveAt(at);
            }
        }

        return shown;
    }

    /// <summary>Whether a shown column may be hidden.</summary>
    /// <param name="shown">The columns shown.</param>
    /// <param name="index">The index, among them, of the column.</param>
    /// <returns>False for the name, for the actions and a leading picture, and for the last column of words.</returns>
    public static bool CanHide(IReadOnlyList<ColInfo> shown, int index)
    {
        ArgumentNullException.ThrowIfNull(shown);
        ArgumentOutOfRangeException.ThrowIfNegative(index);
        ArgumentOutOfRangeException.ThrowIfGreaterThanOrEqual(index, shown.Count);
        ColInfo column = shown[index];
        if (column.Kind is ColumnKind.Title or ColumnKind.Controls or ColumnKind.Leading)
        {
            return false;
        }

        for (int i = 0; i < shown.Count; i++)
        {
            if (i != index && shown[i].Kind is ColumnKind.Title or ColumnKind.Words or ColumnKind.Number or ColumnKind.Status)
            {
                return true;
            }
        }

        return false;
    }

    /// <summary>How many columns are frozen at the start unless the person says otherwise.</summary>
    /// <param name="shown">The columns shown.</param>
    /// <returns>Two when a picture leads the name, one for any other table, zero for none.</returns>
    public static int DefaultFrozen(IReadOnlyList<ColInfo> shown)
    {
        ArgumentNullException.ThrowIfNull(shown);
        if (shown.Count == 0)
        {
            return 0;
        }

        return shown.Count >= 2 && shown[0].Kind == ColumnKind.Leading && shown[1].Kind == ColumnKind.Title ? 2 : 1;
    }
}

/// <summary>The order of a table's columns.</summary>
public static class ColumnOrder
{
    /// <summary>A stored order made to fit the columns the server now has.</summary>
    /// <param name="serverKeys">The columns' keys as the server lists them.</param>
    /// <param name="stored">The keys in the order the person kept.</param>
    /// <returns>The keys in the person's order, a new column after the one before it in the server's order.</returns>
    public static IReadOnlyList<string> Reconcile(IReadOnlyList<string> serverKeys, IReadOnlyList<string> stored)
    {
        ArgumentNullException.ThrowIfNull(serverKeys);
        ArgumentNullException.ThrowIfNull(stored);
        HashSet<string> server = [.. serverKeys];
        List<string> order = [];
        foreach (string key in stored)
        {
            if (server.Contains(key) && !order.Contains(key))
            {
                order.Add(key);
            }
        }

        if (order.Count == 0)
        {
            return [.. serverKeys];
        }

        for (int i = 0; i < serverKeys.Count; i++)
        {
            string key = serverKeys[i];
            if (order.Contains(key))
            {
                continue;
            }

            int at = 0;
            for (int before = i - 1; before >= 0; before--)
            {
                int found = order.IndexOf(serverKeys[before]);
                if (found >= 0)
                {
                    at = found + 1;
                    break;
                }
            }

            order.Insert(at, key);
        }

        return order;
    }

    /// <summary>An order with one column moved.</summary>
    /// <param name="order">The keys.</param>
    /// <param name="from">The index of the column to move.</param>
    /// <param name="to">The index it takes.</param>
    /// <returns>The new order.</returns>
    public static IReadOnlyList<string> Move(IReadOnlyList<string> order, int from, int to)
    {
        ArgumentNullException.ThrowIfNull(order);
        ArgumentOutOfRangeException.ThrowIfNegative(from);
        ArgumentOutOfRangeException.ThrowIfNegative(to);
        ArgumentOutOfRangeException.ThrowIfGreaterThanOrEqual(from, order.Count);
        ArgumentOutOfRangeException.ThrowIfGreaterThanOrEqual(to, order.Count);
        List<string> moved = [.. order];
        string key = moved[from];
        moved.RemoveAt(from);
        moved.Insert(to, key);
        return moved;
    }

    /// <summary>The place a dragged column would take, from where the pointer is.</summary>
    /// <param name="dragged">The index of the column being dragged.</param>
    /// <param name="pointerX">The pointer's position.</param>
    /// <param name="centres">The centre of each column.</param>
    /// <param name="regionStart">The first index the column may take.</param>
    /// <param name="regionEnd">The last index the column may take.</param>
    /// <returns>The index: how many other columns have their centre before the pointer, kept in the region.</returns>
    public static int DropIndex(int dragged, double pointerX, IReadOnlyList<double> centres, int regionStart, int regionEnd)
    {
        ArgumentNullException.ThrowIfNull(centres);
        int before = 0;
        for (int i = 0; i < centres.Count; i++)
        {
            if (i != dragged && centres[i] < pointerX)
            {
                before++;
            }
        }

        return Math.Clamp(before, regionStart, regionEnd);
    }

    /// <summary>Whether a column may move one step.</summary>
    /// <param name="index">The column's index.</param>
    /// <param name="delta">One for a step toward the end, minus one toward the start.</param>
    /// <param name="frozenStart">How many columns are frozen at the start.</param>
    /// <param name="frozenEnd">How many columns are frozen at the end.</param>
    /// <param name="count">How many columns there are.</param>
    /// <returns>True when the step stays inside the column's own region.</returns>
    public static bool CanMove(int index, int delta, int frozenStart, int frozenEnd, int count)
    {
        int target = index + delta;
        if (index < 0 || index >= count || target < 0 || target >= count)
        {
            return false;
        }

        return RegionOf(index, frozenStart, frozenEnd, count) == RegionOf(target, frozenStart, frozenEnd, count);
    }

    private static int RegionOf(int index, int frozenStart, int frozenEnd, int count)
    {
        if (index < frozenStart)
        {
            return 0;
        }

        return index >= count - frozenEnd ? 2 : 1;
    }
}
