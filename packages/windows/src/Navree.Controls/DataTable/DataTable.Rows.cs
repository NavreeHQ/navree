using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Design.Tables;
using Geometry = Navree.Design.Tables.Geometry;

namespace Navree.Controls;

public sealed partial class DataTable
{
    /// <summary>What a detail is thought to need before its first measure.</summary>
    private const double DetailEstimate = 160;

    private readonly HashSet<string> openKeys = new(StringComparer.Ordinal);
    private readonly Dictionary<string, double> detailExtras = new(StringComparer.Ordinal);
    private readonly HashSet<DataTableRow> liveRows = [];
    private double rowHeight = 40;
    private double headerHeight = 40;
    private RowShape shape = RowShape.OneLine;
    private bool sortAsked;
    private int askedAfterAt = -1;
    private int askedBeforeAt = -1;
    private bool heldLimitRaised;
    private bool detailGeometryQueued;
    private DispatcherQueueTimer? pinTimer;

    /// <summary>The height of a row.</summary>
    internal double RowHeight => rowHeight;

    /// <summary>The height of the row of headings.</summary>
    internal double HeaderHeight => headerHeight;

    /// <summary>One line in each cell or two.</summary>
    internal RowShape Shape => shape;

    private void ModelChanged(IDataTableModel? previous, IDataTableModel? current)
    {
        if (previous is not null)
        {
            previous.Changed -= OnModelEvent;
        }

        if (current is not null)
        {
            current.Changed += OnModelEvent;
        }

        choice = new ChoiceState(new HashSet<string>(StringComparer.Ordinal), false);
        activeRow = -1;
        activeColumn = 0;
        activeSet = false;
        openKeys.Clear();
        detailExtras.Clear();
        if (templateWired)
        {
            ModelAttached();
        }
    }

    private void PresenterChanged()
    {
        if (templateWired)
        {
            ModelAttached();
        }
    }

    // Everything is drawn again for a model and a presenter: the columns are measured, the rows are listed.
    private void ModelAttached()
    {
        askedAfterAt = askedBeforeAt = -1;
        heldLimitRaised = false;
        RecomputeMetrics();
        RebuildColumns(remeasure: true);
        ReloadRows();
        UpdateState();
        UpdateFooter();
        UpdateBulkBar();
    }

    private void OnModelEvent(object? sender, DataTableModelChangedEventArgs e)
    {
        if (!templateWired)
        {
            return;
        }

        if (e.Kind is DataTableChangeKind.Reset or DataTableChangeKind.Insert or DataTableChangeKind.Remove or DataTableChangeKind.Replace)
        {
            rowNames = null;
        }

        switch (e.Kind)
        {
            case DataTableChangeKind.Reset:
                RecomputeMetrics();
                RebuildColumns(remeasure: false);
                ReloadRows();
                break;
            case DataTableChangeKind.Insert:
                RowsInserted(e.Index, e.Count);
                break;
            case DataTableChangeKind.Remove:
                RowsRemoved(e.Index, e.Count);
                break;
            case DataTableChangeKind.Replace:
                for (int i = 0; i < e.Count; i++)
                {
                    rows.Redraw(e.Index + i);
                }

                break;
            case DataTableChangeKind.Columns:
                RebuildColumns(remeasure: false);
                if (sortAsked && SortStatus() is { } sorted)
                {
                    sortAsked = false;
                    Announce(sorted);
                }

                break;
            case DataTableChangeKind.State:
                RebuildColumns(remeasure: false);
                UpdateState();
                UpdateBulkBar();
                break;
            case DataTableChangeKind.Cursor:
                askedAfterAt = askedBeforeAt = -1;
                UpdateFooter();
                CheckCursors();
                break;
            case DataTableChangeKind.Summary:
                UpdateFooter();
                break;
        }

        if (e.Kind is DataTableChangeKind.Reset or DataTableChangeKind.Insert or DataTableChangeKind.Remove)
        {
            UpdateState();
            UpdateFooter();
            UpdateBulkBar();
        }
    }

