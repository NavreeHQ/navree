using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Navree.Design.Tables;
using Windows.Foundation;

namespace Navree.Controls;

/// <summary>Makes the rows the repeater asks for and keeps the ones it gives back.</summary>
internal sealed partial class RowFactory : IElementFactory
{
    /// <summary>The most rows kept for the next ones: more than a view and its cache hold, so scrolling takes from the pool and does not make rows.</summary>
    internal const int MostKept = 96;

    private readonly ElementPool<DataTableRow> pool;

    /// <summary>Makes a factory.</summary>
    /// <param name="dropped">Told of a row that is let go for good: it is not kept for the next ones.</param>
    internal RowFactory(Action<DataTableRow> dropped)
    {
        pool = new ElementPool<DataTableRow>(MostKept, dropped);
    }

    /// <inheritdoc />
    public UIElement GetElement(ElementFactoryGetArgs args) => pool.Take() ?? new DataTableRow();

    /// <inheritdoc />
    public void RecycleElement(ElementFactoryRecycleArgs args)
    {
        if (args.Element is DataTableRow row)
        {
            pool.Give(row);
        }
    }
}

public sealed partial class DataTable
{
    private readonly RowsSource rows = new();
    private readonly DataTableRowsLayout rowsLayout = new();
    private readonly RowFactory factory;
    private readonly TextMeasurer measurer = new();
    private Border? headerHost;
    private DataTableRow? header;
    private ScrollView? scroll;
    private ItemsRepeater? repeater;
    private Border? edgeStart;
    private Border? edgeEnd;
    private StickyCoordinator? sticky;
    private bool loaded;
    private bool templateWired;
    private bool stickyAttached;
    private readonly MetricsWatch metricsWatch = new();
    private XamlRoot? watchedRoot;
    private Microsoft.UI.System.ThemeSettings? themeSettings;

    /// <summary>The scroll view the rows are in; null before the template is made.</summary>
    internal ScrollView? Scroller => scroll;

    /// <summary>The repeater that holds the rows.</summary>
    internal ItemsRepeater? RowsRepeater => repeater;

    /// <summary>The table's rows layout.</summary>
    internal DataTableRowsLayout RowsLayout => rowsLayout;

    /// <summary>The rows list the repeater reads.</summary>
    internal RowsSource RowsList => rows;

    /// <summary>The row of headings.</summary>
    internal DataTableRow? HeaderRow => header;

    /// <inheritdoc />
    protected override void OnApplyTemplate()
    {
        base.OnApplyTemplate();
        WireTemplate();
    }

    private void WireTemplate()
    {
        headerHost = GetTemplateChild("PART_HeaderHost") as Border;
        scroll = GetTemplateChild("PART_Scroll") as ScrollView;
        repeater = GetTemplateChild("PART_Rows") as ItemsRepeater;
        edgeStart = GetTemplateChild("PART_EdgeStart") as Border;
        edgeEnd = GetTemplateChild("PART_EdgeEnd") as Border;
        WireStateParts();
        WireBulkBar();
        WireListParts();
        if (scroll is null || repeater is null || headerHost is null || templateWired)
        {
            return;
        }

        templateWired = true;
        repeater.Layout = rowsLayout;
        repeater.ItemTemplate = factory;
        repeater.ItemsSource = rows;
        repeater.VerticalCacheLength = 2.0;
        repeater.ElementPrepared += OnRowPrepared;
        repeater.ElementClearing += OnRowClearing;
        repeater.ElementIndexChanged += OnRowIndexChanged;
        rowsLayout.DetailMeasured += OnDetailMeasured;
        scroll.ViewChanged += (_, _) => OnScrolled();
        scroll.ExtentChanged += (_, _) => OnScrolled();
        scroll.SizeChanged += (_, _) => OnViewportChanged();
        Border host = headerHost;
        host.SizeChanged += (_, e) =>
            host.Clip = new RectangleGeometry { Rect = new Rect(0, 0, e.NewSize.Width, e.NewSize.Height) };

        header = new DataTableRow { Owner = this, IsHeader = true };

        // On a canvas the row is arranged at the width it asks for, so the slot it is given cuts none of it off
        // when it is moved against the scroll; the host's own clip is what crops it to the viewport.
        Canvas strip = new();
        strip.Children.Add(header);
        headerHost.Child = strip;
        sticky = new StickyCoordinator(scroll);
        if (edgeStart is not null)
        {
            sticky.Register(edgeStart, StickyRole.EdgeStart);
        }

        if (edgeEnd is not null)
        {
            sticky.Register(edgeEnd, StickyRole.EdgeEnd);
        }

        sticky.Register(header, StickyRole.Header);
        if (loaded)
        {
            AttachSticky();
        }

        ModelAttached();
    }

