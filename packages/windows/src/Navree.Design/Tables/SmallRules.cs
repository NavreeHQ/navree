using System.Globalization;
using System.Text;

namespace Navree.Design.Tables;

/// <summary>Finding a row by typing the start of its name.</summary>
public static class TypeAhead
{
    /// <summary>The next row, after the active one and wrapping once, whose name starts with what was typed.</summary>
    /// <param name="names">The names of the held rows.</param>
    /// <param name="activeIndex">The active row.</param>
    /// <param name="typed">What was typed.</param>
    /// <param name="culture">The culture that compares the words.</param>
    /// <returns>The index, or minus one for none.</returns>
    public static int Next(IReadOnlyList<string> names, int activeIndex, string typed, CultureInfo culture)
    {
        ArgumentNullException.ThrowIfNull(names);
        ArgumentNullException.ThrowIfNull(typed);
        ArgumentNullException.ThrowIfNull(culture);
        if (typed.Length == 0 || names.Count == 0)
        {
            return -1;
        }

        for (int step = 1; step <= names.Count; step++)
        {
            int at = ((activeIndex + step) % names.Count + names.Count) % names.Count;
            if (culture.CompareInfo.IsPrefix(names[at], typed, CompareOptions.IgnoreCase))
            {
                return at;
            }
        }

        return -1;
    }
}

/// <summary>Scrolling by itself while a column is dragged near an edge.</summary>
public static class AutoScroll
{
    /// <summary>How fast to scroll for a pointer's place.</summary>
    /// <param name="pointerX">The pointer's position in the viewport.</param>
    /// <param name="viewport">The width of the viewport.</param>
    /// <param name="edge">How near an edge starts it.</param>
    /// <param name="max">The greatest speed, in effective pixels a second.</param>
    /// <returns>The velocity: negative toward the start, positive toward the end, zero in the middle.</returns>
    public static double Velocity(double pointerX, double viewport, double edge = 48, double max = 1200)
    {
        if (viewport < 2 * edge)
        {
            return 0;
        }

        double velocity = 0;
        if (pointerX < edge)
        {
            velocity = -max * (edge - pointerX) / edge;
        }
        else if (pointerX > viewport - edge)
        {
            velocity = max * (pointerX - (viewport - edge)) / edge;
        }

        return Math.Clamp(velocity, -max, max);
    }
}

/// <summary>The text that Copy puts on the clipboard.</summary>
public static class ClipboardText
{
    /// <summary>The text of one cell.</summary>
    /// <param name="cellText">The cell's words.</param>
    /// <returns>The words with tabs and line breaks made spaces.</returns>
    public static string ForCell(string cellText)
    {
        ArgumentNullException.ThrowIfNull(cellText);
        return cellText.Replace('\t', ' ').Replace('\r', ' ').Replace('\n', ' ');
    }

    /// <summary>The text of rows: a line for the headings, then a line for each row, cells apart by tabs.</summary>
    /// <param name="headers">The headings.</param>
    /// <param name="rows">The cells of each row.</param>
    /// <returns>The text.</returns>
    public static string ForRows(IReadOnlyList<string> headers, IReadOnlyList<IReadOnlyList<string>> rows)
    {
        ArgumentNullException.ThrowIfNull(headers);
        ArgumentNullException.ThrowIfNull(rows);
        StringBuilder text = new();
        text.Append(string.Join('\t', headers.Select(ForCell)));
        foreach (IReadOnlyList<string> row in rows)
        {
            text.Append("\r\n");
            text.Append(string.Join('\t', row.Select(ForCell)));
        }

        return text.ToString();
    }
}

/// <summary>A message to say to a screen reader: the resource that holds its words, and what goes into them.</summary>
/// <param name="Id">The resource id.</param>
/// <param name="Arguments">The values the words hold places for.</param>
public sealed record Announcement(string Id, IReadOnlyList<object> Arguments);

/// <summary>The messages a table says, as resource ids and values.</summary>
public static class Announcements
{
    /// <summary>A sort the person asked for was answered.</summary>
    /// <param name="label">The heading.</param>
    /// <param name="ascending">Whether the order is ascending.</param>
    /// <returns>The message.</returns>
    public static Announcement Sorted(string label, bool ascending) =>
        new(ascending ? "DataTable_Sorted_Ascending" : "DataTable_Sorted_Descending", [label]);

    /// <summary>How many rows are chosen.</summary>
    /// <param name="count">The rows chosen, when not across.</param>
    /// <param name="acrossTotal">The count of every matching record, when the server gave one.</param>
    /// <param name="across">Whether every matching record is chosen.</param>
    /// <returns>The message.</returns>
    public static Announcement Chosen(long count, long? acrossTotal, bool across)
    {
        if (across)
        {
            return acrossTotal is { } total
                ? new Announcement("DataTable_Chosen_All", [total])
                : new Announcement("DataTable_Chosen_AllUnknown", []);
        }

        return count <= 0
            ? new Announcement("DataTable_Chosen_None", [])
            : new Announcement("DataTable_Chosen_Count", [count]);
    }

    /// <summary>Rows joined the table.</summary>
    /// <param name="count">How many.</param>
    /// <param name="earlier">Whether they joined the start.</param>
    /// <returns>The message.</returns>
    public static Announcement RowsAdded(int count, bool earlier) =>
        new(earlier ? "DataTable_RowsAdded_Earlier" : "DataTable_RowsAdded_Later", [count]);

    /// <summary>A column was made wider or narrower by key.</summary>
    /// <param name="label">The heading.</param>
    /// <param name="width">The width now.</param>
    /// <returns>The message.</returns>
    public static Announcement ColumnWidth(string label, int width) => new("DataTable_ColumnWidth", [label, width]);

    /// <summary>A column moved.</summary>
    /// <param name="label">The heading.</param>
    /// <param name="position">Its place, counting from one.</param>
    /// <param name="count">How many columns.</param>
    /// <returns>The message.</returns>
    public static Announcement ColumnMoved(string label, int position, int count) => new("DataTable_ColumnMoved", [label, position, count]);

    /// <summary>A column was hidden or shown.</summary>
    /// <param name="label">The heading.</param>
    /// <param name="shown">Whether it is shown now.</param>
    /// <returns>The message.</returns>
    public static Announcement ColumnVisibility(string label, bool shown) =>
        new(shown ? "DataTable_ColumnShown" : "DataTable_ColumnHidden", [label]);

    /// <summary>A row's detail opened or shut.</summary>
    /// <param name="open">Whether it is open now.</param>
    /// <returns>The message.</returns>
    public static Announcement Detail(bool open) => new(open ? "DataTable_Details_Shown" : "DataTable_Details_Hidden", []);
}
