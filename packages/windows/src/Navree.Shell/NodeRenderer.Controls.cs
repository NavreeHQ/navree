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
        control.Click += (_, _) => Act(action, ask, from);
        return control;
    }

    private UIElement DrawLink(Models.Link link)
    {
        object content = link.Children.Count > 0 ? Column(link.Children, 4) : link.Label;
        HyperlinkButton control = new() { Content = content, Padding = new Thickness(0) };
        AutomationProperties.SetName(control, link.Label);
        Models.Action action = link.Action;
        string? from = link.Id;
        control.Click += (_, _) => Act(action, null, from);
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
        foreach (Models.Node node in items)
        {
            switch (node)
            {
                case Models.Button entry:
                    MenuFlyoutItem item = new() { Text = entry.Label, IsEnabled = !entry.Disabled };
                    if (entry.Standard is Models.Standard standard)
                    {
                        item.Icon = Look.Glyph(SystemGlyphs.Of(standard));
                    }

                    Models.Action? action = entry.Action;
                    Models.Question? ask = entry.Ask;
                    string? from = entry.Id;
                    item.Click += (_, _) => Act(action, ask, from);
                    flyout.Items.Add(item);
                    break;
                case Models.Divider:
                    flyout.Items.Add(new MenuFlyoutSeparator());
                    break;
                case Models.Menu nested:
                    MenuFlyoutSubItem sub = new() { Text = nested.Label };
                    FillSub(sub, nested.Items);
                    flyout.Items.Add(sub);
                    break;
                default:
                    break;
            }
        }

        return flyout;
    }

    private void FillSub(MenuFlyoutSubItem sub, IEnumerable<Models.Node> items)
    {
        foreach (Models.Node node in items)
        {
            switch (node)
            {
                case Models.Button entry:
                    MenuFlyoutItem item = new() { Text = entry.Label, IsEnabled = !entry.Disabled };
                    Models.Action? action = entry.Action;
                    Models.Question? ask = entry.Ask;
                    string? from = entry.Id;
                    item.Click += (_, _) => Act(action, ask, from);
                    sub.Items.Add(item);
                    break;
                case Models.Divider:
                    sub.Items.Add(new MenuFlyoutSeparator());
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
        ListView view = new()
        {
            SelectionMode = ListViewSelectionMode.None,
            IsItemClickEnabled = list.Children.OfType<Models.Row>().Any(row => row.Action is not null),
        };
        ScrollViewer.SetVerticalScrollMode(view, ScrollMode.Disabled);
        ScrollViewer.SetVerticalScrollBarVisibility(view, ScrollBarVisibility.Disabled);
        foreach (Models.Node child in list.Children)
        {
            view.Items.Add(Render(child));
        }

        view.ItemClick += (_, e) =>
        {
            if (e.ClickedItem is FrameworkElement { Tag: RowAction tap })
            {
                Act(tap.Action, null, tap.From);
            }
        };
        panel.Children.Add(view);

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

        if (row.Menu.Count > 0)
        {
            grid.ContextFlyout = MenuFor(row.Menu);
        }

        AutomationProperties.SetName(grid, row.Title + (row.Subtitle is null ? string.Empty : ", " + row.Subtitle));
        return grid;
    }

    // A table is drawn as a list for now: each row folded by its columns' roles, as the other clients fold it
    // when there is no room for columns. The table of our own (columns, frozen cells, choosing) is not drawn yet.
    private UIElement DrawTable(Models.Table table)
    {
        StackPanel panel = new() { Spacing = 4 };
        if (table.Caption is not null)
        {
            panel.Children.Add(Look.Label(table.Caption, "BodyStrongTextBlockStyle"));
        }

        if (table.Query is not null)
        {
            panel.Children.Add(Render(table.Query));
        }

        if (table.Rows.Count == 0 && table.Empty is not null)
        {
            panel.Children.Add(Render(table.Empty));
        }

        int lead = ColumnWith(table, Models.ColumnRole.Title, 0);
        int sub = ColumnWith(table, Models.ColumnRole.Subtitle, -1);
        List<int> end = [];
        for (int at = 0; at < table.Columns.Count; at++)
        {
            if (at != lead && at != sub && (table.Columns[at].Role is Models.ColumnRole.Status or Models.ColumnRole.Trailing || table.Columns[at].Numeric))
            {
                end.Add(at);
            }
        }

        ListView view = new() { SelectionMode = ListViewSelectionMode.None, IsItemClickEnabled = table.Rows.Any(row => row.Action is not null) };
        ScrollViewer.SetVerticalScrollMode(view, ScrollMode.Disabled);
        ScrollViewer.SetVerticalScrollBarVisibility(view, ScrollBarVisibility.Disabled);
        foreach (Models.TableRow row in table.Rows)
        {
            Grid grid = new() { ColumnSpacing = 12, RowSpacing = 6, Padding = new Thickness(0, 6, 0, 6), MinHeight = 40 };
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

            // What the person does to a row (a form, a button, a menu) goes under what the row says, across its whole width.
            StackPanel doing = new() { Orientation = Orientation.Horizontal, Spacing = 8 };
            StackPanel text = new() { VerticalAlignment = VerticalAlignment.Center };
            if (lead < row.Cells.Count)
            {
                text.Children.Add(Render(row.Cells[lead]));
            }

            StackPanel secondary = new() { Orientation = context.Width == WidthClass.Compact ? Orientation.Vertical : Orientation.Horizontal, Spacing = context.Width == WidthClass.Compact ? 2 : 8 };
            for (int at = 0; at < row.Cells.Count && at < table.Columns.Count; at++)
            {
                if (at == lead || end.Contains(at))
                {
                    continue;
                }

                if (row.Cells[at] is Models.Form or Models.Button or Models.Menu or Models.Field)
                {
                    doing.Children.Add(Render(row.Cells[at]));
                }
                else if (at == sub || table.Columns[at].Secondary || table.Columns[at].Role is null)
                {
                    secondary.Children.Add(Render(row.Cells[at]));
                }
            }

            foreach (Models.Node action in row.Actions)
            {
                doing.Children.Add(Render(action));
            }

            if (secondary.Children.Count > 0)
            {
                text.Children.Add(secondary);
            }

            grid.Children.Add(text);
            StackPanel trailing = new() { Orientation = Orientation.Horizontal, Spacing = 12, VerticalAlignment = VerticalAlignment.Center };
            foreach (int column in end.Where(index => index < row.Cells.Count))
            {
                trailing.Children.Add(Render(row.Cells[column]));
            }

            Grid.SetColumn(trailing, 1);
            grid.Children.Add(trailing);
            if (doing.Children.Count > 0)
            {
                Grid.SetRow(doing, 1);
                Grid.SetColumnSpan(doing, 2);
                grid.Children.Add(doing);
            }

            if (row.Action is not null)
            {
                grid.Tag = new RowAction(row.Action, row.Id);
            }

            view.Items.Add(grid);
        }

        view.ItemClick += (_, e) =>
        {
            if (e.ClickedItem is FrameworkElement { Tag: RowAction tap })
            {
                Act(tap.Action, null, tap.From);
            }
        };
        panel.Children.Add(view);
        if (table.Summary is not null)
        {
            panel.Children.Add(Look.Label(table.Summary, "CaptionTextBlockStyle", muted: true));
        }

        return panel;
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
                    password.PasswordChanged += (_, _) => send(new TextValue(password.Password));
                    control = password;
                    show = view => password.IsEnabled = !field.Disabled;
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

                    box.TextChanged += (_, _) =>
                    {
                        FieldView? latest = context.Screen.FieldOf(form, id);
                        if (latest?.Value is not TextValue { } same || same.Text != box.Text)
                        {
                            send(new TextValue(box.Text));
                        }
                    };
                    control = box;
                    show = view =>
                    {
                        if (view?.Value is TextValue echoed && echoed.Text != box.Text && box.FocusState == FocusState.Unfocused)
                        {
                            box.Text = echoed.Text;
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
                }
                else
                {
                    ToggleSwitch sw = new() { Header = field.Label, IsOn = on, IsEnabled = !field.Disabled };
                    sw.Toggled += (_, _) => send(new FlagValue(sw.IsOn));
                    control = sw;
                }

                break;
            case Models.ChoiceInput choice:
                IReadOnlyList<Models.ChoiceOption> options = held?.Options ?? choice.Options;
                string chosen = held?.Value is TextValue held2 ? held2.Text : choice.Value ?? string.Empty;
                if (choice.Kind is Models.ChoiceKind.Radio or Models.ChoiceKind.Segmented)
                {
                    RadioButtons radios = new() { Header = header, IsEnabled = !field.Disabled };
                    foreach (Models.ChoiceOption option in options)
                    {
                        radios.Items.Add(option.Label);
                    }

                    radios.SelectedIndex = IndexOfValue(options, chosen);
                    radios.SelectionChanged += (_, _) =>
                    {
                        if (radios.SelectedIndex >= 0 && radios.SelectedIndex < options.Count)
                        {
                            send(new TextValue(options[radios.SelectedIndex].Value));
                        }
                    };
                    control = radios;
                }
                else
                {
                    ComboBox combo = new() { Header = header, PlaceholderText = choice.Placeholder ?? string.Empty, IsEnabled = !field.Disabled, HorizontalAlignment = HorizontalAlignment.Stretch };
                    foreach (Models.ChoiceOption option in options)
                    {
                        combo.Items.Add(option.Label);
                    }

                    combo.SelectedIndex = IndexOfValue(options, chosen);
                    combo.SelectionChanged += (_, _) =>
                    {
                        if (combo.SelectedIndex >= 0 && combo.SelectedIndex < options.Count)
                        {
                            send(new TextValue(options[combo.SelectedIndex].Value));
                        }
                    };
                    control = combo;
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
                CalendarDatePicker picker = new() { Header = header, IsEnabled = !field.Disabled };
                if (DateTimeOffset.TryParse(day, System.Globalization.CultureInfo.InvariantCulture, System.Globalization.DateTimeStyles.AssumeLocal, out DateTimeOffset moment))
                {
                    picker.Date = moment;
                }

                picker.DateChanged += (_, e) =>
                {
                    if (e.NewDate is DateTimeOffset chosenDate)
                    {
                        send(new TextValue(chosenDate.ToString("yyyy-MM-dd", System.Globalization.CultureInfo.InvariantCulture)));
                    }
                };
                control = picker;
                break;
            case Models.SliderInput slider:
                string number = held?.Value is TextValue held5 ? held5.Text : slider.Value;
                double.TryParse(slider.Min, System.Globalization.CultureInfo.InvariantCulture, out double low);
                double.TryParse(slider.Max, System.Globalization.CultureInfo.InvariantCulture, out double high);
                double.TryParse(number, System.Globalization.CultureInfo.InvariantCulture, out double current);
                Slider bar = new() { Header = header, Minimum = low, Maximum = high > low ? high : low + 1, Value = current, IsEnabled = !field.Disabled };
                bar.ValueChanged += (_, e) => send(new TextValue(e.NewValue.ToString(System.Globalization.CultureInfo.InvariantCulture)));
                control = bar;
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
        context.Fields[(form, id)] = new FieldHandle { Show = view => { show(view); ShowErrors(view); } };
        return panel;
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
