using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Automation.Provider;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Navree.Design;
using Windows.Foundation;
using Windows.System;
using Border = Microsoft.UI.Xaml.Controls.Border;

namespace Navree.Controls.Drawing;

/// <summary>One line of a read-out: a colour and what it says.</summary>
/// <param name="Colour">The mark's colour, or none for a line of words.</param>
/// <param name="Text">The words.</param>
internal sealed record ProbeLine(Windows.UI.Color? Colour, string Text);

/// <summary>One place of a drawing that can be read out: where it is, what it says, and the marks it lights.</summary>
/// <param name="X">The place across, as drawn.</param>
/// <param name="Y">The place down, as drawn.</param>
/// <param name="Hit">The area of the drawing that stands for it.</param>
/// <param name="Title">The words over the read-out.</param>
/// <param name="Lines">The lines of the read-out.</param>
/// <param name="Spoken">What a screen reader is told when the place is reached.</param>
/// <param name="Guide">Whether a line is drawn down through the place.</param>
/// <param name="Marks">Discs to draw on the place: where, and in what colour.</param>
internal sealed record Probe(
    double X,
    double Y,
    Rect Hit,
    string Title,
    IReadOnlyList<ProbeLine> Lines,
    string Spoken,
    bool Guide = false,
    IReadOnlyList<(Point At, Windows.UI.Color Colour)>? Marks = null);

/// <summary>What a drawing is painted on: two layers (shapes, which flip across in right to left, and texts), and the places it can be read out.</summary>
internal sealed class PlotCanvas
{
    /// <summary>The layer of shapes.</summary>
    internal required Canvas Shapes { get; init; }

    /// <summary>The layer of texts.</summary>
    internal required Canvas Labels { get; init; }

    /// <summary>How wide the drawing is.</summary>
    internal double Width { get; init; }

    /// <summary>How high the drawing is.</summary>
    internal double Height { get; init; }

    /// <summary>The colours of the theme.</summary>
    internal required Palette Palette { get; init; }

    /// <summary>Whether across runs the other way (right to left).</summary>
    internal bool Mirror { get; init; }

    /// <summary>The places that can be read out, in the order the keys step through them.</summary>
    internal List<Probe> Probes { get; } = [];

    /// <summary>Across as it is drawn.</summary>
    internal double Mx(double x) => Mirror ? Width - x : x;

    /// <summary>A rectangle across as it is drawn.</summary>
    internal Rect Mrect(double x, double y, double width, double height) =>
        Ink.Area(Mirror ? Width - x - Guard.Length(width) : x, y, width, height);

    /// <summary>Puts a text where shapes would be put if they were not flipped.</summary>
    internal TextBlock Text(TextBlock block, double x, double y, Across across = Across.Start, Down down = Down.Top)
    {
        Across placed = Mirror ? across switch { Across.Start => Across.End, Across.End => Across.Start, _ => across } : across;
        return Labels.Say(block, Mx(x), y, placed, down);
    }

    /// <summary>A text of the usual small size.</summary>
    internal TextBlock Caption(string text, Brush? brush = null, bool strong = false) =>
        Ink.Words(text, 11, brush ?? Palette.Secondary, strong);
}

