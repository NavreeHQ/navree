using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Navree.Controls.Drawing;
using Navree.Controls.Health;
using Navree.Design;
using Navree.Protocol;
using Windows.Foundation;
using ShapePath = Microsoft.UI.Xaml.Shapes.Path;

namespace Navree.Controls.Charts;

/// <summary>
/// A chart drawn from the numbers the server sent: each series in its colour, a gap left a gap, the page's reference lines dashed with
/// their words, its zones as bands, its long axes thinned to a handful of the page's own words. A chart of levels (a night's stages) is
/// drawn in lanes. It reads out the place under the pointer or the one the arrow keys have reached, and to a screen reader it is one
/// element that says the page's summary.
/// </summary>
public sealed partial class ChartView : KindControl
{
    private const double LabelSize = 11;
    private readonly Models.Chart chart;
    private readonly bool small;
    private readonly bool titled;

    /// <summary>Draws a chart.</summary>
    /// <param name="chart">The chart.</param>
    /// <param name="small">Whether it stands under a figure's number: smaller, with no heading and no value axis.</param>
    /// <param name="titled">Whether it says its own title (not when the screen has just said the same words over it).</param>
    public ChartView(Models.Chart chart, bool small = false, bool titled = true)
    {
        this.chart = chart;
        this.small = small;
        this.titled = titled;
        Start();
    }

    private bool Spark => chart.Kind == Models.ChartKind.Sparkline;

    private bool Lanes => chart.Levels.Count > 0;

    private bool Horizontal => chart.Horizontal || chart.Kind == Models.ChartKind.Funnel;

    private double PlotHeight
    {
        get
        {
            double scale = TextScale.Capped(1.4);
            if (Spark)
            {
                return 36 * scale;
            }

            if (Lanes)
            {
                return (34 * scale * chart.Levels.Count) + 24;
            }

            if (chart.Kind == Models.ChartKind.Donut)
            {
                return 176 * scale;
            }

            return (small ? 84 : 180) * scale;
        }
    }

    internal override UIElement Build(Palette palette)
    {
        StackPanel stack = new() { Spacing = 8 };
        if (!small && !Spark)
        {
            UIElement? heading = HealthParts.Heading(chart.Title, chart.Hint, titled);
            string? unit = chart.Levels.Count == 0 && chart.Kind != Models.ChartKind.Donut ? chart.Y?.Unit : null;
            if (heading is not null || unit is not null)
            {
                Grid top = new() { ColumnSpacing = 8 };
                top.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                top.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
                if (heading is not null)
                {
                    top.Children.Add(heading);
                }

                if (unit is not null)
                {
                    TextBlock words = HealthParts.Hidden(Ink.Words(unit, 12, palette.Secondary));
                    words.VerticalAlignment = VerticalAlignment.Center;
                    Grid.SetColumn(words, 1);
                    top.Children.Add(words);
                }

                stack.Children.Add(top);
            }
        }

        PlotSurface plot = new()
        {
            Height = PlotHeight,
            Reads = !Spark,
            MirrorInRtl = true,
            AccessName = string.IsNullOrEmpty(chart.Title) ? chart.Summary ?? Sentence() : SpeechRules.Fact(chart.Title, SpeechRules.Value(chart.Summary ?? Sentence(), chart.Title)),
            AccessType = KindStrings.TypeChart,
        };
        plot.Painter = Paint;
        stack.Children.Add(plot);

        if (chart.Kind == Models.ChartKind.Donut)
        {
            stack.Children.Add(HealthParts.Key(palette, DonutEntries(palette)));
        }
        else if (chart.Series.Count > 1 && !Lanes)
        {
            stack.Children.Add(HealthParts.Key(palette, chart.Series.Select((series, at) => (palette.Series(series.Colour, at), series.Label, (string?)null))));
        }

        UIElement? notes = Notes(palette);
        if (notes is not null)
        {
            stack.Children.Add(notes);
        }

        return stack;
    }

