using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Design.Tables;

namespace Navree.Controls;

public sealed partial class DataTable
{
    /// <summary>The column index of the one cell of a row of a narrow table.</summary>
    internal const int ListColumn = -2;

    private Grid? listBand;
    private Button? sortButton;
    private TextBlock? sortText;
    private double textScale = 1.0;

    /// <summary>Whether the table is drawn as a list.</summary>
    internal bool IsList => Mode == DataTableMode.List;

    /// <summary>How many columns the grid's keys move among: one in a list.</summary>
    internal int GridColumnCount => IsList ? 1 : shown.Count;

    private void WireListParts()
    {
        listBand = GetTemplateChild("PART_ListBand") as Grid;
        sortButton = GetTemplateChild("PART_SortButton") as Button;
        sortText = GetTemplateChild("PART_SortText") as TextBlock;
        if (sortText is not null)
        {
            sortText.Text = DataTableStrings.Get("DataTable_Sort_Button");
        }

        if (sortButton is not null)
        {
            sortButton.Click -= OnSortButton;
            sortButton.Click += OnSortButton;
        }
    }

    // Decides whether the table is a table or a list, by the rule every client shares; true when that changed.
    private bool UpdateMode(IReadOnlyList<DataTableColumn> columns, List<int> order, double gutter)
    {
        try
        {
            textScale = new Windows.UI.ViewManagement.UISettings().TextScaleFactor;
        }
        catch (Exception)
        {
            textScale = 1.0;
        }

        double least = order.Sum(i => ColumnSizing.LeastWidth(columns[i].Kind));
        bool list = RowFolding.FoldsToList(ActualWidth, order.Count, least, gutter, textScale);
        DataTableMode wanted = list ? DataTableMode.List : DataTableMode.Table;
        if (wanted == Mode)
        {
            return false;
        }

        Mode = wanted;
        if (headerHost is not null)
        {
            headerHost.Visibility = list ? Visibility.Collapsed : Visibility.Visible;
        }

        if (listBand is not null)
        {
            listBand.Visibility = list ? Visibility.Visible : Visibility.Collapsed;
        }

        RaiseModeChanged();
        return true;
    }

    // A record read as one row of a list, with the words of every column.
    private (FoldedRow Folded, string[] Texts) FoldRecord(IDataTableModel model, IDataTablePresenter presenter, DataTableRecord record)
    {
        IReadOnlyList<DataTableColumn> columns = model.Columns;
        string[] texts = new string[columns.Count];
        List<FoldColumn> foldColumns = new(columns.Count);
        List<FoldCell> cells = new(columns.Count);
        for (int i = 0; i < columns.Count; i++)
        {
            DataTableColumn column = columns[i];
            texts[i] = presenter.CellText(record, i);
            int controls = presenter.ControlCount(record, i);
            foldColumns.Add(new FoldColumn(column.Role, column.Kind == ColumnKind.Number, column.Secondary));
            IReadOnlyList<string> said = string.IsNullOrWhiteSpace(texts[i]) ? [] : [texts[i]];
            if (column.Role == ColumnRole.Title && presenter.SecondLine(record) is { Length: > 0 } second)
            {
                // A name with a line under it says the name first and the line after it, as two lines of the row.
                said = [presenter.RowName(record), second];
            }

            cells.Add(new FoldCell(controls > 0 ? CellKind.Control : CellKind.Text, said, controls));
        }

        return (RowFolding.Fold(foldColumns, cells), texts);
    }

    /// <summary>Draws a row of a narrow table from its record.</summary>
    /// <param name="cell">The cell that holds the list content.</param>
    /// <param name="record">The record.</param>
    internal void BindList(DataTableCell cell, DataTableRecord record)
    {
        if (cell.Content is not DataTableListContent content || Presenter is not { } presenter || Model is not { } model)
        {
            return;
        }

        (FoldedRow folded, string[] texts) = FoldRecord(model, presenter, record);
        string states = string.Join(" · ", folded.StatusIndexes.Select(i => texts[i]).Where(t => t.Length > 0));
        string trailing = folded.TrailingIndex is { } at ? texts[at] : string.Empty;
        content.Set(folded.Headline, folded.Lines, states, trailing, record.HasMenu);
    }

    /// <summary>The words of a row of a list, for assistive technology.</summary>
    /// <param name="record">The record.</param>
    internal string ListWords(DataTableRecord record)
    {
        if (Model is not { } model || Presenter is not { } presenter)
        {
            return record.Key ?? string.Empty;
        }

        (FoldedRow folded, string[] texts) = FoldRecord(model, presenter, record);
        List<string> words = [folded.Headline];
        words.AddRange(folded.Lines);
        words.AddRange(folded.StatusIndexes.Select(i => texts[i]));
        if (folded.TrailingIndex is { } at)
        {
            words.Add(texts[at]);
        }

        return string.Join(", ", words.Where(w => w.Length > 0));
    }

    // The height every row of a list has: the tallest of the first rows decides.
    private double ListHeight(double line, double second, double scale)
    {
        List<FoldedRow> folded = [];
        if (Model is { } model && Presenter is { } presenter)
        {
            int sample = Math.Min(60, model.RowCount);
            for (int i = 0; i < sample; i++)
            {
                folded.Add(FoldRecord(model, presenter, model.GetRow(i)).Folded);
            }
        }

        int shapeLines = RowFolding.ListShape(folded, RowFolding.TrailingBelow(textScale));
        return RowFolding.ListRowHeight(shapeLines, line, second, scale);
    }

    private void OnSortButton(object sender, RoutedEventArgs e)
    {
        if (Model is not { } model)
        {
            return;
        }

        Microsoft.UI.Xaml.Controls.MenuFlyout menu = new();
        foreach (DataTableColumn column in model.Columns)
        {
            if (column.Sort is not { } sort)
            {
                continue;
            }

            DataTableColumn captured = column;
            string mark = sort.Direction switch
            {
                DataTableSortDirection.Ascending => "  ▲",
                DataTableSortDirection.Descending => "  ▼",
                _ => string.Empty,
            };
            MenuFlyoutItem item = new() { Text = column.Header + mark };
            item.Click += (_, _) => RaiseSortRequested(captured);
            menu.Items.Add(item);
        }

        if (menu.Items.Count > 0 && sortButton is not null)
        {
            menu.ShowAt(sortButton);
        }
    }
}
