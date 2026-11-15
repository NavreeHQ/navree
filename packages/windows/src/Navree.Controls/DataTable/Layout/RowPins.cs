namespace Navree.Controls;

/// <summary>Why a row is kept made wherever it stands.</summary>
internal enum PinOwner
{
    /// <summary>The row of the active cell, which holds keyboard focus.</summary>
    Active,

    /// <summary>The row the view is on while rows join or leave above it.</summary>
    Anchor,

    /// <summary>A row assistive technology asked for.</summary>
    Assistive,
}

/// <summary>
/// The rows kept made wherever they stand, a set with one place for each owner: one owner letting go of its row, or
/// asking for another, does not take another owner's row away.
/// </summary>
internal sealed class RowPins
{
    private readonly int[] places = [-1, -1, -1];

    /// <summary>The distinct places pinned, in ascending order.</summary>
    internal IReadOnlyList<int> Indices => [.. places.Where(place => place >= 0).Distinct().Order()];

    /// <summary>The place an owner has pinned, or minus one.</summary>
    /// <param name="owner">The owner.</param>
    /// <returns>The place.</returns>
    internal int Of(PinOwner owner) => places[(int)owner];

    /// <summary>Pins a place for an owner, which lets go of the one it had.</summary>
    /// <param name="owner">The owner.</param>
    /// <param name="index">The place; a negative number lets go.</param>
    internal void Set(PinOwner owner, int index) => places[(int)owner] = Math.Max(-1, index);

    /// <summary>Lets every owner's row go.</summary>
    internal void Clear() => Array.Fill(places, -1);

    /// <summary>Lets an owner's row go.</summary>
    /// <param name="owner">The owner.</param>
    internal void Release(PinOwner owner) => places[(int)owner] = -1;

    /// <summary>Moves the pins at or after a place when rows joined there.</summary>
    /// <param name="at">The place of the first row that joined.</param>
    /// <param name="count">How many joined.</param>
    internal void RowsJoined(int at, int count)
    {
        for (int i = 0; i < places.Length; i++)
        {
            if (places[i] >= at)
            {
                places[i] += count;
            }
        }
    }

    /// <summary>Moves the pins after rows that left, and lets go of those whose row left.</summary>
    /// <param name="at">The place of the first row that left.</param>
    /// <param name="count">How many left.</param>
    internal void RowsLeft(int at, int count)
    {
        for (int i = 0; i < places.Length; i++)
        {
            if (places[i] >= at + count)
            {
                places[i] -= count;
            }
            else if (places[i] >= at)
            {
                places[i] = -1;
            }
        }
    }
}

/// <summary>The names of the held rows that typing looks through, read once for the rows held and forgotten when they change.</summary>
internal sealed class RowNameCache
{
    private List<string>? names;

    /// <summary>Whether the names have been read and not forgotten.</summary>
    internal bool HasNames => names is not null;

    /// <summary>The names, read from the model when there are none yet.</summary>
    /// <param name="count">How many rows are held.</param>
    /// <param name="name">The name of the row at a place.</param>
    /// <returns>The names.</returns>
    internal IReadOnlyList<string> Get(int count, Func<int, string> name)
    {
        ArgumentNullException.ThrowIfNull(name);
        return names ??= Enumerable.Range(0, count).Select(name).ToList();
    }

    /// <summary>Forgets the names: the rows changed.</summary>
    internal void Clear() => names = null;
}