    // A row let go for good keeps nothing the coordinator holds: its layers are no longer moved against the scroll.
    private void RowDropped(DataTableRow row)
    {
        if (sticky is null)
        {
            return;
        }

        foreach (UIElement layer in row.StickyLayers())
        {
            sticky.Unregister(layer);
        }
    }

    /// <summary>Called by a row when it has its layers: they are given to the coordinator once.</summary>
    /// <param name="row">The row.</param>
    internal void RowPartsMade(DataTableRow row)
    {
        if (sticky is null || row.ChromeLayer is null || row.FrozenStart is null || row.FrozenEnd is null || row.ScrollCells is null)
        {
            return;
        }

        sticky.Register(row.ChromeLayer, StickyRole.Start);
        sticky.Register(row.FrozenStart, StickyRole.Start);
        sticky.Register(row.FrozenEnd, StickyRole.End);
        sticky.Register(row.ScrollCells, StickyRole.ScrollCells);
        if (row.DetailLayer is not null)
        {
            sticky.Register(row.DetailLayer, StickyRole.Start);
        }
    }

    // The table listens to its model only while it is in the tree, and runs nothing while it is out of it.
    private void OnLoaded()
    {
        loaded = true;
        modelLink.Attach(Model, OnModelEvent);
        WatchMetrics();
        AttachSticky();
        RecomputeMetrics();
        RebuildColumns(remeasure: true);
        if (templateWired && Model is { } model && StampOf(model) != modelStamp)
        {
            // The model changed while the table was out of the tree, and nothing told it.
            ModelAttached();
        }
    }

    private void OnUnloaded()
    {
        loaded = false;
        modelLink.Detach();
        UnwatchMetrics();
        StopTimers();
        SaveLayoutNow();
    }

    // The widths of columns and the height of a row follow the text size and the display's scale while the table is in the tree.
    private void WatchMetrics()
    {
        metricsWatch.Changed(CurrentTextScale(), XamlRoot?.RasterizationScale ?? 1.0);
        try
        {
            if (uiSettings is not null)
            {
                uiSettings.TextScaleFactorChanged -= OnTextScaleChanged;
                uiSettings.TextScaleFactorChanged += OnTextScaleChanged;
            }
        }
        catch (Exception)
        {
            // The setting cannot be watched here; the metrics are read again at the next reset.
        }

        // The contrast colours are heard of through the settings of the window: the event of the settings class that needs a core window
        // is not raised for a desktop window.
        if (themeSettings is null && XamlRoot?.ContentIslandEnvironment is { } environment)
        {
            themeSettings = Microsoft.UI.System.ThemeSettings.CreateForWindowId(environment.AppWindowId);
            themeSettings.Changed += OnHighContrastChanged;
        }

        if (XamlRoot is { } root)
        {
            root.Changed -= OnXamlRootChanged;
            root.Changed += OnXamlRootChanged;
            watchedRoot = root;
        }
    }

    private void UnwatchMetrics()
    {
        if (uiSettings is not null)
        {
            uiSettings.TextScaleFactorChanged -= OnTextScaleChanged;
        }

        if (themeSettings is not null)
        {
            themeSettings.Changed -= OnHighContrastChanged;
            themeSettings = null;
        }

        if (watchedRoot is not null)
        {
            watchedRoot.Changed -= OnXamlRootChanged;
            watchedRoot = null;
        }
    }

