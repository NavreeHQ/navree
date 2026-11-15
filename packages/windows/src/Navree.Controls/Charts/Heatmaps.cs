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

/// <summary>
/// The calendar of days: a week a column and a weekday a row, from the person's own first weekday, the months named along the top in the
/// person's language, four shades of one colour from the page's steps, and no shade at all for a day with no reading (no data is not a
/// day of none).
/// </summary>
public sealed partial class CalendarHeatView : KindControl
{
    private const double Label = 11;
    private readonly Models.CalendarHeatmap heatmap;
    private readonly CultureInfo culture = ClientCulture.Current;

    /// <summary>Draws a calendar.</summary>
    /// <param name="heatmap">The calendar.</param>
    public CalendarHeatView(Models.CalendarHeatmap heatmap)
    {
        this.heatmap = heatmap;
        Start();
    }

    private DayOfWeek FirstDay => culture.DateTimeFormat.FirstDayOfWeek;

    private List<(DateOnly Date, double? Value)> Days =>
        [.. heatmap.Days.Select(day => (Date: PagerRules.Day(day.Date), day.Value)).Where(day => day.Date.HasValue).Select(day => (day.Date!.Value, day.Value))];

    private (DateOnly First, DateOnly Last) Span()
    {
        List<(DateOnly Date, double? Value)> days = Days;
        DateOnly today = DateOnly.FromDateTime(DateTime.Today);
        DateOnly first = PagerRules.Day(heatmap.First) ?? (days.Count > 0 ? days.Min(day => day.Date) : today);
        DateOnly last = PagerRules.Day(heatmap.Last) ?? (days.Count > 0 ? days.Max(day => day.Date) : first);
        return (first, last);
    }

    private double Highest => Days.Where(day => day.Value.HasValue).Select(day => day.Value!.Value).DefaultIfEmpty(1).Max();

    private double Step(double width)
    {
        (DateOnly first, DateOnly last) = Span();
        int weeks = HeatRules.Weeks(first, last, FirstDay);
        return Math.Min((width - (Label * 1.6)) / weeks, 20);
    }

    internal override UIElement Build(Palette palette)
    {
        StackPanel stack = new() { Spacing = 10 };
        UIElement? heading = HealthParts.Heading(heatmap.Title, heatmap.Hint);
        if (heading is not null)
        {
            stack.Children.Add(heading);
        }

        string said = SpeechRules.Value(heatmap.Summary ?? string.Empty, heatmap.Title);
        PlotSurface plot = new()
        {
            Reads = true,
            MirrorInRtl = true,
            AccessName = string.IsNullOrEmpty(heatmap.Title) ? said : SpeechRules.Fact(heatmap.Title, said),
            AccessType = "calendar",
            HeightFor = width => Label + 6 + (Math.Max(Step(width), 4) * 7),
        };
        plot.Painter = Paint;
        stack.Children.Add(plot);
        stack.Children.Add(HealthParts.Hidden(Legend(palette)));
        return stack;
    }

    private UIElement Legend(Palette palette)
    {
        double[] bounds = HeatRules.Bounds(heatmap.Steps, Highest);
        string[] words = ["0", .. bounds.Select(bound => NumberWords.Amount(bound, culture))];
        Windows.UI.Color colour = palette.Of(heatmap.Colour, Models.Hue.Accent);
        Composition.WrapPanel flow = new() { HorizontalSpacing = 12, VerticalSpacing = 4 };
        if (heatmap.Label is not null)
        {
            flow.Children.Add(Ink.Words(heatmap.Label, 12, palette.Secondary));
        }

        for (int shade = 0; shade < 4; shade++)
        {
            StackPanel entry = new() { Orientation = Orientation.Horizontal, Spacing = 4 };
            entry.Children.Add(Ink.Box(11, 11, Palette.Mark(colour, HeatRules.Opacities[shade]), 2.5));
            entry.Children.Add(Ink.Words(shade == 3 ? words[shade] + "+" : words[shade], 12, palette.Secondary));
            flow.Children.Add(entry);
        }

        return flow;
    }

