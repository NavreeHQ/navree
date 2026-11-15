using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Navree.Controls.Drawing;
using Navree.Controls.Health;
using Navree.Design;
using Navree.Protocol;
using Windows.Foundation;

namespace Navree.Controls.Charts;

/// <summary>Where the parts of a bar stand along it: the same arithmetic the bar is drawn with, so a read-out and a word inside a part land on it.</summary>
internal static class BarSlots
{
    /// <summary>The start and size of each part along a bar this long, by its share, with a hair between parts.</summary>
    internal static IReadOnlyList<(int Index, double Start, double Size)> Of(double length, IReadOnlyList<double> shares, double hair) =>
        BarRules.Slots(length, shares, hair);

    /// <summary>White or black words, whichever reads on a colour.</summary>
    internal static Windows.UI.Color Words(Windows.UI.Color ground)
    {
        Rgb rgb = new(ground.R / 255.0, ground.G / 255.0, ground.B / 255.0);
        return ContrastRules.Contrast(rgb, new Rgb(1, 1, 1)) >= 3 ? Microsoft.UI.Colors.White : Microsoft.UI.Colors.Black;
    }
}

/// <summary>A meter: one bar of segments that add up to the whole, each in its colour with a hair between them and its share written inside where it fits; under it, each segment's amount in the page's words and what the page says of it.</summary>
public sealed partial class MeterView : KindControl
{
    private readonly Models.Meter meter;

    /// <summary>Draws a meter.</summary>
    /// <param name="meter">The meter.</param>
    public MeterView(Models.Meter meter)
    {
        this.meter = meter;
        Start();
    }

    private string Sentence() =>
        string.Join(", ", meter.Segments.Select(segment => string.Join(" ", new[] { segment.Name, segment.Shown, segment.Detail }.Where(part => !string.IsNullOrEmpty(part)))));

    private Windows.UI.Color Colour(Palette palette, int at) => palette.Of(meter.Segments[at].Colour, HueRules.SeriesHue(at));

    internal override UIElement Build(Palette palette)
    {
        StackPanel stack = new() { Spacing = 12 };
        UIElement? heading = HealthParts.Heading(meter.Title, meter.Hint);
        if (heading is not null)
        {
            stack.Children.Add(heading);
        }

        string said = SpeechRules.Value(meter.Summary ?? Sentence(), meter.Title);
        PlotSurface bar = new()
        {
            Height = 26 * TextScale.Capped(1.5),
            Reads = true,
            MirrorInRtl = true,
            AccessName = string.IsNullOrEmpty(meter.Title) ? said : SpeechRules.Fact(meter.Title, said),
            AccessType = KindStrings.TypeMeter,
        };
        bar.Painter = Paint;
        stack.Children.Add(bar);

        Grid rows = new() { ColumnSpacing = 10, RowSpacing = 8 };
        rows.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        rows.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        rows.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        for (int at = 0; at < meter.Segments.Count; at++)
        {
            Models.Segment segment = meter.Segments[at];
            rows.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            Microsoft.UI.Xaml.Shapes.Ellipse dot = new() { Width = 9, Height = 9, Fill = Palette.Mark(Colour(palette, at)), VerticalAlignment = VerticalAlignment.Center };
            Grid.SetRow(dot, at);
            rows.Children.Add(dot);
            StackPanel names = new();
            names.Children.Add(new TextBlock { Text = segment.Name, TextWrapping = TextWrapping.Wrap });
            if (segment.Detail is not null)
            {
                names.Children.Add(Ink.Words(segment.Detail, 12, palette.Secondary));
            }

            Grid.SetRow(names, at);
            Grid.SetColumn(names, 1);
            rows.Children.Add(names);
            TextBlock value = new()
            {
                Text = segment.Shown ?? (segment.Value is double amount ? NumberWords.Amount(amount) : "–"),
                FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
                VerticalAlignment = VerticalAlignment.Top,
            };
            Grid.SetRow(value, at);
            Grid.SetColumn(value, 2);
            rows.Children.Add(value);
        }

        stack.Children.Add(HealthParts.Hidden(rows));
        return stack;
    }

