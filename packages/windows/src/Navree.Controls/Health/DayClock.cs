using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Navree.Controls.Drawing;
using Navree.Design;
using Navree.Protocol;

namespace Navree.Controls.Health;

/// <summary>
/// The day clock: 24 hours round a dial, midnight at the top. Each hour is a bar outwards as long as how much happened in it, in the
/// colour of what the hour was; the quarters are named at the edge in the person's own clock; the day's moments are marked on one line
/// round the bars at their time; the hour that is now has a rim; the day's total is in the middle. Under the dial, the key with each
/// kind's total and the moments with their times.
/// </summary>
public sealed partial class DayClockView : KindControl
{
    private readonly Models.DayClock clock;
    private readonly CultureInfo culture = ClientCulture.Current;
    private readonly double scale = TextScale.Capped(1.25);

    /// <summary>Draws a day clock.</summary>
    /// <param name="clock">The clock.</param>
    public DayClockView(Models.DayClock clock)
    {
        this.clock = clock;
        Start();
    }

    private bool Uses24 => ClockRules.Uses24(culture);

    private string Sentence()
    {
        List<string> parts = [];
        if (clock.Shown is not null)
        {
            parts.Add(string.Join(" ", new[] { clock.Shown, clock.Unit }.Where(text => !string.IsNullOrEmpty(text))));
        }

        parts.AddRange(clock.Key.Select(entry => KeyRules.Entry(entry.Name, entry.Detail)));
        parts.AddRange(clock.Moments.Select(moment => ClockRules.MomentEntry(moment.Name, moment.At, Uses24, culture)));
        return string.Join(". ", parts);
    }

    internal override UIElement Build(Palette palette)
    {
        StackPanel stack = new() { Spacing = 14 };
        UIElement? heading = HealthParts.Heading(clock.Title, clock.Hint);
        if (heading is not null)
        {
            stack.Children.Add(heading);
        }

        double side = ClockRules.Diameter * scale;
        string name = clock.Title ?? clock.Label ?? string.Empty;
        string said = SpeechRules.Value(clock.Summary ?? Sentence(), name);
        PlotSurface dial = new()
        {
            Height = side,
            MaxWidth = side,
            Ideal = side,
            Reads = true,
            AccessName = string.IsNullOrEmpty(name) ? said : SpeechRules.Fact(name, said),
            AccessType = KindStrings.TypeDayClock,
        };
        dial.Painter = Paint;
        stack.Children.Add(dial);

        StackPanel under = new() { Spacing = 10 };
        if (clock.Key.Count > 0)
        {
            under.Children.Add(HealthParts.Key(palette, clock.Key.Select((entry, at) => (palette.Of(entry.Colour, HueRules.SeriesHue(at)), entry.Name, entry.Detail))));
        }

        foreach (Models.ClockMoment moment in clock.Moments)
        {
            Grid line = new() { ColumnSpacing = 8 };
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(24) });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            Windows.UI.Color colour = palette.Of(moment.Colour, Models.Hue.Neutral);
            if (moment.Icon is not null)
            {
                UIElement icon = KindHooks.DrawIcon(moment.Icon, 16, colour);
                if (icon is FrameworkElement placed)
                {
                    placed.VerticalAlignment = VerticalAlignment.Center;
                }

                line.Children.Add(icon);
            }

            TextBlock words = new() { Text = moment.Name, TextWrapping = TextWrapping.Wrap };
            Grid.SetColumn(words, 1);
            line.Children.Add(words);
            if (!ClockRules.HoldsTime(moment.Name))
            {
                TextBlock time = Ink.Words(ClockRules.TimeLabel(moment.At, Uses24, culture), 14, palette.Secondary);
                Grid.SetColumn(time, 2);
                line.Children.Add(time);
            }

