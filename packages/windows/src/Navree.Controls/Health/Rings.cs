using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Navree.Controls.Composition;
using Navree.Controls.Drawing;
using Navree.Design;
using Navree.Protocol;

namespace Navree.Controls.Health;

/// <summary>What the shell gives the drawn kinds that they cannot make themselves: how to draw an icon of the page's own.</summary>
public static class KindHooks
{
    /// <summary>Draws a page's icon at a size in a colour; set by the shell, which knows how its icons are drawn.</summary>
    public static Func<Models.Icon, double, Windows.UI.Color, UIElement>? Icon { get; set; }

    /// <summary>A brush of a part's colour (or of a fallback hue when the page names none) in the theme the app runs in; the same colours the drawn kinds use.</summary>
    /// <param name="colour">The page's colour.</param>
    /// <param name="fallback">The hue to take when there is none.</param>
    /// <param name="opacity">How strong the brush is.</param>
    /// <returns>The brush.</returns>
    public static Brush HueBrush(Models.Colour? colour, Models.Hue fallback, double opacity = 1) =>
        new Palette(new Microsoft.UI.Xaml.Controls.Border()).Fill(colour, fallback, opacity);

    /// <summary>A brush for words in a part's colour, as dark as they need to be to read on a light ground.</summary>
    /// <param name="colour">The page's colour.</param>
    /// <param name="fallback">The hue to take when there is none.</param>
    /// <returns>The brush.</returns>
    public static Brush HueWords(Models.Colour? colour, Models.Hue fallback) =>
        new Palette(new Microsoft.UI.Xaml.Controls.Border()).Words(colour, fallback);

    internal static UIElement DrawIcon(Models.Icon icon, double size, Windows.UI.Color colour) =>
        Icon?.Invoke(icon, size, colour)
        ?? new FontIcon
        {
            Glyph = SystemGlyphs.For(icon.Name, icon.Label),
            FontFamily = HealthParts.Icons,
            FontSize = size * 0.8,
            Foreground = Palette.Mark(colour),
        };
}

/// <summary>Arcs the rings and gauges share.</summary>
internal static class RingArcs
{
    /// <summary>One arc of a ring: its track all round in the hue made faint, the value from twelve o'clock clockwise with round ends, and past the goal a second lap over the first.</summary>
    internal static void Draw(PlotCanvas canvas, double cx, double cy, double radius, double stroke, RingRules.Sweep? sweep, Windows.UI.Color start, Windows.UI.Color end)
    {
        canvas.Shapes.Put(Ink.Stroked(Ink.Arc(cx, cy, radius, -90, 359.9), canvas.Palette.Track(start), stroke), 0, 0);
        if (sweep is null)
        {
            return;
        }

        if (sweep.Past)
        {
            // The goal reached: a whole lap, then the part past it.
            Ink.BlendedArc(canvas.Shapes, cx, cy, radius, stroke, -90, 359.9, start, end);
            if (sweep.Part > 0)
            {
                canvas.Shapes.Put(Ink.Stroked(Ink.Arc(cx, cy, radius, -90, 360 * sweep.Part), Palette.Mark(end), stroke, Microsoft.UI.Xaml.Media.PenLineCap.Round), 0, 0);
            }

            Navree.Design.Pt tip = Ink.Polar(cx, cy, radius, -90 + (360 * sweep.Part));
            Microsoft.UI.Xaml.Shapes.Ellipse head = Ink.Disc(stroke, Palette.Mark(end), Palette.Mark(Microsoft.UI.Colors.Black, 0.35), 1);
            canvas.Shapes.Put(head, tip.X - (stroke / 2), tip.Y - (stroke / 2));
        }
        else if (sweep.Part > 0)
        {
            Ink.BlendedArc(canvas.Shapes, cx, cy, radius, stroke, -90, 360 * sweep.Part, start, end);
        }
    }
}

/// <summary>
/// A ring: one value against its goal. The value and its unit stand in the middle; under the ring, its label (with the info button for
/// its hint), then how it stands in a tinted capsule and which way it is going. A small ring is the arc alone.
/// </summary>
public sealed partial class RingView : KindControl
{
    private readonly Models.Ring ring;
    private readonly bool small;

    /// <summary>Draws a ring.</summary>
    /// <param name="ring">The ring.</param>
    /// <param name="small">Whether it stands beside a figure's words, where it is a small ring whatever it says.</param>
    public RingView(Models.Ring ring, bool small = false)
    {
        this.ring = ring;
        this.small = small;
        Start();
    }

