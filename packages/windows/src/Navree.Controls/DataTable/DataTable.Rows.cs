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
    private readonly ModelLink modelLink = new();
    private ModelStamp modelStamp;
    private readonly CursorWatch cursorWatch = new();
    private readonly DetailSeeds detailSeeds = new();
    private readonly RowShapeWatch shapeWatch = new();
    private bool detailGeometryQueued;
    private DispatcherQueueTimer? anchorPinTimer;
    private DispatcherQueueTimer? assistivePinTimer;
    private bool holdingFirstRow;

    /// <summary>The height of a row.</summary>
    internal double RowHeight => rowHeight;

    /// <summary>The height of the row of headings.</summary>
    internal double HeaderHeight => headerHeight;

    /// <summary>One line in each cell or two.</summary>
    internal RowShape Shape => shape;

    /// <summary>What a model said of itself when the table last had it in step.</summary>
    private readonly record struct ModelStamp(int Rows, string? First, string? Last, DataTableLoadState State, string? Summary, DataTableCursor Later, DataTableCursor Earlier);

    private static ModelStamp StampOf(IDataTableModel model) => new(
        model.RowCount,
        model.RowCount > 0 ? model.GetRow(0).Key : null,
        model.RowCount > 0 ? model.GetRow(model.RowCount - 1).Key : null,
        model.State,
        model.Summary,
        model.Later,
        model.Earlier);

    private void ModelChanged(IDataTableModel? previous, IDataTableModel? current)
    {
        // The model is listened to only while the table is in the tree (see OnLoaded).
        if (loaded)
        {
            modelLink.Attach(current, OnModelEvent);
        }
        else
        {
            modelLink.Detach();
        }

        choice = new ChoiceState(new HashSet<string>(StringComparer.Ordinal), false);
        pins.Clear();
        SetActiveRow(-1);
        activeKey = null;
        activeColumn = 0;
        activeSet = false;
        nameCache.Clear();
        openKeys.Clear();
        detailSeeds.Forget();
        shapeWatch.Forget();
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
        cursorWatch.Forget();
        RecomputeMetrics();
        RebuildColumns(remeasure: true);
        ReloadRows();
        UpdateState();
        UpdateFooter();
        UpdateBulkBar();
        if (Model is { } attached)
        {
            modelStamp = StampOf(attached);
        }
    }

    private void OnModelEvent(object? sender, DataTableModelChangedEventArgs e)
    {
        if (!templateWired)
        {
            return;
        }

        if (e.Kind is DataTableChangeKind.Reset or DataTableChangeKind.Insert or DataTableChangeKind.Remove or DataTableChangeKind.Replace)
        {
            nameCache.Clear();
        }

        switch (e.Kind)
        {
            case DataTableChangeKind.Reset:
                cursorWatch.Forget();
                shapeWatch.Forget();
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
                RowsReplaced(e.Index, e.Count);
                break;
            case DataTableChangeKind.Columns:
                RebuildColumns(remeasure: false);
                if (sortAsked && SortedBy() is { } sorted)
                {
                    sortAsked = false;
                    Say(sorted);
                }

                break;
            case DataTableChangeKind.State:
                RebuildColumns(remeasure: false);
                UpdateState();
                UpdateBulkBar();
                if (Model is { } stated && stated.State != lastLoadState)
                {
                    lastLoadState = stated.State;
                    if (stated.State == DataTableLoadState.Loading)
                    {
                        Say(new Announcement("DataTable_Loading", []));
                    }
                    else if (stated.State == DataTableLoadState.Failed && stated.FailureMessage is { } why)
                    {
                        Announce(why, AnnouncementKind.Other, "DataTable.State");
                    }
                }

                break;
            case DataTableChangeKind.Cursor:
                cursorWatch.Asked();
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

        if (Model is { } current)
        {
            modelStamp = StampOf(current);
            RaiseCountChanged(current.RowCount);
        }

        if (e.Kind is DataTableChangeKind.Reset or DataTableChangeKind.Insert or DataTableChangeKind.Remove)
        {
            QueueStructureEvent();
        }
    }

    // The handles of rows that arrive. A row that arrives with its detail open is opened once; and a row that needs what the rows
    // so far did not (a second line, a detail) is said by the two flags, so the table can be shaped for it.
    private List<RowHandle> Handles(IDataTableModel model, int first, int count, out bool newSecondLine, out bool newDetail)
    {
        List<RowHandle> handles = new(count);
        IDataTablePresenter? presenter = Presenter;
        newSecondLine = false;
        newDetail = false;
        for (int i = first; i < first + count; i++)
        {
            DataTableRecord record = model.GetRow(i);
            handles.Add(new RowHandle(record.Key ?? ("\u0001" + i.ToString(Culture))));
            if (detailSeeds.Arrive(record.Key, record.HasDetail, record.DetailOpen))
            {
                openKeys.Add(record.Key!);
            }

            bool second = !shapeWatch.SawSecondLine && presenter?.SecondLine(record) is not null;
            (bool newSecond, bool newDetailHere) = shapeWatch.Observe(second, record.HasDetail);
            newSecondLine |= newSecond;
            newDetail |= newDetailHere;
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

        // Focus is put back on the active cell afterwards only when it was in the table.
        bool hadFocus = FocusIsInside();
        if (activeSet)
        {
            // The active row is the same row by key when the new rows have it, else the first row.
            int same = activeKey is { } key ? model.IndexOfKey(key) : -1;
            SetActiveRow(ActiveRowRules.AfterReset(activeRow < 0, same, model.RowCount));
        }
        else
        {
            NormalizeActive();
        }

        List<RowHandle> handles = Handles(model, 0, model.RowCount, out bool newSecondLine, out bool newDetail);
        if (newSecondLine || newDetail)
        {
            // A later row needs a second line or a detail the first rows did not: the shape of the rows and the columns follow.
            RecomputeMetrics();
            RebuildColumns(remeasure: false);
        }

        UpdateGeometry();
        rows.Reset(handles);
        scroll?.ScrollTo(scroll.HorizontalOffset, 0, new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));
        CheckCursors();
        if (hadFocus)
        {
            FocusActiveSoon();
        }
    }

    // Puts focus back on the active cell once the rows have been placed.
    private void FocusActiveSoon() => DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () =>
    {
        if (loaded && Model is not null)
        {
            TryActivate(focus: true, 0);
        }
    });

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
        double offset = scroll?.VerticalOffset ?? 0;
        double within = WithinTopRow(offset, top);
        double heightBefore = rowHeight;
        int heldBefore = rows.Count;
        bool keepFirst = scroll is not null && ActiveRowRules.KeepsFirstRowInView(index, heldBefore, offset);
        bool above = index <= top && heldBefore > 0 && scroll is not null && offset > 0;
        pins.RowsJoined(index, count);
        if (above || keepFirst)
        {
            // The row the view is on is kept made wherever it ends up, so the scroll view can keep it where it was.
            pins.Set(PinOwner.Anchor, top + count);
        }

        if (activeSet)
        {
            SetActiveRow(ActiveRowRules.AfterInsert(activeRow, index, count));
        }
        else
        {
            rowsLayout.Pinned = pins.Indices;
        }

        List<RowHandle> handles = Handles(model, index, count, out bool newSecondLine, out bool newDetail);
        UpdateGeometry();
        rows.InsertRows(index, handles);
        if (above || keepFirst)
        {
            ReleasePinSoon(PinOwner.Anchor);
        }

        if (newSecondLine || newDetail)
        {
            // A page that needs a second line or a detail the rows so far did not: the shape of the rows and the columns follow it.
            RecomputeMetrics();
            RebuildColumns(remeasure: false);
        }

        if (keepFirst || (rowHeight != heightBefore && heldBefore > 0))
        {
            // The view stays on the row it was on: the row that was first when it was at the top, else the row at the top of the view.
            int anchor = keepFirst ? top + count : (index <= top ? top + count : top);
            KeepAnchorInView(anchor, keepFirst ? 0 : within * rowHeight / heightBefore);
        }

        cursorWatch.Asked();
        NoteRowsAdded(count, earlier: index == 0 && rows.Count > count);
        CheckCursors();
    }

    // How far down inside the row at the top of the view the view is, by the rows as they stood.
    private double WithinTopRow(double offset, int top) =>
        rows.Count > 0 && scroll is not null ? Math.Max(0, offset - rowsLayout.Geometry.RowTop(top)) : 0;

    // A view at the very top has nothing above it for the scroll view to anchor to: rows that joined the start are above the row that was
    // first, and the view is moved down to that row. The same is done when the rows change height because of what joined. The place of
    // the row is the arithmetic of the rows now held.
    private void KeepAnchorInView(int anchorIndex, double within)
    {
        if (scroll is not { } view)
        {
            return;
        }

        holdingFirstRow = true;
        void Move()
        {
            double wanted = Anchoring.OffsetAfterChange(anchorIndex, within, rowsLayout.Geometry, rows.Count, view.ViewportHeight);
            if (Math.Abs(view.VerticalOffset - wanted) > 0.5)
            {
                view.ScrollTo(view.HorizontalOffset, wanted, new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));
            }
        }

        view.UpdateLayout();
        Move();
        DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () =>
        {
            Move();
            holdingFirstRow = false;
            CheckCursors();
        });
    }

    private void RowsRemoved(int index, int count)
    {
        if (count <= 0)
        {
            return;
        }

        int top = TopIndex();
        bool above = index + count <= top && scroll is not null && scroll.VerticalOffset > 0;
        bool hadFocus = FocusIsInside();
        pins.RowsLeft(index, count);
        if (above)
        {
            pins.Set(PinOwner.Anchor, top - count);
        }

        if (activeSet)
        {
            (Cell? next, bool activeRemoved) = ActiveRowRules.AfterRemove(activeRow, index, count, Model?.RowCount ?? 0, activeColumn, !IsList);
            SetActiveRow(next?.Row ?? -1);
            if (activeRemoved)
            {
                Say(new Announcement("DataTable_RowRemoved", []));
                if (hadFocus)
                {
                    // Focus goes where the rule for a removed row says: the row now at the place, the one before it, or the heading.
                    FocusActiveSoon();
                }
            }
        }
        else
        {
            rowsLayout.Pinned = pins.Indices;
        }

        UpdateGeometry();
        rows.RemoveRange(index, count);
        if (above)
        {
            ReleasePinSoon(PinOwner.Anchor);
        }

        RemovedFromChoice();
    }

    // Rows drawn again with new data: a row that is made is bound again where it stands, so a field a person is typing in and the
    // focus stay; a row whose key changed is a new row to the repeater.
    private void RowsReplaced(int index, int count)
    {
        if (Model is not { } model)
        {
            return;
        }

        for (int i = index; i < index + count && i < model.RowCount; i++)
        {
            string key = model.GetRow(i).Key ?? ("\u0001" + i.ToString(Culture));
            if (rows.PlaceOf(key) != i)
            {
                rows.Replace(i, new RowHandle(key));
            }
            else if (repeater?.TryGetElement(i) is DataTableRow made)
            {
                made.Refresh(keepFocused: true);
            }
        }

        UpdateFooter();
    }

    // Lets an owner's pin go back to nothing once the rows have been placed.
    private void ReleasePinSoon(PinOwner owner)
    {
        ref DispatcherQueueTimer? timer = ref (owner == PinOwner.Anchor ? ref anchorPinTimer : ref assistivePinTimer);
        timer ??= DispatcherQueue.CreateTimer();
        timer.Stop();
        timer.Interval = TimeSpan.FromMilliseconds(400);
        timer.IsRepeating = false;
        timer.Tick -= OnPinTimer;
        timer.Tick += OnPinTimer;
        timer.Start();
    }

    private void OnPinTimer(DispatcherQueueTimer sender, object args)
    {
        PinOwner owner = ReferenceEquals(sender, anchorPinTimer) ? PinOwner.Anchor : PinOwner.Assistive;
        pins.Release(owner);
        rowsLayout.Pinned = pins.Indices;
        rowsLayout.Reflow();
    }

    /// <summary>Works out the row height from the text size and the shape of the rows, and puts it into the layout.</summary>
    private void RecomputeMetrics()
    {
        measurer.Reset();
        shape = shapeWatch.SawSecondLine ? RowShape.TwoLine : RowShape.OneLine;
        if (shape == RowShape.OneLine && Model is { } model && Presenter is { } presenter)
        {
            int sample = Math.Min(60, model.RowCount);
            for (int i = 0; i < sample; i++)
            {
                DataTableRecord record = model.GetRow(i);
                if (presenter.SecondLine(record) is not null)
                {
                    shape = RowShape.TwoLine;
                    shapeWatch.Observe(true, false);
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
                if (loaded)
                {
                    UpdateGeometry();
                }
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
        RaiseExpandState(row, open);
        Say(Announcements.Detail(open));
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
        if (viewport <= 0 || model.RowCount == 0 || holdingFirstRow)
        {
            return;
        }

        int limit = RowGeometry.MaxHeldRows(rowHeight);
        if (model.RowCount >= limit)
        {
            if (cursorWatch.LimitReached())
            {
                RaiseHeldLimitReached();
                Say(new Announcement("DataTable_HeldLimit", [limit]));
                UpdateFooter();
            }

            return;
        }

        if (cursorWatch.ShouldAskAfter(
            model.RowCount,
            model.Later.Available,
            model.Later.State == CursorState.Idle,
            Cursor.ShouldAskMore(scroll.VerticalOffset, viewport, scroll.ExtentHeight)))
        {
            RaiseMoreRequested(MoreDirection.After);
        }

        if (cursorWatch.ShouldAskBefore(
            model.RowCount,
            model.Earlier.Available,
            model.Earlier.State == CursorState.Idle,
            Cursor.ShouldAskBefore(scroll.VerticalOffset, viewport)))
        {
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
    internal void OpenRowMenu(DataTableRow row, FrameworkElement cell) => ShowRowMenu(row, cell, null);

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
