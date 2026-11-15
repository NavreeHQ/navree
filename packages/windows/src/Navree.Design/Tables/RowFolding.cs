namespace Navree.Design.Tables;

/// <summary>What a column is to its row, so a narrow table can draw each record as a list row.</summary>
public enum ColumnRole
{
    /// <summary>No role.</summary>
    None,

    /// <summary>What stands before the name: a picture, an avatar.</summary>
    Leading,

    /// <summary>The record's name.</summary>
    Title,

    /// <summary>A line under the name.</summary>
    Subtitle,

    /// <summary>The record's state, as a badge.</summary>
    Status,

    /// <summary>What stands at the row's end: an amount, a time.</summary>
    Trailing,

    /// <summary>Controls that act on the record.</summary>
    Actions,
}

/// <summary>What a cell holds, as far as folding a row cares.</summary>
public enum CellKind
{
    /// <summary>Words.</summary>
    Text,

    /// <summary>A badge.</summary>
    Badge,

    /// <summary>A control.</summary>
    Control,

    /// <summary>Anything else.</summary>
    Other,
}

/// <summary>A column as folding reads it.</summary>
/// <param name="Role">What the column is to its row.</param>
/// <param name="Numeric">Whether it holds numbers.</param>
/// <param name="Secondary">Whether a narrow screen may leave it out.</param>
public readonly record struct FoldColumn(ColumnRole Role, bool Numeric, bool Secondary);

/// <summary>A cell as folding reads it.</summary>
/// <param name="Kind">What it holds.</param>
/// <param name="Texts">The words in it, one entry for each text.</param>
/// <param name="ControlCount">How many controls it holds.</param>
/// <param name="ControlLabel">When it holds exactly one control, the control's label.</param>
public sealed record FoldCell(CellKind Kind, IReadOnlyList<string> Texts, int ControlCount, string? ControlLabel = null);

/// <summary>A record read as one row of a list.</summary>
/// <param name="LeadingIndex">The cell that leads the row, if any.</param>
/// <param name="Headline">The first line: what names the record.</param>
/// <param name="Lines">The lines under it, each said once.</param>
/// <param name="StatusIndexes">The cells that are states.</param>
/// <param name="TrailingIndex">The cell at the row's end, if any.</param>
/// <param name="MenuIndexes">The cells whose controls are the row's menu.</param>
/// <param name="ActionFromControl">Whether the headline is a control whose action is the row's, for a row with none of its own.</param>
public sealed record FoldedRow(
    int? LeadingIndex,
    string Headline,
    IReadOnlyList<string> Lines,
    IReadOnlyList<int> StatusIndexes,
    int? TrailingIndex,
    IReadOnlyList<int> MenuIndexes,
    bool ActionFromControl);

/// <summary>When a table is a list, and how a record folds into a list row.</summary>
public static class RowFolding
{
    /// <summary>The least width, in effective pixels, at which a table is drawn as a table.</summary>
    public const double TableFromWidth = 641;

    /// <summary>Whether a table is drawn as a list.</summary>
    /// <param name="width">The width of the table.</param>
    /// <param name="shownCount">How many columns it shows.</param>
    /// <param name="leastWidthSum">The sum of the least widths of those columns.</param>
    /// <param name="gutter">The width of the check and chevron cells.</param>
    /// <param name="textScale">The person's text size, 1 for 100 percent.</param>
    /// <returns>True when the table is a list.</returns>
    public static bool FoldsToList(double width, int shownCount, double leastWidthSum, double gutter, double textScale) =>
        width < TableFromWidth
        || shownCount < 3
        || (textScale >= 2.0 && gutter + leastWidthSum > width);

    /// <summary>Whether a row's trailing value moves under its text.</summary>
    /// <param name="textScale">The person's text size.</param>
    /// <returns>True at 200 percent and over.</returns>
    public static bool TrailingBelow(double textScale) => textScale >= 2.0;