    private Models.Prominence Prominence => small ? Models.Prominence.Low : ring.Prominence;

    internal override UIElement Build(Palette palette)
    {
        double ideal = RingRules.Diameter(Prominence) * TextScale.Capped(RingRules.Growth(Prominence));
        StackPanel stack = new() { Spacing = Prominence == Models.Prominence.High ? 10 : 6 };
        string name = ring.Label ?? string.Empty;
        string value = SpeechRules.Value(ring.Summary ?? (ring.Value is null ? "Not measured" : RingRules.Centre(ring)), ring.Label);
        PlotSurface plot = new()
        {
            Height = ideal,
            MaxWidth = ideal,
            Ideal = ideal,
            AccessName = string.IsNullOrEmpty(name) ? value : SpeechRules.Fact(name, value),
            AccessType = "ring",
            Range = ring.Value is double current ? (0, ring.Max, current) : null,
        };
        plot.Painter = Paint;
        stack.Children.Add(plot);

        if (RingRules.SaysWords(Prominence))
        {
            stack.Children.Add(Words(palette));
        }

        return stack;
    }

    private void Paint(PlotCanvas canvas)
    {
        double diameter = Math.Min(canvas.Width, canvas.Height);
        double stroke = RingRules.Stroke(Prominence, diameter);
        double cx = canvas.Width / 2;
        double cy = canvas.Height / 2;
        double radius = (diameter / 2) - (stroke / 2);
        Windows.UI.Color start = canvas.Palette.Of(ring.Colour, Models.Hue.Accent);
        Windows.UI.Color end = canvas.Palette.End(ring.Colour, Models.Hue.Accent);
        RingArcs.Draw(canvas, cx, cy, radius, stroke, RingRules.SweepOf(ring.Value, ring.Max), start, end);
        if (!RingRules.ShowsCentre(Prominence, ring.Centre))
        {
            return;
        }

        double room = Math.Max(diameter - (2 * stroke) - 12, 1);
        if (ring.Centre == Models.Centre.Icon && ring.Icon is not null)
        {
            double side = diameter * 0.3;
            canvas.Labels.Put(KindHooks.DrawIcon(ring.Icon, side, start), cx - (side / 2), cy - (side / 2));
            return;
        }

        double size = RingRules.CentreSize(diameter);
        TextBlock value = Ink.Words(RingRules.Centre(ring), size, canvas.Palette.Primary, strong: true);
        TextBlock? unit = ring.Value is not null && ring.Unit is not null ? Ink.Words(ring.Unit, size * 0.42, canvas.Palette.Secondary, strong: true) : null;
        double wide = Ink.Measured(value).Width + (unit is null ? 0 : Ink.Measured(unit).Width + 2);
        if (wide > room)
        {
            double shrink = Math.Max(room / wide, 0.4);
            value.FontSize = size * shrink;
            if (unit is not null)
            {
                unit.FontSize = size * 0.42 * shrink;
            }
        }

        double valueWidth = Ink.Measured(value).Width;
        double unitWidth = unit is null ? 0 : Ink.Measured(unit).Width + 2;
        double left = cx - ((valueWidth + unitWidth) / 2);
        double height = Ink.Measured(value).Height;
        canvas.Labels.Put(value, left, cy - (height / 2));
        if (unit is not null)
        {
            canvas.Labels.Put(unit, left + valueWidth + 2, cy + (height / 2) - Ink.Measured(unit).Height - 3);
        }
    }