    private void Paint(PlotCanvas canvas)
    {
        Palette palette = canvas.Palette;
        (DateOnly first, DateOnly last) = Span();
        int weeks = HeatRules.Weeks(first, last, FirstDay);
        double gutter = Label * 1.6;
        double step = Math.Max(Math.Min((canvas.Width - gutter) / weeks, 20), 4);
        double cell = Math.Max(step - 2, 2);
        double top = Label + 6;
        DateOnly start = HeatRules.Start(first, FirstDay);
        double highest = Highest;
        Windows.UI.Color colour = palette.Of(heatmap.Colour, Models.Hue.Accent);

        foreach ((int week, int month) in HeatRules.Months(first, weeks, FirstDay))
        {
            canvas.Text(canvas.Caption(culture.DateTimeFormat.AbbreviatedMonthNames[month - 1]), gutter + (week * step), 0, Across.Start, Down.Top);
        }

        foreach (int row in new[] { 1, 3, 5 })
        {
            string name = culture.DateTimeFormat.ShortestDayNames[(((int)FirstDay) + row) % 7];
            canvas.Text(canvas.Caption(name), 0, top + (row * step) + (cell / 2), Across.Start, Down.Middle);
        }

        foreach ((DateOnly date, double? value) in Days)
        {
            (int week, int day) = HeatRules.Cell(date, start);
            if (week < 0 || week >= weeks)
            {
                continue;
            }

            double x = gutter + (week * step);
            double y = top + (day * step);
            int? shade = HeatRules.Shade(value, heatmap.Steps, highest);
            if (shade is int deep)
            {
                canvas.Shapes.Put(Ink.Box(cell, cell, Palette.Mark(colour, HeatRules.Opacities[deep]), cell * 0.22), x, y);
            }
            else
            {
                canvas.Shapes.Put(Ink.Box(cell, cell, null, cell * 0.22, Palette.Mark(palette.Of(Models.Hue.Neutral), 0.35), 1), x, y);
            }

            string words = value is double found ? NumberWords.WithUnit(NumberWords.Amount(found, culture), heatmap.Unit) : "No reading";
            canvas.Probes.Add(new Probe(
                canvas.Mx(x + (cell / 2)),
                y,
                canvas.Mrect(x, y, cell, cell),
                date.ToString("ddd d MMM yyyy", culture),
                [new ProbeLine(shade is null ? null : colour, words)],
                date.ToString("D", culture) + ", " + words));
        }
    }
}

/// <summary>
/// Every measure against every other: a cell a pair, in one colour for measures that rise together and another for those that pull
/// apart, deeper the stronger; a pair that may be chance is grey, one never tested blank.
/// </summary>
public sealed partial class MatrixHeatView : KindControl
{
    private readonly Models.MatrixHeatmap matrix;

    /// <summary>Draws a matrix of relations.</summary>
    /// <param name="matrix">The matrix.</param>
    public MatrixHeatView(Models.MatrixHeatmap matrix)
    {
        this.matrix = matrix;
        Start();
    }

    private List<int> Rows => matrix.Lower ? [.. Enumerable.Range(1, Math.Max(matrix.Measures.Count - 1, 0))] : [.. Enumerable.Range(0, matrix.Measures.Count)];

    private List<int> Columns => matrix.Lower ? [.. Enumerable.Range(0, Math.Max(matrix.Measures.Count - 1, 0))] : [.. Enumerable.Range(0, matrix.Measures.Count)];

    private double LeftWidth() => matrix.Measures.Select(name => Ink.Measured(Ink.Words(name, 12, Palette.Resource("TextFillColorSecondaryBrush"))).Width).DefaultIfEmpty(0).Max() + 8;

    private double CellFor(double width) => Math.Max(Math.Min(44 * TextScale.Capped(1.4), (width - LeftWidth()) / Math.Max(Columns.Count, 1) - 3), 16);

    internal override UIElement Build(Palette palette)
    {
        StackPanel stack = new() { Spacing = 10 };
        UIElement? heading = HealthParts.Heading(matrix.Title, matrix.Hint);
        if (heading is not null)
        {
            stack.Children.Add(heading);
        }

        string said = SpeechRules.Value(matrix.Summary ?? string.Empty, matrix.Title);
        PlotSurface plot = new()
        {
            Reads = true,
            MirrorInRtl = true,
            AccessName = string.IsNullOrEmpty(matrix.Title) ? said : SpeechRules.Fact(matrix.Title, said),
            AccessType = "matrix",
            HeightFor = width => (Rows.Count * (CellFor(width) + 3)) + 22,
        };
        plot.Painter = Paint;
        stack.Children.Add(plot);
        return stack;
    }

