using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

internal sealed partial class NodeRenderer
{
    // ---- buttons, links, menus --------------------------------------------------------------------------

    private UIElement DrawButton(Models.Button button)
    {
        StackPanel content = new() { Orientation = Orientation.Horizontal, Spacing = 8 };
        if (button.Pending)
        {
            content.Children.Add(new ProgressRing { IsActive = true, Width = 16, Height = 16 });
        }
        else if (button.Icon is not null)
        {
            content.Children.Add(DrawIcon(button.Icon, 16, null));
        }
        else if (button.Standard is Models.Standard standard)
        {
            content.Children.Add(Look.Glyph(SystemGlyphs.Of(standard)));
        }

        if (!button.IconOnly)
        {
            content.Children.Add(new TextBlock { Text = button.Label, VerticalAlignment = VerticalAlignment.Center });
        }

        Button control = new() { Content = content };
        bool prominent = button.Prominence == Models.Prominence.High || button.Role is Models.Role.Primary or Models.Role.Create or Models.Role.Confirm;
        string? ramp = prominent ? "AccentButtonStyle" : button.Prominence == Models.Prominence.Low ? "TextBlockButtonStyle" : null;
        if (ramp is not null && Look.Ramp(ramp) is Style style)
        {
            control.Style = style;
        }

        if (button.Role == Models.Role.Destructive)
        {
            control.Foreground = Look.ToneBrush(Models.Tone.Danger);
        }

        control.IsEnabled = !(button.Disabled || button.Pending || context.Screen.Has(button.Id, Models.State.Disabled));
        AutomationProperties.SetName(control, button.Label);
        if (button.Reason is not null)
        {
            ToolTipService.SetToolTip(control, button.Reason);
        }
        else if (button.IconOnly)
        {
            ToolTipService.SetToolTip(control, button.Label);
        }

        Models.Action? action = button.Action;
        Models.Question? ask = button.Ask;
        string? from = button.Id;
        string? inForm = currentForm;
        string? inOverlay = currentOverlay;
        string trigger = TriggerKey(button);
        if (current is not null)
        {
            current.Triggers.Add((trigger, control));
        }
        else
        {
            context.LooseTriggers[trigger] = control;
            context.Triggers[trigger] = control;
        }

        control.Click += (_, _) =>
        {
            Triggers.Last = trigger;
            Act(action, ask, from, inForm, inOverlay);
        };
        return control;
    }

    private static string TriggerKey(Models.Button button) => button.Id ?? "label:" + button.Label;

    private UIElement DrawLink(Models.Link link)
    {
        object content = link.Children.Count > 0 ? Column(link.Children, 4) : link.Label;
        HyperlinkButton control = new() { Content = content, Padding = new Thickness(0) };
        AutomationProperties.SetName(control, link.Label);
        Models.Action action = link.Action;
        string? from = link.Id;
        string? inForm = currentForm;
        string? inOverlay = currentOverlay;
        control.Click += (_, _) => Act(action, null, from, inForm, inOverlay);
        return control;
    }

    private UIElement DrawMenu(Models.Menu menu)
    {
        StackPanel content = new() { Orientation = Orientation.Horizontal, Spacing = 8 };
        if (menu.Icon is not null)
        {
            content.Children.Add(DrawIcon(menu.Icon, 16, null));
        }

        content.Children.Add(new TextBlock { Text = menu.Label, VerticalAlignment = VerticalAlignment.Center });
        DropDownButton button = new() { Content = content, Flyout = MenuFor(menu.Items) };
        AutomationProperties.SetName(button, menu.Label);
        return button;
    }

    /// <summary>The items of a menu, as the system's own menu draws them.</summary>
    private MenuFlyout MenuFor(IEnumerable<Models.Node> items)
    {
        MenuFlyout flyout = new();
        FillMenu(flyout.Items, items, currentForm, currentOverlay);
        return flyout;
    }

    // The items of a menu or of a sub-menu: a press is sent with the form and the overlay the menu stood in when it was drawn, however deep the item is.
    private void FillMenu(IList<MenuFlyoutItemBase> into, IEnumerable<Models.Node> items, string? inForm, string? inOverlay)
    {
        foreach (Models.Node node in items)
        {
            switch (node)
            {
                case Models.Button entry:
                    MenuFlyoutItem item = new()
                    {
                        Text = entry.Label,
                        IsEnabled = !(entry.Disabled || entry.Pending || context.Screen.Has(entry.Id, Models.State.Disabled)),
                    };
                    if (entry.Standard is Models.Standard standard)
                    {
                        item.Icon = Look.Glyph(SystemGlyphs.Of(standard));
                    }

                    Models.Action? action = entry.Action;
                    Models.Question? ask = entry.Ask;
                    string? from = entry.Id;
                    item.Click += (_, _) => Act(action, ask, from, inForm, inOverlay);
                    into.Add(item);
                    break;
                case Models.Divider:
                    into.Add(new MenuFlyoutSeparator());
                    break;
                case Models.Menu nested:
                    MenuFlyoutSubItem sub = new() { Text = nested.Label };
                    FillMenu(sub.Items, nested.Items, inForm, inOverlay);
                    into.Add(sub);
                    break;
                default:
                    break;
            }
        }
    }

