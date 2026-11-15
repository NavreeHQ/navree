using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Navree.Controls;
using Navree.Design.Tables;

namespace Navree.Gallery;

/// <summary>
/// The gallery as an app for a person: a list of the data table's states on the left, the one chosen on the right, and what the person does to
/// the table (choosing, sorting, opening a row, asking for more rows) said under it. The look (light, dark, right to left) is chosen beside the list.
/// Started with no arguments, the gallery is this; started with <c>--run</c> it runs the scenarios a run names, as before.
/// </summary>
internal sealed class GalleryBrowser
{
    private static readonly (string Name, string Label)[] States =
    [
        ("standard", "The table"),
        ("chosen", "Rows chosen"),
        ("sorted", "Sorted"),
        ("loading", "Loading"),
        ("empty", "No rows"),
        ("failed", "Could not be loaded"),
        ("scrolled", "Scrolled across, first column held"),
        ("detail", "Rows opened to their detail"),
        ("cursor", "Earlier and later rows by cursor"),
        ("menu", "A column's menu"),
        ("rowmenu", "A row's menu"),
        ("layout", "Columns resized, hidden and moved"),
        ("narrow", "Narrow: the table is a list"),
        ("rtl", "Right to left"),
        ("rtlscrolled", "Right to left, scrolled across"),
        ("more", "More rows as the end is reached"),
        ("keys", "The keyboard"),
    ];

    private readonly MainWindow window;
    private readonly Border content = new();
    private readonly TextBlock said = new() { TextWrapping = TextWrapping.Wrap, Margin = new Thickness(16, 8, 16, 12), Opacity = 0.8 };
    private readonly ListView list = new() { SelectionMode = ListViewSelectionMode.Single };
    private readonly CheckBox rightToLeft = new() { Content = "Right to left" };
    private readonly ComboBox theme = new() { Header = "Look", HorizontalAlignment = HorizontalAlignment.Stretch };
    private readonly List<string> events = [];
    private long shown;

    private GalleryBrowser(MainWindow window) => this.window = window;

    /// <summary>Fills the window with the browser.</summary>
    /// <param name="window">The gallery's window.</param>
    internal static void Show(MainWindow window)
    {
        GalleryBrowser browser = new(window);
        window.StageHost.Child = browser.Build();
        browser.list.SelectedIndex = 0;
    }