    // The reference lines' words and the zones' stand under the plot, where no mark can cover them.
    private UIElement? Notes(Palette palette)
    {
        List<Models.Zone> bands = [.. chart.Zones.Where(zone => zone.Label is not null)];
        List<Models.Reference> named = [.. chart.References.Where(reference => reference.Label is not null && reference.Kind == Models.ReferenceKind.Level)];
        if (bands.Count == 0 && named.Count == 0)
        {
            return null;
        }

        Composition.WrapPanel flow = new() { HorizontalSpacing = 14, VerticalSpacing = 4 };
        foreach (Models.Zone zone in bands)
        {
            StackPanel entry = new() { Orientation = Orientation.Horizontal, Spacing = 5 };
            entry.Children.Add(palette.Contrast
                ? Ink.Box(12, 10, null, 2, palette.Secondary, 1)
                : Ink.Box(12, 10, palette.Fill(zone.Colour, Models.Hue.Neutral, 0.3), 2));
            entry.Children.Add(Ink.Words(zone.Label ?? string.Empty, 12, palette.Secondary));
            flow.Children.Add(entry);
        }

        foreach (Models.Reference reference in named)
        {
            StackPanel entry = new() { Orientation = Orientation.Horizontal, Spacing = 5 };
            Canvas sample = new() { Width = 16, Height = 10 };
            sample.Put(Ink.Rule(0, 5, 16, 5, palette.Fill(reference.Colour, Models.Hue.Neutral), 1.5, [3, 2]), 0, 0);
            entry.Children.Add(sample);
            entry.Children.Add(Ink.Words(reference.Label ?? string.Empty, 12, palette.Secondary));
            flow.Children.Add(entry);
        }

        return HealthParts.Hidden(flow);
    }

    private IEnumerable<(Windows.UI.Color Colour, string Name, string? Detail)> DonutEntries(Palette palette)
    {
        List<(string Label, double Value)> parts = DonutParts();
        double total = parts.Sum(part => part.Value);
        for (int at = 0; at < parts.Count; at++)
        {
            string detail = total > 0 ? ShareRules.Percent(parts[at].Value / total).ToString(CultureInfo.InvariantCulture) + "%" : string.Empty;
            yield return (palette.Series(null, at), parts[at].Label, detail);
        }
    }

    private List<(string Label, double Value)> DonutParts()
    {
        List<(string Label, double Value)> parts = [];
        if (chart.Series.Count == 0)
        {
            return parts;
        }

        IReadOnlyList<double?> values = chart.Series[0].Values;
        for (int at = 0; at < values.Count; at++)
        {
            if (values[at] is double value && value > 0)
            {
                parts.Add((at < chart.Labels.Count ? chart.Labels[at] : (at + 1).ToString(CultureInfo.InvariantCulture), value));
            }
        }

        return parts;
    }

    private string Sentence() =>
        string.Join(". ", chart.Series.Select(series =>
        {
            List<string> values = [];
            for (int at = 0; at < series.Values.Count; at++)
            {
                if (series.Values[at] is double value)
                {
                    string label = at < chart.Labels.Count ? chart.Labels[at] : (at + 1).ToString(CultureInfo.InvariantCulture);
                    string words = at < series.Shown.Count ? series.Shown[at] : NumberWords.Amount(value);
                    values.Add(label + " " + words);
                }
            }

            return series.Label + ": " + string.Join(", ", values);
        }));

    private string ValueWordsOf(Models.Series series, int at, ValueStyle style)
    {
        if (at < series.Shown.Count)
        {
            return series.Shown[at];
        }

        double value = series.Values[at] ?? 0;
        return chart.Stacking == Models.Stacking.Shares ? NumberWords.Amount(value) : ValueWords.Text(value, style);
    }

    // The widest of the page's labels: it does not depend on the size of the drawing, so it is worked out once for the size of the person's text.
    private (int Version, double Width) widestLabel = (-1, 0);

    private double WidestLabel()
    {
        int version = ThemeState.Version;
        if (widestLabel.Version != version)
        {
            widestLabel = (version, chart.Labels.Select(label => Ink.Measure(label, LabelSize).Width).DefaultIfEmpty(0).Max());
        }

        return widestLabel.Width;
    }

    private string LabelAt(int at) => at < chart.Labels.Count ? chart.Labels[at] : (at + 1).ToString(CultureInfo.InvariantCulture);

    private Windows.UI.Color SeriesColour(Palette palette, int at) =>
        palette.Series(at < chart.Series.Count ? chart.Series[at].Colour : null, at);

    // ---- painting ----------------------------------------------------------------------------------------

