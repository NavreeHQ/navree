using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Navree.Controls.Composition;
using Navree.Controls.Drawing;
using Navree.Design;
using Navree.Protocol;
using Border = Microsoft.UI.Xaml.Controls.Border;

namespace Navree.Controls.Health;

/// <summary>
/// A control built from the theme it stands in: it builds its content when it is made, and again when what its colours come from changes
/// (the theme it stands in, a contrast theme turned on or off, the accent the person chose, the size of their text), so the hues of its
/// marks, its key and its words follow all of them.
/// </summary>
public abstract partial class KindControl : UserControl
{
    private string? builtFor;
    private bool watching;
    private Microsoft.UI.Dispatching.DispatcherQueueTimer? settle;

    /// <summary>Starts a control; the derived control calls <see cref="Start"/> once it holds what it draws.</summary>
    protected KindControl()
    {
        Loaded += (_, _) =>
        {
            Watch();
            Refresh();
        };
        Unloaded += (_, _) => Unwatch();
        ActualThemeChanged += (_, _) => Refresh();
    }

    /// <summary>Builds the content for a palette.</summary>
    internal abstract UIElement Build(Palette palette);

    /// <summary>Builds the content for the first time.</summary>
    protected void Start() => Rebuild();

    // While the control is in the tree it listens for a change of the system's settings; out of the tree it does not, so the system holds no reference to it.
    private void Watch()
    {
        if (!watching)
        {
            watching = true;
            ThemeState.Changed += OnSystemChanged;
        }
    }

    private void Unwatch()
    {
        if (watching)
        {
            watching = false;
            ThemeState.Changed -= OnSystemChanged;
        }

        settle?.Stop();
    }

    private void OnSystemChanged() => ThemeState.OnThreadOf(this, () =>
    {
        Refresh();

        // The system's own resources (the accent's brushes) are brought up to date by the framework in its own time; asked again a moment later, they are.
        settle ??= DispatcherQueue.CreateTimer();
        settle.Interval = TimeSpan.FromMilliseconds(400);
        settle.IsRepeating = false;
        settle.Tick -= OnSettled;
        settle.Tick += OnSettled;
        settle.Stop();
        settle.Start();
    });

    private void OnSettled(Microsoft.UI.Dispatching.DispatcherQueueTimer sender, object args)
    {
        sender.Stop();
        if (watching)
        {
            Rebuild();
        }
    }

    private void Refresh()
    {
        if (builtFor != new Palette(this).Signature)
        {
            Rebuild();
        }
    }

    private void Rebuild()
    {
        Palette palette = new(this);
        builtFor = palette.Signature;
        Content = Build(palette);
    }
}

/// <summary>The small parts every health kind shares: a hint behind an info button, a status in a tinted capsule, a trend with its arrow, a key, and a heading.</summary>
public static class HealthParts
{
    /// <summary>The system's icon font.</summary>
    internal static FontFamily Icons => new("Segoe Fluent Icons, Segoe MDL2 Assets");

    /// <summary>Hides a part from a screen reader: the drawing it belongs to says what it says.</summary>
    internal static T Hidden<T>(T element)
        where T : UIElement
    {
        AutomationProperties.SetAccessibilityView(element, AccessibilityView.Raw);

        // A reader skips a part left out of its view, but still reaches what stands inside it: so each part inside is left out too.
        switch (element)
        {
            case Panel panel:
                foreach (UIElement child in panel.Children)
                {
                    Hidden(child);
                }

                break;
            case Border { Child: UIElement inner }:
                Hidden(inner);
                break;
            case ContentControl { Content: UIElement content }:
                Hidden(content);
                break;
            default:
                break;
        }

        return element;
    }

    /// <summary>More about a part, on request: the info button opening the page's words in a flyout.</summary>
    public static Button Hint(Models.Hint hint)
    {
        FontIcon icon = new()
        {
            Glyph = hint.Mark == Models.HintMark.Info ? "" : "",
            FontSize = 14,
            FontFamily = Icons,
        };
        StackPanel words = new() { Spacing = 8, MaxWidth = 320 };
        if (hint.Title is not null)
        {
            TextBlock title = new() { Text = hint.Title, TextWrapping = TextWrapping.Wrap };
            if (Application.Current.Resources.TryGetValue("SubtitleTextBlockStyle", out object? style) && style is Style found)
            {
                title.Style = found;
            }

            words.Children.Add(title);
        }

        words.Children.Add(new TextBlock { Text = hint.Text, TextWrapping = TextWrapping.Wrap });
        Button button = new()
        {
            Content = icon,
            Padding = new Thickness(0),
            MinWidth = 32,
            MinHeight = 32,
            Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
            BorderThickness = new Thickness(0),
            VerticalAlignment = VerticalAlignment.Center,
            Flyout = new Flyout { Content = words },
        };
        AutomationProperties.SetName(button, HintRules.ActionName(hint));
        ToolTipService.SetToolTip(button, HintRules.ActionName(hint));
        return button;
    }