    private UIElement Build()
    {
        Grid layout = new();
        layout.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(300) });
        layout.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        TextBlock title = new() { Text = "Data table", Style = (Style)Application.Current.Resources["SubtitleTextBlockStyle"], Margin = new Thickness(16, 16, 16, 4) };
        AutomationProperties.SetHeadingLevel(title, Microsoft.UI.Xaml.Automation.Peers.AutomationHeadingLevel.Level1);
        TextBlock hint = new() { Text = "Choose a state. The table on the right is the real one: use the pointer or the keys.", TextWrapping = TextWrapping.Wrap, Margin = new Thickness(16, 0, 16, 8), Opacity = 0.8 };

        foreach ((string name, string label) in States)
        {
            list.Items.Add(new ListViewItem { Content = label, Tag = name });
        }

        AutomationProperties.SetName(list, "States of the data table");
        list.SelectionChanged += (_, _) => _ = ShowAsync();

        theme.Items.Add("As the system");
        theme.Items.Add("Light");
        theme.Items.Add("Dark");
        theme.SelectedIndex = 0;
        theme.SelectionChanged += (_, _) =>
        {
            window.ThemeHost.RequestedTheme = theme.SelectedIndex switch
            {
                1 => ElementTheme.Light,
                2 => ElementTheme.Dark,
                _ => ElementTheme.Default,
            };
        };
        rightToLeft.Click += (_, _) => _ = ShowAsync();

        StackPanel look = new() { Spacing = 8, Margin = new Thickness(16) };
        look.Children.Add(theme);
        look.Children.Add(rightToLeft);

        Grid left = new();
        left.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        left.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        left.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        left.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        left.Children.Add(title);
        Grid.SetRow(hint, 1);
        left.Children.Add(hint);
        Grid.SetRow(list, 2);
        left.Children.Add(list);
        Grid.SetRow(look, 3);
        left.Children.Add(look);

        Grid right = new();
        right.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        right.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        right.Children.Add(content);
        Grid.SetRow(said, 1);
        right.Children.Add(said);
        AutomationProperties.SetLiveSetting(said, Microsoft.UI.Xaml.Automation.Peers.AutomationLiveSetting.Polite);
        Grid.SetColumn(right, 1);

        layout.Children.Add(left);
        layout.Children.Add(right);
        return layout;
    }

    // Builds the state the person chose, makes it so, and answers what the person does to it as the host of a table does.
    private async Task ShowAsync()
    {
        if (list.SelectedItem is not ListViewItem { Tag: string name })
        {
            return;
        }

        long mine = ++shown;
        events.Clear();
        said.Text = "Try the table: choose rows, press a heading, open a row, move with the arrow keys.";
        MemoryLayoutStore store = new();
        DataTable table = TableScenarios.Build(name, store, out ListDataTableModel<Customer>? model);
        table.Margin = new Thickness(12);
        if (rightToLeft.IsChecked == true)
        {
            table.FlowDirection = FlowDirection.RightToLeft;
        }

        if (model is not null)
        {
            Answer(table, model);
        }

        content.Child = table;
        await Waiting.LoadedAsync(table);
        await Task.Delay(700);
        if (mine != shown)
        {
            return;
        }

        await TableScenarios.Prepare(name, table, model);
        if (name == "keys")
        {
            table.RestoreFocus();
        }
    }

    // What the host of a table does with what the table asks: the chosen rows are held and given back, more rows are made when the end is reached.
    private void Answer(DataTable table, ListDataTableModel<Customer> model)
    {
        HashSet<string> chosen = new(StringComparer.Ordinal);
        bool across = false;
        table.ChoiceRequested += (_, e) =>
        {
            if (e.Across)
            {
                across = true;
            }
            else if (e.All)
            {
                across = false;
                chosen.Clear();
                if (e.On)
                {
                    for (int at = 0; at < model.RowCount; at++)
                    {
                        if (model.GetRow(at).Key is string key)
                        {
                            chosen.Add(key);
                        }
                    }
                }
            }
            else
            {
                foreach (string key in e.Keys)
                {
                    _ = e.On ? chosen.Add(key) : chosen.Remove(key);
                }
            }

            table.ApplyChoice(chosen, across);
            Say(across ? "every row that matches is chosen" : chosen.Count + " chosen");
        };
        table.RowInvoked += (_, e) => Say("the row " + e.Key + " was opened");
        table.EditRequested += (_, e) => Say("edit was asked for the row " + e.Key);
        table.SortRequested += (_, e) => Say("a sort by " + e.Column.Key + " was asked for (the gallery's rows stay as they are)");
        int earlierMade = 0;
        table.MoreRequested += (_, e) =>
        {
            Say(e.Direction == MoreDirection.After ? "later rows were asked for" : "earlier rows were asked for");
            if (e.Direction == MoreDirection.After)
            {
                model.Later = new DataTableCursor(true, CursorState.Loading);
                table.DispatcherQueue.TryEnqueue(async () =>
                {
                    await Task.Delay(300);
                    model.Append(DemoData.Make(model.RowCount, 100));
                    model.Later = new DataTableCursor(model.RowCount < 900, CursorState.Idle);
                });
            }
            else
            {
                model.Earlier = new DataTableCursor(true, CursorState.Loading);
                table.DispatcherQueue.TryEnqueue(async () =>
                {
                    await Task.Delay(300);
                    model.Prepend(DemoData.Make(1000 + (earlierMade * 50), 50));
                    earlierMade++;
                    model.Earlier = new DataTableCursor(earlierMade < 2, CursorState.Idle);
                });
            }
        };
    }

    private void Say(string line)
    {
        events.Add(line);
        said.Text = string.Join("  |  ", events.TakeLast(3));
    }
}
