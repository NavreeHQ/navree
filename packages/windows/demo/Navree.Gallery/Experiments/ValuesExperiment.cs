using System.Globalization;
using Microsoft.UI.Composition;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.UI;

namespace Navree.Gallery;

/// <summary>
/// A scroll view with a bar beside it for each of the numbers it gives to animations: each bar is
/// moved from the same place by its number times a scale, so a picture shows the numbers, with
/// their signs, in left to right and in right to left.
/// </summary>
internal sealed class ValuesPage : Grid
{
    internal const double Base = 500;

    private static readonly (string Name, byte R, byte G, byte B, string Source, float Scale)[] Markers =
    [
        ("Position", 255, 0, 0, "scroll.Position.X", 0.2f),
        ("MinPosition", 0, 200, 0, "scroll.MinPosition.X", 0.2f),
        ("MaxPosition", 0, 0, 255, "scroll.MaxPosition.X", 0.2f),
        ("Offset", 255, 128, 0, "scroll.Offset.X", 0.2f),
        ("Extent", 0, 255, 255, "scroll.Extent.X", 0.1f),
        ("Viewport", 255, 0, 255, "scroll.Viewport.X", 0.1f),
    ];

    private readonly Canvas overlay = new() { Height = 100, FlowDirection = FlowDirection.LeftToRight };
    private readonly List<Rectangle> bars = [];

    internal ValuesPage(bool rightToLeft)
    {
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        Children.Add(overlay);

        Canvas content = new() { Width = 3320, Height = 300 };
        for (int x = 0; x < 3320; x += 200)
        {
            Rectangle stripe = new() { Width = 100, Height = 300, Fill = new SolidColorBrush(Color.FromArgb(0xFF, 0xB0, 0xB0, 0xB0)) };
            Canvas.SetLeft(stripe, x);
            content.Children.Add(stripe);
            TextBlock label = new() { Text = x.ToString(CultureInfo.InvariantCulture), FontSize = 14 };
            Canvas.SetLeft(label, x + 8);
            Canvas.SetTop(label, 8);
            content.Children.Add(label);
        }

        Scroll.ContentOrientation = ScrollingContentOrientation.Both;
        Scroll.FlowDirection = rightToLeft ? FlowDirection.RightToLeft : FlowDirection.LeftToRight;
        Scroll.Content = content;
        SetRow(Scroll, 1);
        Children.Add(Scroll);

        for (int i = 0; i < Markers.Length; i++)
        {
            Rectangle bar = new()
            {
                Width = 4,
                Height = 12,
                Fill = new SolidColorBrush(Color.FromArgb(0xFF, Markers[i].R, Markers[i].G, Markers[i].B)),
            };
            Canvas.SetLeft(bar, Base);
            Canvas.SetTop(bar, i * 15);
            ElementCompositionPreview.SetIsTranslationEnabled(bar, true);
            overlay.Children.Add(bar);
            bars.Add(bar);
        }
    }

    internal ScrollView Scroll { get; } = new();

    /// <summary>Starts the bars; false when the scroll view has no property set yet.</summary>
    internal bool Start()
    {
        CompositionPropertySet? sources = Scroll.ExpressionAnimationSources;
        if (sources is null)
        {
            return false;
        }

        Compositor compositor = ElementCompositionPreview.GetElementVisual(Scroll).Compositor;
        for (int i = 0; i < Markers.Length; i++)
        {
            ExpressionAnimation animation = compositor.CreateExpressionAnimation(
                string.Create(CultureInfo.InvariantCulture, $"{Markers[i].Source} * {Markers[i].Scale}"));
            animation.SetReferenceParameter("scroll", sources);
            ElementCompositionPreview.GetElementVisual(bars[i]).StartAnimation("Translation.X", animation);
        }

        return true;
    }
}

/// <summary>Pictures of the numbers a scroll view gives to animations, at three scroll positions, in both flow directions.</summary>
internal static class ValuesExperiment
{
    internal static async Task RunAsync(MainWindow window)
    {
        foreach (bool rtl in new[] { false, true })
        {
            string tag = rtl ? "rtl" : "ltr";
            Report.Line("== values " + tag);
            ValuesPage page = new(rtl);
            window.StageHost.Child = page;
            await Waiting.LoadedAsync(page);
            await Task.Delay(1200);

            ScrollView scroll = page.Scroll;
            bool started = page.Start();
            Report.Line($"[values-{tag}] bars started: {started}; extent width {scroll.ExtentWidth}, viewport width {scroll.ViewportWidth}, scrollable width {scroll.ScrollableWidth}");
            double reach = scroll.ScrollableWidth;
            foreach (double offset in new[] { 0, 400, reach })
            {
                await Waiting.ScrollToAsync(scroll, offset, 0);
                Report.Line($"[values-{tag}] asked {offset:0}, horizontal offset {scroll.HorizontalOffset:0.##}");
                await Handshake.StepAsync($"values-{tag}-{offset:00000}", $"asked={offset:0} horizontalOffset={scroll.HorizontalOffset:0.##}");
            }
        }
    }
}
