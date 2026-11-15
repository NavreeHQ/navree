using System.Globalization;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Navree.Design.Tables;
using Windows.Foundation;
using VirtualKey = Windows.System.VirtualKey;

namespace Navree.Controls;

public sealed partial class DataTable
{
    private readonly HashSet<string> personWidths = new(StringComparer.Ordinal);
    private List<string> columnOrder = [];
    private DispatcherQueueTimer? saveTimer;
    private readonly LayoutKeeper keeper = new();
    private string? resizingKey;
    private double resizeStartWidth;
    private double gripWidth = 8;

    // A touch needs a wider strip to take hold of than a pen, and a pen than a mouse.
    private void PointerKindSeen(Microsoft.UI.Input.PointerDeviceType kind)
    {
        double width = kind switch
        {
            Microsoft.UI.Input.PointerDeviceType.Touch => 24,
            Microsoft.UI.Input.PointerDeviceType.Pen => 12,
            _ => 8,
        };
        if (header is null || Math.Abs(gripWidth - width) < 0.5)
        {
            return;
        }

        gripWidth = width;
        foreach (DataTableHeaderCell cell in header.Headers)
        {
            if (cell.Grip is { } grip)
            {
                grip.Width = width;
            }
        }
    }

    /// <summary>The width a resize strip has for the pointer last seen.</summary>
    internal double GripWidth => gripWidth;

    // A line for a measuring run to read; a build without the measuring symbol drops every call and what it would have written.
    partial void Note(string line);

    private DataTableHeaderCell? reordering;
    private int dropAt;
    private double reorderStartOffset;
    private double reorderDx;
    private double reorderPointerX;
    private DispatcherQueueTimer? edgeTimer;
    private System.Diagnostics.Stopwatch edgeClock = new();

    /// <summary>A pointer lifted a heading to move its column.</summary>
    internal void ReorderStarted(DataTableHeaderCell cell)
    {
        reordering = cell;
        dropAt = cell.GridColumn;
        reorderStartOffset = scroll?.HorizontalOffset ?? 0;
        reorderDx = 0;
        edgeClock = System.Diagnostics.Stopwatch.StartNew();
        Note($"[drag] a heading was lifted: column {cell.GridColumn}");
        Microsoft.UI.Xaml.Hosting.ElementCompositionPreview.SetIsTranslationEnabled(cell, true);
        cell.Opacity = 0.8;
        Canvas.SetZIndex(cell, 10);
        edgeTimer ??= DispatcherQueue.CreateTimer();
        edgeTimer.Interval = TimeSpan.FromMilliseconds(16);
        edgeTimer.IsRepeating = true;
        edgeTimer.Tick -= OnEdgeTick;
        edgeTimer.Tick += OnEdgeTick;
        edgeTimer.Start();
    }

    /// <summary>The pointer moved with a lifted heading, by this much since it began, in the window's coordinates.</summary>
    internal void ReorderMoved(DataTableHeaderCell cell, double dx, double pointerX)
    {
        if (reordering is null || cell.GridColumn < 0 || cell.GridColumn >= shown.Count)
        {
            return;
        }

        reorderDx = dx;
        reorderPointerX = pointerX;
        FollowPointer(cell);
    }

    // The lifted heading stands under the pointer, and the place it would drop at is where its centre is among the others'. A heading
    // of the scrolling columns moves with the content as the view is scrolled, so the distance scrolled since it was lifted is added.
    private void FollowPointer(DataTableHeaderCell cell)
    {
        ShownColumn dragged = shown[cell.GridColumn];
        double logical = FlowDirection == FlowDirection.RightToLeft ? -reorderDx : reorderDx;
        double scrolled = dragged.Placed.Group == ColumnGroup.Scroll ? (scroll?.HorizontalOffset ?? 0) - reorderStartOffset : 0;
        cell.Translation = new System.Numerics.Vector3((float)(logical + scrolled), 0, 0);
        double centre = dragged.Placed.X + (dragged.Placed.Width / 2) + logical + scrolled;
        List<double> centres = shown.Select(s => s.Placed.X + (s.Placed.Width / 2)).ToList();
        int applied = layout?.FrozenStartApplied ?? 0;
        int endCount = shown.Count(s => s.Placed.Group == ColumnGroup.FrozenEnd);
        (int from, int to) = dragged.Placed.Group switch
        {
            ColumnGroup.FrozenStart => (0, applied - 1),
            ColumnGroup.FrozenEnd => (shown.Count - endCount, shown.Count - 1),
            _ => (applied, shown.Count - endCount - 1),
        };
        dropAt = ColumnOrder.DropIndex(cell.GridColumn, centre, centres, from, Math.Max(from, to));
        Note($"[drag] the heading moved {reorderDx:0.#}, view scrolled {scrolled:0.#}: it would drop at {dropAt}");
    }