    private UIElement Words(Palette palette)
    {
        StackPanel words = new() { Spacing = 6, HorizontalAlignment = HorizontalAlignment.Center };
        if (ring.Label is not null || ring.Hint is not null)
        {
            StackPanel line = new() { Orientation = Orientation.Horizontal, Spacing = 4, HorizontalAlignment = HorizontalAlignment.Center };
            if (ring.Label is not null)
            {
                TextBlock label = new() { Text = ring.Label, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, TextWrapping = TextWrapping.Wrap, TextAlignment = TextAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
                if (ring.Prominence == Models.Prominence.High)
                {
                    label.FontSize = 18;
                }

                line.Children.Add(HealthParts.Hidden(label));
            }

            if (ring.Hint is not null)
            {
                line.Children.Add(HealthParts.Hint(ring.Hint));
            }

            words.Children.Add(line);
        }

        if (ring.Caption is not null)
        {
            words.Children.Add(HealthParts.Hidden(new TextBlock { Text = ring.Caption, Foreground = palette.Secondary, FontSize = 12, TextWrapping = TextWrapping.Wrap, TextAlignment = TextAlignment.Center }));
        }

        if (ring.Status is not null || ring.Trend is not null)
        {
            WrapPanel flow = new() { HorizontalSpacing = 10, VerticalSpacing = 6, HorizontalAlignment = HorizontalAlignment.Center };
            if (ring.Status is not null)
            {
                flow.Children.Add(HealthParts.Status(ring.Status, this));
            }

            if (ring.Trend is not null)
            {
                flow.Children.Add(HealthParts.Trend(ring.Trend, this));
            }

            words.Children.Add(HealthParts.Hidden(flow));
        }

        return words;
    }
}

/// <summary>Concentric rings, the first outside, with the values over their goals beside them in each ring's colour.</summary>
public sealed partial class RingsView : KindControl
{
    private readonly Models.Rings rings;

    /// <summary>Draws rings.</summary>
    /// <param name="rings">The rings.</param>
    public RingsView(Models.Rings rings)
    {
        this.rings = rings;
        Start();
    }

    private IReadOnlyList<Models.RingValue> Shown => [.. rings.RingsField.Take(RingsRules.Most)];

    internal override UIElement Build(Palette palette)
    {
        bool low = rings.Prominence == Models.Prominence.Low;
        double diameter = RingsRules.Diameter(rings.Prominence, Shown.Count) * TextScale.Capped(low ? 1 : 1.3);
        string title = rings.Title ?? string.Empty;
        string said = SpeechRules.Value(rings.Summary ?? string.Join(", ", Shown.Select(item => item.Name + " " + RingsRules.Value(item))), rings.Title);
        PlotSurface dial = new()
        {
            Width = diameter,
            Height = diameter,
            AccessName = string.IsNullOrEmpty(title) ? said : SpeechRules.Fact(title, said),
            AccessType = "rings",
            HorizontalAlignment = HorizontalAlignment.Left,
        };
        dial.Painter = Paint;
        if (!RingsRules.HasKey(rings))
        {
            return dial;
        }

        WrapPanel flow = new() { HorizontalSpacing = 24, VerticalSpacing = 16 };
        flow.Children.Add(dial);
        StackPanel key = new() { Spacing = 8, VerticalAlignment = VerticalAlignment.Center };
        foreach (Models.RingValue item in Shown)
        {
            StackPanel entry = new();
            if (rings.Legend)
            {
                entry.Children.Add(new TextBlock { Text = item.Name, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold });
            }

            TextBlock value = new() { Text = RingsRules.Value(item), FontSize = 20, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, Foreground = palette.Words(item.Colour, Models.Hue.Accent) };
            Typography.SetNumeralAlignment(value, FontNumeralAlignment.Tabular);
            entry.Children.Add(value);
            key.Children.Add(entry);
        }

        flow.Children.Add(HealthParts.Hidden(key));
        return flow;
    }

    private void Paint(PlotCanvas canvas)
    {
        IReadOnlyList<Models.RingValue> shown = Shown;
        double diameter = Math.Min(canvas.Width, canvas.Height);
        double stroke = RingsRules.Stroke(diameter, shown.Count, rings.Prominence);
        double cx = canvas.Width / 2;
        double cy = canvas.Height / 2;
        for (int at = 0; at < shown.Count; at++)
        {
            double radius = RingsRules.RingDiameter(diameter, at, stroke) / 2;
            if (radius <= 0)
            {
                continue;
            }

            Models.RingValue item = shown[at];
            RingArcs.Draw(
                canvas,
                cx,
                cy,
                radius,
                stroke,
                RingRules.SweepOf(item.Value, item.Max),
                canvas.Palette.Of(item.Colour, HueRules.SeriesHue(at)),
                canvas.Palette.End(item.Colour, HueRules.SeriesHue(at)));
        }

        if (rings.Centre == Models.RingsCentre.Icon && rings.Icon is not null)
        {
            double side = diameter * 0.16;
            canvas.Labels.Put(KindHooks.DrawIcon(rings.Icon, side, Microsoft.UI.Colors.Gray), cx - (side / 2), cy - (side / 2));
        }
    }
}
