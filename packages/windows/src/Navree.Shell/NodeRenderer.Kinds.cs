using System.Runtime.CompilerServices;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Navree.Controls.Charts;
using Navree.Controls.Composition;
using Navree.Controls.Flags;
using Navree.Controls.Health;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

/// <summary>
/// The kinds of part that are drawn by controls of our own: the charts, rings, gauges and the rest of the health kinds, the composition
/// kinds, and flags. What each one draws, and how, is in <c>Navree.Controls</c>; this file gives each kind its control and carries what
/// the control's person does to the core.
/// </summary>
internal sealed partial class NodeRenderer
{
    // What the drawn kinds ask of the shell, once: how an icon of the page's own is drawn.
    static NodeRenderer()
    {
        KindHooks.Icon = (icon, size, colour) => DrawIcon(icon, size, new SolidColorBrush(colour));
    }

    // The words of the text drawn last, so a chart does not say its own title again just under the same words.
    private string? wordsAbove;

    // Which parts of a screen have been drawn already: a part that joins a screen already drawn comes in as it says, and a screen's
    // first drawing is never animated part by part.
    private static readonly ConditionalWeakTable<RenderContext, PassKnown> Passes = new();
    private static readonly Dictionary<long, HashSet<string>> Known = new();

    private sealed class PassKnown
    {
        internal bool First { get; init; }
    }

    private bool ArrivesNew(string? id)
    {
        if (!Passes.TryGetValue(context, out PassKnown? pass))
        {
            pass = new PassKnown { First = !Known.ContainsKey(context.Screen.Id) };
            Passes.Add(context, pass);
            if (!Known.ContainsKey(context.Screen.Id))
            {
                Known[context.Screen.Id] = new HashSet<string>();
            }
        }

        if (id is null)
        {
            return false;
        }

        bool fresh = Known[context.Screen.Id].Add(id);
        return fresh && !pass.First;
    }

    private UIElement Arrive(UIElement shown, string? id, Models.Appear? appear)
    {
        if (appear is not null && shown is FrameworkElement element && ArrivesNew(id))
        {
            Arrivals.Play(element, appear);
        }

        return shown;
    }

    /// <summary>Draws the kinds this file knows; none for any other.</summary>
    private UIElement? DrawKind(Models.Node node)
    {
        switch (node)
        {
            case Models.Chart chart:
                return new ChartView(chart, small: false, titled: HintRules.ShowsTitle(chart.Title, wordsAbove is null ? Array.Empty<string>() : new[] { wordsAbove }));
            case Models.Scatter scatter:
                return new ScatterView(scatter);
            case Models.Ring ring:
                return new RingView(ring);
            case Models.Rings rings:
                return new RingsView(rings);
            case Models.Gauge gauge:
                return new GaugeView(gauge);
            case Models.Meter meter:
                return new MeterView(meter);
            case Models.MiniColumns columns:
                return new ColumnsView(columns);
            case Models.ColorStrip strip:
                return new StripView(strip);
            case Models.RangeScale range:
                return new RangeView(range);
            case Models.DayClock clock:
                return new DayClockView(clock);
            case Models.CalendarHeatmap calendar:
                return new CalendarHeatView(calendar);
            case Models.MatrixHeatmap matrix:
                return new MatrixHeatView(matrix);
            case Models.Timeline timeline:
                return new TimelineView(timeline, Render);
            case Models.Figure figure:
                return new FigureView(figure, Render, figure.Action is null ? null : () => Act(figure.Action, null, figure.Id));
            case Models.DayPager pager:
                return DrawPager(pager);
            case Models.Progress progress:
                return DrawProgressBar(progress);
            case Models.Badge { Dot: true } badge:
                return HealthParts.Status(badge);
            case Models.Text text:
                return DrawWords(text);
            case Models.Stack stack when stack.Wrap || stack.Appear is not null:
                return DrawComposedStack(stack);
            case Models.Card card:
                return DrawTintedCard(card);
            case Models.Layers layers:
                return Composed.Layers(layers, Render);
            case Models.Shape shape:
                return Composed.Shape(shape);
            case Models.Carousel carousel:
                return Composed.Carousel(carousel, Render);
            case Models.Adaptive adaptive:
                return Column(context.Width != WidthClass.Compact ? adaptive.Wide : adaptive.Compact);
            case Models.FeedRow row:
                return DrawFeedRow(row);
            case Models.Message message:
                return DrawMessage(message);
            case Models.Toolbar toolbar:
                return DrawToolbar(toolbar);
            case Models.Icon { Colour: not null } icon:
                return DrawIcon(icon, 20, KindHooks.HueBrush(icon.Colour, HueRules.OfTone(icon.Tone)));
            default:
                return null;
        }
    }