            under.Children.Add(line);
        }

        stack.Children.Add(HealthParts.Hidden(under));
        return stack;
    }

    private void Paint(PlotCanvas canvas)
    {
        Palette palette = canvas.Palette;
        double side = Math.Min(canvas.Width, canvas.Height);
        double half = side / 2;
        double cx = canvas.Width / 2;
        double cy = canvas.Height / 2;
        double label = 15 * scale;
        double mark = 22 * scale;
        ClockRules.Dial dial = ClockRules.DialOf(half, label, mark);
        double? most = ClockRules.Most(clock);
        double width = ClockRules.BarWidth(dial.Inner);
        if (!ClockRules.Draws(dial))
        {
            return;
        }

        double length = Math.Max(dial.Outer - dial.Inner - width, 0);
        const double stub = 0.06;

        // The dial's face: a faint band from the hole to the rim the moments stand on.
        double rim = dial.Moments + (mark / 2) + 3;
        double middle = (dial.Inner + rim) / 2;
        canvas.Shapes.Put(Ink.Stroked(Ink.Arc(cx, cy, middle, -90, 359.9), Palette.Mark(palette.Of(Models.Hue.Neutral), palette.Dark ? 0.14 : 0.08), rim - dial.Inner), 0, 0);

        Windows.UI.Color fallback = palette.Of(clock.Colour, Models.Hue.Accent);
        for (int at = 0; at < Math.Min(clock.Hours.Count, 24); at++)
        {
            Models.Hour hour = clock.Hours[at];
            Windows.UI.Color colour = hour.Category is long kind && kind >= 0 && kind < clock.Key.Count
                ? palette.Of(clock.Key[(int)kind].Colour, HueRules.SeriesHue((int)kind))
                : fallback;
            double? reach = ClockRules.Reach(hour, most);
            double startRadius = dial.Inner + (width / 2);
            double endRadius = startRadius + (length * Math.Max(reach ?? 0, stub));
            double angle = ClockRules.Angle(at + 0.5);
            Pt near = Ink.Polar(cx, cy, startRadius, angle);
            Pt far = Ink.Polar(cx, cy, endRadius, angle);
            bool faint = reach is null || reach < stub;
            double opacity = faint ? (reach is null ? 0.18 : 0.4) : 1;
            if (clock.Now == at)
            {
                canvas.Shapes.Put(Ink.Stroked(Ink.Lines([near, far]), palette.Primary, width + 5, PenLineCap.Round), 0, 0);
            }

            canvas.Shapes.Put(Ink.Stroked(Ink.Lines([near, far]), Palette.Mark(colour, clock.Now == at ? 1 : opacity), width, PenLineCap.Round), 0, 0);

            string name = clock.Key.Count > 0 && hour.Category is long found && found >= 0 && found < clock.Key.Count ? clock.Key[(int)found].Name : string.Empty;
            string words = (reach is null ? KindStrings.NoReading : hour.Value is double value ? NumberWords.Amount(value, culture) : NumberWords.Amount((reach ?? 0) * 100, culture) + "%")
                + (name.Length > 0 ? ", " + name : string.Empty);
            string time = ClockRules.TimeLabel(at, Uses24, culture);
            canvas.Probes.Add(new Probe(far.X, far.Y, default, time, [new ProbeLine(reach is null ? null : colour, words)], time + ", " + words, Marks: [(new Windows.Foundation.Point(far.X, far.Y), colour)]));
        }

        foreach (int quarter in ClockRules.Quarters)
        {
            Pt at = Ink.Polar(cx, cy, dial.Labels, ClockRules.Angle(quarter));
            canvas.Labels.Say(Ink.Words(ClockRules.QuarterLabel(quarter, Uses24, culture), 11 * scale, palette.Secondary, strong: true), at.X, at.Y, Across.Middle, Down.Middle);
        }

        // Two marks never closer than their own width along the line.
        double gap = (mark + 3) / (2 * Math.PI * dial.Moments) * 24;
        IReadOnlyList<double> placed = ClockRules.Spread([.. clock.Moments.Select(moment => moment.At)], gap);
        for (int at = 0; at < clock.Moments.Count; at++)
        {
            Models.ClockMoment moment = clock.Moments[at];
            Windows.UI.Color colour = palette.Of(moment.Colour, Models.Hue.Neutral);
            Pt spot = Ink.Polar(cx, cy, dial.Moments, ClockRules.Angle(placed[at]));
            canvas.Shapes.Put(Ink.Disc(mark, Palette.Mark(colour), palette.Card, 1.5), spot.X - (mark / 2), spot.Y - (mark / 2));
            if (moment.Icon is not null)
            {
                double inner = mark * 0.5;
                canvas.Labels.Put(KindHooks.DrawIcon(moment.Icon, inner, Microsoft.UI.Colors.White), spot.X - (inner / 2), spot.Y - (inner / 2));
            }
        }

        // The day's total in the hole.
        List<UIElement> lines = [];
        if (clock.Icon is not null)
        {
            lines.Add(KindHooks.DrawIcon(clock.Icon, 15 * scale, palette.Of(clock.Colour, Models.Hue.Neutral)));
        }

        if (clock.Shown is not null)
        {
            TextBlock total = Ink.Words(clock.Shown, 30 * scale, palette.Primary, strong: true);
            total.FontSize *= TextFit.Shrink(dial.Inner * 1.5, Ink.Measured(total).Width, 0.5);

            lines.Add(total);
        }

        string? unit = clock.Unit ?? clock.Label;
        if (unit is not null)
        {
            lines.Add(Ink.Words(unit, 13 * scale, palette.Secondary, strong: true));
        }

        double high = lines.Sum(line => line is TextBlock text ? Ink.Measured(text).Height : 15 * scale);
        double y = cy - (high / 2);
        foreach (UIElement line in lines)
        {
            if (line is TextBlock text)
            {
                canvas.Labels.Say(text, cx, y, Across.Middle, Down.Top);
                y += Ink.Measured(text).Height;
            }
            else
            {
                canvas.Labels.Put(line, cx - (15 * scale / 2), y);
                y += 15 * scale;
            }
        }
    }
}