    private void Paint(PlotCanvas canvas)
    {
        Palette palette = canvas.Palette;
        IReadOnlyList<double> shares = ShareRules.Shares([.. meter.Segments.Select(segment => segment.Value)]);
        const double hair = 2;
        Ink.StackedBar(
            canvas.Shapes,
            across: true,
            0,
            0,
            canvas.Width,
            canvas.Height,
            [.. shares.Select((share, at) => (share, (Brush)Palette.Mark(Colour(palette, at))))],
            hair,
            6);

        foreach ((int index, double start, double size) in BarSlots.Of(canvas.Width, shares, hair))
        {
            Models.Segment segment = meter.Segments[index];
            string words = ShareRules.Percent(shares[index]).ToString(CultureInfo.InvariantCulture) + "%";
            TextBlock inside = Ink.Words(words, 12, Palette.Mark(BarSlots.Words(Colour(palette, index))), strong: true);
            if (ShareRules.SaysInside(shares[index], canvas.Width, Ink.Measured(inside).Width))
            {
                canvas.Text(inside, start + (size / 2), canvas.Height / 2, Across.Middle, Down.Middle);
            }

            string amount = segment.Shown ?? (segment.Value is double value ? NumberWords.Amount(value) : "–");
            List<ProbeLine> lines = [new ProbeLine(Colour(palette, index), amount + " (" + words + ")")];
            if (segment.Detail is not null)
            {
                lines.Add(new ProbeLine(null, segment.Detail));
            }

            canvas.Probes.Add(new Probe(
                canvas.Mx(start + (size / 2)),
                0,
                canvas.Mrect(start, 0, size, canvas.Height),
                segment.Name,
                lines,
                segment.Name + ", " + amount + ", " + words));
        }
    }
}

/// <summary>Columns: one an item, each split into the same parts and drawn whole, so they compare what each is made of rather than how big it is. The column that stands out is drawn in full and the others quieter.</summary>
public sealed partial class ColumnsView : KindControl
{
    private readonly Models.MiniColumns columns;

    /// <summary>Draws columns.</summary>
    /// <param name="columns">The columns.</param>
    public ColumnsView(Models.MiniColumns columns)
    {
        this.columns = columns;
        Start();
    }

    private Windows.UI.Color Colour(Palette palette, int part) =>
        palette.Of(part < columns.Parts.Count ? columns.Parts[part].Colour : null, HueRules.SeriesHue(part));

    private string Sentence() =>
        string.Join(
            ", ",
            columns.Columns.Select(column =>
            {
                IEnumerable<string> parts = columns.Parts.Zip(column.Values, (part, value) => value is double found ? part.Name + " " + NumberWords.Amount(found) : string.Empty).Where(text => text.Length > 0);
                return string.Join(" ", new[] { column.Label, column.Shown }.Where(text => !string.IsNullOrEmpty(text)).Concat(parts));
            }));

    internal override UIElement Build(Palette palette)
    {
        StackPanel stack = new() { Spacing = 12 };
        UIElement? heading = HealthParts.Heading(columns.Title, columns.Hint);
        if (heading is not null)
        {
            stack.Children.Add(heading);
        }

        string said = SpeechRules.Value(columns.Summary ?? Sentence(), columns.Title);
        PlotSurface plot = new()
        {
            Height = (120 * TextScale.Capped(1.4)) + 40,
            Reads = true,
            MirrorInRtl = true,
            AccessName = string.IsNullOrEmpty(columns.Title) ? said : SpeechRules.Fact(columns.Title, said),
            AccessType = KindStrings.TypeChart,
        };
        plot.Painter = Paint;
        stack.Children.Add(plot);
        stack.Children.Add(HealthParts.Key(palette, columns.Parts.Select((part, at) => (Colour(palette, at), part.Name, part.Detail))));
        return stack;
    }

    private void Paint(PlotCanvas canvas)
    {
        Palette palette = canvas.Palette;
        int count = columns.Columns.Count;
        if (count == 0)
        {
            return;
        }

        const double top = 18;
        const double foot = 20;
        (double width, double gap) = BarRules.Columns(canvas.Width, count, 10);
        double height = Math.Max(canvas.Height - top - foot, 10);
        for (int at = 0; at < count; at++)
        {
            Models.MiniColumn column = columns.Columns[at];
            double x = at * (width + gap);
            bool quiet = columns.Highlight is long chosen && chosen != at;
            IReadOnlyList<double> shares = ShareRules.Shares(column.Values);
            Ink.StackedBar(
                canvas.Shapes,
                across: false,
                x,
                top,
                width,
                height,
                [.. shares.Select((share, part) => (share, (Brush)Palette.Mark(Colour(palette, part), quiet ? 0.4 : 1)))],
                1.5,
                5);
            if (column.Shown is not null)
            {
                canvas.Text(Ink.Words(column.Shown, 11, palette.Primary, strong: true), x + (width / 2), top - 4, Across.Middle, Down.Bottom);
            }

            TextBlock label = canvas.Caption(column.Label);
            label.MaxWidth = Math.Max(width, 20);
            label.TextTrimming = TextTrimming.CharacterEllipsis;
            canvas.Text(label, x + (width / 2), top + height + 4, Across.Middle, Down.Top);

            List<ProbeLine> lines = [];
            for (int part = 0; part < shares.Count && part < columns.Parts.Count; part++)
            {
                if (column.Values[part] is double value)
                {
                    lines.Add(new ProbeLine(Colour(palette, part), columns.Parts[part].Name + "  " + NumberWords.Amount(value)));
                }
            }

            canvas.Probes.Add(new Probe(
                canvas.Mx(x + (width / 2)),
                top,
                canvas.Mrect(x, 0, width, canvas.Height),
                column.Shown is null ? column.Label : column.Label + " · " + column.Shown,
                lines,
                column.Label + ", " + string.Join(", ", lines.Select(line => line.Text))));
        }
    }
}

