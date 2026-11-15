using Navree.Protocol;

namespace Navree.Design;

/// <summary>A point of a drawing, in effective pixels.</summary>
/// <param name="X">Across.</param>
/// <param name="Y">Down.</param>
public readonly record struct Pt(double X, double Y);

/// <summary>A cubic piece of a smooth line: two control points and the point it ends at.</summary>
/// <param name="First">The first control point.</param>
/// <param name="Second">The second control point.</param>
/// <param name="To">Where the piece ends.</param>
public readonly record struct Bezier(Pt First, Pt Second, Pt To);

/// <summary>The bounds and the marks of a chart's axes, as plain functions.</summary>
public static class AxisRules
{
    /// <summary>The two ends of an axis.</summary>
    /// <param name="Low">The lower end.</param>
    /// <param name="High">The upper end.</param>
    public readonly record struct Bounds(double Low, double High);

    /// <summary>
    /// The value axis of a chart: the measure's own bounds where it gives them; else from zero (bars always stand on zero) to the
    /// highest value, reference or zone, rounded out to numbers read at a glance.
    /// </summary>
    /// <param name="chart">The chart.</param>
    /// <returns>The bounds.</returns>
    public static Bounds Values(Models.Chart chart)
    {
        if (chart.Stacking == Models.Stacking.Shares)
        {
            return new Bounds(0, 1);
        }

        if (chart.Levels.Count > 0)
        {
            return new Bounds(0, Math.Max(chart.Levels.Count - 1, 1));
        }

        int count = chart.Series.Count == 0 ? 0 : chart.Series.Max(series => series.Values.Count);
        bool stacked = chart.Stacking == Models.Stacking.Values && chart.Kind == Models.ChartKind.Bar && chart.Series.Count > 1;
        List<double> all = [];
        if (stacked)
        {
            for (int at = 0; at < count; at++)
            {
                double top = 0;
                foreach (Models.Series series in chart.Series)
                {
                    top += Math.Max(at < series.Values.Count ? series.Values[at] ?? 0 : 0, 0);
                }

                all.Add(top);
            }
        }
        else
        {
            foreach (Models.Series series in chart.Series)
            {
                all.AddRange(series.Values.Where(value => value.HasValue).Select(value => value!.Value));
            }
        }

        all.AddRange(chart.References.Where(reference => reference.Value.HasValue).Select(reference => reference.Value!.Value));
        foreach (Models.Zone zone in chart.Zones)
        {
            all.Add(zone.From);
            all.Add(zone.To);
        }

        all = [.. all.Where(double.IsFinite)];
        double lowest = all.Count == 0 ? 0 : all.Min();
        double highest = all.Count == 0 ? 1 : all.Max();
        bool bars = chart.Kind == Models.ChartKind.Bar || chart.Kind == Models.ChartKind.Funnel;
        double? yMin = Number(chart.Y?.Min);
        double? yMax = Number(chart.Y?.Max);
        double low = yMin ?? ((bars || (lowest >= 0 && lowest < highest * 0.5)) ? 0 : lowest - ((highest - lowest) * 0.15));
        double high = yMax ?? (highest + ((highest - low) * 0.05));
        Bounds rounded = Nice(low, high > low ? high : low + 1);
        return Sound(new Bounds(yMin ?? rounded.Low, yMax ?? rounded.High));
    }

    /// <summary>Two ends rounded out to a step of 1, 2, 2.5 or 5 times a power of ten, about five steps apart.</summary>
    /// <param name="low">The lower end.</param>
    /// <param name="high">The upper end.</param>
    /// <returns>The rounded ends.</returns>
    public static Bounds Nice(double low, double high)
    {
        if (!double.IsFinite(low) || !double.IsFinite(high) || !double.IsFinite(high - low))
        {
            return new Bounds(0, 1);
        }

        double span = high - low;
        if (span <= 0)
        {
            return new Bounds(low, low + 1);
        }

        double step = NiceStep(span / 5);
        return new Bounds(Math.Floor(low / step) * step, Math.Ceiling(high / step) * step);
    }

    /// <summary>Ends that are numbers: a page that gives an end that is not one gets the axis from 0 to 1 instead.</summary>
    /// <param name="bounds">The ends.</param>
    /// <returns>Ends that are numbers.</returns>
    public static Bounds Sound(Bounds bounds)
    {
        if (!double.IsFinite(bounds.Low) || !double.IsFinite(bounds.High))
        {
            return new Bounds(0, 1);
        }

        // A range given the wrong way round is the same range.
        return bounds.Low > bounds.High ? new Bounds(bounds.High, bounds.Low) : bounds;
    }

    private static double? Number(double? value) => value is double found && double.IsFinite(found) ? found : null;