/// <summary>Moments in order, as the system lists them: each with its symbol in the colour of its tone, its words, and its time at the end.</summary>
public sealed partial class TimelineView : KindControl
{
    private readonly Models.Timeline timeline;
    private readonly Func<Models.Node, UIElement>? render;

    /// <summary>Draws a timeline.</summary>
    /// <param name="timeline">The timeline.</param>
    /// <param name="render">How the shell draws a part held in a moment's detail.</param>
    public TimelineView(Models.Timeline timeline, Func<Models.Node, UIElement>? render = null)
    {
        this.timeline = timeline;
        this.render = render;
        Start();
    }

    internal override UIElement Build(Palette palette)
    {
        StackPanel stack = new() { Spacing = 12 };
        double mark = 30 * TextScale.Capped(1.4);
        foreach (Models.Moment moment in timeline.Items)
        {
            Windows.UI.Color colour = palette.Of(moment.Tone is Models.Tone tone ? HueRules.OfTone(tone) : Models.Hue.Accent);
            Grid line = new() { ColumnSpacing = 12 };
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

            Grid badge = new() { Width = mark, Height = mark, VerticalAlignment = VerticalAlignment.Center };
            badge.Children.Add(Ink.Disc(mark, Palette.Mark(colour, 0.15)));
            if (moment.Icon is not null)
            {
                UIElement icon = KindHooks.DrawIcon(moment.Icon, mark * 0.55, colour);
                if (icon is FrameworkElement placed)
                {
                    placed.HorizontalAlignment = HorizontalAlignment.Center;
                    placed.VerticalAlignment = VerticalAlignment.Center;
                }

                badge.Children.Add(icon);
            }
            else if (moment.State is Models.RowState state)
            {
                badge.Children.Add(StateMark(state, colour, mark * 0.4));
            }

            line.Children.Add(HealthParts.Hidden(badge));

            StackPanel words = new() { Spacing = 2 };
            words.Children.Add(new TextBlock { Text = moment.Title, TextWrapping = TextWrapping.Wrap, Foreground = moment.Muted ? palette.Secondary : palette.Primary });
            if (moment.Subtitle is not null)
            {
                words.Children.Add(new TextBlock { Text = moment.Subtitle, FontSize = 12, Foreground = palette.Secondary, TextWrapping = TextWrapping.Wrap });
            }

            if (render is not null)
            {
                foreach (Models.Node part in moment.Detail)
                {
                    words.Children.Add(render(part));
                }
            }

            Grid.SetColumn(words, 1);
            line.Children.Add(words);

            if (moment.Time is not null)
            {
                TextBlock time = new() { Text = moment.Time, Foreground = palette.Secondary, VerticalAlignment = VerticalAlignment.Center };
                Typography.SetNumeralAlignment(time, FontNumeralAlignment.Tabular);
                Grid.SetColumn(time, 2);
                line.Children.Add(time);
            }

            AutomationProperties.SetName(line, moment.Title + (moment.Time is null ? string.Empty : ", " + moment.Time));
            stack.Children.Add(line);
        }

        return stack;
    }

    // Done is a disc with a tick, current a ring with a dot, upcoming an empty ring.
    private static UIElement StateMark(Models.RowState state, Windows.UI.Color colour, double size)
    {
        Brush ink = Palette.Mark(colour);
        Grid mark = new() { Width = size, Height = size, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
        switch (state)
        {
            case Models.RowState.Done:
                mark.Children.Add(Ink.Disc(size, ink));
                mark.Children.Add(new FontIcon { Glyph = "", FontFamily = HealthParts.Icons, FontSize = size * 0.7, Foreground = Palette.Mark(Microsoft.UI.Colors.White), HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center });
                break;
            case Models.RowState.Current:
                mark.Children.Add(Ink.Disc(size, null, ink, 2));
                mark.Children.Add(Ink.Disc(size * 0.4, ink));
                ((FrameworkElement)mark.Children[1]).HorizontalAlignment = HorizontalAlignment.Center;
                ((FrameworkElement)mark.Children[1]).VerticalAlignment = VerticalAlignment.Center;
                break;
            default:
                mark.Children.Add(Ink.Disc(size, null, Palette.Mark(colour, 0.6), 1.5));
                break;
        }

        return mark;
    }
}