    // The text scale changes on a thread of the system's own.
    private void OnTextScaleChanged(Windows.UI.ViewManagement.UISettings sender, object args) =>
        DispatcherQueue.TryEnqueue(MetricsMayHaveChanged);

    // The contrast colours were turned on or off while the table is in the tree: every part is drawn in the state it is in again.
    private void OnHighContrastChanged(Microsoft.UI.System.ThemeSettings sender, object args) =>
        DispatcherQueue.TryEnqueue(() =>
        {
            if (loaded && templateWired)
            {
                RefreshAllRows(numbersOnly: false);
            }
        });

    private void OnXamlRootChanged(XamlRoot sender, XamlRootChangedEventArgs args) => MetricsMayHaveChanged();

    private void MetricsMayHaveChanged()
    {
        if (loaded && templateWired && metricsWatch.Changed(CurrentTextScale(), XamlRoot?.RasterizationScale ?? 1.0))
        {
            RecomputeMetrics();
            RebuildColumns(remeasure: true);
            RefreshAllRows(numbersOnly: false);
        }
    }

    // Every timer the table started stops, and what they would have said or let go of is dropped or done now.
    private void StopTimers()
    {
        choiceTimer?.Stop();
        addedTimer?.Stop();
        structureTimer?.Stop();
        edgeTimer?.Stop();
        anchorPinTimer?.Stop();
        assistivePinTimer?.Stop();
        addedLater = 0;
        addedEarlier = 0;
        pins.Release(PinOwner.Anchor);
        pins.Release(PinOwner.Assistive);
        rowsLayout.Pinned = pins.Indices;
    }

    private void AttachSticky()
    {
        if (sticky is null || scroll is null || stickyAttached)
        {
            return;
        }

        stickyAttached = true;
        sticky.Attach(FlowDirection == FlowDirection.RightToLeft);
        if (CurrentLayout is { } layout)
        {
            sticky.SetFrozenWidths(layout.Fs, layout.Fe);
        }
    }

    private void OnSizeChanged()
    {
        if (!loaded)
        {
            return;
        }

        OnViewportChanged();
    }

    private void OnViewportChanged()
    {
        if (!loaded || scroll is null)
        {
            return;
        }

        RebuildColumns(remeasure: false);
    }

    private void OnScrolled()
    {
        if (scroll is null)
        {
            return;
        }

        CheckCursors();
    }

    private void OnRowPrepared(ItemsRepeater sender, ItemsRepeaterElementPreparedEventArgs args)
    {
        if (args.Element is DataTableRow row)
        {
            liveRows.Add(row);
            row.Bind(this, args.Index);
            QueueStructureEvent();
        }
    }

    private void OnRowClearing(ItemsRepeater sender, ItemsRepeaterElementClearingEventArgs args)
    {
        if (args.Element is DataTableRow row)
        {
            // The active row is pinned, so it is not recycled to make room; when it is cleared with focus on it, which a
            // reset or a row drawn again can do, focus is put back on the active cell once the rows are in place.
            bool heldFocus = row.Index >= 0 && row.Index == activeRow && FocusIsInside(row);
            row.Unbind();
            liveRows.Remove(row);
            QueueStructureEvent();
            if (heldFocus)
            {
                FocusActiveSoon();
            }
        }
    }

    private void OnRowIndexChanged(ItemsRepeater sender, ItemsRepeaterElementIndexChangedEventArgs args)
    {
        if (args.Element is DataTableRow row)
        {
            row.MoveTo(args.NewIndex);
        }
    }

    /// <summary>The widest a row is: the viewport's width, or the width of the table before it has a viewport.</summary>
    private double Viewport
    {
        get
        {
            double width = scroll?.ViewportWidth ?? 0;
            return width > 0 ? width : Math.Max(0, ActualWidth - 2);
        }
    }
}