/// <summary>
/// The surface a drawn kind is painted on. It paints itself again when its width or its theme changes, reads out the place under the
/// pointer or the place the arrow keys have reached, and is one element to a screen reader: the part's name and what it says.
/// </summary>
internal sealed partial class PlotSurface : UserControl
{
    private readonly Grid root = new() { Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
    private readonly Canvas shapes = new() { IsHitTestVisible = false };
    private readonly Canvas labels = new() { IsHitTestVisible = false };
    private readonly Canvas guide = new() { IsHitTestVisible = false };
    private readonly List<Probe> probes = [];
    private int current = -1;
    private double paintedWidth = -1;

    /// <summary>Starts a surface.</summary>
    internal PlotSurface()
    {
        // The drawing keeps its own frame: it flips across itself where the part should read right to left.
        root.FlowDirection = FlowDirection.LeftToRight;
        Content = root;
        root.Children.Add(shapes);
        root.Children.Add(labels);
        root.Children.Add(guide);
        shapes.RenderTransformOrigin = new Point(0.5, 0.5);
        HorizontalAlignment = HorizontalAlignment.Stretch;
        IsTabStop = false;
        SizeChanged += (_, e) =>
        {
            // What is drawn never shows past the surface's edge, whatever room its parts were laid out for.
            shapes.Clip = new RectangleGeometry { Rect = new Rect(0, 0, e.NewSize.Width, e.NewSize.Height) };
            labels.Clip = new RectangleGeometry { Rect = new Rect(0, 0, e.NewSize.Width, e.NewSize.Height) };
            Paint();
        };
        ActualThemeChanged += (_, _) => Paint(force: true);
        Loaded += (_, _) => Paint(force: true);
        PointerMoved += OnPointerMoved;
        PointerExited += (_, _) =>
        {
            if (FocusState == FocusState.Unfocused)
            {
                Hide();
            }
        };
        KeyDown += OnKeyDown;
        GotFocus += (_, _) => Reach(current < 0 ? 0 : current, speak: false);
        LostFocus += (_, _) => Hide();
    }

    /// <summary>Paints the surface: given the canvas and what to paint on it.</summary>
    internal Action<PlotCanvas>? Painter { get; set; }

    /// <summary>How high the drawing is for a width, when that follows from the width (a calendar of square days); the surface then takes that height.</summary>
    internal Func<double, double>? HeightFor { get; set; }

    /// <summary>The width the drawing wants when its place gives it none (a ring in a column that is as wide as its words); less when the place is narrower.</summary>
    internal double Ideal { get; set; }

    /// <summary>Whether the drawing runs the other way across in right to left.</summary>
    internal bool MirrorInRtl { get; set; }

    /// <summary>Whether the surface can be reached with the keyboard and read out place by place.</summary>
    internal bool Reads
    {
        get => IsTabStop;
        set
        {
            IsTabStop = value;
            UseSystemFocusVisuals = value;
        }
    }

    /// <summary>The part's name and what it says, as one stop for a screen reader.</summary>
    internal string AccessName { get; set; } = string.Empty;

    /// <summary>More words for a screen reader.</summary>
    internal string AccessHelp { get; set; } = string.Empty;

    /// <summary>What kind of thing a screen reader says this is.</summary>
    internal string AccessType { get; set; } = "chart";

    /// <summary>A value the part holds against a range, for screen readers that ask for it.</summary>
    internal (double Min, double Max, double Value)? Range { get; set; }

    /// <summary>The places that can be read out.</summary>
    internal IReadOnlyList<Probe> Probes => probes;

    /// <summary>The place being read out, or none.</summary>
    internal int Current => current;

    /// <summary>Paints again.</summary>
    internal void Repaint() => Paint(force: true);

    /// <summary>Takes the read-out away.</summary>
    internal void Dismiss()
    {
        Hide();
        current = -1;
    }

    /// <summary>Reads out the place at an index, as the keys do.</summary>
    internal void Reach(int index, bool speak = true)
    {
        if (probes.Count == 0)
        {
            return;
        }

        current = Guard.Place(index, probes.Count);
        Show(probes[current]);
        if (speak)
        {
            Tell(probes[current].Spoken);
        }
    }

    /// <inheritdoc />
    protected override Size MeasureOverride(Size availableSize)
    {
        Size measured = base.MeasureOverride(availableSize);
        if (Ideal <= 0)
        {
            return measured;
        }

        double room = double.IsInfinity(availableSize.Width) ? Ideal : Math.Min(availableSize.Width, Ideal);
        return new Size(Math.Max(measured.Width, room), measured.Height);
    }

    /// <inheritdoc />
    protected override AutomationPeer OnCreateAutomationPeer() => new PlotPeer(this);

    private void Paint(bool force = false)
    {
        double width = ActualWidth;
        if (width <= 0 || Painter is null)
        {
            return;
        }

        if (HeightFor is not null)
        {
            double wanted = HeightFor(width);
            if (double.IsNaN(Height) || Math.Abs(wanted - Height) > 0.5)
            {
                Height = wanted;
                return;
            }
        }

        double height = ActualHeight;
        if (height <= 0)
        {
            return;
        }

        if (!force && Math.Abs(width - paintedWidth) < 0.5 && shapes.Children.Count > 0)
        {
            return;
        }

        paintedWidth = width;
        shapes.Children.Clear();
        labels.Children.Clear();
        guide.Children.Clear();
        probes.Clear();
        bool mirror = MirrorInRtl && FlowDirection == FlowDirection.RightToLeft;
        shapes.RenderTransform = mirror ? new ScaleTransform { ScaleX = -1 } : null;
        PlotCanvas plot = new()
        {
            Shapes = shapes,
            Labels = labels,
            Width = width,
            Height = height,
            Palette = new Palette(this),
            Mirror = mirror,
        };
        try
        {
            Painter(plot);
        }
        catch (Exception)
        {
            // A drawing is the picture of what the part says, and the part's name still says it to a screen reader: a drawing that cannot
            // be made for the data or the size it was given is left empty, and never faults the window it stands in.
            shapes.Children.Clear();
            labels.Children.Clear();
            plot.Probes.Clear();
        }

        probes.AddRange(plot.Probes);
        if (current >= probes.Count)
        {
            current = -1;
        }

        if (current >= 0 && FocusState != FocusState.Unfocused)
        {
            Show(probes[current]);
        }
    }

    // ---- reading out a place -----------------------------------------------------------------------------

    private void OnPointerMoved(object sender, PointerRoutedEventArgs e)
    {
        if (probes.Count == 0)
        {
            return;
        }

        Point at = e.GetCurrentPoint(root).Position;
        int found = Nearest(at);
        if (found < 0)
        {
            Hide();
            return;
        }

        if (found != current || guide.Children.Count == 0)
        {
            current = found;
            Show(probes[found]);
        }
    }

    private int Nearest(Point at)
    {
        int best = -1;
        double bestDistance = double.MaxValue;
        for (int index = 0; index < probes.Count; index++)
        {
            Probe probe = probes[index];
            double distance;
            if (probe.Hit.Width > 0 && probe.Hit.Height > 0)
            {
                // A place with an area of its own is read out while the pointer is inside it.
                bool inside = at.X >= probe.Hit.X && at.X <= probe.Hit.X + probe.Hit.Width && at.Y >= probe.Hit.Y && at.Y <= probe.Hit.Y + probe.Hit.Height;
                distance = inside ? 0 : double.MaxValue;
            }
            else
            {
                distance = Math.Abs(probe.X - at.X) + (probe.Guide ? 0 : Math.Abs(probe.Y - at.Y));
            }

            if (distance < bestDistance)
            {
                best = index;
                bestDistance = distance;
            }
        }

        return bestDistance == double.MaxValue || (bestDistance > 48 && !probes[best].Guide) ? -1 : best;
    }

    private void OnKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (!Reads || probes.Count == 0)
        {
            return;
        }

        bool rtl = FlowDirection == FlowDirection.RightToLeft && MirrorInRtl;
        int next = current < 0 ? 0 : current;
        switch (e.Key)
        {
            case VirtualKey.Right:
            case VirtualKey.Down:
                next += e.Key == VirtualKey.Right && rtl ? -1 : 1;
                break;
            case VirtualKey.Left:
            case VirtualKey.Up:
                next += e.Key == VirtualKey.Left && rtl ? 1 : -1;
                break;
            case VirtualKey.Home:
                next = 0;
                break;
            case VirtualKey.End:
                next = probes.Count - 1;
                break;
            case VirtualKey.Escape:
                Hide();
                return;
            default:
                return;
        }

        e.Handled = true;
        Reach(next);
    }