    /// <summary>A step a person reads: 1, 2, 2.5, 5 or 10 times a power of ten.</summary>
    /// <param name="raw">About the step wanted.</param>
    /// <returns>The step.</returns>
    public static double NiceStep(double raw)
    {
        if (!double.IsFinite(raw) || raw <= 0)
        {
            return 1;
        }

        double power = Math.Pow(10, Math.Floor(Math.Log10(raw)));
        double n = raw / power;
        double unit = n <= 1 ? 1 : n <= 2 ? 2 : n <= 2.5 ? 2.5 : n <= 5 ? 5 : 10;
        return unit * power;
    }

    /// <summary>The marks of a value axis: the numbers from the lower end to the upper at the nice step.</summary>
    /// <param name="bounds">The bounds.</param>
    /// <param name="wanted">About how many marks.</param>
    /// <returns>The values to mark.</returns>
    public static IReadOnlyList<double> Marks(Bounds bounds, int wanted)
    {
        double span = bounds.High - bounds.Low;
        if (!double.IsFinite(span) || span <= 0 || wanted < 1)
        {
            return [Guard.Finite(bounds.Low)];
        }

        double step = NiceStep(span / wanted);
        List<double> marks = [];
        double first = Math.Ceiling((bounds.Low / step) - 1e-9) * step;
        for (double value = first; value <= bounds.High + (step * 1e-9) && marks.Count < 64; value += step)
        {
            marks.Add(Math.Round(value / step) * step);
        }

        return marks;
    }

    /// <summary>A scatter's two axes; with equal axes, one range for both.</summary>
    /// <param name="plot">The scatter.</param>
    /// <returns>The bounds of across and of down.</returns>
    public static (Bounds X, Bounds Y) Scatter(Models.Scatter plot)
    {
        List<Models.Point> points = [.. plot.Series.SelectMany(series => series.Points)];
        static Bounds Padded(IReadOnlyList<double> values, Models.Measure? measure)
        {
            double low = values.Count == 0 ? 0 : values.Min();
            double high = values.Count == 0 ? 1 : values.Max();
            double pad = high > low ? (high - low) * 0.08 : 1;
            Bounds rounded = Nice(low - pad, high + pad);
            return Sound(new Bounds(Number(measure?.Min) ?? rounded.Low, Number(measure?.Max) ?? rounded.High));
        }

        Bounds x = Padded([.. points.Select(point => point.X)], plot.X);
        Bounds y = Padded([.. points.Select(point => point.Y)], plot.Y);
        if (!plot.EqualAxes)
        {
            return (x, y);
        }

        Bounds both = new(Math.Min(x.Low, y.Low), Math.Max(x.High, y.High));
        return (both, both);
    }

    /// <summary>
    /// Which of a chart's labels are written along its axis: at most <paramref name="most"/>, a whole step apart from the first, the step
    /// a count a person reads (every 2, 3, 4, 6, 12 or 5, 7, 10). The words are the page's, so no chart can thin them by itself.
    /// </summary>
    /// <param name="count">How many labels there are.</param>
    /// <param name="most">The most that fit.</param>
    /// <returns>The places to write.</returns>
    public static IReadOnlyList<int> Ticks(int count, int most)
    {
        if (count <= 0 || most <= 0)
        {
            return [];
        }

        if (count <= most)
        {
            return [.. Enumerable.Range(0, count)];
        }

        int[] steps = [2, 3, 4, 5, 6, 7, 10, 12, 14, 15, 18, 20, 24, 30, 36, 48, 60, 72, 96, 120];
        int need = (int)Math.Ceiling((double)count / most);
        int step = steps.FirstOrDefault(candidate => candidate >= need);
        if (step == 0)
        {
            step = need;
        }

        List<int> places = [];
        for (int at = 0; at < count; at += step)
        {
            places.Add(at);
        }

        return places;
    }

    /// <summary>How many labels fit along a plot this wide, for words this wide: a gap holds a label and a half and a space.</summary>
    /// <param name="width">The plot's width.</param>
    /// <param name="label">The widest label.</param>
    /// <returns>The count, at least one.</returns>
    public static int Most(double width, double label)
    {
        if (!double.IsFinite(width) || !double.IsFinite(label) || width <= 0)
        {
            return 1;
        }

        return Math.Max(1, (int)(width / ((Math.Max(label, 0) * 1.5) + 12)));
    }

    /// <summary>The runs of a series with no gap in them: a gap is a gap, so the line breaks there and no bar stands in it.</summary>
    /// <param name="values">The values.</param>
    /// <returns>The run each place belongs to.</returns>
    public static IReadOnlyList<int> Runs(IReadOnlyList<double?> values)
    {
        int run = 0;
        bool inGap = false;
        List<int> found = [];
        foreach (double? value in values)
        {
            if (value is null)
            {
                if (!inGap)
                {
                    run++;
                }

                inGap = true;
            }
            else
            {
                inGap = false;
            }

            found.Add(run);
        }

        return found;
    }

