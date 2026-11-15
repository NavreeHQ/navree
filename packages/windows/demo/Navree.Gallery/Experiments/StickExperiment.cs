using System.Globalization;
using System.Numerics;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Navree.Controls;
using Navree.Design.Tables;
using Windows.Foundation;

namespace Navree.Gallery;

/// <summary>
/// A header and rows of marked parts in a scroll view that scrolls both ways, with the parts that
/// stay in place moved by the coordinator the table uses. Pictures of it at several scroll
/// positions show whether they stayed.
/// </summary>
internal sealed class StickPage : Grid
{
    internal const double RowHeight = 32;
    internal const double FrozenStartWidth = 200;
    internal const double FrozenEndWidth = 120;
    internal const double ScrollingWidth = 3000;

    private readonly ProbeRow header = new();
    private readonly Border headerHost = new();
    private readonly ProbeFactory factory = new();
    private readonly Border edgeStart = new();
    private readonly Border edgeEnd = new();
    private readonly bool rtl;

    internal StickPage(StickyMode mode, bool rightToLeft)
    {
        rtl = rightToLeft;
        FlowDirection = rtl ? FlowDirection.RightToLeft : FlowDirection.LeftToRight;
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        headerHost.Height = RowHeight;
        headerHost.Child = header;
        header.HorizontalAlignment = HorizontalAlignment.Left;
        headerHost.SizeChanged += (_, e) =>
            headerHost.Clip = new RectangleGeometry { Rect = new Rect(0, 0, e.NewSize.Width, e.NewSize.Height) };
        Children.Add(headerHost);

        Scroll.ContentOrientation = ScrollingContentOrientation.Both;
        Layout.RowWidth = RowWidth;
        Layout.Geometry = new Geometry(RowHeight, []);
        Repeater.Layout = Layout;
        Repeater.ItemTemplate = factory;
        Repeater.ItemsSource = Source;
        Repeater.VerticalCacheLength = 2.0;
        Repeater.ElementPrepared += OnPrepared;
        Repeater.ElementClearing += OnClearing;
        Scroll.Content = Repeater;

        Grid body = new();
        body.Children.Add(Scroll);
        edgeStart.Width = 3;
        edgeStart.HorizontalAlignment = HorizontalAlignment.Left;
        edgeStart.Margin = new Thickness(FrozenStartWidth, 0, 0, 0);
        edgeStart.IsHitTestVisible = false;
        edgeStart.Background = new Microsoft.UI.Xaml.Media.SolidColorBrush(Windows.UI.Color.FromArgb(0xFF, 0, 0, 0));
        edgeEnd.Width = 3;
        edgeEnd.HorizontalAlignment = HorizontalAlignment.Right;
        edgeEnd.Margin = new Thickness(0, 0, FrozenEndWidth, 0);
        edgeEnd.IsHitTestVisible = false;
        edgeEnd.Background = new Microsoft.UI.Xaml.Media.SolidColorBrush(Windows.UI.Color.FromArgb(0xFF, 0, 0, 0));
        body.Children.Add(edgeStart);
        body.Children.Add(edgeEnd);
        SetRow(body, 1);
        Children.Add(body);

        Sticky = new StickyCoordinator(Scroll) { Mode = mode };
        Source.Reset(Enumerable.Range(0, 300).Select(i => new RowHandle("r" + i.ToString(CultureInfo.InvariantCulture))));
        Loaded += (_, _) => OnLoaded();
    }

    internal static double RowWidth => FrozenStartWidth + ScrollingWidth + FrozenEndWidth;

    internal ScrollView Scroll { get; } = new();

    internal ItemsRepeater Repeater { get; } = new();

    internal DataTableRowsLayout Layout { get; } = new();

    internal RowsSource Source { get; } = new();

    internal StickyCoordinator Sticky { get; }

    internal Border HeaderHost => headerHost;

    private double Viewport => Math.Max(Scroll.ViewportWidth, 960);

    private void OnLoaded()
    {
        header.Build(-1, true, RowWidth, Viewport, FrozenStartWidth, FrozenEndWidth, RowHeight);
        Sticky.Register(header, StickyRole.Header);
        Sticky.Register(header.Chrome, StickyRole.Start);
        Sticky.Register(header.FrozenStart, StickyRole.Start);
        Sticky.Register(header.FrozenEnd, StickyRole.End);
        Sticky.Register(header.ScrollCells, StickyRole.ScrollCells);
        Sticky.Register(edgeStart, StickyRole.EdgeStart);
        Sticky.Register(edgeEnd, StickyRole.EdgeEnd);
        Sticky.Attach(rtl);
        Sticky.SetFrozenWidths(FrozenStartWidth, FrozenEndWidth);
    }

    private void OnPrepared(ItemsRepeater sender, ItemsRepeaterElementPreparedEventArgs args)
    {
        ProbeRow row = (ProbeRow)args.Element;
        row.Build(args.Index, false, RowWidth, Viewport, FrozenStartWidth, FrozenEndWidth, RowHeight);
        Sticky.Register(row.Chrome, StickyRole.Start);
        Sticky.Register(row.FrozenStart, StickyRole.Start);
        Sticky.Register(row.FrozenEnd, StickyRole.End);
        Sticky.Register(row.ScrollCells, StickyRole.ScrollCells);
    }

