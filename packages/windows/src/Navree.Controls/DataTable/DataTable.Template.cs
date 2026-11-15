using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Navree.Design.Tables;
using Windows.Foundation;

namespace Navree.Controls;

/// <summary>Makes the rows the repeater asks for and keeps the ones it gives back.</summary>
internal sealed partial class RowFactory : IElementFactory
{
    private readonly Stack<DataTableRow> pool = [];

    /// <inheritdoc />
    public UIElement GetElement(ElementFactoryGetArgs args) => pool.Count > 0 ? pool.Pop() : new DataTableRow();

    /// <inheritdoc />
    public void RecycleElement(ElementFactoryRecycleArgs args)
    {
        if (args.Element is DataTableRow row)
        {
            pool.Push(row);
        }
    }
}

public sealed partial class DataTable
{
    private readonly RowsSource rows = new();
    private readonly DataTableRowsLayout rowsLayout = new();
    private readonly RowFactory factory = new();
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

    /// <summary>How the header and the frozen columns are kept in place; the way a run chose.</summary>
    internal static StickyMode StickyWay { get; set; } = StickyMode.Compositor;

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
        Diag.Guard("the table's template", WireTemplate);
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
        sticky = new StickyCoordinator(scroll) { Mode = StickyWay };
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

    private void OnLoaded() => Diag.Guard("the table loaded", () =>
    {
        loaded = true;
        AttachSticky();
        RecomputeMetrics();
        RebuildColumns(remeasure: true);
    });

    private void OnUnloaded()
    {
        loaded = false;
        SaveLayoutNow();
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

    private void OnViewportChanged() => Diag.Guard("the viewport changed", () =>
    {
        if (!loaded || scroll is null)
        {
            return;
        }

        RebuildColumns(remeasure: false);
    });

    private void OnScrolled()
    {
        if (scroll is null)
        {
            return;
        }

        CheckCursors();
    }

    private void OnRowPrepared(ItemsRepeater sender, ItemsRepeaterElementPreparedEventArgs args) => Diag.Guard("a row was prepared", () =>
    {
        if (args.Element is DataTableRow row)
        {
            liveRows.Add(row);
            row.Bind(this, args.Index);
        }
    });

    private void OnRowClearing(ItemsRepeater sender, ItemsRepeaterElementClearingEventArgs args)
    {
        if (args.Element is DataTableRow row)
        {
            row.Unbind();
            liveRows.Remove(row);
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
