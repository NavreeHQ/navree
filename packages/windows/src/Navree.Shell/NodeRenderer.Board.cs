using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;
using Windows.ApplicationModel.DataTransfer;
using Windows.System;

namespace Navree.Shell;

// A list the page lets the person reorder, whose rows carry more than a title (the cards of a board): each row is a card with what it holds,
// and a handle that moves it. A card is moved three ways, each telling the core the same thing (a row, the list it left, the list it went to, the
// row it now stands before): by dragging (Windows' drag and drop of a XAML element: the card is the source, the lane the target, a line shows where
// it would stand), by the keyboard (Alt+Shift and an arrow on the handle, as a list view's items are reordered), and by the handle's menu.
internal sealed partial class NodeRenderer
{
    private const string RowKey = "navree-row";
    private const string FromKey = "navree-from";

    // Whether a list is a set of cards: it can be reordered and its rows hold more than a title.
    private static bool HoldsCards(Models.List list) =>
        list.Id is not null && list.Reorder is not null && list.Children.OfType<Models.Row>().Any(row => row.Detail.Count > 0);

    private UIElement CardsOf(Models.List list)
    {
        string listId = list.Id!;
        StackPanel lane = new() { Spacing = 8, AllowDrop = true, MinHeight = 56, Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
        Border indicator = new() { Height = 3, CornerRadius = new CornerRadius(1.5), Background = Look.Resource("AccentFillColorDefaultBrush"), Tag = null };
        AutomationProperties.SetName(lane, "Cards");
        List<Models.Row> rows = [.. list.Children.OfType<Models.Row>()];
        foreach (Models.Row row in rows)
        {
            lane.Children.Add(CardOf(row, list, rows));
        }

        // Where in the lane a card dropped at this height would stand (the rule says: before the first card whose middle is below it).
        string? Before(double y)
        {
            List<FrameworkElement> cards = [.. lane.Children.OfType<FrameworkElement>().Where(child => child.Tag is string)];
            return CardMoves.DropBefore(
                [.. cards.Select(card => (string)card.Tag)],
                [.. cards.Select(card => card.TransformToVisual(lane).TransformPoint(new Windows.Foundation.Point(0, 0)).Y + (card.ActualHeight / 2))],
                y);
        }

        void ShowLine(string? before)
        {
            lane.Children.Remove(indicator);
            int at = lane.Children.Count;
            if (before is not null)
            {
                int found = lane.Children.ToList().FindIndex(child => child is FrameworkElement { Tag: string id } && id == before);
                at = found >= 0 ? found : at;
            }

            lane.Children.Insert(Math.Min(at, lane.Children.Count), indicator);
        }

        lane.DragOver += (_, e) =>
        {
            if (!e.DataView.Properties.ContainsKey(RowKey))
            {
                return;
            }

            e.AcceptedOperation = DataPackageOperation.Move;
            e.DragUIOverride.Caption = "Move here";
            e.DragUIOverride.IsGlyphVisible = false;
            ShowLine(Before(e.GetPosition(lane).Y));
        };
        lane.DragLeave += (_, _) => lane.Children.Remove(indicator);
        lane.Drop += (_, e) =>
        {
            lane.Children.Remove(indicator);
            if (e.DataView.Properties.TryGetValue(RowKey, out object? item) && e.DataView.Properties.TryGetValue(FromKey, out object? from)
                && item is string itemId && from is string fromId)
            {
                string? before = Before(e.GetPosition(lane).Y);
                if (before != itemId)
                {
                    CardFocus.Wanted = itemId;
                    context.Send(Events.Move(context.Screen.Id, itemId, fromId, listId, before));
                }
            }

            e.Handled = true;
        };
        return lane;
    }

    private UIElement CardOf(Models.Row row, Models.List list, IReadOnlyList<Models.Row> rows)
    {
        string rowId = row.Id ?? row.Key ?? row.Title;
        string listId = list.Id!;
        string? inForm = currentForm;
        string? inOverlay = currentOverlay;

        // The card's own parts: what the row holds, the title first, the form that moves it left out (the handle does that).
        List<Models.Node> parts = [];
        foreach (Models.Node node in row.Detail)
        {
            if (node is Models.Stack { Axis: Models.Axis.Vertical } inner)
            {
                parts.AddRange(inner.Children);
            }
            else
            {
                parts.Add(node);
            }
        }

        parts.RemoveAll(part => part is Models.Disclosure);
        Models.Form? peek = parts.OfType<Models.Form>().FirstOrDefault(form => Tree.Walk(form).OfType<Models.Button>().Any(button => button.Label == row.Title));
        StackPanel content = new() { Spacing = 6 };
        content.Children.Add(TitleOf(row, peek, inForm, inOverlay));
        foreach (Models.Node part in parts)
        {
            if (!ReferenceEquals(part, peek))
            {
                content.Children.Add(Render(part));
            }
        }

        Button handle = HandleOf(row, rowId, list, rows);
        Grid grid = new() { ColumnSpacing = 4 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.Children.Add(content);
        Grid.SetColumn(handle, 1);
        grid.Children.Add(handle);

        Border card = new()
        {
            Child = grid,
            Padding = new Thickness(12, 8, 4, 10),
            CornerRadius = new CornerRadius(8),
            BorderThickness = new Thickness(1),
            Background = Look.Resource("CardBackgroundFillColorDefaultBrush"),
            BorderBrush = Look.Resource("CardStrokeColorDefaultBrush"),
            CanDrag = true,
            Tag = rowId,
        };
        ToolTipService.SetToolTip(handle, "Move this card: drag it, press Alt, Shift and an arrow key, or open the menu");
        card.DragStarting += (_, e) =>
        {
            e.Data.RequestedOperation = DataPackageOperation.Move;
            e.Data.SetText(row.Title);
            e.Data.Properties[RowKey] = rowId;
            e.Data.Properties[FromKey] = listId;
        };
        return card;
    }

    // The title: the button that opens the card where the page gave one, else the words; it wraps.
    private UIElement TitleOf(Models.Row row, Models.Form? peek, string? inForm, string? inOverlay)
    {
        Models.Button? opener = peek is null ? null : Tree.Walk(peek).OfType<Models.Button>().FirstOrDefault(button => button.Label == row.Title);
        TextBlock words = new() { Text = row.Title, TextWrapping = TextWrapping.Wrap, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold };
        if (opener?.Action is null || peek is null)
        {
            return words;
        }

        Button title = new()
        {
            Content = words,
            Style = Look.Ramp("TextBlockButtonStyle"),
            Padding = new Thickness(0, 2, 0, 2),
            HorizontalAlignment = HorizontalAlignment.Left,
            HorizontalContentAlignment = HorizontalAlignment.Left,
        };
        words.Foreground = Look.Resource("TextFillColorPrimaryBrush");
        AutomationProperties.SetName(title, row.Title);
        string form = Tree.FormId(peek);
        Models.Action action = opener.Action;
        Models.Question? ask = opener.Ask;
        string? from = opener.Id;
        title.Click += (_, _) => Act(action, ask, from, form, inOverlay);
        _ = inForm;
        return title;
    }

    // The handle: a grip of dots, which is the card's place for the keyboard and for its menu.
    private Button HandleOf(Models.Row row, string rowId, Models.List list, IReadOnlyList<Models.Row> rows)
    {
        Microsoft.UI.Xaml.Controls.Canvas grip = new() { Width = 12, Height = 18 };
        for (int dot = 0; dot < 6; dot++)
        {
            Microsoft.UI.Xaml.Shapes.Ellipse mark = new() { Width = 3, Height = 3, Fill = Look.Muted };
            Microsoft.UI.Xaml.Controls.Canvas.SetLeft(mark, (dot % 2) * 7);
            Microsoft.UI.Xaml.Controls.Canvas.SetTop(mark, (dot / 2) * 7);
            grip.Children.Add(mark);
        }

        Button handle = new()
        {
            Content = grip,
            Padding = new Thickness(6, 4, 6, 4),
            Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
            BorderThickness = new Thickness(0),
            VerticalAlignment = VerticalAlignment.Top,
        };
        AutomationProperties.SetName(handle, "Move " + row.Title);
        AutomationProperties.SetHelpText(handle, "Alt, Shift and an arrow key moves the card; Enter opens a menu of where it can go.");
        int index = rows.ToList().FindIndex(other => (other.Id ?? other.Key ?? other.Title) == rowId);
        List<Models.List> lanes = Lanes(list);
        MenuFlyout menu = MoveMenu(row, rowId, list, rows, index, lanes);
        handle.Flyout = menu;
        handle.KeyDown += (_, e) =>
        {
            bool alt = InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Menu).HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);
            bool shift = InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Shift).HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);
            if (!alt || !shift)
            {
                return;
            }

