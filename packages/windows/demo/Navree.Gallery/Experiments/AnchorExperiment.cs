using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Navree.Controls;
using Navree.Design.Tables;
using Geometry = Navree.Design.Tables.Geometry;
using Windows.Foundation;

namespace Navree.Gallery;

/// <summary>Makes plain rows of text for a repeater and keeps the ones it gives back.</summary>
internal sealed partial class AnchorFactory : IElementFactory
{
    private readonly Stack<Border> pool = [];

    /// <inheritdoc />
    public UIElement GetElement(ElementFactoryGetArgs args)
    {
        if (pool.Count > 0)
        {
            return pool.Pop();
        }

        return new Border
        {
            BorderBrush = new SolidColorBrush(Windows.UI.Color.FromArgb(0x40, 0x80, 0x80, 0x80)),
            BorderThickness = new Thickness(0, 0, 0, 1),
            Padding = new Thickness(12, 4, 12, 0),
            Child = new TextBlock(),
        };
    }

    /// <inheritdoc />
    public void RecycleElement(ElementFactoryRecycleArgs args)
    {
        if (args.Element is Border border)
        {
            pool.Push(border);
        }
    }
}

/// <summary>A long list of plain rows in a scroll view, placed by the table's layout, whose rows can change above the view.</summary>
internal sealed class AnchorPage : Grid
{
    internal const double RowHeight = 32;

    private readonly AnchorFactory factory = new();

    internal AnchorPage()
    {
        Scroll.ContentOrientation = ScrollingContentOrientation.Vertical;
        Layout.Geometry = new Geometry(RowHeight, []);
        Repeater.Layout = Layout;
        Repeater.ItemTemplate = factory;
        Repeater.ItemsSource = Source;
        Repeater.VerticalCacheLength = 2.0;
        Repeater.ElementPrepared += (_, args) =>
        {
            Border border = (Border)args.Element;
            ((TextBlock)border.Child).Text = Source.At(args.Index).Key;
        };
        Scroll.Content = Repeater;
        Children.Add(Scroll);
        Source.Reset(Make("r", 10_000));
    }

    internal ScrollView Scroll { get; } = new();

    internal ItemsRepeater Repeater { get; } = new();

    internal DataTableRowsLayout Layout { get; } = new();

    internal RowsSource Source { get; } = new();

    internal static List<RowHandle> Make(string prefix, int count) =>
        Enumerable.Range(0, count).Select(i => new RowHandle(prefix + i.ToString(CultureInfo.InvariantCulture))).ToList();

    // Where a row's top is in the scroll view, as the UI thread knows it; not a number when the row is not made.
    internal double TopOf(string key)
    {
        int at = Source.PlaceOf(key);
        UIElement? element = at < 0 ? null : Repeater.TryGetElement(at);
        return element is null ? double.NaN : element.TransformToVisual(Scroll).TransformPoint(new Point(0, 0)).Y;
    }
}

/// <summary>Measures whether the view stays where it was when rows change above it.</summary>
internal static class AnchorExperiment
{
    internal static async Task RunAsync(MainWindow window)
    {
        Report.Line("== anchor");
        AnchorPage page = new();
        window.StageHost.Child = page;
        await Waiting.LoadedAsync(page);
        await Task.Delay(1200);

        await CaseAsync(page, "25 rows join the start", () => page.Source.Prepend(AnchorPage.Make("p", 25)), picture: true);
        await CaseAsync(page, "500 rows join the start", () => page.Source.Prepend(AnchorPage.Make("q", 500)), picture: false);
        await CaseAsync(page, "10 rows above are removed", () => page.Source.RemoveRange(0, 10), picture: false);
        await CaseAsync(
            page,
            "a row far above opens its detail (120 more)",
            () =>
            {
                page.Layout.Geometry = new Geometry(AnchorPage.RowHeight, [(100, 120)]);
                page.Layout.Reflow();
            },
            picture: false);
        await CaseAsync(
            page,
            "it shuts again",
            () =>
            {
                page.Layout.Geometry = new Geometry(AnchorPage.RowHeight, []);
                page.Layout.Reflow();
            },
            picture: false);
    }

    private static async Task CaseAsync(AnchorPage page, string name, Action change, bool picture)
    {
        ScrollView scroll = page.Scroll;
        await Waiting.ScrollToAsync(scroll, 0, (5000 * AnchorPage.RowHeight) + 10);
        int count = page.Source.Count;
        (int index, double within) = page.Layout.Geometry.IndexAt(scroll.VerticalOffset, count);
        string key = page.Source.At(index).Key;
        double before = page.TopOf(key);
        double offsetBefore = scroll.VerticalOffset;
        double extentBefore = scroll.ExtentHeight;
        if (picture)
        {
            await Handshake.StepAsync("anchor-before", $"anchor {key} at {before:0.##}, offset {offsetBefore:0.##}");
        }

        List<double> samples = [];
        void Sample(object? sender, object e) => samples.Add(page.TopOf(key));
        CompositionTarget.Rendering += Sample;
        change();
        await Task.Delay(900);
        CompositionTarget.Rendering -= Sample;

        double after = page.TopOf(key);
        double worst = samples.Where(s => !double.IsNaN(s)).Select(s => Math.Abs(s - before)).DefaultIfEmpty(double.NaN).Max();
        int missing = samples.Count(double.IsNaN);
        Report.Line(
            $"[anchor] {name}: row {key} (within {within:0.##}) stood at {before:0.##}, now at {after:0.##}; "
            + $"offset {offsetBefore:0.##} to {scroll.VerticalOffset:0.##}; extent {extentBefore:0} to {scroll.ExtentHeight:0}; "
            + $"{samples.Count} frames sampled, the row was off its place by {worst:0.##} at most, {missing} frames with the row not made");
        if (picture)
        {
            await Handshake.StepAsync("anchor-after", $"anchor {key} at {after:0.##}, offset {scroll.VerticalOffset:0.##}");
        }
    }
}