    // ---- words, with flags -------------------------------------------------------------------------------

    private static double FlagHeight(Models.TextStyle style) =>
        style switch
        {
            Models.TextStyle.Caption => 12,
            Models.TextStyle.Title2 => 20,
            Models.TextStyle.Title => 28,
            Models.TextStyle.LargeTitle => 40,
            Models.TextStyle.Display => 68,
            _ => 14,
        };

    private UIElement DrawWords(Models.Text text)
    {
        wordsAbove = text.Value;
        UIElement drawn;
        if (FlagInline.Holds(text.Value))
        {
            // Windows draws no flag for a pair of regional indicator letters: the client draws it.
            drawn = FlagInline.Create(text.Value, words => (TextBlock)DrawText(text with { Value = words }), FlagHeight(text.Style));
        }
        else
        {
            drawn = DrawText(text);
        }

        if (drawn is TextBlock block)
        {
            if (text.Tabular)
            {
                Typography.SetNumeralAlignment(block, FontNumeralAlignment.Tabular);
            }

            if (text.Colour is not null)
            {
                block.Foreground = KindHooks.HueWords(text.Colour, Models.Hue.Neutral);
            }
        }

        return drawn;
    }

    // ---- progress with its colour ------------------------------------------------------------------------

    private static UIElement DrawProgressBar(Models.Progress progress)
    {
        StackPanel panel = new() { Spacing = 4 };
        if (progress.Label is not null)
        {
            panel.Children.Add(Look.Label(progress.Label, "CaptionTextBlockStyle", muted: true));
        }

        double? fraction = ProgressRules.Fraction(progress.Value);
        ProgressBar bar = new() { Minimum = 0, Maximum = 1, IsIndeterminate = fraction is null };
        if (fraction is double value)
        {
            bar.Value = value;
        }

        bar.Foreground = KindHooks.HueBrush(progress.Colour, ProgressRules.Hue(progress));
        AutomationProperties.SetName(bar, progress.Label ?? string.Empty);
        panel.Children.Add(bar);
        return panel;
    }

    // ---- the day a screen is about -----------------------------------------------------------------------

    private UIElement DrawPager(Models.DayPager pager)
    {
        long screen = context.Screen.Id;
        string? from = pager.Id;
        return new DayPagerView(
            pager,
            Render,
            href => Act(new Models.ActionNavigate { Href = href }, null, from),
            (href, day) => context.Send(Events.Act(screen, new Models.ActionNavigate { Href = href }, currentForm, currentOverlay, day, null, null, from)));
    }

    // ---- composition -------------------------------------------------------------------------------------

    // A stack that wraps onto new lines, and one that comes in as it says.
    private UIElement DrawComposedStack(Models.Stack stack)
    {
        UIElement shown;
        if (stack.Wrap)
        {
            WrapPanel flow = new() { HorizontalSpacing = WindowRules.Space(stack.Gap), VerticalSpacing = WindowRules.Space(stack.Gap), Margin = new Thickness(WindowRules.Space(stack.Pad)) };
            foreach (Models.Node child in stack.Children)
            {
                flow.Children.Add(Render(child));
            }

            shown = flow;
        }
        else
        {
            shown = DrawStack(stack with { Appear = null });
        }

        return Arrive(shown, stack.Id, stack.Appear);
    }

