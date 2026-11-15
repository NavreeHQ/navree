namespace Navree.Design.Tables;

/// <summary>What a column holds, which sets how wide it asks to be.</summary>
public enum ColumnKind
{
    /// <summary>The record's name: the column that takes the room the others leave.</summary>
    Title,

    /// <summary>Any other words.</summary>
    Words,

    /// <summary>A number or an amount.</summary>
    Number,

    /// <summary>A state, in words beside a dot or as a badge.</summary>
    Status,

    /// <summary>The record's controls: as wide as they are, no wider.</summary>
    Controls,

    /// <summary>A picture or an avatar before the name.</summary>
    Leading,
}

/// <summary>The type a measured text is set in.</summary>
public enum TextStyle
{
    /// <summary>Body: the text of a cell.</summary>
    Body,

    /// <summary>Body strong: the text of a heading.</summary>
    BodyStrong,

    /// <summary>Caption: a second line, the summary.</summary>
    Caption,
}

/// <summary>Measures how wide a text is when drawn.</summary>
public interface ITextMeasurer
{
    /// <summary>The width of one line of <paramref name="text"/> in <paramref name="style"/>, in effective pixels.</summary>
    /// <param name="text">The words.</param>
    /// <param name="style">The type they are set in.</param>
    /// <returns>The width.</returns>
    double Width(string text, TextStyle style);
}

/// <summary>How wide columns are, and how far a person may drag them.</summary>
public static class ColumnSizing
{
    private const double CellPadding = 24;
    private const double SortSlot = 20;

    /// <summary>The width a column asks for from what it holds.</summary>
    /// <param name="kind">What the column holds.</param>
    /// <param name="widestText">The width of its widest words, the heading's or a cell's.</param>
    /// <param name="sortable">Whether the heading has room for a sort mark.</param>
    /// <param name="controlWidth">For a controls column, the width of the widest control.</param>
    /// <returns>The width in effective pixels.</returns>
    public static double AutoWidth(ColumnKind kind, double widestText, bool sortable, double controlWidth = 0)
    {
        if (!(widestText >= 0) || !(controlWidth >= 0))
        {
            throw new ArgumentOutOfRangeException(nameof(widestText), widestText, "A width is zero or more.");
        }

        double room = Math.Ceiling(widestText) + CellPadding + (sortable ? SortSlot : 0);
        return kind switch
        {
            ColumnKind.Title => Math.Max(room, 240),
            ColumnKind.Words => Math.Clamp(room, 80, 260),
            ColumnKind.Number => Math.Clamp(room, 64, 160),
            ColumnKind.Status => Math.Clamp(room, 90, 200),
            ColumnKind.Controls => Math.Ceiling(controlWidth) + CellPadding,
            ColumnKind.Leading => 56,
            _ => throw new ArgumentOutOfRangeException(nameof(kind), kind, "Not a kind of column."),
        };
    }

    /// <summary>The narrowest and the widest a person may make a column.</summary>
    /// <param name="kind">What the column holds.</param>
    /// <returns>The range, or zero and zero for a column that is not resized.</returns>
    public static (double Min, double Max) ResizeRange(ColumnKind kind) => kind switch
    {
        ColumnKind.Title => (120, 800),
        ColumnKind.Words => (64, 640),
        ColumnKind.Number => (56, 400),
        ColumnKind.Status => (72, 400),
        ColumnKind.Controls or ColumnKind.Leading => (0, 0),
        _ => throw new ArgumentOutOfRangeException(nameof(kind), kind, "Not a kind of column."),
    };

    /// <summary>A width kept inside the range of its kind; a column that is not resized keeps its width.</summary>
    /// <param name="kind">What the column holds.</param>
    /// <param name="width">The width asked for.</param>
    /// <returns>The width allowed.</returns>
    public static double Clamp(ColumnKind kind, double width)
    {
        (double min, double max) = ResizeRange(kind);
        return max == 0 ? width : Math.Clamp(width, min, max);
    }

    /// <summary>A width changed by one step of the keyboard.</summary>
    /// <param name="kind">What the column holds.</param>
    /// <param name="current">The width now.</param>
    /// <param name="direction">One for wider, minus one for narrower.</param>
    /// <param name="big">Whether the step is the big one (32) and not the small one (8).</param>
    /// <returns>The new width.</returns>
    public static double ResizeByKey(ColumnKind kind, double current, int direction, bool big)
    {
        (_, double max) = ResizeRange(kind);
        if (max == 0)
        {
            return current;
        }

        return Clamp(kind, current + (Math.Sign(direction) * (big ? 32 : 8)));
    }

    /// <summary>The least a column is given in a table that is too narrow for the rest.</summary>
    /// <param name="kind">What the column holds.</param>
    /// <param name="controlWidth">For a controls column, the width of the widest control.</param>
    /// <returns>The width.</returns>
    public static double LeastWidth(ColumnKind kind, double controlWidth = 0) => kind switch
    {
        ColumnKind.Title => 120,
        ColumnKind.Words => 64,
        ColumnKind.Number => 56,
        ColumnKind.Status => 72,
        ColumnKind.Controls => Math.Ceiling(controlWidth) + CellPadding,
        ColumnKind.Leading => 56,
        _ => throw new ArgumentOutOfRangeException(nameof(kind), kind, "Not a kind of column."),
    };
}
