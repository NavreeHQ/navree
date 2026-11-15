using System.Numerics;
using Microsoft.UI.Composition;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Media;
using Navree.Controls.Drawing;
using Navree.Controls.Health;
using Navree.Design;
using Navree.Protocol;
using Windows.UI.ViewManagement;
using Border = Microsoft.UI.Xaml.Controls.Border;

namespace Navree.Controls.Composition;

/// <summary>How a part comes in when an answer adds it to a screen already drawn.</summary>
public static class Arrivals
{
    /// <summary>Whether the person asked for less motion (Settings, Accessibility, Visual effects, Animation effects).</summary>
    public static bool ReduceMotion
    {
        get
        {
            try
            {
                return !new UISettings().AnimationsEnabled;
            }
            catch (Exception)
            {
                return false;
            }
        }
    }

    /// <summary>Plays the arrival of a part once it is on the screen; nothing for a part that says nothing.</summary>
    /// <param name="element">The part.</param>
    /// <param name="appear">What the page says.</param>
    public static void Play(FrameworkElement element, Models.Appear? appear)
    {
        ComposeRules.Arrival arrival = ComposeRules.ArrivalOf(appear, ReduceMotion);
        if (arrival == ComposeRules.Arrival.None)
        {
            return;
        }

        element.Loaded += Start;

        void Start(object sender, RoutedEventArgs e)
        {
            element.Loaded -= Start;
            Visual visual = ElementCompositionPreview.GetElementVisual(element);
            Compositor compositor = visual.Compositor;
            visual.Opacity = 0;
            ScalarKeyFrameAnimation fade = compositor.CreateScalarKeyFrameAnimation();
            fade.InsertKeyFrame(1f, 1f);
            fade.Duration = TimeSpan.FromMilliseconds(240);
            visual.StartAnimation("Opacity", fade);

            if (arrival == ComposeRules.Arrival.Rises)
            {
                ElementCompositionPreview.SetIsTranslationEnabled(element, true);
                visual.Properties.InsertVector3("Translation", new Vector3(0, 12, 0));
                Vector3KeyFrameAnimation rise = compositor.CreateVector3KeyFrameAnimation();
                rise.InsertKeyFrame(1f, Vector3.Zero);
                rise.Duration = TimeSpan.FromMilliseconds(240);
                visual.StartAnimation("Translation", rise);
            }
            else if (arrival == ComposeRules.Arrival.Grows)
            {
                visual.CenterPoint = new Vector3((float)element.ActualWidth / 2, (float)element.ActualHeight / 2, 0);
                visual.Scale = new Vector3(0.96f, 0.96f, 1);
                Vector3KeyFrameAnimation grow = compositor.CreateVector3KeyFrameAnimation();
                grow.InsertKeyFrame(1f, Vector3.One);
                grow.Duration = TimeSpan.FromMilliseconds(240);
                visual.StartAnimation("Scale", grow);
            }
        }
    }
}

/// <summary>The parts of the composition kinds that need no more than layout: a plain shape, parts laid over each other.</summary>
public static class Composed
{
    private static double Scaled(int? size, bool scaled) => size is int value ? value * (scaled ? TextScale.Full : 1) : double.NaN;