    private void Hide()
    {
        guide.Children.Clear();
    }

    private void Show(Probe probe)
    {
        guide.Children.Clear();
        Palette palette = new(this);
        double width = ActualWidth;
        double height = ActualHeight;
        if (probe.Guide)
        {
            Brush line = palette.Secondary;
            guide.Put(Ink.Rule(probe.X, 0, probe.X, height, line, 1, [3, 3]), 0, 0);
        }

        if (probe.Marks is not null)
        {
            foreach ((Point at, Windows.UI.Color colour) in probe.Marks)
            {
                guide.Put(Ink.Disc(9, Palette.Mark(colour), palette.Card, 2), at.X - 4.5, at.Y - 4.5);
            }
        }
        else if (probe.Hit.Width > 0)
        {
            guide.Put(Ink.Box(probe.Hit.Width, probe.Hit.Height, palette.Wash, 3, palette.Secondary, 1), probe.Hit.X, probe.Hit.Y);
        }

        StackPanel body = new() { Spacing = 2 };
        body.Children.Add(Ink.Words(probe.Title, 12, palette.Primary, strong: true));
        foreach (ProbeLine line in probe.Lines)
        {
            StackPanel row = new() { Orientation = Orientation.Horizontal, Spacing = 6 };
            if (line.Colour is Windows.UI.Color colour)
            {
                TextBlock dot = Ink.Words("●", 9, Palette.Mark(colour));
                dot.VerticalAlignment = VerticalAlignment.Center;
                row.Children.Add(dot);
            }

            row.Children.Add(Ink.Words(line.Text, 12, palette.Primary));
            body.Children.Add(row);
        }

        Border card = new()
        {
            Child = body,
            Padding = new Thickness(10, 6, 10, 6),
            CornerRadius = new CornerRadius(4),
            BorderThickness = new Thickness(1),
            Background = palette.Flyout,
            BorderBrush = palette.FlyoutEdge,
            IsHitTestVisible = false,
        };
        card.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        Size size = card.DesiredSize;
        double top = probe.Y - size.Height - 12;
        if (top < 0)
        {
            top = Math.Min(probe.Y + 14, Math.Max(height - size.Height, 0));
        }

        double left = Math.Clamp(probe.X - (size.Width / 2), 0, Math.Max(width - size.Width, 0));
        guide.Put(card, left, top);
    }