    private void Paint(PlotCanvas canvas)
    {
        if (Lanes)
        {
            PaintLanes(canvas);
        }
        else if (Spark)
        {
            PaintSpark(canvas);
        }
        else if (chart.Kind == Models.ChartKind.Donut)
        {
            PaintDonut(canvas);
        }
        else if (chart.Kind is Models.ChartKind.Bar or Models.ChartKind.Funnel)
        {
            PaintBars(canvas);
        }
        else
        {
            PaintLines(canvas);
        }
    }

    /// <summary>Where the plot stands in the surface, and how its values and places are laid along it.</summary>
    private readonly record struct Frame(double X0, double Y0, double X1, double Y1, bool Horizontal, AxisRules.Bounds Bounds, int Count)
    {
        internal double Band => (Horizontal ? Y1 - Y0 : X1 - X0) / Math.Max(Count, 1);

        internal double Along(double value) =>
            Horizontal
                ? X0 + ((X1 - X0) * AxisRules.Position(value, Bounds.Low, Bounds.High))
                : Y1 - ((Y1 - Y0) * AxisRules.Position(value, Bounds.Low, Bounds.High));

        internal double Centre(int place) => (Horizontal ? Y0 : X0) + (Band * (place + 0.5));

        // The area of a place's band between two values.
        internal Rect Cell(double bandStart, double bandSize, double begin, double to)
        {
            double a = Along(Math.Min(begin, to));
            double b = Along(Math.Max(begin, to));
            double low = Math.Min(a, b);
            double length = Math.Abs(a - b);
            return Horizontal ? Ink.Area(low, bandStart, length, bandSize) : Ink.Area(bandStart, low, bandSize, length);
        }

        // A stretch of the whole plot between two values.
        internal Rect Span(double begin, double to) =>
            Cell(Horizontal ? Y0 : X0, Horizontal ? Y1 - Y0 : X1 - X0, begin, to);
    }

    private Frame MakeFrame(PlotCanvas canvas, bool horizontal, AxisRules.Bounds bounds, IReadOnlyList<double> marks, ValueStyle style, int count)
    {
        bool axis = !small;
        double left = 4;
        double bottom = 4;
        double right = 6;
        double top = 6;

        if (!horizontal && axis)
        {
            double widest = marks.Select(mark => Ink.Measure(MarkWords(mark, style), LabelSize).Width).DefaultIfEmpty(0).Max();
            left = widest + 10;
        }

        if (horizontal)
        {
            double widest = 0;
            for (int at = 0; at < count; at++)
            {
                widest = Math.Max(widest, Ink.Measure(LabelAt(at), LabelSize).Width);
            }

            left = Math.Min(widest + 10, canvas.Width * 0.4);
            bottom = axis ? 22 : 4;
        }
        else if (chart.Labels.Count > 0)
        {
            bottom = 22;
        }

        return new Frame(left, top, Math.Max(canvas.Width - right, left + 1), Math.Max(canvas.Height - bottom, top + 1), horizontal, bounds, count);
    }

    private string MarkWords(double mark, ValueStyle style) =>
        chart.Stacking == Models.Stacking.Shares ? ((int)Math.Round(mark * 100)).ToString(CultureInfo.InvariantCulture) + "%" : ValueWords.Text(mark, style);