    /// <summary>Folds a record's cells into a list row by the role of each column.</summary>
    /// <param name="columns">The columns.</param>
    /// <param name="cells">The record's cells, one for each column.</param>
    /// <returns>The folded row.</returns>
    public static FoldedRow Fold(IReadOnlyList<FoldColumn> columns, IReadOnlyList<FoldCell> cells)
    {
        ArgumentNullException.ThrowIfNull(columns);
        ArgumentNullException.ThrowIfNull(cells);
        if (cells.Count == 0)
        {
            return new FoldedRow(null, string.Empty, [], [], null, [], false);
        }

        ColumnRole RoleOf(int i) => i < columns.Count ? columns[i].Role : ColumnRole.None;
        bool NumericAt(int i) => i < columns.Count && columns[i].Numeric;
        bool SecondaryAt(int i) => i < columns.Count && columns[i].Secondary;

        int titleAt = 0;
        for (int i = 0; i < cells.Count; i++)
        {
            if (RoleOf(i) == ColumnRole.Title)
            {
                titleAt = i;
                break;
            }
        }

        List<int> kept = [];
        for (int i = 0; i < cells.Count; i++)
        {
            if (i != titleAt && !SecondaryAt(i))
            {
                kept.Add(i);
            }
        }

        int? leadingAt = kept.Where(i => RoleOf(i) == ColumnRole.Leading).Select(i => (int?)i).FirstOrDefault();
        List<int> actionsAt = kept.Where(i => RoleOf(i) == ColumnRole.Actions).ToList();
        int? trailingAt = kept.Where(i => RoleOf(i) == ColumnRole.Trailing).Select(i => (int?)i).FirstOrDefault()
            ?? kept.Where(i => RoleOf(i) == ColumnRole.None && NumericAt(i)).Select(i => (int?)i).FirstOrDefault();
        if (trailingAt is null && kept.Count > 0 && cells.Count >= 3)
        {
            bool anyRoleOrNumeric = false;
            for (int i = 0; i < columns.Count; i++)
            {
                if (columns[i].Role != ColumnRole.None || columns[i].Numeric)
                {
                    anyRoleOrNumeric = true;
                    break;
                }
            }

            if (!anyRoleOrNumeric)
            {
                trailingAt = kept[^1];
            }
        }

        HashSet<int> taken = [.. actionsAt];
        if (leadingAt is { } lead)
        {
            taken.Add(lead);
        }

        if (trailingAt is { } trail)
        {
            taken.Add(trail);
        }

        List<int> statuses = kept
            .Where(i => !taken.Contains(i) && (RoleOf(i) == ColumnRole.Status || (RoleOf(i) == ColumnRole.None && cells[i].Kind == CellKind.Badge)))
            .ToList();
        List<int> subtitles = kept
            .Where(i => !taken.Contains(i) && !statuses.Contains(i) && RoleOf(i) == ColumnRole.Subtitle)
            .ToList();
        List<int> others = kept
            .Where(i => !taken.Contains(i) && !statuses.Contains(i) && !subtitles.Contains(i))
            .ToList();

        FoldCell title = cells[titleAt];
        List<string> texts = title.Texts.Where(text => !string.IsNullOrWhiteSpace(text)).ToList();
        bool control = title.ControlCount == 1 && title.ControlLabel is not null;
        string headline = control ? title.ControlLabel! : (texts.Count > 0 ? texts[0] : string.Empty);
        IEnumerable<string> rest = control ? texts : texts.Skip(1);

        List<string> lines = [];
        void AddLine(string line)
        {
            string trimmed = line.Trim();
            if (trimmed.Length > 0 && !lines.Contains(trimmed))
            {
                lines.Add(trimmed);
            }
        }

        foreach (string line in rest)
        {
            AddLine(line);
        }

        foreach (int i in subtitles)
        {
            foreach (string line in cells[i].Texts)
            {
                AddLine(line);
            }
        }

        foreach (int i in others)
        {
            string? first = cells[i].Texts.FirstOrDefault(text => !string.IsNullOrWhiteSpace(text));
            if (first is not null)
            {
                AddLine(first);
            }
        }

        // A line that says what the headline says is not said twice.
        lines.RemoveAll(line => line == headline.Trim());
        return new FoldedRow(leadingAt, headline, lines, statuses, trailingAt, actionsAt, control);
    }

    /// <summary>How many lines of text the tallest of the first rows holds, so that every row of the list has one height.</summary>
    /// <param name="firstRows">The first rows, folded.</param>
    /// <param name="trailingBelow">Whether a trailing value stands under the text.</param>
    /// <returns>From one to four.</returns>
    public static int ListShape(IEnumerable<FoldedRow> firstRows, bool trailingBelow)
    {
        ArgumentNullException.ThrowIfNull(firstRows);
        int shape = 1;
        foreach (FoldedRow row in firstRows)
        {
            int lines = 1
                + Math.Min(row.Lines.Count, 2)
                + (row.StatusIndexes.Count > 0 ? 1 : 0)
                + (trailingBelow && row.TrailingIndex is not null ? 1 : 0);
            shape = Math.Max(shape, Math.Min(4, lines));
        }

        return shape;
    }

    /// <summary>The height of every row of a list.</summary>
    /// <param name="shape">The lines in the tallest row, one to four.</param>
    /// <param name="bodyLine">The height of a line of body text.</param>
    /// <param name="captionLine">The height of a line of caption text.</param>
    /// <param name="rasterizationScale">Physical pixels for each effective pixel.</param>
    /// <returns>The height in effective pixels.</returns>
    public static double ListRowHeight(int shape, double bodyLine, double captionLine, double rasterizationScale)
    {
        if (shape is < 1 or > 4)
        {
            throw new ArgumentOutOfRangeException(nameof(shape), shape, "A list row holds one to four lines.");
        }

        double[] minimum = [48, 64, 80, 96];
        double content = bodyLine + (captionLine * (shape - 1));
        double height = Math.Max(minimum[shape - 1], content + 24 + 1);
        height = Math.Ceiling(height / 4) * 4;
        return Pixels.RoundUp(height, rasterizationScale);
    }
}
