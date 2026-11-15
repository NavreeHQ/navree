using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Navree.Controls.Charts;
using Navree.Controls.Composition;
using Navree.Controls.Drawing;
using Navree.Design;
using Navree.Protocol;

namespace Navree.Controls.Health;

/// <summary>
/// A figure: one number that matters: its label with its icon in the measure's colour, the number
/// large with its unit beside it, how it stands and which way it is going under it, the page's line after that, a small ring or gauge
/// at its end and a small chart under it. When it opens something, a tap on it does.
/// </summary>
public sealed partial class FigureView : KindControl
{
    private readonly Models.Figure figure;
    private readonly Func<Models.Node, UIElement> render;
    private readonly Action? open;

    /// <summary>Draws a figure.</summary>
    /// <param name="figure">The figure.</param>
    /// <param name="render">How the shell draws a part this control does not know how to draw itself.</param>
    /// <param name="open">What a tap on the figure does, or none when it opens nothing.</param>
    public FigureView(Models.Figure figure, Func<Models.Node, UIElement> render, Action? open = null)
    {
        this.figure = figure;
        this.render = render;
        this.open = open;
        Start();
    }

    internal override UIElement Build(Palette palette)
    {
        Models.Hue fallback = figure.Tone is Models.Tone tone ? HueRules.OfTone(tone) : Models.Hue.Neutral;
        Brush hue = figure.Icon is not null || figure.Colour is not null ? palette.Words(figure.Colour, fallback) : palette.Secondary;
        Windows.UI.Color mark = palette.Of(figure.Colour, fallback);

        StackPanel words = new() { Spacing = 3 };
        StackPanel head = new() { Orientation = Orientation.Horizontal, Spacing = 5 };
        if (figure.Icon is not null)
        {
            UIElement icon = KindHooks.DrawIcon(figure.Icon, 16, mark);
            if (icon is FrameworkElement placed)
            {
                placed.VerticalAlignment = VerticalAlignment.Center;
            }

            head.Children.Add(HealthParts.Hidden(icon));
        }

        TextBlock label = new() { Text = figure.Label, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, Foreground = hue, TextWrapping = TextWrapping.Wrap, VerticalAlignment = VerticalAlignment.Center, MaxLines = 2, TextTrimming = TextTrimming.CharacterEllipsis };
        head.Children.Add(HealthParts.Hidden(label));
        if (figure.Hint is not null && open is null)
        {
            head.Children.Add(HealthParts.Hint(figure.Hint));
        }

        words.Children.Add(head);

        StackPanel number = new() { Orientation = Orientation.Horizontal, Spacing = 4 };
        TextBlock value = new() { Text = figure.Value, FontSize = 28, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, TextWrapping = TextWrapping.NoWrap, TextTrimming = TextTrimming.CharacterEllipsis };
        Typography.SetNumeralAlignment(value, FontNumeralAlignment.Tabular);
        number.Children.Add(HealthParts.Hidden(value));
        if (figure.Unit is not null)
        {
            TextBlock unit = new() { Text = figure.Unit, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, Foreground = palette.Secondary, VerticalAlignment = VerticalAlignment.Bottom, Margin = new Thickness(0, 0, 0, 4) };
            number.Children.Add(HealthParts.Hidden(unit));
        }

        words.Children.Add(number);

        if (figure.Status is not null || figure.Trend is not null)
        {
            WrapPanel flow = new() { HorizontalSpacing = 8, VerticalSpacing = 4 };
            if (figure.Status is not null)
            {
                flow.Children.Add(HealthParts.Status(figure.Status, this));
            }

            if (figure.Trend is not null)
            {
                flow.Children.Add(HealthParts.Trend(figure.Trend, this));
            }

            words.Children.Add(HealthParts.Hidden(flow));
        }

        if (figure.Detail is not null)
        {
            words.Children.Add(HealthParts.Hidden(new TextBlock { Text = figure.Detail, FontSize = 12, Foreground = palette.Secondary, TextWrapping = TextWrapping.Wrap }));
        }

        Grid top = new() { ColumnSpacing = 10 };
        top.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        top.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        top.Children.Add(words);
        if (figure.Aside is not null)
        {
            UIElement aside = figure.Aside switch
            {
                Models.Ring ring => new RingView(ring, small: true),
                Models.Gauge gauge => new GaugeView(gauge, aside: true),
                _ => render(figure.Aside),
            };
            if (aside is FrameworkElement stood)
            {
                stood.VerticalAlignment = VerticalAlignment.Top;
                Grid.SetColumn(stood, 1);
            }

            top.Children.Add(aside);
        }

        StackPanel stack = new() { Spacing = 8 };
        stack.Children.Add(top);
        if (figure.Chart is not null)
        {
            stack.Children.Add(figure.Chart is Models.Chart chart ? new ChartView(chart, small: true) : render(figure.Chart));
        }

        string said = string.Join(", ", new[] { figure.Label, figure.Value + (figure.Unit is null ? string.Empty : " " + figure.Unit), figure.Status?.Text, figure.Trend is null ? null : TrendRules.Word(figure.Trend.Direction) + (figure.Trend.Text is null ? string.Empty : " " + figure.Trend.Text), figure.Detail }.Where(part => !string.IsNullOrEmpty(part)));
        if (open is null)
        {
            AutomationProperties.SetName(stack, said);
            AutomationProperties.SetAccessibilityView(stack, Microsoft.UI.Xaml.Automation.Peers.AccessibilityView.Content);
            return stack;
        }

        Button button = new()
        {
            Content = stack,
            Padding = new Thickness(8),
            HorizontalAlignment = HorizontalAlignment.Stretch,
            VerticalAlignment = VerticalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Stretch,
            VerticalContentAlignment = VerticalAlignment.Top,
            Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
            BorderThickness = new Thickness(0),
        };
        AutomationProperties.SetName(button, said);
        Action action = open;
        button.Click += (_, _) => action();
        return button;
    }
}