    // The grid, the value labels, the zones and the references: what stands behind the marks.
    private void PaintGuides(PlotCanvas canvas, Frame frame, IReadOnlyList<double> marks, ValueStyle style)
    {
        Palette palette = canvas.Palette;
        foreach (Models.Zone zone in chart.Zones)
        {
            Rect area = frame.Span(zone.From, zone.To);
            if (palette.Contrast)
            {
                // No tint in a contrast theme: the zone is the stretch between two dashed lines.
                foreach (double edge in frame.Horizontal ? new[] { area.X, area.X + area.Width } : new[] { area.Y, area.Y + area.Height })
                {
                    canvas.Shapes.Put(
                        frame.Horizontal ? Ink.Rule(edge, frame.Y0, edge, frame.Y1, palette.Secondary, 1, [2, 3]) : Ink.Rule(frame.X0, edge, frame.X1, edge, palette.Secondary, 1, [2, 3]),
                        0,
                        0);
                }

                continue;
            }

            canvas.Shapes.Put(Ink.Box(area.Width, area.Height, palette.Fill(zone.Colour, Models.Hue.Neutral, 0.14)), area.X, area.Y);
        }

        foreach (double mark in marks)
        {
            double along = frame.Along(mark);
            if (frame.Horizontal)
            {
                canvas.Shapes.Put(Ink.Rule(along, frame.Y0, along, frame.Y1, palette.Divider), 0, 0);
                if (!small)
                {
                    string words = MarkWords(mark, style);
                    canvas.Text(canvas.Caption(words), AxisRules.LabelStart(along, Ink.Measure(words, LabelSize).Width, canvas.Width), frame.Y1 + 4, Across.Start, Down.Top);
                }
            }
            else
            {
                canvas.Shapes.Put(Ink.Rule(frame.X0, along, frame.X1, along, palette.Divider), 0, 0);
                if (!small)
                {
                    canvas.Text(canvas.Caption(MarkWords(mark, style)), frame.X0 - 6, along, Across.End, Down.Middle);
                }
            }
        }

        foreach (Models.Reference reference in chart.References)
        {
            if (reference.Value is double value && reference.Kind == Models.ReferenceKind.Level)
            {
                double along = frame.Along(value);
                Brush ink = palette.Fill(reference.Colour, Models.Hue.Neutral);
                canvas.Shapes.Put(
                    frame.Horizontal ? Ink.Rule(along, frame.Y0, along, frame.Y1, ink, 1, [4, 3]) : Ink.Rule(frame.X0, along, frame.X1, along, ink, 1, [4, 3]),
                    0,
                    0);
            }
        }
    }

    // The words of the places along the foot (or down the side), thinned to the number that fit.
    private void PaintPlaces(PlotCanvas canvas, Frame frame)
    {
        if (chart.Labels.Count == 0)
        {
            return;
        }

        if (frame.Horizontal)
        {
            foreach (int at in ThinRules.Places(frame.Count, canvas.Height / 14))
            {
                TextBlock words = canvas.Caption(LabelAt(at));
                words.MaxWidth = Math.Max(frame.X0 - 10, 20);
                words.TextTrimming = TextTrimming.CharacterEllipsis;
                canvas.Text(words, frame.X0 - 6, frame.Centre(at), Across.End, Down.Middle);
            }

            return;
        }

        double widest = WidestLabel();
        IReadOnlyList<int> ticks = AxisRules.Ticks(frame.Count, small ? 3 : AxisRules.Most(frame.X1 - frame.X0, widest));
        foreach (int at in ticks)
        {
            if (at >= chart.Labels.Count)
            {
                continue;
            }

            TextBlock words = canvas.Caption(chart.Labels[at]);
            double width = Ink.Measured(words).Width;
            double middle = frame.Centre(at);
            double left = Math.Clamp(middle - (width / 2), 0, Math.Max(canvas.Width - width, 0));
            canvas.Text(words, left, frame.Y1 + 5, Across.Start, Down.Top);
        }
    }

