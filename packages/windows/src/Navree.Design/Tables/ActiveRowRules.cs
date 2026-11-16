namespace Navree.Design.Tables;

/// <summary>Where the active row stands when the rows change: it follows its row by key, not by place.</summary>
public static class ActiveRowRules
{
    /// <summary>Where the active row is after every row was replaced.</summary>
    /// <param name="onHeader">Whether the active place was a heading.</param>
    /// <param name="indexOfKey">The place the same row's key has in the new rows, or minus one when the new rows do not have it.</param>
    /// <param name="rowCount">How many rows there are now.</param>
    /// <returns>The same row when the new rows have it, else the first row; a heading stays a heading; minus one when there are no rows.</returns>
    public static int AfterReset(bool onHeader, int indexOfKey, int rowCount)
    {
        if (onHeader || rowCount <= 0)
        {
            return -1;
        }

        return indexOfKey >= 0 && indexOfKey < rowCount ? indexOfKey : 0;
    }

    /// <summary>Where the active row is after rows joined.</summary>
    /// <param name="active">The active row's place before; minus one for a heading.</param>
    /// <param name="at">The place of the first row that joined.</param>
    /// <param name="count">How many joined.</param>
    /// <returns>The place of the same row.</returns>
    public static int AfterInsert(int active, int at, int count) => active >= 0 && active >= at ? active + count : active;

    /// <summary>Where the active row is after rows left.</summary>
    /// <param name="active">The active row's place before; minus one for a heading.</param>
    /// <param name="at">The place of the first row that left.</param>
    /// <param name="count">How many left.</param>
    /// <param name="rowCountAfter">How many rows are left.</param>
    /// <param name="column">The active column.</param>
    /// <param name="hasHeader">Whether the headings are part of the grid.</param>
    /// <returns>
    /// The place of the same row when it stayed; when it left, the cell now at its place (the last row when it was the last, the
    /// heading when none is left, and null when there is nothing), with <c>Removed</c> true.
    /// </returns>
    public static (Cell? Active, bool Removed) AfterRemove(int active, int at, int count, int rowCountAfter, int column, bool hasHeader)
    {
        if (active < 0)
        {
            return (new Cell(active, column), false);
        }

        if (active >= at + count)
        {
            return (new Cell(active - count, column), false);
        }

        if (active < at)
        {
            return (new Cell(active, column), false);
        }

        return (GridNavigation.FocusAfterRemoval(at, rowCountAfter, column, hasHeader), true);
    }

    /// <summary>Whether rows that joined the start leave the view on the row that was first: only a view at the very top does not already keep its row.</summary>
    /// <param name="at">The place of the first row that joined.</param>
    /// <param name="rowsBefore">How many rows were held before.</param>
    /// <param name="offset">The vertical scroll offset.</param>
    /// <returns>True when the view is moved down by the rows that joined.</returns>
    public static bool KeepsFirstRowInView(int at, int rowsBefore, double offset) => at == 0 && rowsBefore > 0 && !(offset > 0.5);
}