    // While a heading of the scrolling columns is held near an edge of the view the view scrolls, faster the nearer the edge.
    private void OnEdgeTick(DispatcherQueueTimer sender, object args)
    {
        double seconds = edgeClock.Elapsed.TotalSeconds;
        edgeClock.Restart();
        if (reordering is not { } cell || scroll is null || cell.GridColumn < 0 || cell.GridColumn >= shown.Count || shown[cell.GridColumn].Placed.Group != ColumnGroup.Scroll)
        {
            return;
        }

        double left = scroll.TransformToVisual(null).TransformPoint(new Point(0, 0)).X;
        double velocity = AutoScroll.Velocity(reorderPointerX - left, scroll.ViewportWidth);
        if (velocity == 0)
        {
            return;
        }

        // The velocity is in the window's directions: toward the physical left is negative. In a right to left flow the end of the rows is on the left.
        double delta = (FlowDirection == FlowDirection.RightToLeft ? -velocity : velocity) * Math.Min(seconds, 0.1);
        scroll.ScrollBy(delta, 0, new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));
        FollowPointer(cell);
    }

    /// <summary>The pointer let go of a lifted heading.</summary>
    internal void ReorderEnded(DataTableHeaderCell cell, bool keep)
    {
        Note($"[drag] the heading was let go, kept: {keep}, from {cell.GridColumn} to {dropAt}");
        edgeTimer?.Stop();
        cell.Translation = System.Numerics.Vector3.Zero;
        cell.Opacity = 1;
        Canvas.SetZIndex(cell, 0);
        reordering = null;
        if (!keep || cell.GridColumn < 0 || dropAt == cell.GridColumn || dropAt < 0 || dropAt >= shown.Count)
        {
            return;
        }

        DataTableColumn column = ColumnAt(cell.ColumnIndex);
        List<string> all = FullOrder();
        string target = shown[dropAt].Column.Key;
        columnOrder = [.. ColumnOrder.Move(all, all.IndexOf(column.Key), all.IndexOf(target))];
        ColumnsChangedByPerson(Announcements.ColumnMoved(column.Header, dropAt + 1, shown.Count));
    }

    private string KeyOf(DataTableHeaderCell cell) => ColumnAt(cell.ColumnIndex).Key;

    // The keys that work on a heading: widen and narrow, move, fit, the menu.
    private bool HeaderKey(DataTableHeaderCell cell, VirtualKey key, bool ctrl, bool shift, bool alt)
    {
        if (Model is null || cell.ColumnIndex < 0)
        {
            return false;
        }

        bool rtl = FlowDirection == FlowDirection.RightToLeft;
        DataTableColumn column = ColumnAt(cell.ColumnIndex);
        if (alt && key is VirtualKey.Left or VirtualKey.Right)
        {
            int direction = (key == VirtualKey.Right) != rtl ? 1 : -1;
            double current = widths.TryGetValue(column.Key, out double known) ? known : 0;
            widths[column.Key] = ColumnSizing.ResizeByKey(column.Kind, current, direction, shift);
            personWidths.Add(column.Key);
            ColumnsChangedByPerson(Announcements.ColumnWidth(column.Header, (int)Math.Round(widths[column.Key])));
            return true;
        }

        if (ctrl && key is VirtualKey.Left or VirtualKey.Right)
        {
            MoveColumn(column, (key == VirtualKey.Right) != rtl ? 1 : -1);
            DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () => SetActive(new Cell(-1, activeColumn), focus: true));
            return true;
        }

        if (ctrl && key is VirtualKey.Number0 or VirtualKey.NumberPad0)
        {
            FitColumn(cell);
            return true;
        }

        if (key == VirtualKey.Application || (shift && key == VirtualKey.F10))
        {
            ShowColumnMenu(cell, null);
            return true;
        }

        return false;
    }

    /// <summary>A pointer began to widen a column.</summary>
    internal void ResizeStarted(DataTableHeaderCell cell)
    {
        resizingKey = KeyOf(cell);
        resizeStartWidth = widths.TryGetValue(resizingKey, out double known) ? known : 0;
        Note($"[drag] a strip was taken hold of on {resizingKey}, now {resizeStartWidth:0} wide");
    }

    /// <summary>A pointer is widening a column.</summary>
    internal void ResizeMoved(DataTableHeaderCell cell, double dx)
    {
        if (resizingKey is null)
        {
            return;
        }

        double delta = FlowDirection == FlowDirection.RightToLeft ? -dx : dx;
        widths[resizingKey] = ColumnSizing.Clamp(ColumnAt(cell.ColumnIndex).Kind, resizeStartWidth + delta);
        Note($"[drag] the strip moved {dx:0.#}: {resizingKey} is {widths[resizingKey]:0} wide");
        RebuildColumns(remeasure: false);
    }

    /// <summary>A pointer stopped widening a column.</summary>
    internal void ResizeEnded(DataTableHeaderCell cell, bool keep)
    {
        if (resizingKey is not { } key)
        {
            return;
        }

        resizingKey = null;
        Note($"[drag] the strip on {key} was let go, kept: {keep}");
        if (!keep)
        {
            widths[key] = resizeStartWidth;
            RebuildColumns(remeasure: false);
            return;
        }

        personWidths.Add(key);
        ColumnsChangedByPerson(Announcements.ColumnWidth(ColumnAt(cell.ColumnIndex).Header, (int)Math.Round(widths[key])));
    }

    /// <summary>Gives a column a width, as assistive technology asks.</summary>
    internal void ResizeTo(DataTableHeaderCell cell, double width)
    {
        DataTableColumn column = ColumnAt(cell.ColumnIndex);
        widths[column.Key] = ColumnSizing.Clamp(column.Kind, width);
        personWidths.Add(column.Key);
        ColumnsChangedByPerson(Announcements.ColumnWidth(column.Header, (int)Math.Round(widths[column.Key])));
    }

    /// <summary>Makes a column as wide as what it holds needs.</summary>
    internal void FitColumn(DataTableHeaderCell cell)
    {
        if (Model is not { } model || Presenter is not { } presenter)
        {
            return;
        }

        DataTableColumn column = ColumnAt(cell.ColumnIndex);
        double widest = measurer.Width(column.Header, TextStyle.BodyStrong);
        int count = Math.Min(model.RowCount, 180);
        for (int i = 0; i < count; i++)
        {
            widest = Math.Max(widest, measurer.Width(presenter.CellText(model.GetRow(i), cell.ColumnIndex), TextStyle.Body));
        }

        widths[column.Key] = ColumnSizing.FitWidth(column.Kind, widest, column.Sort is not null);
        personWidths.Add(column.Key);
        ColumnsChangedByPerson(Announcements.ColumnWidth(column.Header, (int)Math.Round(widths[column.Key])));
    }

    private void ResetWidth(string key)
    {
        personWidths.Remove(key);
        widths.Remove(key);
        ColumnsChangedByPerson(null);
    }

    private void ColumnsChangedByPerson(Announcement? say)
    {
        RebuildColumns(remeasure: false);
        ScheduleSave();
        RaiseLayoutChanged();
        if (say is not null)
        {
            Say(say);
        }
    }

    // Opens the menu of a column: from a right click at a point, from a key beside the cell.
    internal void ShowColumnMenu(DataTableHeaderCell cell, Point? at)
    {
        if (Model is null || cell.ColumnIndex < 0)
        {
            return;
        }

        DataTableColumn column = ColumnAt(cell.ColumnIndex);
        if (column.Role == ColumnRole.Actions)
        {
            return;
        }

        int grid = cell.GridColumn;
        bool resizable = ColumnSizing.ResizeRange(column.Kind).Max > 0;
        List<ColInfo> shownInfos = shown.Select(s => new ColInfo(s.Column.Key, s.Column.Kind, s.Column.Role, s.Column.Secondary)).ToList();
        int endCount = shown.Count(s => s.Placed.Group == ColumnGroup.FrozenEnd);
        int applied = layout?.FrozenStartApplied ?? 0;
        MenuFlyout menu = new();
        menu.Items.Add(Item("DataTable_Menu_Sort", column.Sort is not null, () => RaiseSortRequested(column)));
        menu.Items.Add(Item("DataTable_Menu_Fit", resizable, () => FitColumn(cell)));
        menu.Items.Add(Item("DataTable_Menu_ResetWidth", personWidths.Contains(column.Key), () => ResetWidth(column.Key)));
        menu.Items.Add(Item("DataTable_Menu_Hide", grid >= 0 && ColumnSet.CanHide(shownInfos, grid), () => HideColumn(column)));
        menu.Items.Add(new MenuFlyoutSeparator());
        menu.Items.Add(Item("DataTable_Menu_MoveEarlier", ColumnOrder.CanMove(grid, -1, applied, endCount, shown.Count), () => MoveColumn(column, -1)));
        menu.Items.Add(Item("DataTable_Menu_MoveLater", ColumnOrder.CanMove(grid, 1, applied, endCount, shown.Count), () => MoveColumn(column, 1)));
        menu.Items.Add(Item("DataTable_Menu_Freeze", grid >= 0 && grid >= applied, () => FreezeUpTo(grid)));
        menu.Items.Add(Item("DataTable_Menu_Unfreeze", applied > 0, Unfreeze));
        menu.Items.Add(new MenuFlyoutSeparator());

        MenuFlyoutSubItem columnsMenu = new() { Text = DataTableStrings.Get("DataTable_Menu_Columns") };
        foreach (DataTableColumn each in Model.Columns)
        {
            if (each.Role is ColumnRole.Subtitle or ColumnRole.Actions)
            {
                continue;
            }

            DataTableColumn captured = each;
            ToggleMenuFlyoutItem toggle = new() { Text = each.Header, IsChecked = !hiddenKeys.Contains(each.Key) };
            toggle.Click += (_, _) =>
            {
                if (hiddenKeys.Contains(captured.Key))
                {
                    ShowColumn(captured);
                }
                else
                {
                    HideColumn(captured);
                }
            };
            columnsMenu.Items.Add(toggle);
        }

        menu.Items.Add(columnsMenu);
        MenuFlyoutSubItem densityMenu = new() { Text = DataTableStrings.Get("DataTable_Menu_Density") };
        foreach ((Density value, string id) in new[] { (Density.Compact, "DataTable_Density_Compact"), (Density.Standard, "DataTable_Density_Standard"), (Density.Comfortable, "DataTable_Density_Comfortable") })
        {
            Density captured = value;
            RadioMenuFlyoutItem item = new() { Text = DataTableStrings.Get(id), GroupName = "DataTableDensity", IsChecked = density == value };
            item.Click += (_, _) =>
            {
                Density = captured;
                ScheduleSave();
            };
            densityMenu.Items.Add(item);
        }

        menu.Items.Add(densityMenu);
        menu.Items.Add(Item("DataTable_Menu_Reset", personWidths.Count > 0 || hiddenKeys.Count > 0 || forcedShownKeys.Count > 0 || columnOrder.Count > 0 || frozenWish is not null, ResetColumns));
        menu.Closed += (_, _) => RestoreFocus();
        if (at is { } point)
        {
            menu.ShowAt(this, new FlyoutShowOptions { Position = point });
        }
        else
        {
            menu.ShowAt(cell);
        }
    }

    private static MenuFlyoutItem Item(string id, bool enabled, Action act)
    {
        MenuFlyoutItem item = new() { Text = DataTableStrings.Get(id), IsEnabled = enabled };
        item.Click += (_, _) => act();
        return item;
    }

    internal void HideColumn(DataTableColumn column)
    {
        hiddenKeys.Add(column.Key);
        forcedShownKeys.Remove(column.Key);
        ColumnsChangedByPerson(Announcements.ColumnVisibility(column.Header, false));
    }

    private void ShowColumn(DataTableColumn column)
    {
        hiddenKeys.Remove(column.Key);
        if (column.Secondary)
        {
            forcedShownKeys.Add(column.Key);
        }

        ColumnsChangedByPerson(Announcements.ColumnVisibility(column.Header, true));
    }

    // Moves a column one step past its neighbour among the columns shown.
    internal void MoveColumn(DataTableColumn column, int delta)
    {
        int grid = GridColumnOf(column.Key);
        int endCount = shown.Count(s => s.Placed.Group == ColumnGroup.FrozenEnd);
        if (grid < 0 || !ColumnOrder.CanMove(grid, delta, layout?.FrozenStartApplied ?? 0, endCount, shown.Count))
        {
            return;
        }

        List<string> all = FullOrder();
        string neighbour = shown[grid + delta].Column.Key;
        columnOrder = [.. ColumnOrder.Move(all, all.IndexOf(column.Key), all.IndexOf(neighbour))];
        ColumnsChangedByPerson(Announcements.ColumnMoved(column.Header, grid + delta + 1, shown.Count));
        activeColumn = Math.Clamp(grid + delta, 0, shown.Count - 1);
    }

    private List<string> FullOrder() => [.. ColumnOrder.Reconcile(Model!.Columns.Select(c => c.Key).ToList(), columnOrder)];

    internal void FreezeUpTo(int gridColumn)
    {
        int before = frozenWish ?? ColumnSet.DefaultFrozen(shown.Select(s => new ColInfo(s.Column.Key, s.Column.Kind, s.Column.Role, s.Column.Secondary)).ToList());
        frozenWish = gridColumn + 1;
        RebuildColumns(remeasure: false);
        if ((layout?.FrozenStartApplied ?? 0) < gridColumn + 1)
        {
            frozenWish = before;
            RebuildColumns(remeasure: false);
            Announce(DataTableStrings.Get("DataTable_NoRoomToFreeze"));
            return;
        }

        ColumnsChangedByPerson(null);
    }

    private void Unfreeze()
    {
        frozenWish = 0;
        ColumnsChangedByPerson(null);
    }

    private void ResetColumns()
    {
        columnOrder = [];
        personWidths.Clear();
        ForgetLayout();
        ScheduleSave();
        RaiseLayoutChanged();
    }

    // What a person has kept of the columns is read when the columns first arrive for a key; true when it changed the density.
    private bool LoadStoredLayout(IReadOnlyList<DataTableColumn> columns)
    {
        if (layoutKey is not { } key || layoutStore is not { } store)
        {
            return false;
        }

        if (!keeper.ShouldRead(key, columns.Select(c => c.Key).ToList()))
        {
            return false;
        }

        if (!store.TryLoad(key, out string json) || LayoutText.Parse(json) is not { } stored)
        {
            return false;
        }

        if (StoredLayoutApplier.Apply(stored, columns, widths, personWidths, hiddenKeys, forcedShownKeys) is not { } applied)
        {
            return false;
        }

        columnOrder = [.. applied.Order];
        frozenWish = applied.Frozen;
        keeper.Resume(applied.Seen);
        if (applied.Density is { } kept && kept != density)
        {
            density = kept;
            return true;
        }

        return false;
    }

    private void ScheduleSave()
    {
        if (layoutKey is null || layoutStore is null)
        {
            return;
        }

        keeper.Changed();
        saveTimer ??= DispatcherQueue.CreateTimer();
        saveTimer.Stop();
        saveTimer.Interval = TimeSpan.FromMilliseconds(400);
        saveTimer.IsRepeating = false;
        saveTimer.Tick -= OnSaveTimer;
        saveTimer.Tick += OnSaveTimer;
        saveTimer.Start();
    }

    private void OnSaveTimer(DispatcherQueueTimer sender, object args) => SaveLayoutNow();

    /// <summary>Writes what a person changed of the layout now, when there is a key and a store and the layout was read.</summary>
    internal void SaveLayoutNow()
    {
        saveTimer?.Stop();
        if (layoutKey is not { } key || layoutStore is not { } store || Model is not { } model)
        {
            return;
        }

        keeper.TrySave(store, key, count =>
        {
            Dictionary<string, StoredColumn> columns = [];
            foreach (DataTableColumn column in model.Columns)
            {
                double? width = personWidths.Contains(column.Key) && widths.TryGetValue(column.Key, out double w) ? w : null;
                bool? hidden = hiddenKeys.Contains(column.Key) ? true : forcedShownKeys.Contains(column.Key) ? false : null;
                if (width is not null || hidden is not null)
                {
                    columns[column.Key] = new StoredColumn(column.Kind.ToString().ToLowerInvariant(), width, hidden, count);
                }
            }

            int frozen = frozenWish ?? ColumnSet.DefaultFrozen(shown.Select(s => new ColInfo(s.Column.Key, s.Column.Kind, s.Column.Role, s.Column.Secondary)).ToList());
            return new StoredLayout(LayoutText.CurrentVersion, density.ToString().ToLowerInvariant(), frozen, FullOrder(), columns);
        });
    }
}