    private void PaintBars(PlotCanvas canvas)
    {
        AxisRules.Bounds bounds = AxisRules.Values(chart);
        IReadOnlyList<double> marks = AxisRules.Marks(bounds, 4);
        ValueStyle style = ValueWords.Style(chart);
        int count = AxisRules.Count(chart);
        if (count == 0 || chart.Series.Count == 0)
        {
            return;
        }

        Frame frame = MakeFrame(canvas, Horizontal, bounds, marks, style, count);
        PaintGuides(canvas, frame, marks, style);

        Palette palette = canvas.Palette;
        int many = chart.Series.Count;
        bool stacked = chart.Stacking != Models.Stacking.Apart && many > 1;
        bool shares = chart.Stacking == Models.Stacking.Shares;
        double bar = frame.Band * (stacked || many == 1 ? 0.62 : 0.78) / (stacked || many == 1 ? 1 : many);
        double[] tops = new double[count];
        Array.Fill(tops, double.MaxValue);

        // A chart of very many places draws and reads out a spread of them (see ThinRules).
        IReadOnlyList<int> kept = ThinRules.Places(count, frame.Horizontal ? canvas.Height : canvas.Width);
        foreach (int place in kept)
        {
            double sum = stacked && shares ? chart.Series.Sum(series => place < series.Values.Count ? Math.Max(series.Values[place] ?? 0, 0) : 0) : 1;
            double running = 0;
            for (int at = 0; at < many; at++)
            {
                Models.Series series = chart.Series[at];
                if (place >= series.Values.Count || series.Values[place] is not double value || value == 0)
                {
                    continue;
                }

                double begin;
                double to;
                double start;
                if (stacked)
                {
                    double part = Math.Max(value, 0) / (shares ? Math.Max(sum, 1e-9) : 1);
                    begin = running;
                    to = running + part;
                    running = to;
                    start = frame.Centre(place) - (bar / 2);
                }
                else
                {
                    begin = Math.Max(bounds.Low, 0);
                    to = value;
                    start = many == 1 ? frame.Centre(place) - (bar / 2) : frame.Centre(place) - (frame.Band * 0.39) + (at * bar);
                }

                Rect cell = frame.Cell(start, bar, begin, to);
                double thin = Math.Max(Math.Min(cell.Width, cell.Height), 0);
                (Brush fill, Brush? edge, double edgeWidth) = palette.Surface(series.Colour, HueRules.SeriesHue(at), at);
                canvas.Shapes.Put(Ink.Box(Math.Max(cell.Width, frame.Horizontal ? 1 : 0), Math.Max(cell.Height, frame.Horizontal ? 0 : 1), fill, Math.Min(3, thin / 2), edge, edgeWidth), cell.X, cell.Y);
                tops[place] = Math.Min(tops[place], cell.Y);
            }
        }

        PaintPlaces(canvas, frame);

        foreach (int place in kept)
        {
            List<ProbeLine> lines = [];
            List<string> spoken = [];
            for (int at = 0; at < many; at++)
            {
                Models.Series series = chart.Series[at];
                if (place >= series.Values.Count || series.Values[place] is null)
                {
                    continue;
                }

                string words = ValueWordsOf(series, place, style);
                lines.Add(new ProbeLine(SeriesColour(palette, at), many > 1 ? series.Label + "  " + words : words));
                spoken.Add(series.Label + " " + words);
            }

            double centre = frame.Centre(place);
            Rect hit = frame.Horizontal
                ? canvas.Mrect(frame.X0, centre - (frame.Band / 2), frame.X1 - frame.X0, frame.Band)
                : canvas.Mrect(centre - (frame.Band / 2), frame.Y0, frame.Band, frame.Y1 - frame.Y0);
            double anchorY = frame.Horizontal ? centre : Math.Min(tops[place], frame.Y1);
            double anchorX = frame.Horizontal ? canvas.Mx((frame.X0 + frame.X1) / 2) : canvas.Mx(centre);
            canvas.Probes.Add(new Probe(anchorX, anchorY, hit, LabelAt(place), lines, LabelAt(place) + ", " + string.Join(", ", spoken)));
        }
    }

