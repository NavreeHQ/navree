using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Markup;
using Microsoft.UI.Xaml.Media;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;
using ShapePath = Microsoft.UI.Xaml.Shapes.Path;

namespace Navree.Shell;

/// <summary>
/// Draws the parts of a screen's tree with the system's own controls. The core has made the tree ready: what is
/// open, hidden, disabled or waiting is already in it, or in the screen's states. What a client draws it draws
/// as the platform draws it; it never restyles a system control.
/// </summary>
internal sealed partial class NodeRenderer(RenderContext context)
{
    private string? currentForm;
    private string? currentOverlay;

    /// <summary>Draws a node.</summary>
    internal UIElement Render(Models.Node node)
    {
        UIElement element;
        try
        {
            element = Draw(node);
        }
        catch (Exception problem) when (problem is not OutOfMemoryException)
        {
            // A part that cannot be drawn (a path that is not path data, two rows with one id, a control that throws) is that part's state, not the app's end.
            System.Diagnostics.Debug.WriteLine("a part could not be drawn: " + node.GetType().Name + ": " + problem.Message);
            element = CouldNotShow(node);
        }

        string? id = Tree.IdOf(node);
        if (id is not null && element is FrameworkElement shown)
        {
            // Every drawn node with an id carries it, so what the person has on it (the focus, a caret, a table's active cell) can be found
            // again in the page that is drawn after the next answer, and the core's request to move the focus has a place to go.
            PageKeeper.Mark(shown, id);
            if (context.Screen.Has(id, Models.State.Hidden))
            {
                shown.Visibility = Visibility.Collapsed;
            }
        }

        return element;
    }

    /// <summary>Draws a list of nodes into a vertical stack.</summary>
    internal UIElement Column(IEnumerable<Models.Node> nodes, double spacing = 8)
    {
        StackPanel panel = new() { Spacing = spacing };
        foreach (Models.Node node in nodes)
        {
            panel.Children.Add(Render(node));
        }

        return panel;
    }

    // The kinds drawn by controls of our own (NodeRenderer.Kinds.cs) come first; the rest are drawn here.
    private UIElement Draw(Models.Node node)
    {
        // The screen's title is drawn above the page already: the first heading of the page that says the same words is not said again.
        if (!titleCovered && node is Models.Text { Heading: not null } heading && context.Screen.Screen?.Title == heading.Value)
        {
            titleCovered = true;
            return Empty();
        }

        return DrawKind(node) ?? DrawPlain(node);
    }

    private bool titleCovered;

    private UIElement DrawPlain(Models.Node node) =>
        node switch
        {
            Models.Stack stack => DrawStack(stack),
            Models.Section section => DrawSection(section),
            Models.List list => DrawList(list),
            Models.Row row => DrawRow(row),
            Models.Split split => DrawSplit(split),
            Models.Tabs tabs => DrawTabs(tabs),
            Models.Disclosure disclosure => DrawDisclosure(disclosure),
            Models.Divider => Divider(),
            Models.Spacer => new Microsoft.UI.Xaml.Controls.Border { Height = WindowRules.Space(2) },
            Models.Region region => DrawRegion(region),
            Models.Rich rich => DrawRich(rich),
            Models.Image image => DrawImage(image),
            Models.Icon icon => DrawIcon(icon, 20, null),
            Models.Avatar avatar => DrawAvatar(avatar),
            Models.Badge badge => DrawBadge(badge),
            Models.Banner banner => DrawBanner(banner),
            Models.Empty empty => DrawEmpty(empty),
            Models.Facts facts => DrawFacts(facts),
            Models.Table table => DrawTable(table),
            Models.Code code => DrawCode(code),
            Models.Button button => DrawButton(button),
            Models.Link link => DrawLink(link),
            Models.Menu menu => DrawMenu(menu),
            Models.Form form => DrawForm(form),
            Models.Field field => DrawField(field),
            Models.Sheet sheet => Overlay(sheet, sheet.Id),
            Models.Dialog dialog => Overlay(dialog, dialog.Id),
            Models.Popover popover => Overlay(popover, popover.Id),
            Models.Pane pane => DrawPane(pane),
            Models.NativeView native => native.Alt is null ? Empty() : Render(native.Alt),
            Models.NodeUnknown unknown => unknown.Alt is null ? Empty() : Render(unknown.Alt),
            _ => NotDrawn(node),
        };