    // ---- lists and rows ----------------------------------------------------------------------------------

    private UIElement DrawList(Models.List list)
    {
        StackPanel panel = new() { Spacing = 0 };
        if (list.Id is string beforeId && list.Before is not null)
        {
            // Older rows are above the first (a conversation's earlier messages): they are asked for by a press, not by reaching the top, or a
            // list that starts at its top would load all of its history one page after another.
            ListState? older = context.Screen.Lists.TryGetValue(beforeId, out ListState? olderState) ? olderState : null;
            if (older?.LoadingBefore == true)
            {
                panel.Children.Add(new ProgressRing { IsActive = true, Width = 24, Height = 24, Margin = new Thickness(12), HorizontalAlignment = HorizontalAlignment.Center });
            }
            else
            {
                Button earlier = new() { Content = older?.FailedBefore == true ? "Try loading earlier again" : "Show earlier", HorizontalAlignment = HorizontalAlignment.Center };
                earlier.Click += (_, _) => context.Send(Events.More(context.Screen.Id, beforeId, before: true));
                panel.Children.Add(earlier);
            }
        }

        if (HoldsCards(list))
        {
            // The cards of a board: each a card with what it holds, moved by dragging, by the keys or by its handle's menu.
            panel.Children.Add(CardsOf(list));
        }
        else
        {
            ListView view = new()
            {
                SelectionMode = ListViewSelectionMode.None,
                IsItemClickEnabled = list.Children.OfType<Models.Row>().Any(row => row.Action is not null),
            };
            ScrollViewer.SetVerticalScrollMode(view, ScrollMode.Disabled);
            ScrollViewer.SetVerticalScrollBarVisibility(view, ScrollBarVisibility.Disabled);
            int place = 0;
            foreach (Models.Node child in list.Children)
            {
                UIElement drawn = Render(child);
                if (list.Ordered && child is Models.Row && drawn is FrameworkElement rowElement)
                {
                    // A numbered list says its numbers; the row's tap and its name stay the row's.
                    place++;
                    Grid numbered = new() { ColumnSpacing = 8, Tag = rowElement.Tag };
                    numbered.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
                    numbered.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                    TextBlock number = Look.Label(place.ToString(System.Globalization.CultureInfo.CurrentCulture) + ".", "BodyTextBlockStyle", muted: true);
                    number.VerticalAlignment = VerticalAlignment.Center;
                    number.MinWidth = 24;
                    AutomationProperties.SetAccessibilityView(number, Microsoft.UI.Xaml.Automation.Peers.AccessibilityView.Raw);
                    Grid.SetColumn(rowElement, 1);
                    numbered.Children.Add(number);
                    numbered.Children.Add(rowElement);
                    drawn = numbered;
                }

                view.Items.Add(drawn);
            }

            string? tapForm = currentForm;
            string? tapOverlay = currentOverlay;
            view.ItemClick += (_, e) =>
            {
                if (e.ClickedItem is FrameworkElement { Tag: RowAction tap })
                {
                    Act(tap.Action, null, tap.From, tapForm, tapOverlay);
                }
            };
            panel.Children.Add(view);
        }

        if (list.Id is string id && list.More is not null)
        {
            ListState? loading = context.Screen.Lists.TryGetValue(id, out ListState? current) ? current : null;
            if (loading?.Failed == true)
            {
                Button again = new() { Content = "Try loading more again", HorizontalAlignment = HorizontalAlignment.Center };
                again.Click += (_, _) => context.Send(Events.More(context.Screen.Id, id));
                panel.Children.Add(again);
            }
            else
            {
                // The end of the list is near when this is: rows come by cursor, never all at once.
                FrameworkElement sentinel = loading?.Loading == true
                    ? new ProgressRing { IsActive = true, Width = 24, Height = 24, Margin = new Thickness(12) }
                    : new Microsoft.UI.Xaml.Controls.Border { Height = 1 };
                bool asked = false;
                sentinel.EffectiveViewportChanged += (_, e) =>
                {
                    if (!asked && loading?.Loading != true && e.BringIntoViewDistanceY < 240)
                    {
                        asked = true;
                        context.Send(Events.More(context.Screen.Id, id));
                    }
                };
                panel.Children.Add(sentinel);
            }
        }

        return panel;
    }