    private void PaintLines(PlotCanvas canvas)
    {
        AxisRules.Bounds bounds = AxisRules.Values(chart);
        IReadOnlyList<double> marks = AxisRules.Marks(bounds, 4);
        ValueStyle style = ValueWords.Style(chart);
        int count = AxisRules.Count(chart);
        if (count == 0 || chart.Series.Count == 0)
        {
            return;
        }

        Frame frame = MakeFrame(canvas, false, bounds, marks, style, count);
        PaintGuides(canvas, frame, marks, style);
        Palette palette = canvas.Palette;
        bool area = chart.Kind == Models.ChartKind.Area;
        IReadOnlyList<int> kept = ThinRules.Places(count, canvas.Width);
        HashSet<int>? keeps = kept.Count == count ? null : [.. kept];

        for (int at = 0; at < chart.Series.Count; at++)
        {
            Models.Series series = chart.Series[at];
            Windows.UI.Color colour = SeriesColour(palette, at);
            IReadOnlyList<int> runs = AxisRules.Runs(series.Values);
            foreach (IGrouping<int, int> run in Enumerable.Range(0, series.Values.Count).Where(place => series.Values[place].HasValue && (keeps is null || keeps.Contains(place))).GroupBy(place => runs[place]))
            {
                List<Pt> points = [.. run.Select(place => new Pt(frame.Centre(place), frame.Along(series.Values[place]!.Value)))];
                SeriesLook? look = palette.Look(at);
                if (points.Count == 1)
                {
                    PutMark(canvas, palette, look, colour, points[0]);
                    continue;
                }

                IReadOnlyList<Pt> drawn = chart.Curve == Models.Curve.Step ? Curves.StepsCentred(points) : points;
                PathGeometry line = chart.Curve == Models.Curve.Smooth ? Ink.Smooth(points) : Ink.Lines(drawn);
                if (area && !palette.Contrast)
                {
                    PathGeometry under = chart.Curve == Models.Curve.Smooth ? Ink.Smooth(points, frame.Y1) : Ink.Lines(drawn, fillTo: frame.Y1);
                    canvas.Shapes.Put(Ink.Filled(under, Fade(colour, 0.32, 0.04)), 0, 0);
                }

                canvas.Shapes.Put(Ink.Stroked(line, Palette.Mark(colour), 2, look is null ? PenLineCap.Round : PenLineCap.Flat, look?.Dash.Count > 0 ? [.. look.Value.Dash] : null), 0, 0);
                if (look is not null)
                {
                    // A mark at some of the places, so the series is known by its shape as well as its colour and its dashes.
                    int every = Math.Max(1, (int)Math.Ceiling(points.Count / 24.0));
                    for (int mark = 0; mark < points.Count; mark += every)
                    {
                        PutMark(canvas, palette, look, colour, points[mark]);
                    }
                }
            }
        }

        PaintPlaces(canvas, frame);

        foreach (int place in kept)
        {
            List<ProbeLine> lines = [];
            List<(Point At, Windows.UI.Color Colour)> marksAt = [];
            List<string> spoken = [];
            double top = frame.Y1;
            for (int at = 0; at < chart.Series.Count; at++)
            {
                Models.Series series = chart.Series[at];
                if (place >= series.Values.Count || series.Values[place] is not double value)
                {
                    continue;
                }

                string words = ValueWordsOf(series, place, style);
                Windows.UI.Color colour = SeriesColour(palette, at);
                double y = frame.Along(value);
                top = Math.Min(top, y);
                lines.Add(new ProbeLine(colour, chart.Series.Count > 1 ? series.Label + "  " + words : words));
                marksAt.Add((new Point(canvas.Mx(frame.Centre(place)), y), colour));
                spoken.Add(series.Label + " " + words);
            }

            if (lines.Count == 0)
            {
                continue;
            }

            canvas.Probes.Add(new Probe(canvas.Mx(frame.Centre(place)), top, default, LabelAt(place), lines, LabelAt(place) + ", " + string.Join(", ", spoken), Guide: true, Marks: marksAt));
        }
    }

    // A mark on a series' place: in a contrast theme the shape of its look, else a disc.
    private static void PutMark(PlotCanvas canvas, Palette palette, SeriesLook? look, Windows.UI.Color colour, Pt at)
    {
        if (look is SeriesLook found)
        {
            canvas.Shapes.Put(Ink.Marker(found.Shape, 10, Palette.Mark(palette.InkOf(found))), at.X - 5, at.Y - 5);
            return;
        }

        canvas.Shapes.Put(Ink.Disc(7, Palette.Mark(colour)), at.X - 3.5, at.Y - 3.5);
    }

    private static LinearGradientBrush Fade(Windows.UI.Color colour, double begin, double to)
    {
        LinearGradientBrush brush = new() { StartPoint = new Point(0, 0), EndPoint = new Point(0, 1) };
        brush.GradientStops.Add(new GradientStop { Color = Windows.UI.Color.FromArgb((byte)(begin * 255), colour.R, colour.G, colour.B), Offset = 0 });
        brush.GradientStops.Add(new GradientStop { Color = Windows.UI.Color.FromArgb((byte)(to * 255), colour.R, colour.G, colour.B), Offset = 1 });
        return brush;
    }