            (string Lane, string? Before)? target = e.Key switch
            {
                VirtualKey.Up => Within(list, rows, index, up: true),
                VirtualKey.Down => Within(list, rows, index, up: false),
                VirtualKey.Left => Beside(lanes, list, index, -1),
                VirtualKey.Right => Beside(lanes, list, index, 1),
                _ => null,
            };
            if (target is { } there)
            {
                e.Handled = true;
                CardFocus.Wanted = rowId;
                context.Send(Events.Move(context.Screen.Id, rowId, list.Id!, there.Lane, there.Before));
            }
        };
        if (CardFocus.Wanted == rowId)
        {
            // The card was just moved: the screen is drawn again, and the handle takes the focus back so the keys go on moving it.
            handle.Loaded += (_, _) =>
            {
                if (CardFocus.Wanted == rowId)
                {
                    CardFocus.Wanted = null;
                    _ = handle.Focus(FocusState.Keyboard);
                }
            };
        }

        return handle;
    }

    private MenuFlyout MoveMenu(Models.Row row, string rowId, Models.List list, IReadOnlyList<Models.Row> rows, int index, IReadOnlyList<Models.List> lanes)
    {
        MenuFlyout menu = new();
        MenuFlyoutItem up = new() { Text = "Move up", IsEnabled = index > 0 };
        up.Click += (_, _) => Send(Within(list, rows, index, up: true));
        MenuFlyoutItem down = new() { Text = "Move down", IsEnabled = index >= 0 && index < rows.Count - 1 };
        down.Click += (_, _) => Send(Within(list, rows, index, up: false));
        menu.Items.Add(up);
        menu.Items.Add(down);

        // Where else it can go: the lanes the move form of the card offers, a lane that is full or that the page forbids shown but not offered.
        Models.Field? to = row.Detail.SelectMany(Tree.Walk).OfType<Models.Field>().FirstOrDefault(field => field.Name == "to_list");
        if (to?.Input is Models.ChoiceInput choice && choice.Options.Count > 0)
        {
            menu.Items.Add(new MenuFlyoutSeparator());
            foreach (Models.ChoiceOption option in choice.Options)
            {
                Models.List? lane = lanes.FirstOrDefault(candidate => candidate.Reorder?.Key == option.Value);
                bool here = lane is not null && lane.Id == list.Id;
                MenuFlyoutItem item = new() { Text = "Move to " + option.Label, IsEnabled = lane is not null && !here && !option.Disabled };
                if (lane?.Id is string laneId)
                {
                    item.Click += (_, _) => Send((laneId, null));
                }

                menu.Items.Add(item);
            }
        }

        return menu;

        void Send((string Lane, string? Before)? target)
        {
            if (target is { } there)
            {
                CardFocus.Wanted = rowId;
                context.Send(Events.Move(context.Screen.Id, rowId, list.Id!, there.Lane, there.Before));
            }
        }
    }

    // The lists of the card's group, in the order they stand on the screen.
    private List<Models.List> Lanes(Models.List list)
    {
        string? group = list.Reorder?.Group;
        if (group is null || context.Screen.Screen is not Models.Screen screen)
        {
            return [list];
        }

        return [.. Tree.Nodes(screen).OfType<Models.List>().Where(other => other.Id is not null && other.Reorder?.Group == group)];
    }

    private static string RowOf(Models.Row row) => row.Id ?? row.Key ?? row.Title;

    // One place up or down in the lane, by the rule of the steps; nowhere from an end.
    private static (string, string?)? Within(Models.List list, IReadOnlyList<Models.Row> rows, int index, bool up)
    {
        List<string> ids = [.. rows.Select(RowOf)];
        CardStep step = up ? CardMoves.Up(ids, index) : CardMoves.Down(ids, index);
        return step.Possible ? (list.Id!, step.Before) : null;
    }

    // The next lane across, at the same height where it has a card there, else at its end.
    private static (string, string?)? Beside(IReadOnlyList<Models.List> lanes, Models.List list, int index, int step)
    {
        int at = CardMoves.Neighbour(lanes.ToList().FindIndex(lane => lane.Id == list.Id), lanes.Count, step);
        if (at < 0)
        {
            return null;
        }

        List<string> there = [.. lanes[at].Children.OfType<Models.Row>().Select(RowOf)];
        return (lanes[at].Id!, CardMoves.Across(there, index).Before);
    }
}

/// <summary>The card that was just moved, so the screen drawn again for the move gives its handle the focus back.</summary>
internal static class CardFocus
{
    /// <summary>The id of the row, or none.</summary>
    internal static string? Wanted { get; set; }
}
