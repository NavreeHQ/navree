namespace Navree.Design.Tables;

/// <summary>Keeping the view where it was when rows change above it.</summary>
public static class Anchoring
{
    /// <summary>The row at the top of the viewport, and how far down inside it the top is.</summary>
    /// <param name="geometry">The rows' geometry.</param>
    /// <param name="offset">The vertical scroll offset.</param>
    /// <param name="count">How many rows the table holds.</param>
    /// <returns>The row's index and the distance inside it.</returns>
    public static (int Index, double Within) AnchorAt(Geometry geometry, double offset, int count)
    {
        ArgumentNullException.ThrowIfNull(geometry);
        return geometry.IndexAt(offset, count);
    }

    /// <summary>Where the offset must be for the anchor row to stand where it stood in the viewport.</summary>
    /// <param name="newAnchorIndex">The anchor row's index after the change.</param>
    /// <param name="within">How far down inside the row the viewport's top was.</param>
    /// <param name="after">The geometry after the change.</param>
    /// <param name="countAfter">How many rows the table holds after the change.</param>
    /// <param name="viewport">The height of the viewport.</param>
    /// <returns>The offset, never less than zero or past the end.</returns>
    public static double OffsetAfterChange(int newAnchorIndex, double within, Geometry after, int countAfter, double viewport)
    {
        ArgumentNullException.ThrowIfNull(after);
        double offset = after.RowTop(newAnchorIndex) + within;
        double most = Math.Max(0, after.Extent(countAfter) - viewport);
        return Math.Clamp(offset, 0, most);
    }
}

/// <summary>When to ask for more rows.</summary>
public static class Cursor
{
    /// <summary>Whether the end of the rows is near enough to ask for the rows after them.</summary>
    /// <param name="offset">The vertical scroll offset.</param>
    /// <param name="viewport">The height of the viewport.</param>
    /// <param name="extent">The height of all the rows held.</param>
    /// <param name="nearViewports">How many viewports from the end counts as near.</param>
    /// <returns>True when the end is near.</returns>
    public static bool ShouldAskMore(double offset, double viewport, double extent, double nearViewports = 2) =>
        extent - (offset + viewport) <= nearViewports * viewport;

    /// <summary>Whether the start of the rows is near enough to ask for the rows before them.</summary>
    /// <param name="offset">The vertical scroll offset.</param>
    /// <param name="viewport">The height of the viewport.</param>
    /// <param name="nearViewports">How many viewports from the start counts as near.</param>
    /// <returns>True when the start is near.</returns>
    public static bool ShouldAskBefore(double offset, double viewport, double nearViewports = 1) =>
        offset <= nearViewports * viewport;
}
