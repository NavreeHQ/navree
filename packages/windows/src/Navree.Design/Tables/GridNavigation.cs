namespace Navree.Design.Tables;

/// <summary>A cell of the grid: a row and a column. The header is row minus one.</summary>
/// <param name="Row">The row; minus one for the header.</param>
/// <param name="Column">The column; zero is the inline start.</param>
public readonly record struct Cell(int Row, int Column);

/// <summary>The keys that move between cells.</summary>
public enum NavKey
{
    /// <summary>The left arrow.</summary>
    Left,

    /// <summary>The right arrow.</summary>
    Right,

    /// <summary>The up arrow.</summary>
    Up,

    /// <summary>The down arrow.</summary>
    Down,

    /// <summary>Home.</summary>
    Home,

    /// <summary>End.</summary>
    End,

    /// <summary>Page up.</summary>
    PageUp,

    /// <summary>Page down.</summary>
    PageDown,
}

/// <summary>The modifier keys held with a key.</summary>
[Flags]
public enum NavMods
{
    /// <summary>None.</summary>
    None = 0,

    /// <summary>Control.</summary>
    Ctrl = 1,

    /// <summary>Shift.</summary>
    Shift = 2,
}

/// <summary>What Enter does in a cell.</summary>
public enum ActivationTarget
{
    /// <summary>It invokes the one control the cell holds.</summary>
    CellControl,

    /// <summary>It puts focus on the first of the cell's controls.</summary>
    EnterCell,

    /// <summary>It runs the row's action.</summary>
    RowAction,

    /// <summary>It opens or shuts the row's detail.</summary>
    ToggleDetail,

    /// <summary>It does nothing.</summary>
    None,
}

/// <summary>Moving between the cells of the grid, and keeping the active one in view.</summary>
public static class GridNavigation
{
    /// <summary>Where a key moves the active cell.</summary>
    /// <param name="current">The active cell.</param>
    /// <param name="key">The key.</param>
    /// <param name="mods">The modifier keys held.</param>
    /// <param name="rowCount">How many rows the table holds.</param>
    /// <param name="columnCount">How many columns it shows.</param>
    /// <param name="hasHeader">Whether the header is part of the grid.</param>
    /// <param name="pageRows">How many rows a page key moves.</param>
    /// <param name="rtl">Whether the flow is right to left, so that Left goes to the next column.</param>
    /// <returns>The cell the key goes to, or null when it stays where it is.</returns>
    public static Cell? Move(Cell current, NavKey key, NavMods mods, int rowCount, int columnCount, bool hasHeader, int pageRows, bool rtl)
    {
        if (columnCount <= 0)
        {
            return null;
        }

        bool ctrl = mods.HasFlag(NavMods.Ctrl);
        bool inHeader = current.Row < 0;
        int row = current.Row;
        int column = current.Column;
        Cell? target = null;
        switch (key)
        {
            case NavKey.Left:
            case NavKey.Right:
                {
                    bool forward = (key == NavKey.Right) != rtl;
                    target = new Cell(row, Math.Clamp(column + (forward ? 1 : -1), 0, columnCount - 1));
                    break;
                }

            case NavKey.Up:
                if (row > 0)
                {
                    target = new Cell(row - 1, column);
                }
                else if (row == 0 && hasHeader)
                {
                    target = new Cell(-1, column);
                }

                break;
            case NavKey.Down:
                if (inHeader)
                {
                    target = rowCount > 0 ? new Cell(0, column) : null;
                }
                else if (row < rowCount - 1)
                {
                    target = new Cell(row + 1, column);
                }

                break;
            case NavKey.Home:
                target = ctrl && !inHeader && rowCount > 0 ? new Cell(0, 0) : new Cell(row, 0);
                break;
            case NavKey.End:
                target = ctrl && !inHeader && rowCount > 0 ? new Cell(rowCount - 1, columnCount - 1) : new Cell(row, columnCount - 1);
                break;
            case NavKey.PageUp:
                if (!inHeader)
                {
                    target = new Cell(Math.Max(0, row - Math.Max(1, pageRows)), column);
                }

                break;
            case NavKey.PageDown:
                if (!inHeader)
                {
                    target = new Cell(Math.Min(rowCount - 1, row + Math.Max(1, pageRows)), column);
                }

                break;
            default:
                throw new ArgumentOutOfRangeException(nameof(key), key, "Not a navigation key.");
        }

        return target is { } cell && cell != current ? cell : null;
    }