    private void Paint(PlotCanvas canvas)
    {
        Palette palette = canvas.Palette;
        List<int> rows = Rows;
        List<int> columns = Columns;
        double left = LeftWidth();
        double cell = CellFor(canvas.Width);
        double gap = 3;
        for (int row = 0; row < rows.Count; row++)
        {
            double y = row * (cell + gap);
            canvas.Text(Ink.Words(matrix.Measures[rows[row]], 12, palette.Secondary), left - 8, y + (cell / 2), Across.End, Down.Middle);
            for (int column = 0; column < columns.Count; column++)
            {
                int a = rows[row];
                int b = columns[column];
                if (!MatrixRules.Drawn(a, b, matrix.Lower))
                {
                    continue;
                }

                double x = left + (column * (cell + gap));
                Models.Relation? relation = MatrixRules.Relation(matrix.Relations, a, b);
                double strength = MatrixRules.Strength(relation, matrix.Peak);
                double value = relation?.Value ?? 0;
                Brush? fill;
                if (relation?.Value is null)
                {
                    fill = null;
                }
                else if (!relation.Significant)
                {
                    fill = Palette.Mark(palette.Of(Models.Hue.Neutral), 0.25);
                }
                else
                {
                    fill = Palette.Mark(palette.Of(value >= 0 ? matrix.Positive : matrix.Negative, value >= 0 ? Models.Hue.Blue : Models.Hue.Orange), 0.15 + (0.85 * strength));
                }

                canvas.Shapes.Put(
                    Ink.Box(cell, cell, fill, 6, fill is null ? Palette.Mark(palette.Of(Models.Hue.Neutral), 0.35) : null, fill is null ? 1 : 0),
                    x,
                    y);
                string words = relation?.Value is null ? "Not tested" : value.ToString("0.00", ClientCulture.Current);
                if (relation?.Value is not null)
                {
                    Brush ink = strength > 0.55 && relation.Significant ? Palette.Mark(Microsoft.UI.Colors.White) : palette.Primary;
                    canvas.Text(Ink.Words(words, 11, ink, strong: true), x + (cell / 2), y + (cell / 2), Across.Middle, Down.Middle);
                }

                string pair = matrix.Measures[a] + " and " + matrix.Measures[b];
                List<ProbeLine> lines = [new ProbeLine(null, words)];
                if (relation is not null && !relation.Significant)
                {
                    lines.Add(new ProbeLine(null, "May be chance"));
                }

                if (relation?.Note is string note)
                {
                    lines.Add(new ProbeLine(null, note));
                }

                canvas.Probes.Add(new Probe(canvas.Mx(x + (cell / 2)), y, canvas.Mrect(x, y, cell, cell), pair, lines, pair + ", " + string.Join(", ", lines.Select(line => line.Text))));
            }
        }

        double foot = rows.Count * (cell + gap);
        for (int column = 0; column < columns.Count; column++)
        {
            TextBlock name = Ink.Words(matrix.Measures[columns[column]], 12, palette.Secondary);
            name.MaxWidth = cell + gap;
            name.TextTrimming = TextTrimming.CharacterEllipsis;
            canvas.Text(name, left + (column * (cell + gap)) + (cell / 2), foot + 2, Across.Middle, Down.Top);
        }
    }
}

/// <summary>
/// Clouds of points on two numeric axes, each cloud in its colour and seen through, with the page's reference lines; with equal axes
/// the plot is square and a diagonal is the line of no change.
/// </summary>
public sealed partial class ScatterView : KindControl
{
    private readonly Models.Scatter scatter;

    /// <summary>Draws a scatter.</summary>
    /// <param name="scatter">The scatter.</param>
    public ScatterView(Models.Scatter scatter)
    {
        this.scatter = scatter;
        Start();
    }

    private static string AxisWords(Models.Measure? measure) =>
        measure is null ? string.Empty : string.Join(" ", new[] { measure.Name, measure.Unit is null ? null : "(" + measure.Unit + ")" }.Where(part => !string.IsNullOrEmpty(part)));