    /// <summary>A plain shape, filled or outlined in a colour by name: the ground behind a symbol, a dot in a key, a bar at the edge of a card.</summary>
    /// <param name="shape">The shape.</param>
    /// <param name="owner">The element it will stand in, for the theme it is drawn in.</param>
    /// <returns>The shape.</returns>
    public static UIElement Shape(Models.Shape shape, FrameworkElement? owner = null)
    {
        Palette palette = new(owner ?? new Border());
        bool scaled = shape.Size?.Scaled ?? false;
        double width = Scaled(shape.Size?.Width, scaled);
        double height = Scaled(shape.Size?.Height, scaled);
        if (shape.Form == Models.ShapeForm.Circle)
        {
            double side = double.IsNaN(width) ? height : double.IsNaN(height) ? width : Math.Min(width, height);
            width = side;
            height = side;
        }

        Border box = new()
        {
            Width = width,
            Height = height,
            MinWidth = double.IsNaN(width) ? 4 : 0,
            MinHeight = double.IsNaN(height) ? 4 : 0,
            CornerRadius = new CornerRadius(shape.Form == Models.ShapeForm.Rect ? 0 : 8),
        };

        // A capsule and a circle are round at the shorter side, whatever size the place gives them.
        box.SizeChanged += (_, e) => box.CornerRadius = new CornerRadius(ComposeRules.Corner(shape.Form, e.NewSize.Width, e.NewSize.Height));
        if (shape.Fill is not null)
        {
            Models.Colour fill = shape.Fill;
            if (shape.Soft)
            {
                box.Background = palette.Fill(fill, Models.Hue.Neutral, palette.Dark ? 0.22 : 0.14);
            }
            else if (fill.To is not null)
            {
                LinearGradientBrush blend = new() { StartPoint = new Windows.Foundation.Point(0, 0), EndPoint = new Windows.Foundation.Point(1, 1) };
                blend.GradientStops.Add(new GradientStop { Color = palette.Of(fill, Models.Hue.Neutral), Offset = 0 });
                blend.GradientStops.Add(new GradientStop { Color = palette.End(fill, Models.Hue.Neutral), Offset = 1 });
                box.Background = blend;
            }
            else
            {
                box.Background = palette.Fill(fill, Models.Hue.Neutral);
            }
        }

        if (shape.Stroke is not null)
        {
            box.BorderBrush = palette.Fill(shape.Stroke, Models.Hue.Neutral);
            box.BorderThickness = new Thickness(1);
        }

        if (shape.Label is not null)
        {
            AutomationProperties.SetName(box, shape.Label);
        }
        else
        {
            // A shape is decoration unless it says something: a colour is never the only thing that does.
            AutomationProperties.SetAccessibilityView(box, Microsoft.UI.Xaml.Automation.Peers.AccessibilityView.Raw);
        }

        return box;
    }

    private static (HorizontalAlignment Across, VerticalAlignment Down) Placed(Models.Place place) =>
        place switch
        {
            Models.Place.TopStart => (HorizontalAlignment.Left, VerticalAlignment.Top),
            Models.Place.Top => (HorizontalAlignment.Center, VerticalAlignment.Top),
            Models.Place.TopEnd => (HorizontalAlignment.Right, VerticalAlignment.Top),
            Models.Place.Start => (HorizontalAlignment.Left, VerticalAlignment.Center),
            Models.Place.End => (HorizontalAlignment.Right, VerticalAlignment.Center),
            Models.Place.BottomStart => (HorizontalAlignment.Left, VerticalAlignment.Bottom),
            Models.Place.Bottom => (HorizontalAlignment.Center, VerticalAlignment.Bottom),
            Models.Place.BottomEnd => (HorizontalAlignment.Right, VerticalAlignment.Bottom),
            _ => (HorizontalAlignment.Center, VerticalAlignment.Center),
        };

    /// <summary>Parts over each other, the first at the back: a symbol on a shape, a word in a corner of a picture.</summary>
    /// <param name="layers">The layers.</param>
    /// <param name="render">Draws one part.</param>
    /// <returns>The box.</returns>
    public static UIElement Layers(Models.Layers layers, Func<Models.Node, UIElement> render)
    {
        bool scaled = layers.Size?.Scaled ?? false;
        Grid box = new()
        {
            Width = Scaled(layers.Size?.Width, scaled),
            Height = Scaled(layers.Size?.Height, scaled),
            Padding = new Thickness(layers.Pad * 4),
        };
        (HorizontalAlignment across, VerticalAlignment down) = Placed(layers.Place);
        foreach (Models.Node child in layers.Children)
        {
            UIElement part = render(child);
            if (part is FrameworkElement placed)
            {
                // A part that has no size of its own (a shape, a ring) fills the box.
                bool fills = child is Models.Shape { Size: null };
                placed.HorizontalAlignment = fills ? HorizontalAlignment.Stretch : across;
                placed.VerticalAlignment = fills ? VerticalAlignment.Stretch : down;
            }

            box.Children.Add(part);
        }

        return box;
    }