    /// <summary>The stretches of a series of levels: each run of one level as the first and last place it holds.</summary>
    /// <param name="values">The levels.</param>
    /// <returns>The stretches.</returns>
    public static IReadOnlyList<(int Level, int From, int To)> Stretches(IReadOnlyList<double?> values)
    {
        List<(int Level, int From, int To)> found = [];
        for (int at = 0; at < values.Count; at++)
        {
            if (values[at] is not double value)
            {
                continue;
            }

            int level = (int)Math.Round(value, MidpointRounding.AwayFromZero);
            if (found.Count > 0 && found[^1].Level == level && found[^1].To == at - 1)
            {
                (int Level, int From, int To) last = found[^1];
                found[^1] = (last.Level, last.From, at);
            }
            else
            {
                found.Add((level, at, at));
            }
        }

        return found;
    }

    /// <summary>How many places a chart has: its labels or its longest series.</summary>
    /// <param name="chart">The chart.</param>
    /// <returns>The count.</returns>
    public static int Count(Models.Chart chart) =>
        Math.Max(chart.Labels.Count, chart.Series.Count == 0 ? 0 : chart.Series.Max(series => series.Values.Count));

    /// <summary>Where a value stands between two ends, 0 to 1; outside, at the nearer end.</summary>
    /// <param name="value">The value.</param>
    /// <param name="low">The lower end.</param>
    /// <param name="high">The upper end.</param>
    /// <returns>The place.</returns>
    public static double Position(double value, double low, double high)
    {
        if (double.IsNaN(value) || !double.IsFinite(low) || !double.IsFinite(high) || !(high > low))
        {
            return 0;
        }

        // Written so that a span too big to hold (two huge numbers of opposite signs) still gives a place.
        double span = high - low;
        return double.IsFinite(span) ? Math.Clamp((value - low) / span, 0, 1) : Math.Clamp((value / 2 - low / 2) / (high / 2 - low / 2), 0, 1);
    }
}

/// <summary>Smooth lines through points.</summary>
public static class Curves
{
    /// <summary>
    /// A line through the points that never goes past the points it joins (a monotone cubic): each piece as the cubic curve from the
    /// point before it. The points must be in order across.
    /// </summary>
    /// <param name="points">The points, at least two.</param>
    /// <returns>One piece for each gap between points.</returns>
    public static IReadOnlyList<Bezier> Monotone(IReadOnlyList<Pt> points)
    {
        int n = points.Count;
        if (n < 2)
        {
            return [];
        }

        double[] h = new double[n - 1];
        double[] slope = new double[n - 1];
        for (int at = 0; at < n - 1; at++)
        {
            h[at] = points[at + 1].X - points[at].X;
            slope[at] = h[at] == 0 ? 0 : (points[at + 1].Y - points[at].Y) / h[at];
        }

        double[] tangent = new double[n];
        tangent[0] = slope[0];
        tangent[n - 1] = slope[n - 2];
        for (int at = 1; at < n - 1; at++)
        {
            tangent[at] = slope[at - 1] * slope[at] <= 0 ? 0 : (slope[at - 1] + slope[at]) / 2;
        }

        for (int at = 0; at < n - 1; at++)
        {
            if (slope[at] == 0)
            {
                tangent[at] = 0;
                tangent[at + 1] = 0;
                continue;
            }

            double a = tangent[at] / slope[at];
            double b = tangent[at + 1] / slope[at];
            double size = (a * a) + (b * b);
            if (size > 9)
            {
                double scale = 3 / Math.Sqrt(size);
                tangent[at] = scale * a * slope[at];
                tangent[at + 1] = scale * b * slope[at];
            }
        }

        List<Bezier> pieces = [];
        for (int at = 0; at < n - 1; at++)
        {
            double third = h[at] / 3;
            pieces.Add(new Bezier(
                new Pt(points[at].X + third, points[at].Y + (tangent[at] * third)),
                new Pt(points[at + 1].X - third, points[at + 1].Y - (tangent[at + 1] * third)),
                points[at + 1]));
        }

        return pieces;
    }

    /// <summary>The corners of a step line: across to the next place, then up or down to its value.</summary>
    /// <param name="points">The points, in order across.</param>
    /// <returns>The corners, starting at the first point.</returns>
    public static IReadOnlyList<Pt> Steps(IReadOnlyList<Pt> points)
    {
        List<Pt> corners = [];
        for (int at = 0; at < points.Count; at++)
        {
            if (at > 0)
            {
                corners.Add(new Pt(points[at].X, points[at - 1].Y));
            }

            corners.Add(points[at]);
        }

        return corners;
    }