    /// <summary>How a ring or a figure says how its value stands: the word in its tone, in a small capsule tinted with that tone (or, in a row, the dot and the word alone).</summary>
    public static UIElement Status(Models.Badge badge, FrameworkElement? owner = null, bool capsule = true)
    {
        Palette palette = new(owner ?? new Border());
        Models.Hue hue = HueRules.OfTone(badge.Tone);
        StackPanel line = new() { Orientation = Orientation.Horizontal, Spacing = 6 };
        if (badge.Dot)
        {
            line.Children.Add(new Microsoft.UI.Xaml.Shapes.Ellipse { Width = 7, Height = 7, Fill = palette.Fill(hue), VerticalAlignment = VerticalAlignment.Center });
        }

        TextBlock text = Ink.Words(badge.Text, 12, palette.Words(badge.Tone), strong: true);
        text.TextWrapping = TextWrapping.Wrap;
        text.VerticalAlignment = VerticalAlignment.Center;
        line.Children.Add(text);
        if (!capsule)
        {
            // In a row of a list or a table: the dot and its words, with no ground of their own.
            line.HorizontalAlignment = HorizontalAlignment.Left;
            AutomationProperties.SetName(line, badge.Text);
            return line;
        }

        Border tinted = new()
        {
            Child = line,
            Padding = new Thickness(10, 2, 10, 3),
            CornerRadius = new CornerRadius(12),

            // A contrast theme has no tint: the capsule is an outline in the text colour.
            Background = palette.Contrast ? null : palette.Fill(hue, 0.15),
            BorderBrush = palette.Contrast ? palette.Primary : null,
            BorderThickness = new Thickness(palette.Contrast ? 1 : 0),
            HorizontalAlignment = HorizontalAlignment.Left,
        };
        AutomationProperties.SetName(tinted, badge.Text);
        return tinted;
    }

    /// <summary>Which way a value is going, in the system's arrow and in the colour of its news rather than of its direction.</summary>
    public static UIElement Trend(Models.Trend trend, FrameworkElement? owner = null)
    {
        Palette palette = new(owner ?? new Border());
        Brush ink = trend.Tone is Models.Tone tone && tone != Models.Tone.Neutral ? palette.Words(tone) : palette.Secondary;
        StackPanel line = new() { Orientation = Orientation.Horizontal, Spacing = 4 };
        FontIcon arrow = new() { Glyph = TrendRules.Glyph(trend.Direction), FontSize = 12, FontFamily = Icons, Foreground = ink, VerticalAlignment = VerticalAlignment.Center };
        line.Children.Add(arrow);
        if (trend.Text is not null)
        {
            TextBlock words = Ink.Words(trend.Text, 12, ink, strong: true);
            words.TextWrapping = TextWrapping.Wrap;
            line.Children.Add(words);
        }

        AutomationProperties.SetName(line, TrendRules.Word(trend.Direction) + (trend.Text is null ? string.Empty : ", " + trend.Text));
        return line;
    }

    /// <summary>A chart's heading: its title, and its hint beside it; nothing when it has neither.</summary>
    public static UIElement? Heading(string? title, Models.Hint? hint, bool showTitle = true)
    {
        bool hasTitle = showTitle && !string.IsNullOrEmpty(title);
        if (!hasTitle && hint is null)
        {
            return null;
        }

        StackPanel line = new() { Orientation = Orientation.Horizontal, Spacing = 4 };
        if (hasTitle)
        {
            TextBlock text = new() { Text = title ?? string.Empty, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, TextWrapping = TextWrapping.Wrap, VerticalAlignment = VerticalAlignment.Center };
            AutomationProperties.SetHeadingLevel(text, AutomationHeadingLevel.Level3);
            line.Children.Add(text);
        }

        if (hint is not null)
        {
            line.Children.Add(Hint(hint));
        }

        return line;
    }

    /// <summary>The dot of a key or a list, in the colour of the part at a place; in a contrast theme the mark of that part's look, so it can be told from the others without its colour.</summary>
    internal static FrameworkElement Dot(Palette palette, Windows.UI.Color colour, int index, double size = 8)
    {
        if (palette.Look(index) is SeriesLook look)
        {
            Brush ink = Palette.Mark(palette.InkOf(look));
            FrameworkElement marker = (FrameworkElement)Ink.Marker(look.Shape, size + 2, ink);
            marker.VerticalAlignment = VerticalAlignment.Center;
            return marker;
        }

        return new Microsoft.UI.Xaml.Shapes.Ellipse { Width = size, Height = size, Fill = Palette.Mark(colour), VerticalAlignment = VerticalAlignment.Center };
    }

    /// <summary>One entry of a key: a dot in its colour, its name, and the page's words of its share or total.</summary>
    internal static UIElement KeyEntry(Palette palette, Windows.UI.Color colour, string name, string? detail, bool emphasised = true, int index = 0)
    {
        StackPanel entry = new() { Orientation = Orientation.Horizontal, Spacing = 6 };
        entry.Children.Add(Dot(palette, colour, index));
        TextBlock words = Ink.Words(name, 12, emphasised ? palette.Primary : palette.Secondary);
        entry.Children.Add(words);
        if (!string.IsNullOrEmpty(detail))
        {
            entry.Children.Add(Ink.Words(detail, 12, palette.Secondary));
        }

        return entry;
    }

    /// <summary>A key's entries one after another, onto new lines as the room runs out. Hidden from a screen reader, which is told by the drawing.</summary>
    internal static UIElement Key(Palette palette, IEnumerable<(Windows.UI.Color Colour, string Name, string? Detail)> entries, int? current = null)
    {
        WrapPanel flow = new() { HorizontalSpacing = 14, VerticalSpacing = 6 };
        int at = 0;
        foreach ((Windows.UI.Color colour, string name, string? detail) in entries)
        {
            flow.Children.Add(KeyEntry(palette, colour, name, detail, current is null || current == at, at));
            at++;
        }

        return Hidden(flow);
    }
}