/// <summary>
/// The day a screen is about: the day's small score rings, its name over its date, between the steps to the day before and the day
/// after. A tap on the day opens a calendar bounded by the days there are. Each step is the page's own address; a chosen day is sent
/// with the pager's pick address and the day as text.
/// </summary>
public sealed partial class DayPagerView : KindControl
{
    private readonly Models.DayPager pager;
    private readonly Func<Models.Node, UIElement> render;
    private readonly Action<string> step;
    private readonly Action<string, string> pick;

    /// <summary>Draws a day pager.</summary>
    /// <param name="pager">The pager.</param>
    /// <param name="render">How the shell draws the day's score parts.</param>
    /// <param name="step">Goes to the page at an address (the day before, the day after, today).</param>
    /// <param name="pick">Goes to the pick address with the chosen day as text.</param>
    public DayPagerView(Models.DayPager pager, Func<Models.Node, UIElement> render, Action<string> step, Action<string, string> pick)
    {
        this.pager = pager;
        this.render = render;
        this.step = step;
        this.pick = pick;
        Start();
    }

    internal override UIElement Build(Palette palette)
    {
        Grid bar = new() { ColumnSpacing = 4 };
        bar.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        bar.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        bar.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        bar.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        bar.HorizontalAlignment = HorizontalAlignment.Left;

        UIElement previous = Step(pager.Previous, "", pager.Words?.Previous ?? KindStrings.PreviousDay);
        bar.Children.Add(previous);

        Button day = Day();
        Grid.SetColumn(day, 1);
        bar.Children.Add(day);

        UIElement next = Step(pager.Next, "", pager.Words?.Next ?? KindStrings.NextDay);
        if (next is FrameworkElement placed)
        {
            Grid.SetColumn(placed, 2);
        }

        bar.Children.Add(next);

        if (pager.Today is string today)
        {
            Button now = new() { Content = KindStrings.Today, VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(8, 0, 0, 0) };
            now.Click += (_, _) => step(today);
            Grid.SetColumn(now, 3);
            bar.Children.Add(now);
        }

        return bar;
    }

    private UIElement Step(string? href, string glyph, string words)
    {
        if (href is null)
        {
            // No day there: nothing is drawn (the place stays, so the day does not move), and no reader is told of it.
            return HealthParts.Hidden(new Border { Width = 40, Height = 40, VerticalAlignment = VerticalAlignment.Center });
        }

        Button button = new()
        {
            Content = new FontIcon { Glyph = glyph, FontFamily = HealthParts.Icons, FontSize = 14 },
            MinWidth = 40,
            MinHeight = 40,
            Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
            BorderThickness = new Thickness(0),
            VerticalAlignment = VerticalAlignment.Center,
        };
        AutomationProperties.SetName(button, words);
        ToolTipService.SetToolTip(button, words);
        button.Click += (_, _) => step(href);
        return button;
    }

    // The day's rings and its name over its date; a tap opens the calendar.
    private Button Day()
    {
        StackPanel content = new() { Orientation = Orientation.Horizontal, Spacing = 8 };
        if (pager.Scores is not null)
        {
            content.Children.Add(render(pager.Scores));
        }

        StackPanel names = new() { VerticalAlignment = VerticalAlignment.Center };
        names.Children.Add(new TextBlock { Text = pager.Day, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold });
        names.Children.Add(new TextBlock { Text = PagerRules.Words(pager.Date), FontSize = 12, Opacity = 0.7 });
        content.Children.Add(names);
        Button button = new()
        {
            Content = content,
            MinHeight = 40,
            Padding = new Thickness(8, 2, 8, 2),
            Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
            BorderThickness = new Thickness(0),
            IsEnabled = pager.Pick is not null,
        };
        AutomationProperties.SetName(button, PagerRules.Say(pager));
        AutomationProperties.SetHelpText(button, pager.Words?.Choose ?? string.Empty);
        if (pager.Pick is Models.DayPick choose)
        {
            button.Flyout = Calendar(choose);
        }

        return button;
    }

    private Flyout Calendar(Models.DayPick choose)
    {
        CalendarView calendar = new() { SelectionMode = CalendarViewSelectionMode.Single };
        if (PagerRules.Day(pager.First) is DateOnly low)
        {
            calendar.MinDate = PagerRules.Midnight(low, TimeZoneInfo.Local);
        }

        if (PagerRules.Day(pager.Last) is DateOnly high)
        {
            calendar.MaxDate = PagerRules.Midnight(high, TimeZoneInfo.Local);
        }

        if (PagerRules.Day(pager.Date) is DateOnly shown)
        {
            DateTimeOffset at = PagerRules.Midnight(shown, TimeZoneInfo.Local);
            calendar.SetDisplayDate(at);
            calendar.SelectedDates.Add(at);
        }

        Flyout flyout = new() { Content = calendar };
        calendar.SelectedDatesChanged += (view, args) =>
        {
            if (args.AddedDates.Count == 0)
            {
                return;
            }

            DateOnly chosen = PagerRules.DayOf(args.AddedDates[0], TimeZoneInfo.Local);
            string text = PagerRules.Text(chosen);

            // The day already shown is selected when the calendar opens: choosing it again is not a change.
            if (text != pager.Date && PagerRules.Choosable(text, pager.First, pager.Last))
            {
                flyout.Hide();
                pick(choose.Href, text);
            }
        };
        return flyout;
    }
}