    internal override UIElement Build(Palette palette)
    {
        StackPanel stack = new() { Spacing = 10 };
        UIElement? heading = HealthParts.Heading(scatter.Title, scatter.Hint);
        if (heading is not null)
        {
            stack.Children.Add(heading);
        }

        string said = SpeechRules.Value(scatter.Summary ?? string.Empty, scatter.Title);
        PlotSurface plot = new()
        {
            Reads = true,
            MirrorInRtl = true,
            AccessName = string.IsNullOrEmpty(scatter.Title) ? said : SpeechRules.Fact(scatter.Title, said),
            AccessType = "scatter chart",
            HeightFor = width => Math.Max(scatter.EqualAxes ? Math.Min(width, 380) : Math.Min(width / 1.4, 320), 160),
        };
        plot.Painter = Paint;
        stack.Children.Add(plot);
        if (scatter.Series.Count > 1)
        {
            stack.Children.Add(HealthParts.Key(palette, scatter.Series.Select((series, at) => (palette.Of(series.Colour, HueRules.SeriesHue(at)), series.Name, (string?)null))));
        }

        return stack;
    }

    private void Paint(PlotCanvas canvas)
    {
        Palette palette = canvas.Palette;
        (AxisRules.Bounds x, AxisRules.Bounds y) = AxisRules.Scatter(scatter);
        IReadOnlyList<double> xMarks = AxisRules.Marks(x, 5);
        IReadOnlyList<double> yMarks = AxisRules.Marks(y, 4);
        string xName = AxisWords(scatter.X);
        string yName = AxisWords(scatter.Y);
        double widest = yMarks.Select(mark => Ink.Measured(canvas.Caption(NumberWords.Amount(mark))).Width).DefaultIfEmpty(0).Max();
        double x0 = widest + 10;
        double x1 = canvas.Width - 10;
        double y0 = yName.Length > 0 ? 18 : 6;
        double y1 = canvas.Height - (xName.Length > 0 ? 38 : 22);
        double X(double value) => x0 + ((x1 - x0) * AxisRules.Position(value, x.Low, x.High));
        double Y(double value) => y1 - ((y1 - y0) * AxisRules.Position(value, y.Low, y.High));

        foreach (double mark in yMarks)
        {
            canvas.Shapes.Put(Ink.Rule(x0, Y(mark), x1, Y(mark), palette.Divider), 0, 0);
            canvas.Text(canvas.Caption(NumberWords.Amount(mark)), x0 - 6, Y(mark), Across.End, Down.Middle);
        }

        foreach (double mark in xMarks)
        {
            canvas.Shapes.Put(Ink.Rule(X(mark), y0, X(mark), y1, palette.Divider), 0, 0);
            canvas.Text(canvas.Caption(NumberWords.Amount(mark)), X(mark), y1 + 5, Across.Middle, Down.Top);
        }

        if (yName.Length > 0)
        {
            canvas.Text(canvas.Caption(yName), 0, 0, Across.Start, Down.Top);
        }

        if (xName.Length > 0)
        {
            canvas.Text(canvas.Caption(xName), (x0 + x1) / 2, canvas.Height - 2, Across.Middle, Down.Bottom);
        }

        foreach (Models.Reference reference in scatter.References)
        {
            Brush ink = palette.Secondary;
            if (reference.Kind == Models.ReferenceKind.Diagonal)
            {
                canvas.Shapes.Put(Ink.Rule(X(x.Low), Y(x.Low), X(x.High), Y(x.High), ink, 1, [4, 3]), 0, 0);
            }
            else if (reference.Value is double value)
            {
                canvas.Shapes.Put(Ink.Rule(x0, Y(value), x1, Y(value), ink, 1, [4, 3]), 0, 0);
            }
        }

        for (int at = 0; at < scatter.Series.Count; at++)
        {
            Models.PointSeries series = scatter.Series[at];
            Windows.UI.Color colour = palette.Of(series.Colour, HueRules.SeriesHue(at));
            foreach (Models.Point point in series.Points)
            {
                double px = X(point.X);
                double py = Y(point.Y);
                canvas.Shapes.Put(Ink.Disc(8, Palette.Mark(colour, 0.6)), px - 4, py - 4);
                string words = (scatter.X?.Name ?? "x") + " " + NumberWords.Amount(point.X) + ", " + (scatter.Y?.Name ?? "y") + " " + NumberWords.Amount(point.Y);
                canvas.Probes.Add(new Probe(
                    canvas.Mx(px),
                    py,
                    default,
                    point.Label ?? series.Name,
                    [new ProbeLine(colour, words)],
                    (point.Label ?? series.Name) + ", " + words,
                    Marks: [(new Point(canvas.Mx(px), py), colour)]));
            }
        }
    }
}
