using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Design.Tables;
using Windows.Foundation;

namespace Navree.Controls;

/// <summary>Measures text the way the table draws it: a text block set in the same type, read after a measure.</summary>
internal sealed class TextMeasurer : ITextMeasurer
{
    private readonly TextBlock probe = new() { TextWrapping = TextWrapping.NoWrap, TextTrimming = TextTrimming.None };
    private readonly Dictionary<(string Text, TextStyle Style), double> cache = [];

    /// <summary>Forgets what was measured: the text size or the culture changed.</summary>
    internal void Reset() => cache.Clear();

    /// <summary>The height of one line in a style.</summary>
    /// <param name="style">The type.</param>
    /// <returns>The height.</returns>
    internal double LineHeight(TextStyle style)
    {
        Setup(style);
        probe.Text = "Ag";
        probe.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        return probe.DesiredSize.Height;
    }

    /// <inheritdoc />
    public double Width(string text, TextStyle style)
    {
        if (string.IsNullOrEmpty(text))
        {
            return 0;
        }

        if (cache.TryGetValue((text, style), out double known))
        {
            return known;
        }

        Setup(style);
        probe.Text = text;
        probe.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        double width = probe.DesiredSize.Width;
        cache[(text, style)] = width;
        return width;
    }

    private void Setup(TextStyle style)
    {
        probe.FontSize = style == TextStyle.Caption ? 12 : 14;
        probe.FontWeight = style == TextStyle.BodyStrong ? Microsoft.UI.Text.FontWeights.SemiBold : Microsoft.UI.Text.FontWeights.Normal;
    }
}

/// <summary>The words the table says itself, in English, which a host can replace.</summary>
public static class DataTableStrings
{
    private static readonly Dictionary<string, string> Defaults = new(StringComparer.Ordinal)
    {
        ["DataTable_Name_Default"] = "Table",
        ["DataTable_Header_Row"] = "Column headers",
        ["DataTable_Sortable_Help"] = "Sortable. Press Enter to sort.",
        ["DataTable_Status_Ascending"] = "Ascending",
        ["DataTable_Status_Descending"] = "Descending",
        ["DataTable_Sorted_Ascending"] = "Sorted by {0}, ascending",
        ["DataTable_Sorted_Descending"] = "Sorted by {0}, descending",
        ["DataTable_Chosen_None"] = "None chosen",
        ["DataTable_Chosen_Count"] = "{0} chosen",
        ["DataTable_Chosen_All"] = "All {0} chosen",
        ["DataTable_Chosen_AllUnknown"] = "All chosen",
        ["DataTable_ChooseAll"] = "Choose all {0}",
        ["DataTable_ChooseAllMatching"] = "Choose all that match",
        ["DataTable_ClearChoice"] = "Clear choice",
        ["DataTable_ChooseAllShown"] = "Choose all rows shown",
        ["DataTable_RowsAdded_Later"] = "{0} more rows loaded",
        ["DataTable_RowsAdded_Earlier"] = "{0} earlier rows loaded",
        ["DataTable_RowRemoved"] = "Row removed",
        ["DataTable_Details_Show"] = "Show details",
        ["DataTable_Details_Hide"] = "Hide details",
        ["DataTable_Details_Shown"] = "Details shown",
        ["DataTable_Details_Hidden"] = "Details hidden",
        ["DataTable_MoreActions"] = "More actions for {0}",
        ["DataTable_Empty_Title"] = "Nothing here",
        ["DataTable_Loading"] = "Loading",
        ["DataTable_Retry"] = "Try again",
        ["DataTable_LoadingMore"] = "Loading more rows",
        ["DataTable_LoadMoreFailed"] = "Couldn't load more rows",
        ["DataTable_LoadEarlierFailed"] = "Couldn't load earlier rows",
        ["DataTable_HeldLimit"] = "Showing the first {0} rows. Narrow the search to see the rest.",
        ["DataTable_Menu_Sort"] = "Sort",
        ["DataTable_Menu_Fit"] = "Fit to content",
        ["DataTable_Menu_ResetWidth"] = "Reset width",
        ["DataTable_Menu_Hide"] = "Hide column",
        ["DataTable_Menu_MoveEarlier"] = "Move earlier",
        ["DataTable_Menu_MoveLater"] = "Move later",
        ["DataTable_Menu_Freeze"] = "Freeze up to here",
        ["DataTable_Menu_Unfreeze"] = "Unfreeze",
        ["DataTable_Menu_Columns"] = "Columns",
        ["DataTable_Menu_Density"] = "Density",
        ["DataTable_Menu_Reset"] = "Reset columns",
        ["DataTable_Density_Compact"] = "Compact",
        ["DataTable_Density_Standard"] = "Standard",
        ["DataTable_Density_Comfortable"] = "Comfortable",
        ["DataTable_NoRoomToFreeze"] = "There is not enough room",
        ["DataTable_ColumnWidth"] = "{0}, width {1}",
        ["DataTable_ColumnMoved"] = "{0} moved to position {1} of {2}",
        ["DataTable_ColumnShown"] = "{0} shown",
        ["DataTable_ColumnHidden"] = "{0} hidden",
        ["DataTable_Sort_Button"] = "Sort",
        ["DataTable_ControlType_Table"] = "table",
        ["DataTable_ControlType_Row"] = "row",
        ["DataTable_ControlType_Cell"] = "cell",
        ["DataTable_ControlType_ColumnHeader"] = "column header",
    };

    /// <summary>Gives a host the means to put its own words in: it is asked first, and answers null for any it does not have.</summary>
    public static Func<string, string?>? Provider { get; set; }

    /// <summary>The words for an identity, with its numbers and names put in.</summary>
    /// <param name="id">The identity, such as <c>DataTable_Loading</c>.</param>
    /// <param name="arguments">What is put into the words.</param>
    /// <returns>The words.</returns>
    public static string Get(string id, params object[] arguments)
    {
        string? text = Provider?.Invoke(id);
        if (text is null && !Defaults.TryGetValue(id, out text))
        {
            return id;
        }

        return arguments.Length == 0 ? text : string.Format(CultureInfo.CurrentCulture, text, arguments);
    }

    /// <summary>The words for an announcement the plain rules gave, with its numbers grouped as the table's culture groups them.</summary>
    /// <param name="announcement">The announcement.</param>
    /// <param name="culture">The culture of the table.</param>
    /// <returns>The words.</returns>
    internal static string Get(Announcement announcement, CultureInfo culture) =>
        Get(
            announcement.Id,
            [.. announcement.Arguments.Select(argument => argument switch
            {
                int whole => whole.ToString("N0", culture),
                long whole => whole.ToString("N0", culture),
                _ => argument,
            })]);
}