    private void OnClearing(ItemsRepeater sender, ItemsRepeaterElementClearingEventArgs args)
    {
        ProbeRow row = (ProbeRow)args.Element;
        Sticky.Unregister(row.Chrome);
        Sticky.Unregister(row.FrozenStart);
        Sticky.Unregister(row.FrozenEnd);
        Sticky.Unregister(row.ScrollCells);
    }
}

/// <summary>Runs the pictures and measurements of the page that keeps parts in place.</summary>
internal static class StickExperiment
{
    internal static async Task RunAsync(MainWindow window, StickyMode mode, bool rtl)
    {
        string tag = mode.ToString().ToLowerInvariant() + (rtl ? "-rtl" : "-ltr");
        Report.Line("== stick " + tag);
        StickPage page = new(mode, rtl);
        window.StageHost.Child = page;
        await Waiting.LoadedAsync(page);
        await Task.Delay(1500);

        ScrollView scroll = page.Scroll;
        Report.Line($"[{tag}] extent width {scroll.ExtentWidth}, viewport width {scroll.ViewportWidth}, scrollable width {scroll.ScrollableWidth}, parts kept {page.Sticky.Count}");
        foreach (string problem in page.Sticky.Problems)
        {
            Report.Line($"[{tag}] problem: {problem}");
        }

        double reach = scroll.ScrollableWidth;
        double[] offsets = [0, 120, 400, Math.Max(0, reach - 8), reach];
        foreach (double offset in offsets)
        {
            await Waiting.ScrollToAsync(scroll, offset, 0);
            string hits = HitTests(page);
            Report.Line($"[{tag}] asked {offset:0}, horizontal offset {scroll.HorizontalOffset:0.##}, rows made {page.Layout.LastRealized.Count}; {hits}");
            await Handshake.StepAsync($"stick-{tag}-{offset:00000}", $"offset={offset:0} horizontalOffset={scroll.HorizontalOffset:0.##}");
        }

        await Waiting.ScrollToAsync(scroll, 0, 0);
        scroll.AddScrollVelocity(new Vector2(3000, 0), new Vector2(0.99f, 0.99f));
        await Task.Delay(60);
        Report.Line($"[{tag}] moving: horizontal offset {scroll.HorizontalOffset:0.##}");
        await Handshake.StepAsync($"stick-{tag}-moving", $"horizontalOffset={scroll.HorizontalOffset:0.##}");
        await Task.Delay(1500);
        Report.Line($"[{tag}] after the motion: horizontal offset {scroll.HorizontalOffset:0.##}; {HitTests(page)}");
    }

    // What the XAML hit test finds where a frozen cell, a scrolling cell and the end cell are drawn.
    // In right to left it is tried both with places counted from the start and from the left.
    private static string HitTests(StickPage page)
    {
        bool rtl = page.FlowDirection == FlowDirection.RightToLeft;
        string logical = HitTestsAt(page, false);
        return rtl ? logical + " | counted from the left: " + HitTestsAt(page, true) : logical;
    }

    private static string HitTestsAt(StickPage page, bool fromLeft)
    {
        UIElement root = page.XamlRoot.Content;
        double viewport = Math.Max(page.Scroll.ViewportWidth, 960);
        GeneralTransform toRoot = page.Scroll.TransformToVisual(root);
        GeneralTransform headerToRoot = page.HeaderHost.TransformToVisual(root);
        double y = StickPage.RowHeight / 2;
        double startX = StickPage.FrozenStartWidth - 20;
        double scrollX = StickPage.FrozenStartWidth + 30;
        double endX = viewport - 30;
        double Place(double x) => fromLeft ? viewport - x : x;

        string One(string name, GeneralTransform transform, double x) =>
            name + " -> '" + FirstTag(VisualTreeHelper.FindElementsInHostCoordinates(transform.TransformPoint(new Point(Place(x), y)), root)) + "'";

        return string.Join(
            "; ",
            One("frozen cell (want start)", toRoot, startX),
            One("scrolling cell (want scroll)", toRoot, scrollX),
            One("end cell (want end)", toRoot, endX),
            One("header's frozen cell (want start)", headerToRoot, startX));
    }

    private static string FirstTag(IEnumerable<UIElement> hits)
    {
        foreach (UIElement hit in hits)
        {
            if (hit is FrameworkElement { Tag: string tag })
            {
                return tag;
            }
        }

        return "nothing tagged";
    }
}

/// <summary>Small waits the experiments share.</summary>
internal static class Waiting
{
    internal static async Task LoadedAsync(FrameworkElement element)
    {
        if (element.IsLoaded)
        {
            return;
        }

        TaskCompletionSource done = new();
        void Handler(object sender, RoutedEventArgs e) => done.TrySetResult();
        element.Loaded += Handler;
        await done.Task;
        element.Loaded -= Handler;
    }

    internal static async Task ScrollToAsync(ScrollView scroll, double x, double y)
    {
        TaskCompletionSource done = new();
        void Handler(ScrollView sender, ScrollingScrollCompletedEventArgs e) => done.TrySetResult();
        scroll.ScrollCompleted += Handler;
        scroll.ScrollTo(x, y, new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));
        await Task.WhenAny(done.Task, Task.Delay(1500));
        scroll.ScrollCompleted -= Handler;
        await Task.Delay(500);
    }
}