    // A card with its tint, its menu, and the way it comes in; one control when it has an action.
    private UIElement DrawTintedCard(Models.Card card)
    {
        StackPanel body = new() { Spacing = 8 };
        if (card.Label is not null && card.Action is null)
        {
            body.Children.Add(Look.Label(card.Label, "BodyStrongTextBlockStyle"));
        }

        foreach (Models.Node child in card.Children)
        {
            body.Children.Add(Render(child));
        }

        double pad = card.Pad is int steps ? WindowRules.Space(steps) : 16;
        Microsoft.UI.Xaml.Controls.Border surface = new()
        {
            Child = body,
            Padding = new Thickness(pad),
            CornerRadius = new CornerRadius(8),
            BorderThickness = new Thickness(1),
            Background = Look.Resource("CardBackgroundFillColorDefaultBrush"),
            BorderBrush = Look.Resource("CardStrokeColorDefaultBrush"),
        };
        if (card.Colour is not null)
        {
            // The tint says which thing the card is about and never carries meaning alone: a soft wash over the card's own ground.
            bool dark = Application.Current.RequestedTheme == ApplicationTheme.Dark;
            (double ground, double edge) = ComposeRules.Tint(dark);
            surface.Background = KindHooks.HueBrush(card.Colour, Models.Hue.Neutral, ground);
            surface.BorderBrush = KindHooks.HueBrush(card.Colour, Models.Hue.Neutral, edge);
            Microsoft.UI.Xaml.Controls.Border under = new() { Child = surface, CornerRadius = new CornerRadius(8), Background = Look.Resource("CardBackgroundFillColorDefaultBrush") };
            return Finish(under, card);
        }

        return Finish(surface, card);
    }

    private UIElement Finish(FrameworkElement shown, Models.Card card)
    {
        FrameworkElement result = shown;
        if (card.Action is not null)
        {
            Button button = new()
            {
                Content = shown,
                Padding = new Thickness(0),
                HorizontalAlignment = HorizontalAlignment.Stretch,
                HorizontalContentAlignment = HorizontalAlignment.Stretch,
                Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
                BorderThickness = new Thickness(0),
                CornerRadius = new CornerRadius(8),
            };
            AutomationProperties.SetName(button, card.Label ?? string.Empty);
            Models.Action action = card.Action;
            string? from = card.Id;
            button.Click += (_, _) => Act(action, null, from);
            result = button;
        }

        if (card.Menu.Count > 0)
        {
            result.ContextFlyout = MenuFor(card.Menu);
        }

        return Arrive(result, card.Id, card.Appear);
    }

    // ---- activity ----------------------------------------------------------------------------------------

    private static string ToneGlyph(Models.Tone? tone) =>
        tone switch
        {
            Models.Tone.Success => "",
            Models.Tone.Warning => "",
            Models.Tone.Danger => "",
            _ => "",
        };

    // Something that happened, as a row of an activity list: its symbol in the colour of what it means (or the person who did it), the
    // words with their names in bold, when it happened under them, and the thing it is about opened by a tap.
    private UIElement DrawFeedRow(Models.FeedRow row)
    {
        Grid grid = new() { ColumnSpacing = 12, Padding = new Thickness(0, 4, 0, 4) };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(32) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        UIElement lead;
        if (row.Actor is not null)
        {
            lead = DrawAvatar(row.Actor);
        }
        else if (row.Icon is not null)
        {
            lead = DrawIcon(row.Icon, 20, row.Muted ? Look.Muted : KindHooks.HueBrush(row.Icon.Colour, HueRules.OfTone(row.Tone ?? Models.Tone.Accent)));
        }
        else
        {
            FontIcon glyph = Look.Glyph(ToneGlyph(row.Tone), 16);
            glyph.Foreground = row.Muted ? Look.Muted : Look.ToneBrush(row.Tone);
            lead = glyph;
        }

        if (lead is FrameworkElement placed)
        {
            placed.VerticalAlignment = VerticalAlignment.Top;
            placed.HorizontalAlignment = HorizontalAlignment.Center;
        }

        AutomationProperties.SetAccessibilityView(lead, Microsoft.UI.Xaml.Automation.Peers.AccessibilityView.Raw);
        grid.Children.Add(lead);

        StackPanel words = new() { Spacing = 3 };
        words.Children.Add(Inlines(row.Runs, "BodyTextBlockStyle", row.Muted));
        foreach (Models.Node part in row.Detail)
        {
            words.Children.Add(Render(part));
        }

        if (row.Time is not null)
        {
            words.Children.Add(Look.Label(row.Time, "CaptionTextBlockStyle", muted: true));
        }

        Grid.SetColumn(words, 1);
        grid.Children.Add(words);
        if (row.Action is null)
        {
            return grid;
        }

        Button button = new()
        {
            Content = grid,
            Padding = new Thickness(4, 0, 4, 0),
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Stretch,
            Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
            BorderThickness = new Thickness(0),
        };
        AutomationProperties.SetName(button, string.Concat(row.Runs.Select(run => run.Text)) + (row.Time is null ? string.Empty : ", " + row.Time));
        Models.Action action = row.Action;
        string? from = row.Id;
        button.Click += (_, _) => Act(action, null, from);
        return button;
    }