    private void Tell(string words)
    {
        if (string.IsNullOrEmpty(words))
        {
            return;
        }

        AutomationPeer? peer = FrameworkElementAutomationPeer.FromElement(this) ?? FrameworkElementAutomationPeer.CreatePeerForElement(this);
        peer?.RaiseNotificationEvent(AutomationNotificationKind.ActionCompleted, AutomationNotificationProcessing.ImportantMostRecent, words, "navree-plot");
    }
}

/// <summary>What a screen reader is given for a drawn part: one element with the part's name and words, and its value against a range where it has one.</summary>
internal sealed partial class PlotPeer(PlotSurface owner) : FrameworkElementAutomationPeer(owner), IRangeValueProvider
{
    private readonly PlotSurface surface = owner;

    /// <inheritdoc />
    protected override string GetClassNameCore() => "NavreePlot";

    /// <inheritdoc />
    protected override string GetNameCore() => surface.AccessName;

    /// <inheritdoc />
    protected override string GetHelpTextCore() => surface.AccessHelp;

    /// <inheritdoc />
    protected override string GetLocalizedControlTypeCore() => surface.AccessType;

    /// <inheritdoc />
    protected override AutomationControlType GetAutomationControlTypeCore() =>
        surface.Reads ? AutomationControlType.Custom : AutomationControlType.Image;

#pragma warning disable CS8764, CS8603
    /// <inheritdoc />
    protected override object GetPatternCore(PatternInterface patternInterface) =>
        patternInterface == PatternInterface.RangeValue && surface.Range is not null ? this : base.GetPatternCore(patternInterface);
#pragma warning restore CS8764, CS8603

    /// <inheritdoc />
    public bool IsReadOnly => true;

