using Microsoft.UI.Composition;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.UI;

namespace Navree.Gallery;

/// <summary>
/// Bars in a container that flows right to left, moved by a written translation, by an animated
/// one, and cut by a written and by an animated clip, so a picture shows whether translations and
/// clips are counted from the start or from the left in that flow.
/// </summary>
internal static class MirrorScenario
{
    internal static async Task RunAsync(MainWindow window)
    {
        Report.Line("== mirror");
        Canvas canvas = new() { Width = 700, Height = 160 };
        Grid host = new() { Width = 700, Height = 160, FlowDirection = FlowDirection.RightToLeft, Margin = new Thickness(100, 40, 0, 0), HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Top };
        host.Children.Add(canvas);

        Rectangle Bar(byte r, byte g, byte b, double width, double top)
        {
            Rectangle bar = new() { Width = width, Height = 20, Fill = new SolidColorBrush(Color.FromArgb(0xFF, r, g, b)) };
            Canvas.SetLeft(bar, 0);
            Canvas.SetTop(bar, top);
            ElementCompositionPreview.SetIsTranslationEnabled(bar, true);
            canvas.Children.Add(bar);
            return bar;
        }

        Rectangle written = Bar(255, 0, 0, 40, 0);
        Rectangle animated = Bar(0, 0, 255, 40, 30);
        Rectangle clipped = Bar(0, 200, 0, 300, 60);
        Rectangle clippedByAnimation = Bar(255, 128, 0, 300, 90);
        Bar(150, 0, 150, 40, 120);
        window.StageHost.Child = host;
        await Waiting.LoadedAsync(host);
        await Task.Delay(800);

        Compositor compositor = ElementCompositionPreview.GetElementVisual(host).Compositor;
        CompositionPropertySet source = compositor.CreatePropertySet();
        source.InsertScalar("V", 100f);

        written.Translation = new System.Numerics.Vector3(100, 0, 0);

        ExpressionAnimation move = compositor.CreateExpressionAnimation("source.V");
        move.SetReferenceParameter("source", source);
        ElementCompositionPreview.GetElementVisual(animated).StartAnimation("Translation.X", move);

        InsetClip staticClip = compositor.CreateInsetClip();
        staticClip.LeftInset = 100;
        ElementCompositionPreview.GetElementVisual(clipped).Clip = staticClip;

        InsetClip movingClip = compositor.CreateInsetClip();
        ExpressionAnimation inset = compositor.CreateExpressionAnimation("source.V");
        inset.SetReferenceParameter("source", source);
        movingClip.StartAnimation("LeftInset", inset);
        ElementCompositionPreview.GetElementVisual(clippedByAnimation).Clip = movingClip;

        await Task.Delay(800);
        Report.Line("[mirror] red written +100, blue animated +100, green clipped 100 from the left, orange clipped by an animation of 100 from the left");
        await Handshake.StepAsync("mirror-rtl", "bars in a right to left container");
    }
}