    /// <summary>What a tap on a row does: its action, and the part it came from.</summary>
    private sealed record RowAction(Models.Action Action, string? From);

    private UIElement DrawRow(Models.Row row)
    {
        Grid grid = new() { ColumnSpacing = 12, Padding = new Thickness(0, 6, 0, 6), MinHeight = 40 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        UIElement? leading = row.Leading is null ? null : Render(row.Leading);
        if (row.State is Models.RowState state)
        {
            string glyph = state switch
            {
                Models.RowState.Done => "",
                Models.RowState.Current => "",
                _ => "",
            };
            leading = Look.Glyph(glyph);
        }

        if (leading is FrameworkElement lead)
        {
            lead.VerticalAlignment = VerticalAlignment.Center;
            grid.Children.Add(lead);
        }

        StackPanel text = new() { VerticalAlignment = VerticalAlignment.Center };
        text.Children.Add(Look.Label(row.Title, "BodyTextBlockStyle", row.Muted));
        if (row.Subtitle is not null)
        {
            text.Children.Add(Look.Label(row.Subtitle, "CaptionTextBlockStyle", muted: true));
        }

        // What the row holds under its two lines (its labels, a person, counts) is drawn under them.
        foreach (Models.Node more in row.Detail.Where(node => node is not Models.Disclosure))
        {
            text.Children.Add(Render(more));
        }

        Grid.SetColumn(text, 1);
        grid.Children.Add(text);
        if (row.Trailing is not null)
        {
            UIElement trailing = Render(row.Trailing);
            if (trailing is FrameworkElement placed)
            {
                placed.VerticalAlignment = VerticalAlignment.Center;
            }

            Grid.SetColumn((FrameworkElement)trailing, 2);
            grid.Children.Add(trailing);
        }

        if (row.Badge is not null)
        {
            UIElement badge = DrawBadge(row.Badge);
            Grid.SetColumn((FrameworkElement)badge, 3);
            grid.Children.Add(badge);
        }

        if (row.Action is not null)
        {
            grid.Tag = new RowAction(row.Action, row.Id);
        }

        if (row.Menu.Count > 0 || row.Swipe.Count > 0)
        {
            // What a swipe offers is also in the row's menu, so a pointer or a keyboard person has it too.
            MenuFlyout menu = MenuFor(row.Menu);
            if (row.Swipe.Count > 0)
            {
                if (menu.Items.Count > 0)
                {
                    menu.Items.Add(new MenuFlyoutSeparator());
                }

                FillMenu(menu.Items, row.Swipe, currentForm, currentOverlay);
            }

            grid.ContextFlyout = menu;
        }

        AutomationProperties.SetName(grid, row.Title + (row.Subtitle is null ? string.Empty : ", " + row.Subtitle));
        if (row.Swipe.Count > 0)
        {
            // On touch, a swipe across the row offers the same.
            SwipeItems items = new() { Mode = SwipeMode.Reveal };
            string? swipeForm = currentForm;
            string? swipeOverlay = currentOverlay;
            foreach (Models.Button entry in row.Swipe)
            {
                SwipeItem item = new()
                {
                    Text = entry.Label,
                    Background = entry.Role == Models.Role.Destructive ? Look.ToneBrush(Models.Tone.Danger) : Look.Resource("AccentFillColorDefaultBrush"),
                };
                Models.Action? swipeAction = entry.Action;
                Models.Question? swipeAsk = entry.Ask;
                string? swipeFrom = entry.Id;
                item.Invoked += (_, _) => Act(swipeAction, swipeAsk, swipeFrom, swipeForm, swipeOverlay);
                items.Add(item);
            }

            return new SwipeControl { Content = grid, RightItems = items, Tag = grid.Tag };
        }

        return grid;
    }


    private static int ColumnWith(Models.Table table, Models.ColumnRole role, int fallback)
    {
        for (int at = 0; at < table.Columns.Count; at++)
        {
            if (table.Columns[at].Role == role)
            {
                return at;
            }
        }

        return fallback;
    }

    // ---- forms and fields -------------------------------------------------------------------------------

    private UIElement DrawForm(Models.Form form) =>
        InForm(Tree.FormId(form), () => Column(form.Children, 16));

    private UIElement DrawField(Models.Field field)
    {
        string form = currentForm ?? string.Empty;
        string id = field.Id ?? field.Name;
        FieldView? held = context.Screen.FieldOf(form, id);
        StackPanel panel = new() { Spacing = 4 };
        string header = (field.Label ?? string.Empty) + (field.Required && field.Label is not null ? " *" : string.Empty);
        TextBlock errors = Look.Label(string.Empty, "CaptionTextBlockStyle", tone: Models.Tone.Danger);
        errors.Visibility = Visibility.Collapsed;
        Action<FieldValue> send = value => context.Send(Events.Field(context.Screen.Id, currentFormOf(), id, value));
        Action<FieldView?> show = _ => { };
        UIElement control;
        string? currentFormOf() => string.IsNullOrEmpty(form) ? null : form;

        switch (field.Input)
        {
            case Models.TextInput text:
                string start = held?.Value is TextValue held1 ? held1.Text : text.Value;
                if (text.Kind == Models.TextKind.Password)
                {
                    PasswordBox password = new() { Header = header, PlaceholderText = text.Placeholder ?? string.Empty, Password = start, IsEnabled = !field.Disabled };
                    bool passwordApplying = false;
                    password.PasswordChanged += (_, _) =>
                    {
                        if (!passwordApplying)
                        {
                            send(new TextValue(password.Password));
                        }
                    };
                    control = password;
                    show = view =>
                    {
                        password.IsEnabled = !(field.Disabled || context.Screen.Has(id, Models.State.Disabled));
                        if (view?.Value is TextValue echoed && echoed.Text != password.Password && password.FocusState == FocusState.Unfocused)
                        {
                            passwordApplying = true;
                            try
                            {
                                password.Password = echoed.Text;
                            }
                            finally
                            {
                                passwordApplying = false;
                            }
                        }
                    };
                }
                else
                {
                    TextBox box = new()
                    {
                        Header = header,
                        PlaceholderText = text.Placeholder ?? string.Empty,
                        Text = start,
                        IsReadOnly = text.Readonly,
                        IsEnabled = !field.Disabled,
                        AcceptsReturn = text.Kind == Models.TextKind.Multiline,
                        TextWrapping = text.Kind == Models.TextKind.Multiline ? TextWrapping.Wrap : TextWrapping.NoWrap,
                        MinHeight = text.Kind == Models.TextKind.Multiline ? 96 : 0,
                    };
                    if (text.MaxLength is long most and > 0 and < int.MaxValue)
                    {
                        box.MaxLength = (int)most;
                    }

                    // The keyboard a touch or pen person is given, and the words a screen reader says, follow the kind of the field.
                    Microsoft.UI.Xaml.Input.InputScopeNameValue? scope = text.Kind switch
                    {
                        Models.TextKind.Email => Microsoft.UI.Xaml.Input.InputScopeNameValue.EmailSmtpAddress,
                        Models.TextKind.Number or Models.TextKind.Decimal => Microsoft.UI.Xaml.Input.InputScopeNameValue.Number,
                        Models.TextKind.Phone => Microsoft.UI.Xaml.Input.InputScopeNameValue.TelephoneNumber,
                        Models.TextKind.Url => Microsoft.UI.Xaml.Input.InputScopeNameValue.Url,
                        Models.TextKind.Search => Microsoft.UI.Xaml.Input.InputScopeNameValue.Search,
                        Models.TextKind.OneTimeCode => Microsoft.UI.Xaml.Input.InputScopeNameValue.NumericPin,
                        _ => null,
                    };
                    if (scope is { } named)
                    {
                        Microsoft.UI.Xaml.Input.InputScope inputScope = new();
                        inputScope.Names.Add(new Microsoft.UI.Xaml.Input.InputScopeName(named));
                        box.InputScope = inputScope;
                    }

                    // Enter in a one-line field sends its form, as it does in every form of the system.
                    string? enterForm = currentForm;
                    string? enterOverlay = currentOverlay;
                    if (text.Kind != Models.TextKind.Multiline)
                    {
                        box.KeyDown += (_, e) =>
                        {
                            if (e.Key == Windows.System.VirtualKey.Enter && enterForm is not null && SubmitButtonOf(enterForm) is { } submit)
                            {
                                e.Handled = true;
                                Act(submit.Action, submit.Ask, submit.Id, enterForm, enterOverlay);
                            }
                        };
                    }

                    // Every change the person makes is sent: the client holds no copy of the text to compare with (the page it was drawn from
                    // is not the core's state any more), and the core ignores a value it already holds. What the core itself sets in the box is not sent back.
                    bool applying = false;
                    box.TextChanged += (_, _) =>
                    {
                        if (!applying)
                        {
                            send(new TextValue(box.Text));
                        }
                    };
                    control = box;
                    if (text.Prefix is { Length: > 0 } || text.Suffix is { Length: > 0 })
                    {
                        // The rupee sign or the unit stands beside the box, inside the field's own row, and the label stays above the row.
                        Grid row = new() { ColumnSpacing = 6 };
                        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
                        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
                        box.Header = null;
                        Grid.SetColumn(box, 1);
                        if (text.Prefix is { Length: > 0 } prefix)
                        {
                            TextBlock before = Look.Label(prefix, "BodyTextBlockStyle", muted: true);
                            before.VerticalAlignment = VerticalAlignment.Center;
                            row.Children.Add(before);
                        }

                        row.Children.Add(box);
                        if (text.Suffix is { Length: > 0 } suffix)
                        {
                            TextBlock after = Look.Label(suffix, "BodyTextBlockStyle", muted: true);
                            after.VerticalAlignment = VerticalAlignment.Center;
                            Grid.SetColumn(after, 2);
                            row.Children.Add(after);
                        }

                        StackPanel labelled = new() { Spacing = 4 };
                        if (header.Length > 0)
                        {
                            labelled.Children.Add(Look.Label(header, "BodyTextBlockStyle"));
                        }

                        labelled.Children.Add(row);
                        AutomationProperties.SetName(box, field.Label ?? field.Name);
                        control = labelled;
                    }

                    show = view =>
                    {
                        box.IsEnabled = !(field.Disabled || context.Screen.Has(id, Models.State.Disabled));
                        if (view?.Value is TextValue echoed && echoed.Text != box.Text && box.FocusState == FocusState.Unfocused)
                        {
                            applying = true;
                            try
                            {
                                box.Text = echoed.Text;
                            }
                            finally
                            {
                                applying = false;
                            }
                        }
                    };
                }

                break;
            case Models.ToggleInput toggle:
                bool on = held?.Value is FlagValue flag ? flag.On : toggle.On;
                if (toggle.Kind == Models.ToggleKind.Checkbox)
                {
                    CheckBox check = new() { Content = field.Label ?? string.Empty, IsChecked = on, IsEnabled = !field.Disabled };
                    check.Click += (_, _) => send(new FlagValue(check.IsChecked == true));
                    control = check;
                    show = view =>
                    {
                        check.IsEnabled = !(field.Disabled || context.Screen.Has(id, Models.State.Disabled));
                        if (view?.Value is FlagValue now && check.IsChecked != now.On)
                        {
                            check.IsChecked = now.On;
                        }
                    };
                }
                else
                {
                    ToggleSwitch sw = new() { Header = field.Label, IsOn = on, IsEnabled = !field.Disabled };
                    bool switching = false;
                    sw.Toggled += (_, _) =>
                    {
                        if (!switching)
                        {
                            send(new FlagValue(sw.IsOn));
                        }
                    };
                    control = sw;
                    show = view =>
                    {
                        sw.IsEnabled = !(field.Disabled || context.Screen.Has(id, Models.State.Disabled));
                        if (view?.Value is FlagValue now && sw.IsOn != now.On)
                        {
                            switching = true;
                            try
                            {
                                sw.IsOn = now.On;
                            }
                            finally
                            {
                                switching = false;
                            }
                        }
                    };
                }

                break;
            case Models.ChoiceInput choice:
                IReadOnlyList<Models.ChoiceOption> options = held?.Options ?? choice.Options;
                string chosen = held?.Value is TextValue held2 ? held2.Text : choice.Value ?? string.Empty;
                if (choice.Search is { } search)
                {
                    // A choice that is searched on the server: the person types, the core is asked for the options that match (by the words,
                    // and by cursor for more), and the options that come are the suggestions; a way to make a new one stands last.
                    List<(string Label, string? Value, int Kind)> entries = [];
                    AutoSuggestBox finder = new()
                    {
                        Header = header,
                        PlaceholderText = choice.Placeholder ?? string.Empty,
                        IsEnabled = !field.Disabled,
                        Text = options.FirstOrDefault(option => option.Value == chosen)?.Label ?? string.Empty,
                    };
                    void Offer(IReadOnlyList<Models.ChoiceOption> found, bool more, string typed)
                    {
                        entries.Clear();
                        foreach (Models.ChoiceOption option in found)
                        {
                            entries.Add((option.Label, option.Value, option.Disabled ? 3 : 0));
                        }

                        if (more)
                        {
                            entries.Add(("Show more", null, 1));
                        }

                        if (choice.Create is { } make && typed.Length > 0)
                        {
                            entries.Add((make.Label + ": " + typed, null, 2));
                        }

                        finder.ItemsSource = entries.Select(entry => entry.Label).ToList();
                    }

                    Offer(options, held?.More ?? choice.More is not null, finder.Text);
                    string? searchOverlay = currentOverlay;
                    finder.TextChanged += (box, e) =>
                    {
                        if (e.Reason == AutoSuggestionBoxTextChangeReason.UserInput && box.Text.Length >= search.Min)
                        {
                            context.Send(Events.Options(context.Screen.Id, currentFormOf(), id, box.Text));
                        }
                    };
                    finder.QuerySubmitted += (box, e) =>
                    {
                        int at = e.ChosenSuggestion is string label ? entries.FindIndex(entry => entry.Label == label) : -1;
                        if (at < 0)
                        {
                            return;
                        }

                        (string Label, string? Value, int Kind) entry = entries[at];
                        switch (entry.Kind)
                        {
                            case 0 when entry.Value is not null:
                                box.Text = entry.Label;
                                send(new TextValue(entry.Value));
                                break;
                            case 1:
                                context.Send(Events.Options(context.Screen.Id, currentFormOf(), id, box.Text, more: true));
                                break;
                            case 2 when choice.Create?.Href is string href:
                                Act(new Models.ActionNavigate { Href = href }, null, id, currentFormOf(), searchOverlay);
                                break;
                            default:
                                break;
                        }
                    };
                    control = finder;
                    show = view =>
                    {
                        finder.IsEnabled = !(field.Disabled || context.Screen.Has(id, Models.State.Disabled));
                        if (view?.Options is { } latest)
                        {
                            Offer(latest, view.More, finder.Text);
                            if (finder.FocusState != FocusState.Unfocused && latest.Count > 0)
                            {
                                finder.IsSuggestionListOpen = true;
                            }
                        }
                    };
                }
                else if (choice.Kind is Models.ChoiceKind.Radio or Models.ChoiceKind.Segmented)
                {
                    RadioButtons radios = new() { Header = header, IsEnabled = !field.Disabled };
                    foreach (Models.ChoiceOption option in options)
                    {
                        radios.Items.Add(option.Label);
                    }

                    radios.SelectedIndex = IndexOfValue(options, chosen);
                    bool radiosApplying = false;
                    radios.SelectionChanged += (_, _) =>
                    {
                        if (!radiosApplying && radios.SelectedIndex >= 0 && radios.SelectedIndex < options.Count)
                        {
                            send(new TextValue(options[radios.SelectedIndex].Value));
                        }
                    };
                    control = radios;
                    show = view =>
                    {
                        radios.IsEnabled = !(field.Disabled || context.Screen.Has(id, Models.State.Disabled));
                        if (view?.Value is TextValue now && IndexOfValue(options, now.Text) is int at && at != radios.SelectedIndex)
                        {
                            radiosApplying = true;
                            try
                            {
                                radios.SelectedIndex = at;
                            }
                            finally
                            {
                                radiosApplying = false;
                            }
                        }
                    };
                }
                else
                {
                    ComboBox combo = new() { Header = header, PlaceholderText = choice.Placeholder ?? string.Empty, IsEnabled = !field.Disabled, HorizontalAlignment = HorizontalAlignment.Stretch };
                    foreach (Models.ChoiceOption option in options)
                    {
                        combo.Items.Add(option.Label);
                    }

                    // The options go on past those held (a list the server gives by cursor): the last entry asks for the next of them.
                    bool goesOn = held?.More ?? choice.More is not null;
                    if (goesOn)
                    {
                        combo.Items.Add("Show more");
                    }

                    combo.SelectedIndex = IndexOfValue(options, chosen);
                    bool comboApplying = false;
                    combo.SelectionChanged += (_, _) =>
                    {
                        if (comboApplying)
                        {
                            return;
                        }

                        if (combo.SelectedIndex >= 0 && combo.SelectedIndex < options.Count)
                        {
                            send(new TextValue(options[combo.SelectedIndex].Value));
                        }
                        else if (goesOn && combo.SelectedIndex == options.Count)
                        {
                            // Not a choice: the entry that asks for more. The choice that stood stays, and the options that come are put in the list.
                            comboApplying = true;
                            try
                            {
                                combo.SelectedIndex = IndexOfValue(options, chosen);
                            }
                            finally
                            {
                                comboApplying = false;
                            }

                            context.Send(Events.Options(context.Screen.Id, currentFormOf(), id, null, more: true));
                        }
                    };
                    control = combo;
                    show = view =>
                    {
                        combo.IsEnabled = !(field.Disabled || context.Screen.Has(id, Models.State.Disabled));
                        comboApplying = true;
                        try
                        {
                            IReadOnlyList<Models.ChoiceOption> latest = view?.Options ?? options;
                            bool more = view is not null ? view.More : goesOn;
                            int expected = latest.Count + (more ? 1 : 0);
                            if (expected != combo.Items.Count || latest.Select((option, at) => combo.Items[at] as string != option.Label).Any(different => different))
                            {
                                options = latest;
                                goesOn = more;
                                combo.Items.Clear();
                                foreach (Models.ChoiceOption option in latest)
                                {
                                    combo.Items.Add(option.Label);
                                }

                                if (more)
                                {
                                    combo.Items.Add("Show more");
                                }
                            }

                            if (view?.Value is TextValue now && IndexOfValue(latest, now.Text) is int at && at != combo.SelectedIndex)
                            {
                                combo.SelectedIndex = at;
                            }
                        }
                        finally
                        {
                            comboApplying = false;
                        }
                    };
                }

                break;
            case Models.ChoicesInput many:
                StackPanel boxes = new() { Spacing = 2 };
                if (header.Length > 0)
                {
                    boxes.Children.Add(Look.Label(header, "BodyTextBlockStyle"));
                }

                HashSet<string> picked = [.. held?.Value is ManyValue held3 ? held3.Values : many.Values];
                foreach (Models.ChoiceOption option in held?.Options ?? many.Options)
                {
                    CheckBox check = new() { Content = option.Label, IsChecked = picked.Contains(option.Value) };
                    string value = option.Value;
                    check.Click += (_, _) =>
                    {
                        if (check.IsChecked == true)
                        {
                            picked.Add(value);
                        }
                        else
                        {
                            picked.Remove(value);
                        }

                        send(new ManyValue([.. picked]));
                    };
                    boxes.Children.Add(check);
                }

                control = boxes;
                break;
            case Models.DateInput date:
                string day = held?.Value is TextValue held4 ? held4.Text : date.Value;
                bool hasDate = DateTimeOffset.TryParse(day, System.Globalization.CultureInfo.InvariantCulture, System.Globalization.DateTimeStyles.AssumeLocal, out DateTimeOffset moment);
                if (date.Kind == Models.DateKind.Time)
                {
                    // A time is picked with a time picker and sent as hours and minutes, not as a day.
                    TimePicker clock = new() { Header = header, IsEnabled = !field.Disabled, SelectedTime = hasDate || TimeSpan.TryParse(day, System.Globalization.CultureInfo.InvariantCulture, out _) ? (hasDate && day.Contains(':', StringComparison.Ordinal) ? moment.TimeOfDay : TimeSpan.Parse(day, System.Globalization.CultureInfo.InvariantCulture)) : null, MinuteIncrement = 5 };
                    clock.SelectedTimeChanged += (_, e) =>
                    {
                        if (e.NewTime is TimeSpan chosenTime)
                        {
                            send(new TextValue(chosenTime.ToString(@"hh\:mm", System.Globalization.CultureInfo.InvariantCulture)));
                        }
                    };
                    control = clock;
                    show = view => clock.IsEnabled = !(field.Disabled || context.Screen.Has(id, Models.State.Disabled));
                    break;
                }

                CalendarDatePicker picker = new() { Header = header, IsEnabled = !field.Disabled };
                if (hasDate)
                {
                    picker.Date = moment;
                }

                // The days the page allows: not before its least, not after its most, and not on the days it rules out.
                if (DateTimeOffset.TryParse(date.Min, System.Globalization.CultureInfo.InvariantCulture, System.Globalization.DateTimeStyles.AssumeLocal, out DateTimeOffset least))
                {
                    picker.MinDate = least;
                }

                if (DateTimeOffset.TryParse(date.Max, System.Globalization.CultureInfo.InvariantCulture, System.Globalization.DateTimeStyles.AssumeLocal, out DateTimeOffset most1))
                {
                    picker.MaxDate = most1;
                }

                HashSet<string> off = [.. date.OffDays];
                HashSet<DayOfWeek> offWeek = [.. date.OffWeekdays.Select(weekday => Enum.TryParse(weekday.ToString(), true, out DayOfWeek named) ? named : (DayOfWeek)(-1))];
                if (off.Count > 0 || offWeek.Count > 0)
                {
                    picker.CalendarViewDayItemChanging += (_, e) =>
                    {
                        DateTimeOffset item = e.Item.Date;
                        e.Item.IsBlackout = off.Contains(item.ToString("yyyy-MM-dd", System.Globalization.CultureInfo.InvariantCulture)) || offWeek.Contains(item.DayOfWeek);
                    };
                }

                picker.DateChanged += (_, e) =>
                {
                    if (e.NewDate is DateTimeOffset chosenDate)
                    {
                        send(new TextValue(chosenDate.ToString("yyyy-MM-dd", System.Globalization.CultureInfo.InvariantCulture)));
                    }
                };
                control = picker;
                show = view =>
                {
                    picker.IsEnabled = !(field.Disabled || context.Screen.Has(id, Models.State.Disabled));
                    if (view?.Value is TextValue now && DateTimeOffset.TryParse(now.Text, System.Globalization.CultureInfo.InvariantCulture, System.Globalization.DateTimeStyles.AssumeLocal, out DateTimeOffset shown)
                        && picker.Date?.Date != shown.Date)
                    {
                        picker.Date = shown;
                    }
                };
                break;
            case Models.SliderInput slider:
                string number = held?.Value is TextValue held5 ? held5.Text : slider.Value;
                double.TryParse(slider.Min, System.Globalization.CultureInfo.InvariantCulture, out double low);
                double.TryParse(slider.Max, System.Globalization.CultureInfo.InvariantCulture, out double high);
                double.TryParse(number, System.Globalization.CultureInfo.InvariantCulture, out double current);
                Slider bar = new() { Header = header, Minimum = low, Maximum = high > low ? high : low + 1, Value = current, IsEnabled = !field.Disabled };
                bool sliding = false;
                bar.ValueChanged += (_, e) =>
                {
                    if (!sliding)
                    {
                        send(new TextValue(e.NewValue.ToString(System.Globalization.CultureInfo.InvariantCulture)));
                    }
                };
                control = bar;
                show = view =>
                {
                    bar.IsEnabled = !(field.Disabled || context.Screen.Has(id, Models.State.Disabled));
                    if (view?.Value is TextValue now && double.TryParse(now.Text, System.Globalization.CultureInfo.InvariantCulture, out double set) && set != bar.Value)
                    {
                        sliding = true;
                        try
                        {
                            bar.Value = set;
                        }
                        finally
                        {
                            sliding = false;
                        }
                    }
                };
                break;
            default:
                // A kind of input this build does not draw: the name of the field and what it holds, said plainly.
                control = Look.Label((field.Label ?? field.Name) + " (this kind of field is not drawn in this build yet)", "CaptionTextBlockStyle", muted: true);
                break;
        }

        AutomationProperties.SetName(control, field.Label ?? field.Name);
        panel.Children.Add(control);
        if (field.Help is not null)
        {
            panel.Children.Add(Look.Label(field.Help, "CaptionTextBlockStyle", muted: true));
        }

        panel.Children.Add(errors);
        void ShowErrors(FieldView? view)
        {
            List<string> all = [];
            if (view is not null && view.Errors.Count > 0)
            {
                all.AddRange(view.Errors);
            }
            else if (field.Error is not null)
            {
                all.Add(field.Error);
            }

            errors.Text = string.Join(" ", all);
            errors.Visibility = all.Count == 0 ? Visibility.Collapsed : Visibility.Visible;
            AutomationProperties.SetLiveSetting(errors, Microsoft.UI.Xaml.Automation.Peers.AutomationLiveSetting.Assertive);
        }

        ShowErrors(held);
        FieldHandle handle = new() { Show = view => { show(view); ShowErrors(view); } };
        if (current is not null)
        {
            current.Fields.Add(((form, id), handle));
        }
        else
        {
            context.LooseFields[(form, id)] = handle;
            context.Fields[(form, id)] = handle;
        }

        return panel;
    }

    // The button that sends a form: the first with an action that sends, in the form's own parts.
    private Models.Button? SubmitButtonOf(string formId)
    {
        if (context.Screen.Screen is not Models.Screen screen)
        {
            return null;
        }

        Models.Form? form = Tree.Nodes(screen).OfType<Models.Form>().FirstOrDefault(candidate => Tree.FormId(candidate) == formId);
        return form is null
            ? null
            : Tree.Walk(form).OfType<Models.Button>().FirstOrDefault(button => button.Action is Models.ActionSubmit && !button.Disabled && !button.Pending);
    }

    private static int IndexOfValue(IReadOnlyList<Models.ChoiceOption> options, string value)
    {
        for (int at = 0; at < options.Count; at++)
        {
            if (options[at].Value == value)
            {
                return at;
            }
        }

        return -1;
    }
}
