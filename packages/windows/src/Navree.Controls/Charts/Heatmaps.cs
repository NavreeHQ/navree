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

/// <summary>How a shade is drawn where there are no blends of a colour: an outline with a part of its area filled, the more the deeper.</summary>
internal static class Shades
{
    /// <summary>A shade in a contrast theme: the cell's outline, and inside it a square that covers a share of the cell's area.</summary>
    internal static void Put(Canvas canvas, Windows.UI.Color ink, double x, double y, double cell, double share, double radius)
    {
        canvas.Put(Ink.Box(cell, cell, null, radius, Palette.Mark(ink), 1), x, y);
        double inner = Math.Max((cell - 2) * Math.Sqrt(Guard.Fraction(share)), 0);
        if (inner >= 1)
        {
            canvas.Put(Ink.Box(inner, inner, Palette.Mark(ink), Math.Min(radius, inner / 2)), x + ((cell - inner) / 2), y + ((cell - inner) / 2));
        }
    }

    /// <summary>The same as a small swatch of a key.</summary>
    internal static UIElement Swatch(Palette palette, Windows.UI.Color colour, double share, int shade)
    {
        if (!palette.Contrast)
        {
            return Ink.Box(11, 11, Palette.Mark(colour, HeatRules.Opacities[shade]), 2.5);
        }

        Canvas swatch = new() { Width = 11, Height = 11 };
        Put(swatch, colour, 0, 0, 11, share, 2);
        return swatch;
    }
}

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
        return HeatRules.Step(width, Label * 1.6, HeatRules.Window(first, last, FirstDay).Weeks);
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
            AccessType = KindStrings.TypeCalendar,
            HeightFor = width => Label + 6 + (Step(width) * 7),
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
            entry.Children.Add(Shades.Swatch(palette, colour, (shade + 1) / 4.0, shade));
            entry.Children.Add(Ink.Words(shade == 3 ? words[shade] + "+" : words[shade], 12, palette.Secondary));
            flow.Children.Add(entry);
        }

        return flow;
    }

    private void Paint(PlotCanvas canvas)
    {
        Palette palette = canvas.Palette;
        (DateOnly first, DateOnly last) = Span();
        (DateOnly start, int weeks) = HeatRules.Window(first, last, FirstDay);
        double gutter = Label * 1.6;
        double step = HeatRules.Step(canvas.Width, gutter, weeks);
        double cell = HeatRules.Square(step);
        double top = Label + 6;
        double highest = Highest;
        Windows.UI.Color colour = palette.Of(heatmap.Colour, Models.Hue.Accent);

        foreach ((int week, int month) in HeatRules.Months(start, weeks, FirstDay))
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
            if (shade is int deep && palette.Contrast)
            {
                Shades.Put(canvas.Shapes, colour, x, y, cell, (deep + 1) / 4.0, cell * 0.22);
            }
            else if (shade is int shown)
            {
                canvas.Shapes.Put(Ink.Box(cell, cell, Palette.Mark(colour, HeatRules.Opacities[shown]), cell * 0.22), x, y);
            }
            else
            {
                // No reading is an empty outline, in the colour the system gives what is not in use in a contrast theme.
                canvas.Shapes.Put(Ink.Box(cell, cell, null, cell * 0.22, palette.Contrast ? palette.Track(colour) : Palette.Mark(palette.Of(Models.Hue.Neutral), 0.35), 1), x, y);
            }

            string words = value is double found ? NumberWords.WithUnit(NumberWords.Amount(found, culture), heatmap.Unit) : KindStrings.NoReading;
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

    private double LeftWidth(double width) =>
        MatrixRules.Left(matrix.Measures.Select(name => Ink.Measure(name, 12).Width).DefaultIfEmpty(0).Max(), width);

    private double CellFor(double width) => MatrixRules.Cell(width, LeftWidth(width), Columns.Count, 44 * TextScale.Capped(1.4));

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
            AccessType = KindStrings.TypeMatrix,
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
        double left = LeftWidth(canvas.Width);
        double cell = CellFor(canvas.Width);
        double gap = 3;
        for (int row = 0; row < rows.Count; row++)
        {
            double y = row * (cell + gap);
            TextBlock rowName = Ink.Words(matrix.Measures[rows[row]], 12, palette.Secondary);
            rowName.MaxWidth = Math.Max(left - 8, 1);
            canvas.Text(rowName, left - 8, y + (cell / 2), Across.End, Down.Middle);
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
                Windows.UI.Color markColour = default;
                double opacity = 1;
                bool significant = relation?.Value is not null && relation.Significant;
                if (relation?.Value is null)
                {
                    fill = null;
                }
                else if (!relation.Significant)
                {
                    markColour = palette.Of(Models.Hue.Neutral);
                    opacity = 0.25;
                    fill = Palette.Mark(markColour, opacity);
                }
                else
                {
                    markColour = palette.Of(value >= 0 ? matrix.Positive : matrix.Negative, value >= 0 ? Models.Hue.Blue : Models.Hue.Orange);
                    opacity = 0.15 + (0.85 * strength);
                    fill = Palette.Mark(markColour, opacity);
                }

                if (palette.Contrast)
                {
                    // Outlined cells: empty for a pair never tested, a little filled for one that may be chance, filled by how strong the relation is.
                    Windows.UI.Color grey = palette.Track(markColour).Color;
                    Shades.Put(canvas.Shapes, relation?.Value is null ? grey : significant ? markColour : grey, x, y, cell, relation?.Value is null ? 0 : significant ? 0.15 + (0.85 * strength) : 0.2, 6);
                }
                else
                {
                    canvas.Shapes.Put(
                        Ink.Box(cell, cell, fill, 6, fill is null ? Palette.Mark(palette.Of(Models.Hue.Neutral), 0.35) : null, fill is null ? 1 : 0),
                        x,
                        y);
                }
                string words = relation?.Value is null ? KindStrings.NotTested : value.ToString("0.00", ClientCulture.Current);
                if (relation?.Value is not null && MatrixRules.HoldsNumber(cell))
                {
                    // The words are in the ink that reads on what is under them: on a deep cell, the ink of its colour; else the page's text colour.
                    Brush ink = !significant ? palette.Primary
                        : palette.Contrast ? (strength > 0.55 ? palette.OnMark(markColour) : palette.Primary)
                        : palette.OnMark(markColour, opacity);
                    canvas.Text(Ink.Words(words, 11, ink, strong: true), x + (cell / 2), y + (cell / 2), Across.Middle, Down.Middle);
                }

                string pair = KindStrings.Pair(matrix.Measures[a], matrix.Measures[b]);
                List<ProbeLine> lines = [new ProbeLine(null, words)];
                if (relation is not null && !relation.Significant)
                {
                    lines.Add(new ProbeLine(null, KindStrings.MayBeChance));
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
            AccessType = KindStrings.TypeScatter,
            HeightFor = width => Math.Max(scatter.EqualAxes ? Math.Min(width, 380) : Math.Min(width / 1.4, 320), 160),
        };
        plot.Painter = Paint;
        stack.Children.Add(plot);
        if (scatter.Series.Count > 1)
        {
            stack.Children.Add(HealthParts.Key(palette, scatter.Series.Select((series, at) => (palette.Series(series.Colour, at), series.Name, (string?)null))));
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
        double widest = yMarks.Select(mark => Ink.Measure(NumberWords.Amount(mark), 11).Width).DefaultIfEmpty(0).Max();
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
            Windows.UI.Color colour = palette.Series(series.Colour, at);
            SeriesLook? look = palette.Look(at);
            foreach (Models.Point point in series.Points)
            {
                double px = X(point.X);
                double py = Y(point.Y);
                if (look is SeriesLook found)
                {
                    canvas.Shapes.Put(Ink.Marker(found.Shape, 9, Palette.Mark(colour)), px - 4.5, py - 4.5);
                }
                else
                {
                    canvas.Shapes.Put(Ink.Disc(8, Palette.Mark(colour, 0.6)), px - 4, py - 4);
                }

                string words = (scatter.X?.Name ?? KindStrings.AxisAcross) + " " + NumberWords.Amount(point.X) + ", " + (scatter.Y?.Name ?? KindStrings.AxisDown) + " " + NumberWords.Amount(point.Y);
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