/// <summary>A strip: equal cells that touch, each in the colour of its kind and a quiet cell where there is no reading; the first and last cells' words under its ends, and a key that says each kind's share.</summary>
public sealed partial class StripView : KindControl
{
    private readonly Models.ColorStrip strip;

    /// <summary>Draws a strip.</summary>
    /// <param name="strip">The strip.</param>
    public StripView(Models.ColorStrip strip)
    {
        this.strip = strip;
        Start();
    }

    private int? KindOf(Models.StripCell cell) =>
        cell.Category is long category && category >= 0 && category < strip.Key.Count ? (int)category : null;

    private Windows.UI.Color Colour(Palette palette, int kind) => palette.Of(strip.Key[kind].Colour, HueRules.SeriesHue(kind));

    internal override UIElement Build(Palette palette)
    {
        StackPanel stack = new() { Spacing = 8 };
        UIElement? heading = HealthParts.Heading(strip.Title, strip.Hint);
        if (heading is not null)
        {
            stack.Children.Add(heading);
        }

        IReadOnlyList<double> shares = strip.Shares ? ShareRules.CategoryShares(strip.Cells, strip.Key.Count) : [];
        List<(Windows.UI.Color Colour, string Name, string? Detail)> entries = [];
        for (int at = 0; at < strip.Key.Count; at++)
        {
            string? detail = strip.Key[at].Detail ?? (at < shares.Count ? ShareRules.Percent(shares[at]).ToString(CultureInfo.InvariantCulture) + "%" : null);
            entries.Add((Colour(palette, at), strip.Key[at].Name, detail));
        }

        string said = SpeechRules.Value(strip.Summary ?? string.Join(", ", entries.Select(entry => KeyRules.Entry(entry.Name, entry.Detail))), strip.Title);
        PlotSurface bar = new()
        {
            Height = 28 * TextScale.Capped(1.5),
            Reads = true,
            MirrorInRtl = true,
            AccessName = string.IsNullOrEmpty(strip.Title) ? said : SpeechRules.Fact(strip.Title, said),
            AccessType = KindStrings.TypeStrip,
        };
        bar.Painter = Paint;
        stack.Children.Add(bar);

        if (strip.Cells.Count > 0)
        {
            Grid words = new() { ColumnSpacing = 8 };
            words.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            words.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            words.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            words.Children.Add(Ink.Words(strip.Cells[0].Label, 12, palette.Secondary));
            if (strip.Label is not null)
            {
                TextBlock middle = Ink.Words(strip.Label, 12, palette.Secondary);
                middle.HorizontalAlignment = HorizontalAlignment.Center;
                Grid.SetColumn(middle, 1);
                words.Children.Add(middle);
            }

            TextBlock last = Ink.Words(strip.Cells[^1].Label, 12, palette.Secondary);
            Grid.SetColumn(last, 2);
            words.Children.Add(last);
            stack.Children.Add(HealthParts.Hidden(words));
        }

        stack.Children.Add(HealthParts.Key(palette, entries));
        return stack;
    }

    private void Paint(PlotCanvas canvas)
    {
        Palette palette = canvas.Palette;
        int count = strip.Cells.Count;
        if (count == 0)
        {
            return;
        }

        Windows.UI.Color quiet = palette.Of(Models.Hue.Neutral);

        // Neighbouring cells of one kind are one piece, so a night of a hundred cells is a handful of shapes.
        List<(int? Kind, int Cells)> runs = [];
        foreach (Models.StripCell cell in strip.Cells)
        {
            int? kind = KindOf(cell);
            if (runs.Count > 0 && runs[^1].Kind == kind)
            {
                runs[^1] = (kind, runs[^1].Cells + 1);
            }
            else
            {
                runs.Add((kind, 1));
            }
        }

        Ink.StackedBar(
            canvas.Shapes,
            across: true,
            0,
            0,
            canvas.Width,
            canvas.Height,
            [.. runs.Select(run => ((double)run.Cells / count, (Brush)(run.Kind is int kind ? Palette.Mark(Colour(palette, kind)) : Palette.Mark(quiet, 0.15))))],
            0,
            6);

        double width = canvas.Width / count;
        for (int at = 0; at < count; at++)
        {
            Models.StripCell cell = strip.Cells[at];
            int? kind = KindOf(cell);
            string words = kind is int found ? KeyRules.Entry(strip.Key[found].Name, strip.Key[found].Detail) : KindStrings.NoReading;
            canvas.Probes.Add(new Probe(
                canvas.Mx((at + 0.5) * width),
                0,
                canvas.Mrect(at * width, 0, width, canvas.Height),
                cell.Label,
                [new ProbeLine(kind is int shown ? Colour(palette, shown) : null, words)],
                cell.Label + ", " + words));
        }
    }
}