    // A sparkline: the line and its glow, no axes, a dot at the last value.
    private void PaintSpark(PlotCanvas canvas)
    {
        List<(int Place, double Value)> found = [];
        if (chart.Series.Count > 0)
        {
            foreach (int at in ThinRules.Places(chart.Series[0].Values.Count, canvas.Width))
            {
                if (chart.Series[0].Values[at] is double value)
                {
                    found.Add((at, value));
                }
            }
        }

        if (found.Count == 0)
        {
            return;
        }

        int count = Math.Max(AxisRules.Count(chart), 1);
        double low = found.Min(item => item.Value);
        double high = found.Max(item => item.Value);
        double pad = high > low ? (high - low) * 0.12 : 1;
        double x0 = 3;
        double x1 = canvas.Width - 5;
        double y0 = 3;
        double y1 = canvas.Height - 3;
        double X(int place) => count == 1 ? (x0 + x1) / 2 : x0 + ((x1 - x0) * place / (count - 1));
        double Y(double value) => y1 - ((y1 - y0) * (value - (low - pad)) / ((high + pad) - (low - pad)));
        Windows.UI.Color colour = SeriesColour(canvas.Palette, 0);
        IReadOnlyList<int> runs = AxisRules.Runs(chart.Series[0].Values);
        foreach (IGrouping<int, (int Place, double Value)> run in found.GroupBy(item => runs[item.Place]))
        {
            List<Pt> points = [.. run.Select(item => new Pt(X(item.Place), Y(item.Value)))];
            if (points.Count < 2)
            {
                continue;
            }

            if (!canvas.Palette.Contrast)
            {
                canvas.Shapes.Put(Ink.Filled(Ink.Smooth(points, y1), Fade(colour, 0.25, 0)), 0, 0);
            }

            canvas.Shapes.Put(Ink.Stroked(Ink.Smooth(points), Palette.Mark(colour), 1.75, PenLineCap.Round), 0, 0);
        }

        (int lastPlace, double lastValue) = found[^1];
        canvas.Shapes.Put(Ink.Disc(6, Palette.Mark(colour)), X(lastPlace) - 3, Y(lastValue) - 3);

        foreach ((int place, double value) in found)
        {
            string words = ValueWordsOf(chart.Series[0], place, ValueWords.Style(chart));
            canvas.Probes.Add(new Probe(canvas.Mx(X(place)), Y(value), default, LabelAt(place), [new ProbeLine(colour, words)], LabelAt(place) + ", " + words, Guide: true, Marks: [(new Point(canvas.Mx(X(place)), Y(value)), colour)]));
        }
    }

    private void PaintDonut(PlotCanvas canvas)
    {
        List<(string Label, double Value)> parts = DonutParts();
        double total = parts.Sum(part => part.Value);
        if (total <= 0)
        {
            return;
        }

        DonutRules.Ring ring = DonutRules.Of(canvas.Width, canvas.Height);
        if (!ring.Draws)
        {
            return;
        }

        double middle = ring.Middle;
        double width = ring.Width;
        double cx = ring.Cx;
        double cy = ring.Cy;
        Palette palette = canvas.Palette;
        double gap = parts.Count > 1 ? (palette.Contrast ? 3 : 1.5) : 0;
        double[] values = [.. parts.Select(part => part.Value)];
        IReadOnlyList<DonutRules.Slice> slices = DonutRules.Slices(values);
        for (int at = 0; at < parts.Count; at++)
        {
            DonutRules.Slice slice = slices[at];
            Windows.UI.Color colour = palette.Series(null, at);
            (Brush fill, Brush? edge, double _) = palette.Surface(null, HueRules.SeriesHue(at), at);
            double sweep = Math.Max(slice.Sweep - gap, 0.5);
            canvas.Shapes.Put(Ink.Stroked(Ink.Arc(cx, cy, middle, slice.Start + (gap / 2), sweep), edge ?? fill, width), 0, 0);
            if (edge is not null && fill is SolidColorBrush ground && ground.Color != ((SolidColorBrush)edge).Color)
            {
                // An outline in a contrast theme: the ground inside the edge, which is two pixels each side.
                canvas.Shapes.Put(Ink.Stroked(Ink.Arc(cx, cy, middle, slice.Start + (gap / 2), sweep), fill, Math.Max(width - 4, 0)), 0, 0);
            }

            Pt mid = Ink.Polar(cx, cy, middle, slice.Start + (slice.Sweep / 2));
            string words = NumberWords.Amount(parts[at].Value) + " (" + ShareRules.Percent(parts[at].Value / total).ToString(CultureInfo.InvariantCulture) + "%)";
            int slot = at;
            bool mirrored = canvas.Mirror;

            // The pointer reads a slice out wherever it is on the slice's arc, by the angle it is at from the middle.
            canvas.Probes.Add(new Probe(
                canvas.Mx(mid.X),
                mid.Y,
                default,
                parts[at].Label,
                [new ProbeLine(colour, words)],
                parts[at].Label + ", " + words,
                Covers: point => DonutRules.SliceAt(values, ring, point.X, point.Y, 6, mirrored) == slot));
        }
    }