    private static UIElement Empty() => new Microsoft.UI.Xaml.Controls.Border();

    // The place of a part that could not be drawn says so, small, and a screen reader is told.
    private static UIElement CouldNotShow(Models.Node node)
    {
        TextBlock note = Look.Label("(this part could not be shown)", "CaptionTextBlockStyle", muted: true);
        AutomationProperties.SetName(note, "A part of this page could not be shown");
        _ = node;
        return note;
    }

    // A kind this build does not draw is said so, small, where it stands: never silently, never as a fake.
    private static UIElement NotDrawn(Models.Node node)
    {
        TextBlock note = Look.Label("(" + node.GetType().Name + " is not drawn in this build yet)", "CaptionTextBlockStyle", muted: true);
        AutomationProperties.SetAccessibilityView(note, Microsoft.UI.Xaml.Automation.Peers.AccessibilityView.Raw);
        return note;
    }

    private UIElement Overlay(Models.Node node, string? id)
    {
        if (id is not null)
        {
            context.Overlays.Add(new OverlayRequest(node, id));
        }

        return Empty();
    }

    private static UIElement Divider() =>
        new Microsoft.UI.Xaml.Controls.Border { Height = 1, Background = Look.Resource("DividerStrokeColorDefaultBrush"), Margin = new Thickness(0, 4, 0, 4) };

    // ---- layout ------------------------------------------------------------------------------------------

    private static HorizontalAlignment Across(Models.Align? align) =>
        align switch
        {
            Models.Align.Center => HorizontalAlignment.Center,
            Models.Align.End => HorizontalAlignment.Right,
            Models.Align.Stretch => HorizontalAlignment.Stretch,
            _ => HorizontalAlignment.Left,
        };

    private static VerticalAlignment Down(Models.Align? align) =>
        align switch
        {
            Models.Align.Center or Models.Align.Baseline => VerticalAlignment.Center,
            Models.Align.End => VerticalAlignment.Bottom,
            Models.Align.Stretch => VerticalAlignment.Stretch,
            _ => VerticalAlignment.Top,
        };

    private UIElement DrawStack(Models.Stack stack)
    {
        Models.Axis axis = stack.Wide is Models.Axis wide && context.Width != WidthClass.Compact ? wide : stack.Axis;
        if (axis == Models.Axis.Horizontal && WindowRules.FoldsDown(context.Width, stack.Children))
        {
            axis = Models.Axis.Vertical;
        }

        double gap = WindowRules.Space(stack.Gap);
        Thickness pad = new(WindowRules.Space(stack.Pad));
        if (axis == Models.Axis.Vertical)
        {
            StackPanel panel = new() { Spacing = gap, Padding = pad };
            foreach (Models.Node child in stack.Children)
            {
                UIElement element = Render(child);
                if (stack.Align is Models.Align align && element is FrameworkElement placed)
                {
                    // A chart has no width of its own: put in the middle it would be as wide as nothing, so it takes the width of the stack.
                    placed.HorizontalAlignment = child is Models.Chart ? HorizontalAlignment.Stretch : Across(align);
                }

                panel.Children.Add(element);
            }

            return panel;
        }

        // Across, where nothing grows to take what is left and the parts are not lanes: a row that goes on to a second line when its parts do not fit
        // side by side, so a title keeps its line and its words wrap at the spaces (a word is broken only when it alone is wider than the line).
        if (!stack.Children.Any(child => child is Models.Stack { Grow: true } or Models.Chart)
            && !WindowRules.IsLanes(stack.Children)
            && stack.Justify is not (Models.Justify.End or Models.Justify.Center))
        {
            Navree.Controls.Composition.FoldRow folding = new() { Spacing = gap, LineSpacing = Math.Min(gap, 8), Margin = pad, Between = stack.Justify == Models.Justify.Between };
            foreach (Models.Node child in stack.Children)
            {
                UIElement element = Render(child);
                if (element is FrameworkElement placed)
                {
                    placed.VerticalAlignment = stack.Align is null ? VerticalAlignment.Center : Down(stack.Align);
                }

                folding.Children.Add(element);
            }

            return folding;
        }

        // Across: each child in a column of a grid, the ones that grow taking what is left.
        Grid row = new() { ColumnSpacing = gap, Padding = pad };
        int at = 0;
        foreach (Models.Node child in stack.Children)
        {
            bool grows = child is Models.Stack { Grow: true } or Models.Chart;
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = grows ? new GridLength(1, GridUnitType.Star) : GridLength.Auto });
            UIElement element = Render(child);
            if (element is FrameworkElement placed)
            {
                placed.VerticalAlignment = stack.Align is null ? VerticalAlignment.Center : Down(stack.Align);
            }