    /// <inheritdoc />
    public double LargeChange => 0;

    /// <inheritdoc />
    public double SmallChange => 0;

    /// <inheritdoc />
    public double Maximum => surface.Range?.Max ?? 0;

    /// <inheritdoc />
    public double Minimum => surface.Range?.Min ?? 0;

    /// <inheritdoc />
    public double Value => surface.Range?.Value ?? 0;

    /// <inheritdoc />
    public void SetValue(double value)
    {
        // The value is shown, never set by a person.
    }
}

/// <summary>Reads out a place of every drawing under an element, as the arrow keys do: for a picture of what a person sees there.</summary>
public static class Readouts
{
    /// <summary>Takes the read-out away from every drawing under the element.</summary>
    /// <param name="root">The element.</param>
    public static void Clear(DependencyObject root)
    {
        int children = VisualTreeHelper.GetChildrenCount(root);
        for (int at = 0; at < children; at++)
        {
            DependencyObject child = VisualTreeHelper.GetChild(root, at);
            if (child is PlotSurface surface)
            {
                surface.Dismiss();
            }

            Clear(child);
        }
    }

    /// <summary>Reaches the place at an index of each drawing under the element (the last place when there are fewer).</summary>
    /// <param name="root">The element.</param>
    /// <param name="index">The place.</param>
    /// <returns>How many drawings were read out.</returns>
    public static int Show(DependencyObject root, int index)
    {
        int shown = 0;
        int children = VisualTreeHelper.GetChildrenCount(root);
        for (int at = 0; at < children; at++)
        {
            DependencyObject child = VisualTreeHelper.GetChild(root, at);
            if (child is PlotSurface surface && surface.Probes.Count > 0)
            {
                surface.Reach(Math.Min(index, surface.Probes.Count - 1), speak: false);
                shown++;
            }

            shown += Show(child, index);
        }

        return shown;
    }
}

/// <summary>Finds the first drawing under an element that can be read out, and says where its places are: for a test that moves a real pointer and presses real keys.</summary>
public static class FirstDrawing
{
    private static PlotSurface? Find(DependencyObject root)
    {
        int children = VisualTreeHelper.GetChildrenCount(root);
        for (int at = 0; at < children; at++)
        {
            DependencyObject child = VisualTreeHelper.GetChild(root, at);
            if (child is PlotSurface surface && surface.Reads && surface.Probes.Count > 2)
            {
                return surface;
            }

            if (Find(child) is PlotSurface inner)
            {
                return inner;
            }
        }

        return null;
    }

    /// <summary>Where a place of the first drawing is, as the element sees it; none when there is no such drawing.</summary>
    /// <param name="root">Where to look.</param>
    /// <param name="relativeTo">The element the place is told in.</param>
    /// <param name="place">Which place.</param>
    /// <returns>The point.</returns>
    public static Windows.Foundation.Point? Where(DependencyObject root, UIElement relativeTo, int place)
    {
        PlotSurface? surface = Find(root);
        if (surface is null || place >= surface.Probes.Count)
        {
            return null;
        }

        Probe probe = surface.Probes[place];
        double x = probe.Hit.Width > 0 ? probe.Hit.X + (probe.Hit.Width / 2) : probe.X;
        double y = probe.Hit.Height > 0 ? probe.Hit.Y + (probe.Hit.Height / 2) : probe.Y;
        return surface.TransformToVisual(relativeTo).TransformPoint(new Windows.Foundation.Point(x, y));
    }

    /// <summary>The place the first drawing reads out now, or -1.</summary>
    /// <param name="root">Where to look.</param>
    /// <returns>The place.</returns>
    public static int Current(DependencyObject root) => Find(root)?.Current ?? -1;

    /// <summary>Gives the first drawing the keyboard focus, as a Tab would.</summary>
    /// <param name="root">Where to look.</param>
    /// <returns>Whether there was one to give it to.</returns>
    public static bool Focus(DependencyObject root) => Find(root)?.Focus(FocusState.Keyboard) ?? false;
}
