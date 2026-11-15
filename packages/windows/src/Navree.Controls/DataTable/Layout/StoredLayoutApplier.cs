using Navree.Design.Tables;

namespace Navree.Controls;

/// <summary>What a person's kept layout says for the columns the table has, apart from what goes into the sets the table holds.</summary>
/// <param name="Order">The keys of the columns in the person's order.</param>
/// <param name="Frozen">How many columns are frozen at the start.</param>
/// <param name="Density">The density the person chose, or null when the kept text names none.</param>
/// <param name="Seen">The count of the last write the text was made by.</param>
internal sealed record AppliedLayout(IReadOnlyList<string> Order, int Frozen, Density? Density, int Seen);

/// <summary>Puts a kept layout onto a table's columns.</summary>
internal static class StoredLayoutApplier
{
    /// <summary>Puts widths, hidden and shown columns into the sets given, and says the rest.</summary>
    /// <param name="stored">What was kept.</param>
    /// <param name="columns">The columns the server has now.</param>
    /// <param name="widths">The widths of the columns by key; the widths the person gave are put in.</param>
    /// <param name="personWidths">The keys of the columns whose width the person gave.</param>
    /// <param name="hidden">The keys of the columns the person hid.</param>
    /// <param name="forcedShown">The keys of the secondary columns the person showed.</param>
    /// <returns>The rest of what the text says, or null when it names none of the columns the server has now.</returns>
    internal static AppliedLayout? Apply(
        StoredLayout stored,
        IReadOnlyList<DataTableColumn> columns,
        Dictionary<string, double> widths,
        HashSet<string> personWidths,
        HashSet<string> hidden,
        HashSet<string> forcedShown)
    {
        ArgumentNullException.ThrowIfNull(stored);
        ArgumentNullException.ThrowIfNull(columns);
        List<string> serverKeys = columns.Select(c => c.Key).ToList();
        if (!stored.Order.Any(serverKeys.Contains) && stored.Columns.Keys.All(k => !serverKeys.Contains(k)))
        {
            return null;
        }

        foreach (DataTableColumn column in columns)
        {
            if (!stored.Columns.TryGetValue(column.Key, out StoredColumn? kept))
            {
                continue;
            }

            if (kept.Width is { } width && string.Equals(kept.Kind, column.Kind.ToString(), StringComparison.OrdinalIgnoreCase))
            {
                widths[column.Key] = ColumnSizing.Clamp(column.Kind, width);
                personWidths.Add(column.Key);
            }

            if (kept.Hidden is true)
            {
                hidden.Add(column.Key);
            }
            else if (kept.Hidden is false)
            {
                forcedShown.Add(column.Key);
            }
        }

        Density? density = Enum.TryParse(stored.Density, ignoreCase: true, out Density chosen) ? chosen : null;
        return new AppliedLayout(
            [.. ColumnOrder.Reconcile(serverKeys, stored.Order)],
            Math.Clamp(stored.Frozen, 0, columns.Count),
            density,
            stored.Columns.Values.Select(c => c.Seen).DefaultIfEmpty(0).Max());
    }
}
