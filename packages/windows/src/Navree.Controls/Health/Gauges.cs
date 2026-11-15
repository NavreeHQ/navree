using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Navree.Controls.Drawing;
using Navree.Design;
using Navree.Protocol;
using Windows.Foundation;

namespace Navree.Controls.Health;

/// <summary>
/// A gauge. Beside a figure's words it is a small circular gauge. Leading a card it is an open arc of the same form, larger, its zones
/// laid along the track in their colours with a hair between them, a marker at the value, the value and the name of its zone in the
/// middle, the scale's ends under the arc's ends, and a key of the zones under it with the one the value is in standing out.
/// </summary>
public sealed partial class GaugeView : KindControl
{
    private readonly Models.Gauge gauge;
    private readonly bool aside;

    /// <summary>Draws a gauge.</summary>
    /// <param name="gauge">The gauge.</param>
    /// <param name="aside">Whether it stands beside a figure's words.</param>
    public GaugeView(Models.Gauge gauge, bool aside = false)
    {
        this.gauge = gauge;
        this.aside = aside;
        Start();
    }

    private double Position => AxisRules.Position(gauge.Value, gauge.Min, gauge.Max);

    private int? Current => ZoneRules.IndexOf(gauge.Value, gauge.Zones);

    private string Words => gauge.Shown ?? NumberWords.Amount(gauge.Value);

    // The colour of what the value means: the zone it is in, else the part's own colour, else its tone, else the accent.
    private Windows.UI.Color Mark(Palette palette)
    {
        if (Current is int zone)
        {
            return palette.Of(gauge.Zones[zone].Colour, HueRules.OfTone(gauge.Tone));
        }

        return palette.Of(gauge.Colour, gauge.Tone is Models.Tone tone ? HueRules.OfTone(tone) : Models.Hue.Accent);
    }

    internal override UIElement Build(Palette palette)
    {
        string said = SpeechRules.Value(gauge.Summary ?? Words, gauge.Label);
        string name = string.IsNullOrEmpty(gauge.Label) ? said : SpeechRules.Fact(gauge.Label, said);
        (double, double, double)? range = (gauge.Min, gauge.Max, Math.Clamp(gauge.Value, gauge.Min, Math.Max(gauge.Max, gauge.Min)));
        if (aside)
        {
            double side = GaugeRules.Diameter(aside: true) * TextScale.Capped(1.3);
            PlotSurface small = new() { Width = side, Height = side, AccessName = name, AccessType = "gauge", Range = range, HorizontalAlignment = HorizontalAlignment.Left };
            small.Painter = PaintSmall;
            return small;
        }

        double diameter = GaugeRules.Diameter(aside: false) * TextScale.Capped(1.25);
        StackPanel stack = new() { Spacing = 10 };
        PlotSurface plot = new()
        {
            Height = diameter * GaugeRules.HeightShare,
            MaxWidth = diameter,
            Ideal = diameter,
            AccessName = name,
            AccessType = "gauge",
            Range = range,
        };
        plot.Painter = canvas => PaintOpen(canvas, diameter);
        stack.Children.Add(plot);

        if (gauge.Label is not null || gauge.Hint is not null)
        {
            StackPanel line = new() { Orientation = Orientation.Horizontal, Spacing = 4, HorizontalAlignment = HorizontalAlignment.Center };
            if (gauge.Label is not null)
            {
                line.Children.Add(HealthParts.Hidden(new TextBlock { Text = gauge.Label, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, VerticalAlignment = VerticalAlignment.Center, TextWrapping = TextWrapping.Wrap }));
            }

            if (gauge.Hint is not null)
            {
                line.Children.Add(HealthParts.Hint(gauge.Hint));
            }

            stack.Children.Add(line);
        }

        if (gauge.Caption is not null)
        {
            stack.Children.Add(HealthParts.Hidden(new TextBlock { Text = gauge.Caption, Foreground = palette.Secondary, FontSize = 12, HorizontalAlignment = HorizontalAlignment.Center, TextAlignment = TextAlignment.Center, TextWrapping = TextWrapping.Wrap }));
        }

        if (gauge.Zones.Any(zone => zone.Label is not null))
        {
            UIElement key = HealthParts.Key(palette, gauge.Zones.Select(zone => (palette.Of(zone.Colour, Models.Hue.Neutral), zone.Label ?? string.Empty, (string?)null)), Current);
            if (key is FrameworkElement placed)
            {
                placed.HorizontalAlignment = HorizontalAlignment.Center;
            }

            stack.Children.Add(key);
        }

        return stack;
    }