    /// <summary>How many rows a page key moves: the whole rows in the viewport, less one.</summary>
    /// <param name="viewportHeight">The height of the viewport.</param>
    /// <param name="rowHeight">The height of a row.</param>
    /// <returns>At least one.</returns>
    public static int PageRows(double viewportHeight, double rowHeight)
    {
        if (!(rowHeight > 0))
        {
            throw new ArgumentOutOfRangeException(nameof(rowHeight), rowHeight, "A row has a height.");
        }

        return Math.Max(1, (int)Math.Floor(viewportHeight / rowHeight) - 1);
    }

    /// <summary>The least a list must scroll to show a row whole.</summary>
    /// <param name="top">The top of the row.</param>
    /// <param name="height">The height of the row.</param>
    /// <param name="viewTop">The offset of the viewport.</param>
    /// <param name="viewHeight">The height of the viewport.</param>
    /// <returns>The offset to scroll to, or null when the row is already in view.</returns>
    public static double? ScrollOffsetToReveal(double top, double height, double viewTop, double viewHeight)
    {
        if (top < viewTop)
        {
            return top;
        }

        if (top + height > viewTop + viewHeight)
        {
            // A row taller than the view is shown from its top.
            return height > viewHeight ? top : top + height - viewHeight;
        }

        return null;
    }

    /// <summary>The least a table must scroll sideways to show a scrolling cell clear of the frozen columns.</summary>
    /// <param name="cellX">The cell's offset from the row's inline start.</param>
    /// <param name="cellWidth">The cell's width.</param>
    /// <param name="viewX">The horizontal offset of the viewport.</param>
    /// <param name="viewWidth">The width of the viewport.</param>
    /// <param name="frozenStartWidth">The width kept at the start.</param>
    /// <param name="frozenEndWidth">The width kept at the end.</param>
    /// <returns>The offset to scroll to, or null when the cell is already in view.</returns>
    public static double? ScrollOffsetToRevealX(double cellX, double cellWidth, double viewX, double viewWidth, double frozenStartWidth, double frozenEndWidth)
    {
        double bandStart = viewX + frozenStartWidth;
        double bandEnd = viewX + viewWidth - frozenEndWidth;
        if (cellWidth > bandEnd - bandStart || cellX < bandStart)
        {
            return Math.Max(0, cellX - frozenStartWidth);
        }

        if (cellX + cellWidth > bandEnd)
        {
            return Math.Max(0, cellX + cellWidth - (viewWidth - frozenEndWidth));
        }

        return null;
    }

    /// <summary>Where focus goes when the active row is removed.</summary>
    /// <param name="removedRow">The index the row had.</param>
    /// <param name="newRowCount">How many rows remain.</param>
    /// <param name="column">The active column.</param>
    /// <param name="hasHeader">Whether the header is part of the grid.</param>
    /// <returns>The cell now at that place, the header when no row is left, or null when there is nothing.</returns>
    public static Cell? FocusAfterRemoval(int removedRow, int newRowCount, int column, bool hasHeader)
    {
        if (newRowCount > 0)
        {
            return new Cell(Math.Min(Math.Max(0, removedRow), newRowCount - 1), column);
        }

        return hasHeader ? new Cell(-1, column) : null;
    }

    /// <summary>What Enter does in a cell.</summary>
    /// <param name="controlsInCell">How many controls the cell holds.</param>
    /// <param name="isNameCellOfActionRow">Whether the cell is the name cell of a row that has an action.</param>
    /// <param name="hasAction">Whether the row has an action.</param>
    /// <param name="hasDetail">Whether the row has a detail.</param>
    /// <returns>What Enter does.</returns>
    public static ActivationTarget Activation(int controlsInCell, bool isNameCellOfActionRow, bool hasAction, bool hasDetail)
    {
        if (controlsInCell >= 2)
        {
            return ActivationTarget.EnterCell;
        }

        if (controlsInCell == 1 && !(isNameCellOfActionRow && hasAction))
        {
            return ActivationTarget.CellControl;
        }

        if (hasAction)
        {
            return ActivationTarget.RowAction;
        }

        return hasDetail ? ActivationTarget.ToggleDetail : ActivationTarget.None;
    }
}
