using System.Globalization;
using Microsoft.UI.Composition;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.Foundation;
using Windows.UI;

namespace Navree.Gallery;

/// <summary>
/// Bars that are moved sideways by a number written to the element, by an animation on the
/// compositor, and by both in different orders, so a picture shows what the two do together and
/// what is left when the animation is stopped.
/// </summary>
internal sealed class TranslationPage : Grid
{
    internal const double Base = 100;

    private static readonly (string Name, byte R, byte G, byte B)[] Bars =
    [
        ("A written 50", 255, 0, 0),
        ("B written 50 then animated to 80", 0, 200, 0),
        ("C animated to 80 then written 50", 0, 0, 255),
        ("D animated to 80, written 50, animation stopped", 255, 128, 0),
        ("E written 50, animated to 80, animation stopped", 0, 255, 255),
        ("F animated to 80", 255, 0, 255),
    ];

    private readonly Canvas canvas = new() { Height = 220, FlowDirection = FlowDirection.LeftToRight };
    private readonly Rectangle[] rectangles = new Rectangle[Bars.Length];

    internal TranslationPage()
    {
        Children.Add(canvas);
        for (int i = 0; i < Bars.Length; i++)
        {
            Rectangle bar = new()
            {
                Width = 12,
                Height = 24,
                Tag = Bars[i].Name,
                Fill = new SolidColorBrush(Color.FromArgb(0xFF, Bars[i].R, Bars[i].G, Bars[i].B)),
            };
            Canvas.SetLeft(bar, Base);
            Canvas.SetTop(bar, i * 32);
            ElementCompositionPreview.SetIsTranslationEnabled(bar, true);
            canvas.Children.Add(bar);
            rectangles[i] = bar;
        }
    }

    internal Canvas Surface => canvas;

    internal Rectangle Bar(int index) => rectangles[index];

    internal static void Animate(Rectangle bar, CompositionPropertySet source)
    {
        Compositor compositor = ElementCompositionPreview.GetElementVisual(bar).Compositor;
        ExpressionAnimation animation = compositor.CreateExpressionAnimation("source.V");
        animation.SetReferenceParameter("source", source);
        ElementCompositionPreview.GetElementVisual(bar).StartAnimation("Translation.X", animation);
    }

    internal static void StopAnimation(Rectangle bar) =>
        ElementCompositionPreview.GetElementVisual(bar).StopAnimation("Translation.X");
}

/// <summary>Pictures of what a written translation and an animated one do to the same element.</summary>
internal static class TranslationScenario
{
    internal static async Task RunAsync(MainWindow window)
    {
        Report.Line("== translation");
        TranslationPage page = new();
        window.StageHost.Child = page;
        await Waiting.LoadedAsync(page);
        await Task.Delay(800);

        Compositor compositor = ElementCompositionPreview.GetElementVisual(page).Compositor;
        CompositionPropertySet source = compositor.CreatePropertySet();
        source.InsertScalar("V", 80f);

        page.Bar(0).Translation = new System.Numerics.Vector3(50, 0, 0);
        page.Bar(1).Translation = new System.Numerics.Vector3(50, 0, 0);
        TranslationPage.Animate(page.Bar(1), source);
        TranslationPage.Animate(page.Bar(2), source);
        TranslationPage.Animate(page.Bar(3), source);
        page.Bar(4).Translation = new System.Numerics.Vector3(50, 0, 0);
        TranslationPage.Animate(page.Bar(4), source);
        TranslationPage.Animate(page.Bar(5), source);
        await Task.Delay(500);
        page.Bar(2).Translation = new System.Numerics.Vector3(50, 0, 0);
        page.Bar(3).Translation = new System.Numerics.Vector3(50, 0, 0);
        await Task.Delay(500);
        Report.Line("[translation] all animations running; " + Hits(page));
        await Handshake.StepAsync("trans-1-running", "written values and animations together");

        TranslationPage.StopAnimation(page.Bar(3));
        TranslationPage.StopAnimation(page.Bar(4));
        await Task.Delay(500);
        Report.Line("[translation] D and E stopped; " + Hits(page));
        await Handshake.StepAsync("trans-2-stopped", "two animations stopped");

        // The way the table's rest and motion states would do it: write nothing while animating, and
        // write the number, then stop, when the motion ends.
        page.Bar(3).Translation = new System.Numerics.Vector3(0, 0, 0);
        TranslationPage.Animate(page.Bar(3), source);
        source.InsertScalar("V", 120f);
        await Task.Delay(500);
        Report.Line("[translation] D animated again to 120 from a written 0; " + Hits(page));
        await Handshake.StepAsync("trans-3-again", "D animated again to 120 from a written 0");
        page.Bar(3).Translation = new System.Numerics.Vector3(120, 0, 0);
        TranslationPage.StopAnimation(page.Bar(3));
        await Task.Delay(500);
        Report.Line("[translation] D written 120 then stopped; " + Hits(page));
        await Handshake.StepAsync("trans-4-handover", "D written 120 then stopped");
    }

    // Where the UI thread puts each bar, and which bars a hit finds at three places.
    private static string Hits(TranslationPage page)
    {
        UIElement root = page.XamlRoot.Content;
        List<string> parts = [];
        for (int i = 0; i < 6; i++)
        {
            Rectangle bar = page.Bar(i);
            Point at = bar.TransformToVisual(root).TransformPoint(new Point(0, 0));
            parts.Add($"{(char)('A' + i)} at x {at.X:0.#} (written {bar.Translation.X:0.#})");
        }

        foreach (double x in new[] { 106.0, 156.0, 186.0, 206.0, 226.0, 236.0 })
        {
            List<string> found = [];
            for (int i = 0; i < 6; i++)
            {
                Rectangle bar = page.Bar(i);
                Point centre = bar.TransformToVisual(root).TransformPoint(new Point(6, 12));
                Point probe = new(x, centre.Y);
                if (VisualTreeHelper.FindElementsInHostCoordinates(probe, root).Contains(bar))
                {
                    found.Add(((char)('A' + i)).ToString());
                }
            }

            parts.Add($"hit at {x.ToString("0", CultureInfo.InvariantCulture)}: {(found.Count == 0 ? "none" : string.Join(string.Empty, found))}");
        }

        return string.Join("; ", parts);
    }
}
