using Microsoft.UI.Xaml;
using Navree.Design.Tables;

namespace Navree.Controls;

/// <summary>A column of the model that is shown, with where it stands.</summary>
/// <param name="ModelIndex">The column's index among the model's columns.</param>
/// <param name="Column">The column.</param>
/// <param name="Placed">Its width, its group and its offset.</param>
internal sealed record ShownColumn(int ModelIndex, DataTableColumn Column, PlacedColumn Placed);

public sealed partial class DataTable
{
    private readonly Dictionary<string, double> widths = new(StringComparer.Ordinal);
    private readonly Dictionary<int, PlacedColumn> placedByIndex = [];
    private readonly HashSet<string> hiddenKeys = new(StringComparer.Ordinal);
    private readonly HashSet<string> forcedShownKeys = new(StringComparer.Ordinal);
    private ColumnLayout? layout;
    private IReadOnlyList<ShownColumn> shown = [];
    private string shownSignature = string.Empty;
    private int? frozenWish;
    private bool hasCheckColumn;
    private bool hasChevronColumn;

    /// <summary>Where every column stands now; null before there are columns.</summary>
    internal ColumnLayout? CurrentLayout => layout;

    /// <summary>The columns shown, in display order, the end columns last.</summary>
    internal IReadOnlyList<ShownColumn> Shown => shown;

    /// <summary>A number that changes when the set of cells a row holds changes.</summary>
    internal int LayoutVersion { get; private set; }

    /// <summary>Whether rows have a box that chooses them.</summary>
    internal bool HasCheckColumn => hasCheckColumn;

    /// <summary>Whether rows have a chevron that opens a detail.</summary>
    internal bool HasChevronColumn => hasChevronColumn;

    /// <summary>Where the chevron cell stands: after the check cell when there is one.</summary>
    internal double ChevronLeft => hasCheckColumn ? FrozenGutter.CheckWidth : 0;

    /// <summary>A column of the model.</summary>
    /// <param name="modelIndex">Its index.</param>
    /// <returns>The column.</returns>
    internal DataTableColumn ColumnAt(int modelIndex) => Model!.Columns[modelIndex];

    /// <summary>Where a column of the model stands, or null when it is not shown.</summary>
    /// <param name="modelIndex">Its index.</param>
    internal PlacedColumn? PlacedFor(int modelIndex) => placedByIndex.TryGetValue(modelIndex, out PlacedColumn placed) ? placed : null;

    /// <summary>The state of the box in the header.</summary>
    internal HeaderBoxState HeaderBox()
    {
        (int held, int chosen) = CountChoice();
        return Choosing.HeaderBox(held, chosen);
    }

    // Works out which columns are shown, how wide each is, and where each stands, and draws the header again.
    private void RebuildColumns(bool remeasure)
    {
        IDataTableModel? model = Model;
        IDataTablePresenter? presenter = Presenter;
        double viewport = Viewport;
        if (model is null || presenter is null || viewport <= 0)
        {
            if (shown.Count > 0 || layout is not null)
            {
                shown = [];
                layout = null;
                placedByIndex.Clear();
                LayoutVersion++;
                header?.Refresh();
            }

            return;
        }

        IReadOnlyList<DataTableColumn> columns = model.Columns;
        LoadStoredLayout(columns);
        Dictionary<string, int> byKey = [];
        for (int i = 0; i < columns.Count; i++)
        {
            byKey[columns[i].Key] = i;
        }

        List<int> personsOrder = ColumnOrder.Reconcile(columns.Select(c => c.Key).ToList(), columnOrder).Select(k => byKey[k]).ToList();
        List<ColInfo> infos = personsOrder.Select(i => new ColInfo(columns[i].Key, columns[i].Kind, columns[i].Role, columns[i].Secondary)).ToList();
        IReadOnlyList<int> chosen = ColumnSet.Shown(infos, hiddenKeys, forcedShownKeys, viewport);
        List<int> start = chosen.Select(i => personsOrder[i]).ToList();
        List<int> end = [];
        while (start.Count > 0 && end.Count < 2 && columns[start[^1]].Role == ColumnRole.Actions)
        {
            end.Insert(0, start[^1]);
            start.RemoveAt(start.Count - 1);
        }

        MeasureWidths(model, presenter, columns, [.. start, .. end], remeasure);
        ColumnSpec Spec(int index)
        {
            DataTableColumn c = columns[index];
            double width = widths.TryGetValue(c.Key, out double known) ? known : ColumnSizing.LeastWidth(c.Kind);
            return new ColumnSpec(c.Key, c.Kind, width, c.Kind == ColumnKind.Title && !personWidths.Contains(c.Key));
        }

        hasCheckColumn = model.Choice.Available;
        hasChevronColumn = AnyDetail(model);
        double gutter = (hasCheckColumn ? FrozenGutter.CheckWidth : 0) + (hasChevronColumn ? FrozenGutter.ChevronWidth : 0);
        List<ColInfo> startInfos = start.Select(i => new ColInfo(columns[i].Key, columns[i].Kind, columns[i].Role, columns[i].Secondary)).ToList();
        int wish = frozenWish ?? ColumnSet.DefaultFrozen(startInfos);
        ColumnLayout built = ColumnLayoutBuilder.Build(start.Select(Spec).ToList(), wish, end.Select(Spec).ToList(), gutter, viewport);

        List<int> order = [.. start, .. end];
        List<ShownColumn> list = new(order.Count);
        placedByIndex.Clear();
        for (int i = 0; i < order.Count; i++)
        {
            ShownColumn column = new(order[i], columns[order[i]], built.Columns[i]);
            list.Add(column);
            placedByIndex[order[i]] = built.Columns[i];
        }

        string signature = string.Join('|', order.Select(i => columns[i].Key)) + (hasCheckColumn ? "+c" : string.Empty) + (hasChevronColumn ? "+d" : string.Empty);
        bool modeChanged = UpdateMode(columns, order, gutter);
        bool versionChanged = signature != shownSignature || modeChanged;
        if (versionChanged)
        {
            shownSignature = signature;
            LayoutVersion++;
        }

        if (modeChanged)
        {
            RecomputeMetrics();
        }

        shown = list;
        layout = built;
        double fs = IsList ? 0 : built.Fs;
        double fe = IsList ? 0 : built.Fe;
        rowsLayout.RowWidth = IsList ? viewport : built.RowWidth;
        sticky?.SetFrozenWidths(fs, fe);
        if (edgeStart is not null)
        {
            edgeStart.Margin = new Thickness(fs, 0, 0, 0);
        }

        if (edgeEnd is not null)
        {
            edgeEnd.Margin = new Thickness(0, 0, fe, 0);
        }

        RefreshAllRows(numbersOnly: !versionChanged);
    }