    // A small round gauge, for a gauge beside other words.
    private void PaintSmall(PlotCanvas canvas)
    {
        double side = Math.Min(canvas.Width, canvas.Height);
        double stroke = side * 0.11;
        double radius = (side / 2) - (stroke / 2);
        double cx = canvas.Width / 2;
        double cy = canvas.Height / 2;
        Windows.UI.Color colour = Mark(canvas.Palette);
        canvas.Shapes.Put(Ink.Stroked(Ink.Arc(cx, cy, radius, GaugeRules.Angle(0), GaugeRules.Sweep), canvas.Palette.Track(colour), stroke, PenLineCap.Round), 0, 0);
        if (Position > 0)
        {
            canvas.Shapes.Put(Ink.Stroked(Ink.Arc(cx, cy, radius, GaugeRules.Angle(0), GaugeRules.Sweep * Position), Palette.Mark(colour), stroke, PenLineCap.Round), 0, 0);
        }

        (string number, _) = GaugeRules.Split(Words);
        TextBlock text = Ink.Words(number, side * 0.27, canvas.Palette.Primary, strong: true);
        double room = side - (2 * stroke) - 4;
        double width = Ink.Measured(text).Width;
        if (width > room)
        {
            text.FontSize *= Math.Max(room / width, 0.5);
        }

        canvas.Labels.Say(text, cx, cy + (side * 0.04), Across.Middle, Down.Middle);
    }

    private void PaintOpen(PlotCanvas canvas, double baseDiameter)
    {
        Palette palette = canvas.Palette;
        double diameter = Math.Min(canvas.Width, canvas.Height / GaugeRules.HeightShare);
        double stroke = diameter * 0.085;
        double radius = (diameter / 2) - (stroke / 2);
        double cx = canvas.Width / 2;
        double cy = diameter / 2;
        double span = gauge.Max - gauge.Min;
        Windows.UI.Color colour = Mark(palette);

        if (gauge.Zones.Count == 0 || span <= 0)
        {
            canvas.Shapes.Put(Ink.Stroked(Ink.Arc(cx, cy, radius, GaugeRules.Angle(0), GaugeRules.Sweep), palette.Track(colour), stroke, PenLineCap.Round), 0, 0);
            if (Position > 0)
            {
                canvas.Shapes.Put(Ink.Stroked(Ink.Arc(cx, cy, radius, GaugeRules.Angle(0), GaugeRules.Sweep * Position), Palette.Mark(colour), stroke, PenLineCap.Round), 0, 0);
            }
        }
        else
        {
            // A hair between zones, as a share of the whole arc.
            const double hair = 0.008;
            for (int at = 0; at < gauge.Zones.Count; at++)
            {
                Models.Zone zone = gauge.Zones[at];
                double low = AxisRules.Position(Math.Min(zone.From, zone.To), gauge.Min, gauge.Max);
                double high = AxisRules.Position(Math.Max(zone.From, zone.To), gauge.Min, gauge.Max);
                double begin = at == 0 ? low : low + hair;
                double end = at == gauge.Zones.Count - 1 ? high : high - hair;
                if (end > begin)
                {
                    canvas.Shapes.Put(Ink.Stroked(Ink.Arc(cx, cy, radius, GaugeRules.Angle(begin), GaugeRules.Sweep * (end - begin)), Palette.Mark(palette.Of(zone.Colour, Models.Hue.Neutral)), stroke), 0, 0);
                }
            }
        }

        // The marker at the value: a disc in the colour of what it means, cut out of the arc by the card's own colour.
        Pt at2 = Ink.Polar(cx, cy, radius, GaugeRules.Angle(Position));
        double marker = stroke + 6;
        canvas.Shapes.Put(Ink.Disc(marker, Palette.Mark(colour), palette.Card, 3), at2.X - (marker / 2), at2.Y - (marker / 2));

        // The words in the middle: the value large, its unit, and the name of its zone.
        (string number, string? unit) = GaugeRules.Split(Words);
        List<TextBlock> lines = [];
        TextBlock big = Ink.Words(number, diameter * 0.2, palette.Primary, strong: true);
        double room = diameter - (2 * stroke) - 24;
        if (Ink.Measured(big).Width > room)
        {
            big.FontSize *= Math.Max(room / Ink.Measured(big).Width, 0.5);
        }

        lines.Add(big);
        if (unit is not null)
        {
            lines.Add(Ink.Words(unit, 12, palette.Secondary, strong: true));
        }

        if (Current is int zoneAt && gauge.Zones[zoneAt].Label is string zoneName)
        {
            lines.Add(Ink.Words(zoneName, 14, palette.Words(gauge.Zones[zoneAt].Colour, Models.Hue.Neutral), strong: true));
        }

        double total = lines.Sum(line => Ink.Measured(line).Height);
        double top = cy + (diameter * 0.08) - (total / 2);
        foreach (TextBlock line in lines)
        {
            canvas.Labels.Say(line, cx, top, Across.Middle, Down.Top);
            top += Ink.Measured(line).Height;
        }

        // The ends of the scale, under the arc's ends.
        foreach ((double position, double value) in new[] { (0.0, gauge.Min), (1.0, gauge.Max) })
        {
            Pt end = Ink.Polar(cx, cy, radius, GaugeRules.Angle(position));
            canvas.Labels.Say(canvas.Caption(NumberWords.Amount(value)), end.X, end.Y + (stroke / 2) + 10, Across.Middle, Down.Middle);
        }
    }
}

