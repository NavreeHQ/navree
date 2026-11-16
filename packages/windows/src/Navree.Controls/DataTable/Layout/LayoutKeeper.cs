using Navree.Design.Tables;

namespace Navree.Controls;

/// <summary>
/// Decides when a person's table layout is read and when it is written: it is written only after it was
/// read, and only when the person changed something since, so a table that never got to lay out cannot
/// replace what was kept with its defaults.
/// </summary>
internal sealed class LayoutKeeper
{
    private string? readFor;
    private bool changed;
    private int counter;

    /// <summary>Whether what was kept has been read for the columns the table has now.</summary>
    internal bool WasRead => readFor is not null;

    /// <summary>Whether the person changed something since the layout was read or last written.</summary>
    internal bool HasChanges => changed;

    /// <summary>The count of the last write, which marks when a column was last in the table.</summary>
    internal int Counter => counter;

    /// <summary>Says whether the layout is to be read for a key and a set of columns: once for each pair.</summary>
    /// <param name="key">What the layout is kept under.</param>
    /// <param name="columnKeys">The keys of the server's columns.</param>
    /// <returns>True the first time a key and a set of columns are seen.</returns>
    internal bool ShouldRead(string key, IReadOnlyList<string> columnKeys)
    {
        string signature = key + "|" + string.Join(',', columnKeys);
        if (readFor == signature)
        {
            return false;
        }

        readFor = signature;
        return true;
    }

    /// <summary>Takes the count the kept layout was written with.</summary>
    /// <param name="stored">The count.</param>
    internal void Resume(int stored) => counter = Math.Max(counter, stored);

    /// <summary>Notes that a person changed the layout.</summary>
    internal void Changed() => changed = true;

    /// <summary>Forgets the key and the changes: the table is given other columns to read.</summary>
    internal void Forget()
    {
        readFor = null;
        changed = false;
    }

    /// <summary>Writes the layout when it was read and has changed since.</summary>
    /// <param name="store">Where it is kept.</param>
    /// <param name="key">What it is kept under.</param>
    /// <param name="build">Makes the layout to keep, given the count of this write.</param>
    /// <returns>True when something was written.</returns>
    internal bool TrySave(IDataTableLayoutStore store, string key, Func<int, StoredLayout> build)
    {
        ArgumentNullException.ThrowIfNull(store);
        ArgumentNullException.ThrowIfNull(build);
        if (!WasRead || !changed)
        {
            return false;
        }

        counter++;
        store.Save(key, LayoutText.Serialize(build(counter)));
        changed = false;
        return true;
    }
}