    // A message of a conversation, as a thread sets one out: who wrote it beside their picture, when, its words, and how far it got.
    private UIElement DrawMessage(Models.Message message)
    {
        Grid grid = new() { ColumnSpacing = 12, Padding = new Thickness(0, 4, 0, 4) };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        UIElement picture = DrawAvatar(message.Avatar ?? new Models.Avatar { Name = message.Author });
        ((FrameworkElement)picture).VerticalAlignment = VerticalAlignment.Top;
        grid.Children.Add(picture);

        StackPanel column = new() { Spacing = 6 };
        Grid heading = new() { ColumnSpacing = 8 };
        heading.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        heading.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        heading.Children.Add(Look.Label(message.Author, "BodyStrongTextBlockStyle"));
        if (message.Time is not null)
        {
            TextBlock time = Look.Label(message.Time, "CaptionTextBlockStyle", muted: true);
            time.VerticalAlignment = VerticalAlignment.Center;
            Grid.SetColumn(time, 1);
            heading.Children.Add(time);
        }

        column.Children.Add(heading);
        if (message.Note)
        {
            StackPanel note = new() { Orientation = Orientation.Horizontal, Spacing = 6 };
            FontIcon lockGlyph = Look.Glyph("", 14);
            lockGlyph.Foreground = Look.ToneBrush(Models.Tone.Warning);
            note.Children.Add(lockGlyph);
            note.Children.Add(Look.Label(MessageRules.NoteWords, "CaptionTextBlockStyle", tone: Models.Tone.Warning));
            column.Children.Add(note);
        }

        foreach (Models.Node part in message.Body)
        {
            column.Children.Add(Render(part));
        }

        if (message.Delivery is Models.Delivery delivery && MessageRules.Delivery(delivery) is (string words, string glyph))
        {
            StackPanel line = new() { Orientation = Orientation.Horizontal, Spacing = 6 };
            FontIcon mark = Look.Glyph(glyph, 12);
            Brush ink = delivery == Models.Delivery.Failed ? Look.ToneBrush(Models.Tone.Danger) : Look.Muted;
            mark.Foreground = ink;
            line.Children.Add(mark);
            TextBlock said = Look.Label(words, "CaptionTextBlockStyle");
            said.Foreground = ink;
            line.Children.Add(said);
            column.Children.Add(line);
        }

        Grid.SetColumn(column, 1);
        grid.Children.Add(column);
        return grid;
    }

    // A toolbar the page puts in its content is a command bar in place: the actions it names, the loud ones on the bar and the rest in its
    // overflow, each with its words and its symbol.
    private UIElement DrawToolbar(Models.Toolbar toolbar)
    {
        CommandBar bar = new() { DefaultLabelPosition = CommandBarDefaultLabelPosition.Right, IsOpen = false, Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
        foreach (Models.ToolbarItem item in toolbar.Items)
        {
            string glyph = item.Standard is Models.Standard standard ? SystemGlyphs.Of(standard) : SystemGlyphs.For(item.Icon.Name, item.Title);
            AppBarButton button = new() { Label = item.Title, Icon = Look.Glyph(glyph), IsEnabled = !item.Disabled };
            AutomationProperties.SetName(button, item.Title);
            if (item.Reason is not null)
            {
                ToolTipService.SetToolTip(button, item.Reason);
            }

            if (item.Role == Models.Role.Destructive)
            {
                button.Foreground = Look.ToneBrush(Models.Tone.Danger);
            }

            if (item.Menu.Count > 0)
            {
                button.Flyout = MenuFor(item.Menu);
            }
            else
            {
                Models.Action? action = item.Action;
                Models.Question? ask = item.Ask;
                string? from = item.Id;
                button.Click += (_, _) => Act(action, ask, from);
            }

            if (item.Prominence == Models.Prominence.Low)
            {
                bar.SecondaryCommands.Add(button);
            }
            else
            {
                bar.PrimaryCommands.Add(button);
            }
        }

        return bar;
    }
}