/// <summary>
/// Where one value sits between two ends: its title and value over a line, the line holding its zones as bands with their names under
/// them, a marker at the value, and the page's words (or the numbers) under the two ends.
/// </summary>
public sealed partial class RangeView : KindControl
{
    private readonly Models.RangeScale range;

    /// <summary>Draws a range.</summary>
    /// <param name="range">The range.</param>
    public RangeView(Models.RangeScale range)
    {
        this.range = range;
        Start();
    }

    private string Amount(double value) => NumberWords.WithUnit(NumberWords.Amount(value), range.Unit);

    internal override UIElement Build(Palette palette)
    {
        int? current = ZoneRules.IndexOf(range.Value, range.Zones);
        StackPanel stack = new() { Spacing = 8 };

        Grid top = new() { ColumnSpacing = 8 };
        top.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        top.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        UIElement? heading = HealthParts.Heading(range.Title, range.Hint);
        if (heading is not null)
        {
            top.Children.Add(heading);
        }

        StackPanel value = new() { Orientation = Orientation.Horizontal, Spacing = 3, VerticalAlignment = VerticalAlignment.Center };
        TextBlock number = new()
        {
            Text = range.Value is double found ? range.Shown ?? NumberWords.Amount(found) : "–",
            FontSize = 20,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
            VerticalAlignment = VerticalAlignment.Center,
        };
        Typography.SetNumeralAlignment(number, FontNumeralAlignment.Tabular);
        value.Children.Add(HealthParts.Hidden(number));
        if (range.Value is not null && range.Unit is not null)
        {
            TextBlock unit = Ink.Words(range.Unit, 12, palette.Secondary, strong: true);
            unit.VerticalAlignment = VerticalAlignment.Bottom;
            unit.Margin = new Thickness(0, 0, 0, 3);
            value.Children.Add(HealthParts.Hidden(unit));
        }

        Grid.SetColumn(value, 1);
        top.Children.Add(value);
        stack.Children.Add(top);

        string said = SpeechRules.Value(range.Summary ?? range.Shown ?? (range.Value is double v ? Amount(v) : string.Empty), range.Title);
        double height = 8 * TextScale.Capped(1.5);
        PlotSurface line = new()
        {
            Height = Math.Max(height + 10, 18),
            AccessName = string.IsNullOrEmpty(range.Title) ? said : SpeechRules.Fact(range.Title, said),
            AccessType = "range",
            Range = range.Value is double at ? (range.Min, range.Max, Math.Clamp(at, range.Min, range.Max)) : null,
        };
        line.Painter = canvas => PaintLine(canvas, height, current);
        stack.Children.Add(line);

        Grid ends = new() { ColumnSpacing = 8 };
        ends.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        ends.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        ends.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        ends.Children.Add(Ink.Words(range.Low ?? Amount(range.Min), 12, palette.Secondary));
        string? middle = current is int zone ? range.Zones[zone].Label : range.Zones.FirstOrDefault()?.Label;
        if (middle is not null)
        {
            Brush ink = current is int zoned ? palette.Words(range.Zones[zoned].Colour, Models.Hue.Neutral) : palette.Secondary;
            TextBlock label = Ink.Words(middle, 12, ink, strong: current is not null);
            label.HorizontalAlignment = HorizontalAlignment.Center;
            Grid.SetColumn(label, 1);
            ends.Children.Add(label);
        }

        TextBlock high = Ink.Words(range.High ?? Amount(range.Max), 12, palette.Secondary);
        Grid.SetColumn(high, 2);
        ends.Children.Add(high);
        stack.Children.Add(HealthParts.Hidden(ends));
        return stack;
    }

    private void PaintLine(PlotCanvas canvas, double height, int? current)
    {
        Palette palette = canvas.Palette;
        double middle = canvas.Height / 2;
        double width = canvas.Width;
        canvas.Shapes.Put(Ink.Box(width, height, palette.Track(Palette.Resource("TextFillColorSecondaryBrush") is SolidColorBrush ink ? ink.Color : Microsoft.UI.Colors.Gray), height / 2), 0, middle - (height / 2));
        foreach (Models.Zone zone in range.Zones)
        {
            double low = AxisRules.Position(Math.Min(zone.From, zone.To), range.Min, range.Max);
            double high = AxisRules.Position(Math.Max(zone.From, zone.To), range.Min, range.Max);
            double size = Math.Max((high - low) * width, height);
            canvas.Shapes.Put(Ink.Box(size, height, palette.Fill(zone.Colour, Models.Hue.Neutral, 0.55), height / 2), Math.Min(low * width, width - size), middle - (height / 2));
        }

        if (range.Value is double value)
        {
            Windows.UI.Color colour = current is int zone ? palette.Of(range.Zones[zone].Colour, Models.Hue.Neutral) : ((SolidColorBrush)palette.Primary).Color;
            double marker = height + 10;
            double x = Math.Clamp(AxisRules.Position(value, range.Min, range.Max) * width, marker / 2, width - (marker / 2));
            canvas.Shapes.Put(Ink.Disc(marker, Palette.Mark(colour), palette.Card, 3), x - (marker / 2), middle - (marker / 2));
        }
    }
}