    /// <summary>Parts in columns of one width, each row as high as its highest part.</summary>
    /// <param name="grid">The grid.</param>
    /// <param name="render">Draws one part.</param>
    /// <returns>The grid.</returns>
    public static UIElement Grid(Models.Grid grid, Func<Models.Node, UIElement> render)
    {
        bool cards = grid.Children.All(child => child is Models.Card);
        FlowGrid flow = new() { Cols = grid.Cols, Min = grid.Min, Gap = ComposeRules.GridGap(grid.Gap, cards) };
        foreach (Models.Node child in grid.Children)
        {
            UIElement part = render(child);

            // A link or a button that is a cell fills the cell (and centres what it holds), instead of standing at the cell's edge.
            if (part is Microsoft.UI.Xaml.Controls.Primitives.ButtonBase button)
            {
                button.HorizontalAlignment = HorizontalAlignment.Stretch;
                button.HorizontalContentAlignment = HorizontalAlignment.Stretch;
            }

            flow.Children.Add(part);
        }

        return flow;
    }

    /// <summary>Parts side by side that scroll across, stopping at a part: as many as fit at the least width are shown, so a wide room may show them all and scroll nothing; a paged run shows one at a time with the system's marks for which.</summary>
    /// <param name="carousel">The carousel.</param>
    /// <param name="render">Draws one part.</param>
    /// <returns>The run.</returns>
    public static UIElement Carousel(Models.Carousel carousel, Func<Models.Node, UIElement> render)
    {
        double gap = carousel.Gap * 4;
        StackPanel run = new() { Orientation = Orientation.Horizontal, Spacing = gap };
        List<FrameworkElement> parts = [];
        foreach (Models.Node child in carousel.Children)
        {
            UIElement part = render(child);
            Grid cell = new() { VerticalAlignment = VerticalAlignment.Stretch };
            cell.Children.Add(part);
            parts.Add(cell);
            run.Children.Add(cell);
        }

        ScrollViewer scroller = new()
        {
            Content = run,
            HorizontalScrollMode = ScrollMode.Auto,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
            VerticalScrollMode = ScrollMode.Disabled,
            VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
            HorizontalSnapPointsType = SnapPointsType.Mandatory,
            IsTabStop = false,
        };
        if (carousel.Label is not null)
        {
            AutomationProperties.SetName(scroller, carousel.Label);
        }

        StackPanel host = new() { Spacing = 8 };
        host.Children.Add(scroller);
        List<Button> marks = [];
        if (carousel.Paged && parts.Count > 1)
        {
            StackPanel row = new() { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Center };
            for (int at = 0; at < parts.Count; at++)
            {
                int page = at;
                Button mark = new()
                {
                    Content = new Microsoft.UI.Xaml.Shapes.Ellipse { Width = 8, Height = 8 },
                    MinWidth = 28,
                    MinHeight = 28,
                    Padding = new Thickness(0),
                    Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
                    BorderThickness = new Thickness(0),
                };
                AutomationProperties.SetName(mark, "Page " + (page + 1) + " of " + parts.Count);
                mark.Click += (_, _) => scroller.ChangeView(page * (scroller.ViewportWidth + gap), null, null);
                marks.Add(mark);
                row.Children.Add(mark);
            }

            host.Children.Add(row);
        }

        void Mark(int current)
        {
            Windows.UI.Color chosen = Palette.Resource("TextFillColorPrimaryBrush") is SolidColorBrush primary ? primary.Color : Microsoft.UI.Colors.Gray;
            for (int at = 0; at < marks.Count; at++)
            {
                ((Microsoft.UI.Xaml.Shapes.Ellipse)marks[at].Content).Fill = new SolidColorBrush(chosen) { Opacity = at == current ? 1 : 0.3 };
            }
        }

        Mark(0);

        void Fit(double room)
        {
            if (room <= 0)
            {
                return;
            }

            if (carousel.Paged)
            {
                foreach (FrameworkElement cell in parts)
                {
                    cell.Width = room;
                }

                return;
            }

            (int shown, double? width) = ComposeRules.Carousel(room, carousel.Min, gap, parts.Count);
            foreach (FrameworkElement cell in parts)
            {
                cell.Width = width ?? double.NaN;
            }

            scroller.HorizontalScrollMode = ComposeRules.Scrolls(shown, parts.Count) ? ScrollMode.Auto : ScrollMode.Disabled;
        }

        host.SizeChanged += (_, e) => Fit(e.NewSize.Width);
        if (marks.Count > 0)
        {
            scroller.ViewChanged += (_, _) =>
            {
                if (scroller.ViewportWidth > 0)
                {
                    Mark(Math.Clamp((int)Math.Round(scroller.HorizontalOffset / (scroller.ViewportWidth + gap)), 0, parts.Count - 1));
                }
            };
        }

        return host;
    }
}