    /// <summary>The corners of a step line that holds each value across its own place: the change happens halfway between two places.</summary>
    /// <param name="points">The points, in order across.</param>
    /// <returns>The corners, from the first point to the last.</returns>
    public static IReadOnlyList<Pt> StepsCentred(IReadOnlyList<Pt> points)
    {
        List<Pt> corners = [];
        for (int at = 0; at < points.Count; at++)
        {
            if (at == 0)
            {
                corners.Add(points[0]);
                continue;
            }

            double middle = (points[at - 1].X + points[at].X) / 2;
            corners.Add(new Pt(middle, points[at - 1].Y));
            corners.Add(new Pt(middle, points[at].Y));
            if (at == points.Count - 1)
            {
                corners.Add(points[at]);
            }
        }

        return corners;
    }
}

/// <summary>Zones along a scale: a gauge's, a range's, a chart's.</summary>
public static class ZoneRules
{
    /// <summary>The zone a value is in: the last one that holds it, as the protocol says.</summary>
    /// <param name="value">The value, or none.</param>
    /// <param name="zones">The zones.</param>
    /// <returns>The place of the zone, or none.</returns>
    public static int? IndexOf(double? value, IReadOnlyList<Models.Zone> zones)
    {
        if (value is not double found)
        {
            return null;
        }

        for (int at = zones.Count - 1; at >= 0; at--)
        {
            if (found >= Math.Min(zones[at].From, zones[at].To) && found <= Math.Max(zones[at].From, zones[at].To))
            {
                return at;
            }
        }

        return null;
    }
}

/// <summary>Parts of a whole: a meter's segments, a column's parts, a strip's cells.</summary>
public static class ShareRules
{
    /// <summary>Each amount's share of their total; an amount left out counts as none.</summary>
    /// <param name="amounts">The amounts.</param>
    /// <returns>The shares, adding up to 1 (or all none).</returns>
    public static IReadOnlyList<double> Shares(IReadOnlyList<double?> amounts)
    {
        double total = amounts.Sum(amount => Math.Max(amount ?? 0, 0));
        return [.. amounts.Select(amount => total <= 0 ? 0 : Math.Max(amount ?? 0, 0) / total)];
    }

    /// <summary>A share as a whole percentage, never 0 for a part that has some.</summary>
    /// <param name="share">The share.</param>
    /// <returns>The percentage.</returns>
    public static int Percent(double share) =>
        share <= 0 ? 0 : Math.Max(1, (int)Math.Round(share * 100, MidpointRounding.AwayFromZero));

    /// <summary>Whether a share's percentage fits inside its part of a bar this wide, for words this wide.</summary>
    /// <param name="share">The share.</param>
    /// <param name="width">The bar's width.</param>
    /// <param name="textWidth">The words' width.</param>
    /// <returns>Whether it fits.</returns>
    public static bool SaysInside(double share, double width, double textWidth) => (share * width) >= textWidth + 8;

    /// <summary>How many cells of a strip are of each kind of its key, as shares of the cells that have one.</summary>
    /// <param name="cells">The cells.</param>
    /// <param name="keySize">How many kinds the key has.</param>
    /// <returns>The shares.</returns>
    public static IReadOnlyList<double> CategoryShares(IReadOnlyList<Models.StripCell> cells, int keySize)
    {
        int[] counts = new int[Math.Max(keySize, 0)];
        foreach (Models.StripCell cell in cells)
        {
            if (cell.Category is long category && category >= 0 && category < keySize)
            {
                counts[category]++;
            }
        }

        int total = counts.Sum();
        return [.. counts.Select(count => total == 0 ? 0.0 : (double)count / total)];
    }
}

/// <summary>A progress bar's fill and how far along it is.</summary>
public static class ProgressRules
{
    /// <summary>How far along the bar is, 0 to 1; the protocol counts in thousandths. None when it is not known.</summary>
    /// <param name="value">The protocol's value.</param>
    /// <returns>The fraction, or none.</returns>
    public static double? Fraction(int? value) => value is int found ? Math.Clamp(found / 1000.0, 0, 1) : null;

    /// <summary>The hue of the fill: the colour of the measure wins over the colour of what it means; with neither, the accent.</summary>
    /// <param name="progress">The progress.</param>
    /// <returns>The hue.</returns>
    public static Models.Hue Hue(Models.Progress progress) =>
        progress.Colour?.Hue ?? (progress.Tone is Models.Tone tone ? HueRules.OfTone(tone) : Models.Hue.Accent);
}