    private List<RowHandle> Handles(IDataTableModel model, int first, int count)
    {
        List<RowHandle> handles = new(count);
        for (int i = first; i < first + count; i++)
        {
            handles.Add(new RowHandle(model.GetRow(i).Key ?? ("\u0001" + i.ToString(Culture))));
        }

        return handles;
    }

    private void ReloadRows()
    {
        if (Model is not { } model)
        {
            rows.Reset([]);
            return;
        }

        if (activeSet)
        {
            activeRow = Math.Min(activeRow, model.RowCount - 1);
        }
        else
        {
            NormalizeActive();
        }

        rowsLayout.PinnedIndex = activeRow;
        UpdateGeometry();
        rows.Reset(Handles(model, 0, model.RowCount));
        scroll?.ScrollTo(scroll.HorizontalOffset, 0, new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));
        CheckCursors();
    }

    // The place of the row at the top of the view, which is the one to keep when rows join above it.
    private int TopIndex()
    {
        if (scroll is null || rows.Count == 0)
        {
            return 0;
        }

        return rowsLayout.Geometry.IndexAt(scroll.VerticalOffset, rows.Count).Index;
    }

    private void RowsInserted(int index, int count)
    {
        if (Model is not { } model || count <= 0)
        {
            return;
        }

        int top = TopIndex();
        bool above = index <= top && rows.Count > 0 && scroll is not null && scroll.VerticalOffset > 0;
        if (above)
        {
            // The row the view is on is kept made wherever it ends up, so the scroll view can keep it where it was.
            rowsLayout.PinnedIndex = top + count;
        }

        if (activeRow >= index)
        {
            activeRow += count;
        }

        UpdateGeometry();
        rows.InsertRows(index, Handles(model, index, count));
        if (above)
        {
            ReleasePinSoon();
        }

        askedAfterAt = askedBeforeAt = -1;
        CheckCursors();
    }

    private void RowsRemoved(int index, int count)
    {
        if (count <= 0)
        {
            return;
        }

        int top = TopIndex();
        bool above = index + count <= top && scroll is not null && scroll.VerticalOffset > 0;
        if (above)
        {
            rowsLayout.PinnedIndex = top - count;
        }

        bool activeRemoved = activeSet && activeRow >= index && activeRow < index + count;
        if (activeRow >= index + count)
        {
            activeRow -= count;
        }
        else if (activeRemoved)
        {
            Cell? next = GridNavigation.FocusAfterRemoval(index, Model?.RowCount ?? 0, activeColumn, !IsList);
            activeRow = next?.Row ?? -1;
            Announce(DataTableStrings.Get("DataTable_RowRemoved"));
        }

        UpdateGeometry();
        rows.RemoveRange(index, count);
        if (above)
        {
            ReleasePinSoon();
        }

        RemovedFromChoice();
    }

    // Lets the pin go back to the active row once the rows have been placed.
    private void ReleasePinSoon()
    {
        pinTimer ??= DispatcherQueue.CreateTimer();
        pinTimer.Stop();
        pinTimer.Interval = TimeSpan.FromMilliseconds(400);
        pinTimer.IsRepeating = false;
        pinTimer.Tick -= OnPinTimer;
        pinTimer.Tick += OnPinTimer;
        pinTimer.Start();
    }

    private void OnPinTimer(DispatcherQueueTimer sender, object args)
    {
        rowsLayout.PinnedIndex = activeRow;
        rowsLayout.Reflow();
    }

    /// <summary>Works out the row height from the text size and the shape of the rows, and puts it into the layout.</summary>
    private void RecomputeMetrics()
    {
        measurer.Reset();
        shape = RowShape.OneLine;
        if (Model is { } model && Presenter is { } presenter)
        {
            int sample = Math.Min(60, model.RowCount);
            for (int i = 0; i < sample; i++)
            {
                DataTableRecord record = model.GetRow(i);
                if (presenter.SecondLine(record) is not null)
                {
                    shape = RowShape.TwoLine;
                    break;
                }
            }
        }

        double scale = XamlRoot?.RasterizationScale ?? 1.0;
        double line = measurer.LineHeight(TextStyle.Body);
        double second = measurer.LineHeight(TextStyle.Caption);
        bool controls = Model?.Columns.Any(column => column.Kind == ColumnKind.Controls) ?? false;
        rowHeight = IsList ? ListHeight(line, second, scale) : RowGeometry.RowHeight(density, shape, controls, line, second, scale);
        headerHeight = RowGeometry.HeaderHeight(density, controls, line, scale);
        if (headerHost is not null)
        {
            headerHost.Height = headerHeight;
        }

        UpdateGeometry();
    }

    private void RowMetricsChanged()
    {
        if (!templateWired)
        {
            return;
        }

        RecomputeMetrics();
        RefreshAllRows(numbersOnly: true);
        RaiseLayoutChanged();
    }

    /// <summary>Gives the rows layout a snapshot of where every row stands.</summary>
    private void UpdateGeometry()
    {
        List<(int Index, double Extra)> opens = [];
        if (Model is { } model)
        {
            foreach (string key in openKeys)
            {
                int index = model.IndexOfKey(key);
                if (index >= 0)
                {
                    opens.Add((index, detailExtras.TryGetValue(key, out double extra) ? extra : DetailEstimate));
                }
            }
        }

        opens.Sort((a, b) => a.Index.CompareTo(b.Index));
        rowsLayout.Geometry = new Geometry(rowHeight, opens);
        rowsLayout.Reflow();
    }

    private void OnDetailMeasured(int index, double extra)
    {
        if (Model is not { } model || index < 0 || index >= model.RowCount || model.GetRow(index).Key is not { } key)
        {
            return;
        }

        detailExtras[key] = extra;
        if (!detailGeometryQueued)
        {
            detailGeometryQueued = true;
            DispatcherQueue.TryEnqueue(() =>
            {
                detailGeometryQueued = false;
                UpdateGeometry();
            });
        }
    }

    /// <summary>Whether a row's detail is open.</summary>
    /// <param name="key">The row's key.</param>
    internal bool IsDetailOpen(string? key) => key is not null && openKeys.Contains(key);

    /// <summary>Opens or shuts a row's detail.</summary>
    /// <param name="row">The row.</param>
    internal void ToggleDetail(DataTableRow row)
    {
        if (row.Key is not { } key || !row.Record.HasDetail)
        {
            return;
        }

        bool open = openKeys.Add(key);
        if (!open)
        {
            openKeys.Remove(key);
        }

        UpdateGeometry();
        row.Refresh();
        RaiseDetailToggled(key, open);
    }

    private void RefreshAllRows(bool numbersOnly)
    {
        header?.Refresh();
        foreach (DataTableRow row in liveRows)
        {
            if (numbersOnly)
            {
                row.ApplyNumbers(this);
            }
            else
            {
                row.Refresh();
            }
        }

        rowsLayout.Reflow();
    }

    private void CheckCursors()
    {
        if (Model is not { } model || scroll is null)
        {
            return;
        }

        double viewport = scroll.ViewportHeight;
        if (viewport <= 0 || model.RowCount == 0)
        {
            return;
        }

        int limit = RowGeometry.MaxHeldRows(rowHeight);
        if (model.RowCount >= limit)
        {
            if (!heldLimitRaised)
            {
                heldLimitRaised = true;
                RaiseHeldLimitReached();
                UpdateFooter();
            }

            return;
        }

        if (model.Later is { Available: true, State: CursorState.Idle }
            && askedAfterAt != model.RowCount
            && Cursor.ShouldAskMore(scroll.VerticalOffset, viewport, scroll.ExtentHeight))
        {
            askedAfterAt = model.RowCount;
            RaiseMoreRequested(MoreDirection.After);
        }

        if (model.Earlier is { Available: true, State: CursorState.Idle }
            && askedBeforeAt != model.RowCount
            && Cursor.ShouldAskBefore(scroll.VerticalOffset, viewport))
        {
            askedBeforeAt = model.RowCount;
            RaiseMoreRequested(MoreDirection.Before);
        }
    }

    // What the pointer did to a row, the kind of click it was.
    internal void RowTapped(DataTableRow row, Microsoft.UI.Xaml.Input.TappedRoutedEventArgs e)
    {
        if (row.Key is null && !row.Record.HasAction)
        {
            return;
        }

        DataTableCell? cell = FindCell(e.OriginalSource as DependencyObject);
        if (FindGlyph(e.OriginalSource as DependencyObject) is { } glyph)
        {
            if (cell is not null)
            {
                SetActive(new Cell(row.Index, Math.Max(cell.GridColumn, 0)), focus: true);
            }

            ShowRowMenu(row, glyph, null);
            e.Handled = true;
            return;
        }

        bool onBox = cell?.Role == CellRole.Check;
        if (cell?.Role == CellRole.Chevron)
        {
            ToggleDetail(row);
            SetActive(new Cell(row.Index, Math.Max(activeColumn, 0)), focus: true);
            e.Handled = true;
            return;
        }

        if (cell is { Role: CellRole.Data })
        {
            SetActive(new Cell(row.Index, Math.Max(cell.GridColumn, 0)), focus: true);
        }

        bool ctrl = IsKeyDown(Windows.System.VirtualKey.Control);
        bool shift = IsKeyDown(Windows.System.VirtualKey.Shift);
        bool any = ChosenCount > 0;
        switch (Choosing.Click(any, ctrl, shift, onBox, row.Record.HasAction))
        {
            case ClickMeaning.Open:
                if (row.Key is { } key)
                {
                    RaiseRowInvoked(key);
                }

                break;
            case ClickMeaning.Toggle:
                ToggleChoice(row.Index);
                break;
            case ClickMeaning.Range:
                ChooseRangeTo(row.Index);
                break;
        }

        e.Handled = true;
    }

    internal void RowRightTapped(DataTableRow row, Microsoft.UI.Xaml.Input.RightTappedRoutedEventArgs e)
    {
        if (ShowRowMenu(row, e.OriginalSource as FrameworkElement, e.GetPosition(this)))
        {
            e.Handled = true;
        }
    }

    /// <summary>Opens a row's menu beside a cell, as a key does.</summary>
    /// <param name="row">The row.</param>
    /// <param name="cell">The cell.</param>
    internal void ShowRowMenuForRun(DataTableRow row, FrameworkElement cell) => ShowRowMenu(row, cell, null);

    // The row's menu, at a point when the pointer asked and beside the cell when a key did.
    private bool ShowRowMenu(DataTableRow row, FrameworkElement? target, Windows.Foundation.Point? position)
    {
        if (!row.Record.HasMenu || Presenter?.CreateRowMenu(row.Record) is not { } flyout)
        {
            return false;
        }

        if (position is { } point)
        {
            flyout.ShowAt(this, new Microsoft.UI.Xaml.Controls.Primitives.FlyoutShowOptions { Position = point });
        }
        else
        {
            flyout.ShowAt(target ?? row);
        }

        return true;
    }

    private static DataTableCell? FindCell(DependencyObject? source)
    {
        while (source is not null)
        {
            if (source is DataTableCell cell)
            {
                return cell;
            }

            if (source is DataTableRow)
            {
                return null;
            }

            source = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(source);
        }

        return null;
    }

    private static DataTableGlyph? FindGlyph(DependencyObject? source)
    {
        while (source is not null)
        {
            if (source is DataTableGlyph glyph && glyph.Glyph == Glyphs.More)
            {
                return glyph;
            }

            if (source is DataTableRow)
            {
                return null;
            }

            source = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(source);
        }

        return null;
    }

    private static bool IsKeyDown(Windows.System.VirtualKey key) =>
        (Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(key) & Windows.UI.Core.CoreVirtualKeyStates.Down) == Windows.UI.Core.CoreVirtualKeyStates.Down;
}