    // The stages of a night: a lane a level, the first at the foot, each run of one level a bar in its lane with a hairline down to the next.
    private void PaintLanes(PlotCanvas canvas)
    {
        int levels = chart.Levels.Count;
        int count = Math.Max(AxisRules.Count(chart), 1);
        double laneHeight = Guard.Length(canvas.Height - 24) / levels;

        // The names of the levels stand in a gutter of their own at the side, so no stage is drawn under one.
        double x0 = Math.Min(chart.Levels.Select(level => Ink.Measure(level, 11, strong: true).Width).Max() + 10, canvas.Width * 0.4);
        double x1 = canvas.Width;
        double X(double place) => x0 + ((x1 - x0) * place / count);
        double Lane(int level) => laneHeight * (levels - 1 - level);
        Palette palette = canvas.Palette;
        for (int level = 0; level < levels; level++)
        {
            canvas.Shapes.Put(Ink.Rule(x0, Lane(level) + (laneHeight / 2), x1, Lane(level) + (laneHeight / 2), palette.Divider), 0, 0);
        }

        Windows.UI.Color colour = SeriesColour(palette, 0);
        IReadOnlyList<(int Level, int From, int To)> stretches = chart.Series.Count == 0 ? [] : AxisRules.Stretches(chart.Series[0].Values);
        for (int at = 0; at < stretches.Count; at++)
        {
            (int level, int first, int to) = stretches[at];
            double opacity = 0.45 + (0.55 * (1 - (level / Math.Max(levels - 1.0, 1))));
            double top = Lane(Math.Clamp(level, 0, levels - 1)) + (laneHeight * 0.22);
            canvas.Shapes.Put(Ink.Box(Math.Max(X(to + 1) - X(first), 1), laneHeight * 0.56, palette.Fade(colour, opacity), 3), X(first), top);
            if (at + 1 < stretches.Count && stretches[at + 1].From == to + 1)
            {
                (int nextLevel, int nextFrom, _) = stretches[at + 1];
                double a = Lane(Math.Clamp(Math.Min(level, nextLevel), 0, levels - 1)) + (laneHeight * 0.22);
                double b = Lane(Math.Clamp(Math.Max(level, nextLevel), 0, levels - 1)) + (laneHeight * 0.78);
                canvas.Shapes.Put(Ink.Rule(X(nextFrom), Math.Min(a, b), X(nextFrom), Math.Max(a, b), palette.Fade(colour, 0.5)), 0, 0);
            }
        }

        for (int level = 0; level < levels; level++)
        {
            TextBlock name = Ink.Words(chart.Levels[level], 11, palette.Secondary, strong: true);
            canvas.Text(name, 4, Lane(level) + (laneHeight / 2), Across.Start, Down.Middle);
        }

        double widest = WidestLabel();
        foreach (int at in AxisRules.Ticks(count, AxisRules.Most(x1 - x0, widest)))
        {
            if (at < chart.Labels.Count)
            {
                double wide = Ink.Measure(chart.Labels[at], LabelSize).Width;
                canvas.Text(canvas.Caption(chart.Labels[at]), AxisRules.LabelStart(X(at) + (wide / 2), wide, canvas.Width), canvas.Height - 18, Across.Start, Down.Top);
            }
        }

        double barWidth = (x1 - x0) / count;
        for (int place = 0; place < count; place++)
        {
            if (chart.Series.Count == 0 || place >= chart.Series[0].Values.Count || chart.Series[0].Values[place] is not double value)
            {
                continue;
            }

            int level = Math.Clamp((int)Math.Round(value), 0, levels - 1);
            double centre = X(place + 0.5);
            canvas.Probes.Add(new Probe(
                canvas.Mx(centre),
                Lane(level),
                canvas.Mrect(X(place), 0, barWidth, canvas.Height - 24),
                LabelAt(place),
                [new ProbeLine(colour, chart.Levels[level])],
                LabelAt(place) + ", " + chart.Levels[level]));
        }
    }
}
