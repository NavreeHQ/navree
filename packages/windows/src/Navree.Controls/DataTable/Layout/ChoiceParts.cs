using System.Collections;
using Navree.Design.Tables;

namespace Navree.Controls;

/// <summary>
/// How many held rows can be chosen and how many of those are, kept in step with each change of the choice by what changed, so a key
/// held down at a hundred thousand rows does not count them again; it is counted again only after the rows or the whole choice changed.
/// </summary>
internal sealed class ChoiceTally
{
    private int held;
    private int chosen;
    private bool valid;

    /// <summary>The counts, read by <paramref name="recount"/> when they are not known.</summary>
    /// <param name="recount">Counts every held row.</param>
    /// <returns>How many held rows can be chosen, and how many of those are.</returns>
    internal (int Held, int Chosen) Get(Func<(int Held, int Chosen)> recount)
    {
        ArgumentNullException.ThrowIfNull(recount);
        if (!valid)
        {
            (held, chosen) = recount();
            valid = true;
        }

        return (held, chosen);
    }

    /// <summary>Forgets the counts: the rows changed, or the whole choice did.</summary>
    internal void Invalidate() => valid = false;

    /// <summary>Notes that held rows that can be chosen were chosen (a positive number) or cleared (a negative one).</summary>
    /// <param name="delta">How many.</param>
    internal void Changed(int delta)
    {
        if (valid)
        {
            chosen = Math.Clamp(chosen + delta, 0, held);
        }
    }

    /// <summary>Notes that no row is chosen.</summary>
    internal void NoneChosen()
    {
        if (valid)
        {
            chosen = 0;
        }
    }
}

/// <summary>Whether each held row can be chosen, read from the model when asked and not copied: a range of rows reads only the rows in it.</summary>
/// <param name="count">How many rows are held.</param>
/// <param name="at">Whether the row at a place can be chosen.</param>
internal sealed class RowChoosables(int count, Func<int, Choosable> at) : IReadOnlyList<Choosable>
{
    /// <inheritdoc />
    public int Count => count;

    /// <inheritdoc />
    public Choosable this[int index] => at(index);

    /// <inheritdoc />
    public IEnumerator<Choosable> GetEnumerator()
    {
        for (int i = 0; i < count; i++)
        {
            yield return at(i);
        }
    }

    IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
}

/// <summary>Keys for rows the server gave none: each new one is its own, so rows that join the start never take a key a row held already has.</summary>
internal sealed class KeylessKeys
{
    private long made;

    /// <summary>A key no row has had.</summary>
    /// <returns>The key.</returns>
    internal string Next() => "\u0001" + (++made).ToString(System.Globalization.CultureInfo.InvariantCulture);

    /// <summary>Whether a key is one made here.</summary>
    /// <param name="key">The key.</param>
    /// <returns>True for a key of a row the server gave none for.</returns>
    internal static bool IsKeyless(string key) => key.Length > 0 && key[0] == '\u0001';
}

/// <summary>Notices when the text size or the display's scale changed, which is when widths and the height of a row are to be worked out again.</summary>
internal sealed class MetricsWatch
{
    private double textScale = double.NaN;
    private double rasterizationScale = double.NaN;

    /// <summary>Notes the text size and the scale now.</summary>
    /// <param name="newTextScale">The text scale factor.</param>
    /// <param name="newRasterizationScale">The display's scale.</param>
    /// <returns>True when either differs from what was noted before; the first time too.</returns>
    internal bool Changed(double newTextScale, double newRasterizationScale)
    {
        bool changed = !(Math.Abs(newTextScale - textScale) < 1e-9) || !(Math.Abs(newRasterizationScale - rasterizationScale) < 1e-9);
        textScale = newTextScale;
        rasterizationScale = newRasterizationScale;
        return changed;
    }
}
