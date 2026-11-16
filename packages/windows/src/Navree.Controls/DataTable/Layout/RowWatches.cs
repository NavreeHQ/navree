namespace Navree.Controls;

/// <summary>
/// Remembers which ends of the rows more were asked for at which count, so the host is asked once for each count and is asked
/// again when the rows were replaced or a cursor changed; and whether the held limit was said.
/// </summary>
internal sealed class CursorWatch
{
    private int afterAt = -1;
    private int beforeAt = -1;
    private bool limitRaised;

    /// <summary>Forgets everything: the model was given other rows as a whole, or is another model.</summary>
    internal void Forget()
    {
        afterAt = beforeAt = -1;
        limitRaised = false;
    }

    /// <summary>Forgets what was asked: a cursor changed, or rows joined.</summary>
    internal void Asked() => afterAt = beforeAt = -1;

    /// <summary>Whether rows after the last are to be asked for now; true once for each count of rows held.</summary>
    /// <param name="rowCount">How many rows are held.</param>
    /// <param name="available">Whether there are more rows after them.</param>
    /// <param name="idle">Whether nothing is on its way.</param>
    /// <param name="near">Whether the view is near the end.</param>
    /// <returns>True when the host is to be asked.</returns>
    internal bool ShouldAskAfter(int rowCount, bool available, bool idle, bool near)
    {
        if (!available || !idle || !near || afterAt == rowCount)
        {
            return false;
        }

        afterAt = rowCount;
        return true;
    }

    /// <summary>Whether rows before the first are to be asked for now; true once for each count of rows held.</summary>
    /// <param name="rowCount">How many rows are held.</param>
    /// <param name="available">Whether there are more rows before them.</param>
    /// <param name="idle">Whether nothing is on its way.</param>
    /// <param name="near">Whether the view is near the start.</param>
    /// <returns>True when the host is to be asked.</returns>
    internal bool ShouldAskBefore(int rowCount, bool available, bool idle, bool near)
    {
        if (!available || !idle || !near || beforeAt == rowCount)
        {
            return false;
        }

        beforeAt = rowCount;
        return true;
    }

    /// <summary>Whether the held limit is to be said now; true once until the rows are forgotten.</summary>
    /// <returns>True the first time.</returns>
    internal bool LimitReached()
    {
        if (limitRaised)
        {
            return false;
        }

        limitRaised = true;
        return true;
    }
}

/// <summary>Remembers which rows were drawn open by the server, once each, so a person's shutting one is not undone by a row drawn again.</summary>
internal sealed class DetailSeeds
{
    private readonly HashSet<string> seeded = new(StringComparer.Ordinal);

    /// <summary>Forgets every seed: the model is another model.</summary>
    internal void Forget() => seeded.Clear();

    /// <summary>Whether a row that arrived is to be opened.</summary>
    /// <param name="key">The row's key, or null.</param>
    /// <param name="hasDetail">Whether the row has a detail.</param>
    /// <param name="detailOpen">Whether the server says it is open.</param>
    /// <returns>True the first time a row with a detail arrives open.</returns>
    internal bool Arrive(string? key, bool hasDetail, bool detailOpen) => key is not null && hasDetail && detailOpen && seeded.Add(key);
}

/// <summary>
/// Notices when the rows that arrive need what the first rows did not: a second line under the name makes every row two lines tall,
/// and a detail needs a cell to open it by. It says so once for each, until the rows are replaced.
/// </summary>
internal sealed class RowShapeWatch
{
    private bool twoLines;
    private bool details;

    /// <summary>Whether a row that arrived had a second line.</summary>
    internal bool SawSecondLine => twoLines;

    /// <summary>Whether a row that arrived had a detail.</summary>
    internal bool SawDetail => details;

    /// <summary>Forgets what was seen: the rows were replaced as a whole.</summary>
    internal void Forget() => twoLines = details = false;

    /// <summary>Notes a row that arrived.</summary>
    /// <param name="hasSecondLine">Whether it has a second line.</param>
    /// <param name="hasDetail">Whether it has a detail.</param>
    /// <returns>Whether it showed the rows need a second line, and whether it showed they need a detail cell, for the first time.</returns>
    internal (bool NewSecondLine, bool NewDetail) Observe(bool hasSecondLine, bool hasDetail)
    {
        bool newSecond = hasSecondLine && !twoLines;
        bool newDetail = hasDetail && !details;
        twoLines |= hasSecondLine;
        details |= hasDetail;
        return (newSecond, newDetail);
    }
}