    private bool AnyDetail(IDataTableModel model)
    {
        int sample = Math.Min(model.RowCount, 200);
        for (int i = 0; i < sample; i++)
        {
            if (model.GetRow(i).HasDetail)
            {
                return true;
            }
        }

        return false;
    }

    // Measured once for a set of columns: the heading and the first rows decide each width.
    private void MeasureWidths(IDataTableModel model, IDataTablePresenter presenter, IReadOnlyList<DataTableColumn> columns, List<int> order, bool remeasure)
    {
        int sample = Math.Min(60, model.RowCount);
        foreach (int index in order)
        {
            DataTableColumn column = columns[index];
            if ((!remeasure && widths.ContainsKey(column.Key)) || personWidths.Contains(column.Key))
            {
                continue;
            }

            if (column.Kind == ColumnKind.Controls)
            {
                double control = 0;
                for (int r = 0; r < sample; r++)
                {
                    DataTableRecord record = model.GetRow(r);
                    int count = presenter.ControlCount(record, index);
                    control = Math.Max(control, (count * 32) + (Math.Max(0, count - 1) * 8));
                }

                widths[column.Key] = ColumnSizing.AutoWidth(column.Kind, 0, false, Math.Max(control, 32));
                continue;
            }

            double widest = measurer.Width(column.Header, TextStyle.BodyStrong);
            for (int r = 0; r < sample; r++)
            {
                widest = Math.Max(widest, measurer.Width(presenter.CellText(model.GetRow(r), index), TextStyle.Body));
            }

            widths[column.Key] = ColumnSizing.AutoWidth(column.Kind, widest, column.Sort is not null);
        }
    }

    /// <summary>The column a header cell stands for, by its grid column, or minus one.</summary>
    private int GridColumnOf(string columnKey)
    {
        for (int i = 0; i < shown.Count; i++)
        {
            if (shown[i].Column.Key == columnKey)
            {
                return i;
            }
        }

        return -1;
    }

    private string? ActiveColumnKey() => activeColumn >= 0 && activeColumn < shown.Count ? shown[activeColumn].Column.Key : null;

    /// <summary>A heading was pressed: a sort is asked for.</summary>
    internal void SortPressed(DataTableHeaderCell cell)
    {
        if (Model is { } model && cell.ColumnIndex >= 0 && cell.ColumnIndex < model.Columns.Count)
        {
            RaiseSortRequested(model.Columns[cell.ColumnIndex]);
        }
    }

    /// <summary>The pointer was pressed on a heading.</summary>
    internal void HeaderPressed(DataTableHeaderCell cell, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        SetActive(new Cell(-1, Math.Max(cell.GridColumn, 0)), focus: true);
    }

    /// <summary>A heading took focus.</summary>
    internal void HeaderGotFocus(DataTableHeaderCell cell, RoutedEventArgs e)
    {
        activeRow = -1;
        activeColumn = Math.Max(cell.GridColumn, 0);
    }

    private void ForgetLayout()
    {
        hiddenKeys.Clear();
        forcedShownKeys.Clear();
        frozenWish = null;
        widths.Clear();
        if (templateWired)
        {
            RebuildColumns(remeasure: true);
        }
    }

}