            Grid.SetColumn((FrameworkElement)element, at++);
            row.Children.Add(element);
        }

        if (WindowRules.IsLanes(stack.Children))
        {
            return InLanes(row, gap);
        }

        if (stack.Justify is Models.Justify.End or Models.Justify.Center)
        {
            row.HorizontalAlignment = stack.Justify == Models.Justify.End ? HorizontalAlignment.Right : HorizontalAlignment.Center;
        }
        else if (stack.Justify == Models.Justify.Between && row.ColumnDefinitions.Count > 1)
        {
            // The space between the children is shared: a flexible column goes between each pair.
            Grid spread = new() { Padding = pad };
            int column = 0;
            for (int child = 0; child < row.Children.Count; child++)
            {
                if (child > 0)
                {
                    spread.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                    column++;
                }

                spread.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
                UIElement item = row.Children[child];
                Grid.SetColumn((FrameworkElement)item, column++);
            }

            UIElement[] items = [.. row.Children];
            row.Children.Clear();
            foreach (UIElement item in items)
            {
                spread.Children.Add(item);
            }

            return spread;
        }

        return row;
    }

    // Lanes share the room equally down to a least width each; below that the row scrolls across (the bar, the wheel with shift, touch).
    private static UIElement InLanes(Grid row, double gap)
    {
        int lanes = row.ColumnDefinitions.Count;
        foreach (ColumnDefinition column in row.ColumnDefinitions)
        {
            column.Width = new GridLength(1, GridUnitType.Star);
        }

        double least = (lanes * WindowRules.LeastLane) + ((lanes - 1) * gap);
        row.HorizontalAlignment = HorizontalAlignment.Left;
        ScrollViewer scroller = new()
        {
            Content = row,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
            HorizontalScrollMode = ScrollMode.Auto,
            VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollMode = ScrollMode.Disabled,
        };
        scroller.SizeChanged += (_, _) => row.Width = Math.Max(least, scroller.ViewportWidth);
        return scroller;
    }


    private UIElement DrawSection(Models.Section section)
    {
        StackPanel body = new() { Spacing = 12 };
        if (section.Title is not null || section.Subtitle is not null || section.Actions.Count > 0)
        {
            Grid header = new() { ColumnSpacing = 12 };
            header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            StackPanel titles = new();
            if (section.Title is not null)
            {
                TextBlock title = Look.Label(section.Title, "SubtitleTextBlockStyle");
                AutomationProperties.SetHeadingLevel(title, Microsoft.UI.Xaml.Automation.Peers.AutomationHeadingLevel.Level2);
                titles.Children.Add(title);
            }

            if (section.Subtitle is not null)
            {
                titles.Children.Add(Look.Label(section.Subtitle, "CaptionTextBlockStyle", muted: true));
            }

            header.Children.Add(titles);
            if (section.Actions.Count > 0)
            {
                StackPanel actions = new() { Orientation = Orientation.Horizontal, Spacing = 8, VerticalAlignment = VerticalAlignment.Center };
                foreach (Models.Node action in section.Actions)
                {
                    actions.Children.Add(Render(action));
                }

                Grid.SetColumn(actions, 1);
                header.Children.Add(actions);
            }

            body.Children.Add(header);
        }

        if (section.Hint is not null)
        {
            body.Children.Add(DrawHint(section.Hint));
        }

        foreach (Models.Node child in section.Children)
        {
            body.Children.Add(Render(child));
        }

        foreach (Models.Node footer in section.Footer)
        {
            body.Children.Add(Render(footer));
        }

        return section.Style == Models.SectionStyle.Plain ? body : Cardlike(body, 16);
    }

    private static UIElement Cardlike(UIElement content, double padding) =>
        new Microsoft.UI.Xaml.Controls.Border
        {
            Child = content,
            Padding = new Thickness(padding),
            CornerRadius = new CornerRadius(4),
            BorderThickness = new Thickness(1),
            Background = Look.Resource("CardBackgroundFillColorDefaultBrush"),
            BorderBrush = Look.Resource("CardStrokeColorDefaultBrush"),
        };


    // Two panes: side by side where the room they are given is wide enough for both, one under the other where it is not (the pane the server
    // says is in hand first). A split inside a pane of another split chooses by the room the pane leaves it.
    private UIElement DrawSplit(Models.Split split)
    {
        Navree.Controls.Composition.PaneRow row = new() { SecondFirstWhenStacked = HoldsCurrent(split.Detail) && !HoldsCurrent(split.Primary) };
        row.Children.Add(Render(split.Primary));
        row.Children.Add(Render(split.Detail));
        return row;
    }

    private static bool HoldsCurrent(Models.Node node) =>
        node switch
        {
            Models.Pane pane => pane.Current,
            Models.Split inner => HoldsCurrent(inner.Primary) || HoldsCurrent(inner.Detail),
            _ => false,
        };

    // A pane is its parts in a column, under its title where it has one.
    private UIElement DrawPane(Models.Pane pane)
    {
        StackPanel column = new() { Spacing = 8 };
        if (!string.IsNullOrEmpty(pane.Title))
        {
            TextBlock title = Look.Label(pane.Title, "BodyStrongTextBlockStyle");
            AutomationProperties.SetHeadingLevel(title, Microsoft.UI.Xaml.Automation.Peers.AutomationHeadingLevel.Level2);
            column.Children.Add(title);
        }

        foreach (Models.Node child in pane.Children)
        {
            column.Children.Add(Render(child));
        }

        return column;
    }

    private UIElement DrawDisclosure(Models.Disclosure disclosure)
    {
        Expander expander = new()
        {
            Header = disclosure.Label,
            IsExpanded = disclosure.Open,
            Content = Column(disclosure.Children),
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Stretch,
        };
        if (disclosure.Id is string id)
        {
            expander.Expanding += (_, _) => context.Send(Events.Overlay(context.Screen.Id, id, true));
            expander.Collapsed += (_, _) => context.Send(Events.Overlay(context.Screen.Id, id, false));
        }

        return expander;
    }

    private UIElement DrawTabs(Models.Tabs tabs)
    {
        string? tabsForm = currentForm;
        string? tabsOverlay = currentOverlay;
        StackPanel panel = new() { Spacing = 12 };
        SelectorBar bar = new();
        ContentControl host = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch };
        int selected = Math.Max(0, tabs.Items.ToList().FindIndex(tab => tab.Selected));
        for (int at = 0; at < tabs.Items.Count; at++)
        {
            bar.Items.Add(new SelectorBarItem { Text = tabs.Items[at].Label, Tag = at, IsSelected = at == selected });
        }

        void Show(int index)
        {
            host.Content = index < tabs.Items.Count ? Column(tabs.Items[index].Children) : null;
        }

        Show(selected);
        bar.SelectionChanged += (sender, _) =>
        {
            if (sender.SelectedItem?.Tag is int index && index < tabs.Items.Count)
            {
                Models.Tab tab = tabs.Items[index];
                if (tab.Href is not null)
                {
                    Act(new Models.ActionNavigate { Href = tab.Href }, null, tabs.Id, tabsForm, tabsOverlay);
                    return;
                }

                Show(index);
                if (tabs.Id is string id)
                {
                    context.Send(Events.Tab(context.Screen.Id, id, index));
                }
            }
        };
        panel.Children.Add(bar);
        panel.Children.Add(host);
        return panel;
    }

    // ---- reading ----------------------------------------------------------------------------------------

    private static string RampOf(Models.TextStyle style, int? heading) =>
        style switch
        {
            Models.TextStyle.Display => "DisplayTextBlockStyle",
            Models.TextStyle.LargeTitle => "TitleLargeTextBlockStyle",
            Models.TextStyle.Title => "TitleTextBlockStyle",
            Models.TextStyle.Title2 => "SubtitleTextBlockStyle",
            Models.TextStyle.Headline => "BodyStrongTextBlockStyle",
            Models.TextStyle.Caption => "CaptionTextBlockStyle",
            Models.TextStyle.Callout => "BodyTextBlockStyle",
            _ => heading switch
            {
                1 => "TitleTextBlockStyle",
                2 => "SubtitleTextBlockStyle",
                3 => "BodyStrongTextBlockStyle",
                _ => "BodyTextBlockStyle",
            },
        };

    private UIElement DrawText(Models.Text text)
    {
        TextBlock block = Look.Label(text.Value, RampOf(text.Style, text.Heading), text.Muted, text.Tone);
        if (text.Style == Models.TextStyle.Mono)
        {
            block.FontFamily = new FontFamily("Cascadia Mono, Consolas");
        }

        if (text.Weight is Models.Weight weight)
        {
            block.FontWeight = Look.WeightOf(weight);
        }

        if (text.Lines is int lines && lines > 0)
        {
            block.MaxLines = lines;
            block.TextTrimming = TextTrimming.CharacterEllipsis;
        }

        block.TextAlignment = text.Align switch
        {
            Models.Align.Center => TextAlignment.Center,
            Models.Align.End => TextAlignment.End,
            _ => TextAlignment.Start,
        };
        if (text.Heading is int level and >= 1 and <= 6)
        {
            AutomationProperties.SetHeadingLevel(block, (Microsoft.UI.Xaml.Automation.Peers.AutomationHeadingLevel)Math.Min(level, 9));
        }

        block.IsTextSelectionEnabled = true;
        return block;
    }

    private UIElement DrawRich(Models.Rich rich)
    {
        StackPanel panel = new() { Spacing = 8 };
        string ramp = RampOf(rich.Style, null);
        if (rich.Runs.Count > 0)
        {
            panel.Children.Add(Inlines(rich.Runs, ramp, rich.Muted));
        }

        int number = 0;
        foreach (Models.Block block in rich.Blocks)
        {
            number = block.Kind == Models.BlockKind.Numbered ? number + 1 : 0;
            string blockRamp = block.Kind == Models.BlockKind.Heading ? (block.Level <= 1 ? "TitleTextBlockStyle" : block.Level == 2 ? "SubtitleTextBlockStyle" : "BodyStrongTextBlockStyle") : ramp;
            TextBlock paragraph = Inlines(block.Runs, blockRamp, rich.Muted || block.Kind == Models.BlockKind.Quote);
            if (block.Kind is Models.BlockKind.Bullet or Models.BlockKind.Numbered)
            {
                Grid item = new() { ColumnSpacing = 8 };
                item.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
                item.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                item.Children.Add(new TextBlock { Text = block.Kind == Models.BlockKind.Bullet ? "•" : number.ToString(System.Globalization.CultureInfo.CurrentCulture) + "." });
                Grid.SetColumn(paragraph, 1);
                item.Children.Add(paragraph);
                panel.Children.Add(item);
            }
            else
            {
                if (block.Kind == Models.BlockKind.Code)
                {
                    paragraph.FontFamily = new FontFamily("Cascadia Mono, Consolas");
                }

                panel.Children.Add(paragraph);
            }
        }

        return panel;
    }

    private TextBlock Inlines(IReadOnlyList<Models.Run> runs, string ramp, bool muted)
    {
        string? richForm = currentForm;
        string? richOverlay = currentOverlay;
        TextBlock block = Look.Label(string.Empty, ramp, muted);
        foreach (Models.Run run in runs)
        {
            Inline inline;
            if (run.Href is string href)
            {
                Hyperlink link = new();
                link.Inlines.Add(new Run { Text = run.Text });
                link.Click += (_, _) => Act(new Models.ActionNavigate { Href = href }, null, null, richForm, richOverlay);
                inline = link;
            }
            else
            {
                inline = new Run { Text = run.Text };
            }

            if (run.Bold)
            {
                inline.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
            }

            if (run.Italic)
            {
                inline.FontStyle = Windows.UI.Text.FontStyle.Italic;
            }

            if (run.Code)
            {
                inline.FontFamily = new FontFamily("Cascadia Mono, Consolas");
            }

            block.Inlines.Add(inline);
        }

        return block;
    }

    private static UIElement DrawCode(Models.Code code)
    {
        TextBlock text = new() { Text = code.Text, FontFamily = new FontFamily("Cascadia Mono, Consolas"), IsTextSelectionEnabled = true, TextWrapping = TextWrapping.Wrap };
        return Cardlike(new ScrollViewer { Content = text, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto }, 12);
    }

    private static UIElement DrawBadge(Models.Badge badge)
    {
        if (badge.Dot)
        {
            return new Microsoft.UI.Xaml.Shapes.Ellipse { Width = 8, Height = 8, Fill = Look.ToneBrush(badge.Tone ?? Models.Tone.Accent), VerticalAlignment = VerticalAlignment.Center };
        }

        TextBlock text = Look.Label(badge.Text, "CaptionTextBlockStyle", tone: badge.Tone ?? Models.Tone.Neutral);
        text.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
        text.VerticalAlignment = VerticalAlignment.Center;
        return text;
    }


    private UIElement DrawBanner(Models.Banner banner)
    {
        InfoBar bar = new()
        {
            IsOpen = true,
            IsClosable = false,
            Severity = Look.Severity(banner.Tone ?? Models.Tone.Info),
            Title = banner.Title ?? string.Empty,
            Message = banner.Text ?? string.Empty,
        };
        if (banner.Actions.Count > 0 && Render(banner.Actions[0]) is Button action)
        {
            bar.ActionButton = action;
        }

        if (banner.Actions.Count > 1)
        {
            // The banner's other actions stand under its words, beside each other.
            StackPanel more = new() { Orientation = Orientation.Horizontal, Spacing = 8 };
            foreach (Models.Node other in banner.Actions.Skip(1))
            {
                more.Children.Add(Render(other));
            }

            bar.Content = more;
        }

        return bar;
    }

    private UIElement DrawEmpty(Models.Empty empty)
    {
        StackPanel panel = new() { Spacing = 8, HorizontalAlignment = HorizontalAlignment.Center, Padding = new Thickness(24) };
        FontIcon glyph = Look.Glyph("", 40);
        glyph.Foreground = Look.Muted;
        glyph.HorizontalAlignment = HorizontalAlignment.Center;
        panel.Children.Add(glyph);
        TextBlock title = Look.Label(empty.Title, "SubtitleTextBlockStyle");
        title.HorizontalAlignment = HorizontalAlignment.Center;
        title.TextAlignment = TextAlignment.Center;
        panel.Children.Add(title);
        if (empty.Text is not null)
        {
            TextBlock text = Look.Label(empty.Text, muted: true);
            text.TextAlignment = TextAlignment.Center;
            text.HorizontalAlignment = HorizontalAlignment.Center;
            panel.Children.Add(text);
        }

        foreach (Models.Node action in empty.Actions)
        {
            UIElement shown = Render(action);
            if (shown is FrameworkElement placed)
            {
                placed.HorizontalAlignment = HorizontalAlignment.Center;
            }

            panel.Children.Add(shown);
        }

        return panel;
    }

    private UIElement DrawFacts(Models.Facts facts)
    {
        Grid grid = new() { ColumnSpacing = 16, RowSpacing = 10 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(context.Width == WidthClass.Compact ? 110 : 170) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        int row = 0;
        foreach (Models.Fact fact in facts.Items)
        {
            grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            TextBlock label = Look.Label(fact.Label, "BodyTextBlockStyle", muted: true);
            Grid.SetRow(label, row);
            grid.Children.Add(label);
            StackPanel value = new() { Spacing = 2 };
            StackPanel line = new() { Orientation = Orientation.Horizontal, Spacing = 8 };
            foreach (Models.Node node in fact.Value)
            {
                line.Children.Add(Render(node));
            }

            value.Children.Add(line);
            foreach (string detail in fact.Details)
            {
                value.Children.Add(Look.Label(detail, "CaptionTextBlockStyle", muted: true));
            }

            Grid.SetRow(value, row);
            Grid.SetColumn(value, 1);
            grid.Children.Add(value);
            row++;
        }

        return grid;
    }



    private UIElement DrawHint(Models.Hint hint)
    {
        Grid line = new() { ColumnSpacing = 6 };
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        FontIcon mark = Look.Glyph(hint.Mark == Models.HintMark.Question ? "" : "", 14);
        mark.Foreground = Look.Muted;
        mark.VerticalAlignment = VerticalAlignment.Top;
        mark.Margin = new Thickness(0, 2, 0, 0);
        line.Children.Add(mark);
        TextBlock words = Look.Label(hint.Text, "CaptionTextBlockStyle", muted: true);
        Grid.SetColumn(words, 1);
        line.Children.Add(words);
        ToolTipService.SetToolTip(line, hint.About);
        AutomationProperties.SetHelpText(line, hint.About);
        return line;
    }

    // ---- icons, pictures, people -------------------------------------------------------------------------

    private static UIElement DrawIcon(Models.Icon icon, double size, Brush? brush)
    {
        Brush ink = brush ?? Look.ToneBrush(icon.Tone);
        if (icon.Paths.Count == 0 && icon.Fills.Count == 0)
        {
            FontIcon glyph = Look.Glyph(SystemGlyphs.For(icon.Name), size * 0.8);
            glyph.Foreground = ink;
            return glyph;
        }

        double view = 24;
        if (icon.ViewBox is string box)
        {
            string[] parts = box.Split([' ', ','], StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length == 4 && double.TryParse(parts[2], System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out double width) && width > 0)
            {
                view = width;
            }
        }

        Canvas canvas = new() { Width = view, Height = view };
        foreach (string data in icon.Paths.Take(32))
        {
            canvas.Children.Add(PathOf(data, stroke: true, ink));
        }

        foreach (string data in icon.Fills.Take(32))
        {
            canvas.Children.Add(PathOf(data, stroke: false, ink));
        }

        Viewbox drawn = new() { Width = size, Height = size, Child = canvas, VerticalAlignment = VerticalAlignment.Center };
        if (icon.Label is not null)
        {
            AutomationProperties.SetName(drawn, icon.Label);
        }
        else
        {
            AutomationProperties.SetAccessibilityView(drawn, Microsoft.UI.Xaml.Automation.Peers.AccessibilityView.Raw);
        }

        return drawn;
    }

    private static ShapePath PathOf(string data, bool stroke, Brush ink)
    {
        string markup = "<Path xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\" Data=\""
            + System.Security.SecurityElement.Escape(data.Length > 4096 ? string.Empty : data) + "\"/>";
        ShapePath path;
        try
        {
            path = (ShapePath)XamlReader.Load(markup);
        }
        catch (Exception)
        {
            // Text that is not path data is an icon with nothing in it, not an exception for the page.
            path = new ShapePath();
        }

        if (stroke)
        {
            path.Stroke = ink;
            path.StrokeThickness = 2;
            path.StrokeLineJoin = PenLineJoin.Round;
            path.StrokeStartLineCap = PenLineCap.Round;
            path.StrokeEndLineCap = PenLineCap.Round;
        }
        else
        {
            path.Fill = ink;
        }

        return path;
    }

    private UIElement DrawImage(Models.Image image)
    {
        Microsoft.UI.Xaml.Controls.Border frame = new()
        {
            Background = Look.Resource("ControlFillColorSecondaryBrush"),
            CornerRadius = new CornerRadius(4),
            MinHeight = image.Height is int height ? Math.Min(height, 400) : 96,
            Width = image.Width is int width ? Math.Min(width, 600) : double.NaN,
            HorizontalAlignment = HorizontalAlignment.Left,
        };
        StackPanel placeholder = new() { VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center, Spacing = 4, Margin = new Thickness(12) };
        FontIcon glyph = Look.Glyph("", 24);
        glyph.Foreground = Look.Muted;
        placeholder.Children.Add(glyph);
        if (image.Alt.Length > 0)
        {
            placeholder.Children.Add(Look.Label(image.Alt, "CaptionTextBlockStyle", muted: true));
        }

        frame.Child = placeholder;
        AutomationProperties.SetName(frame, image.Alt);
        if (context.Pictures is not null && image.Src.Length > 0)
        {
            _ = ShowPictureAsync(frame, image);
        }

        return frame;
    }

    private async Task ShowPictureAsync(Microsoft.UI.Xaml.Controls.Border frame, Models.Image image)
    {
        try
        {
            Microsoft.UI.Xaml.Media.Imaging.BitmapImage? bitmap = await PictureCache.GetAsync(image.Src, context.Pictures!);
            if (bitmap is null)
            {
                return;
            }

            frame.Child = new Microsoft.UI.Xaml.Controls.Image
            {
                Source = bitmap,
                Stretch = image.Fit == Models.Fit.Cover ? Stretch.UniformToFill : Stretch.Uniform,
                MaxHeight = image.Height is int height ? Math.Min(height, 400) : 400,
            };
        }
        catch (Exception)
        {
            // The placeholder stays: a picture that cannot be had is not a failure of the page.
        }
    }

    private static UIElement DrawAvatar(Models.Avatar avatar)
    {
        PersonPicture picture = new() { DisplayName = avatar.Name, Width = 40, Height = 40 };
        if (avatar.Initials is not null)
        {
            picture.Initials = avatar.Initials;
        }

        return picture;
    }

    // ---- state of a region ------------------------------------------------------------------------------

    private UIElement DrawRegion(Models.Region region)
    {
        StackPanel panel = new() { Spacing = 8 };
        RegionView? state = context.Screen.Regions.TryGetValue(region.Id, out RegionView? held) ? held : null;
        foreach (Models.Node child in region.Children)
        {
            panel.Children.Add(Render(child));
        }

        if (state?.Failure is not null)
        {
            InfoBar failed = new() { IsOpen = true, IsClosable = false, Severity = InfoBarSeverity.Error, Title = Words.Failure(state.Failure.ToString()!.ToLowerInvariant()).Title };
            Button again = new() { Content = "Try again" };
            again.Click += (_, _) => context.Send(Events.RegionRetry(context.Screen.Id, region.Id));
            failed.ActionButton = again;
            panel.Children.Add(failed);
        }
        else if (state?.Loading == true && region.Children.Count == 0)
        {
            panel.Children.Add(new ProgressRing { IsActive = true, Width = 28, Height = 28, HorizontalAlignment = HorizontalAlignment.Center });
        }

        return panel;
    }

    // ---- acting -----------------------------------------------------------------------------------------

    internal void Act(Models.Action? action, Models.Question? ask, string? from) =>
        Act(action, ask, from, currentForm, currentOverlay);

    /// <summary>Sends an action, with the form and the overlay it stood in when it was drawn (a button is pressed long after it was drawn).</summary>
    /// <param name="action">The action.</param>
    /// <param name="ask">The question to ask first, if any.</param>
    /// <param name="from">The part it came from.</param>
    /// <param name="form">The form the part stood in.</param>
    /// <param name="overlay">The overlay the part stood in.</param>
    internal void Act(Models.Action? action, Models.Question? ask, string? from, string? form, string? overlay)
    {
        if (action is null)
        {
            return;
        }

        context.Send(Events.Act(context.Screen.Id, action, form, overlay, null, null, ask, from));
    }

    /// <summary>Draws a node as it stands in a sheet or dialog: what it asks of the core carries the overlay's id.</summary>
    internal UIElement RenderInOverlay(Models.Node node, string overlayId)
    {
        string? before = currentOverlay;
        currentOverlay = overlayId;
        try
        {
            return Render(node);
        }
        finally
        {
            currentOverlay = before;
        }
    }

    private T InForm<T>(string? form, Func<T> draw)
    {
        string? before = currentForm;
        currentForm = form;
        try
        {
            return draw();
        }
        finally
        {
            currentForm = before;
        }
    }
}
