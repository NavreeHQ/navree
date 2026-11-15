using System.Globalization;
using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Controls;
using Navree.Design.Tables;
using Windows.System;

namespace Navree.Gallery;

/// <summary>Shows the table in its states, in light and dark, and drives it from the keyboard.</summary>
internal static class TableScenarios
{
    private static readonly string[] Standard =
        ["standard", "chosen", "sorted", "loading", "empty", "failed", "scrolled", "detail", "cursor", "menu", "rowmenu", "layout", "narrow", "rtl", "rtlscrolled", "more", "keys", "rowstates", "rowpressed"];

    /// <summary>The scenarios a run of "table" covers.</summary>
    internal static IReadOnlyList<string> All => Standard;

    /// <summary>Runs scenarios one after the other, each in light and in dark.</summary>
    /// <param name="window">The window.</param>
    /// <param name="names">The scenarios.</param>
    /// <returns>A task that ends when all are done.</returns>
    internal static async Task RunAsync(MainWindow window, IEnumerable<string> names, bool lightOnly = false)
    {
        foreach (string name in names)
        {
            foreach (ElementTheme theme in lightOnly ? new[] { ElementTheme.Light } : new[] { ElementTheme.Light, ElementTheme.Dark })
            {
                try
                {
                    await OneAsync(window, name, theme);
                }
                catch (Exception problem)
                {
                    Report.Line($"[table-{name}-{theme}] failed: {problem}");
                }
            }
        }
    }

    private static async Task OneAsync(MainWindow window, string name, ElementTheme theme)
    {
        string tag = $"{name}-{theme.ToString().ToLowerInvariant()}";
        Report.Line("== table " + tag);
        window.ThemeHost.RequestedTheme = theme;
        MemoryLayoutStore store = new();
        DataTable table = Build(name, store, out ListDataTableModel<Customer>? model);
        table.Margin = new Thickness(12);
        window.StageHost.Child = table;
        await Waiting.LoadedAsync(table);
        await Task.Delay(1200);
        await Prepare(name, table, model);
        await Task.Delay(500);
        if (name == "layout")
        {
            // A second table with the same key reads what the first one kept.
            Report.Line($"[{tag}] kept: {string.Join(" / ", store.Kept.Select(k => k.Key + " = " + k.Value))}");
            table = Build(name, store, out model);
            table.Margin = new Thickness(12);
            window.StageHost.Child = table;
            await Waiting.LoadedAsync(table);
            await Task.Delay(1200);
        }

        Describe(tag, table);
        if (name is "rowstates" or "rowpressed")
        {
            await RowStatesAsync(window, table, name);
        }

        if (name == "keys")
        {
            await KeysAsync(window, table, tag);
        }

        if (name == "more" && model is not null)
        {
            await MoreAsync(table, model, tag);
        }

        await Handshake.StepAsync("table-" + tag, "the " + name + " table in " + theme);
        if (name == "rowpressed")
        {
            await Mouse.UpAsync();
        }

        if (name is "standard" or "chosen" or "detail" && theme == ElementTheme.Light)
        {
            // The accessibility tree is read while the table is on the screen.
            await Handshake.StepAsync("uia-" + tag, "the accessibility tree of the " + name + " table");
        }
    }

    // Rows in the states a pointer gives them: two chosen, one of them pointed at; or one with the button held down on it.
    private static async Task RowStatesAsync(MainWindow window, DataTable table, string name)
    {
        Keys.Foreground(window);
        table.ApplyChoice(name == "rowstates" ? ["c00002", "c00003"] : [], false);
        await Task.Delay(500);
        int index = name == "rowstates" ? 3 : 5;
        if (table.RowsRepeater?.TryGetElement(index) is DataTableRow row && row.Cells.Count > 0)
        {
            DataTableCell cell = row.Cells[0];
            (int X, int Y) at = Mouse.ScreenOf(window, cell, new Windows.Foundation.Point(Math.Min(60, cell.ActualWidth / 2), cell.ActualHeight / 2));
            await Mouse.MoveAsync(at);
            if (name == "rowpressed")
            {
                await Mouse.DownAsync();
            }

            await Task.Delay(500);
        }
    }

    internal static DataTable Build(string name, MemoryLayoutStore store, out ListDataTableModel<Customer>? model)
    {
        model = DemoData.Model(sorted: name == "sorted" ? "amount" : null, descending: true);
        TextTablePresenter<Customer> presenter = new(model);
        if (name == "detail")
        {
            model.HasDetailOf = _ => true;
            presenter.Detail = c => new TextBlock { Text = $"{c.Name} wrote from {c.City}. Phone {c.Phone}, mail {c.Email}. Owes {DemoData.Rupees(c.Amount)}.", TextWrapping = TextWrapping.Wrap };
        }

        if (name == "rowmenu")
        {
            presenter.Menu = c =>
            {
                MenuFlyout menu = new();
                menu.Items.Add(new MenuFlyoutItem { Text = "Open " + c.Name });
                menu.Items.Add(new MenuFlyoutItem { Text = "Send reminder" });
                menu.Items.Add(new MenuFlyoutSeparator());
                menu.Items.Add(new MenuFlyoutItem { Text = "Delete" });
                return menu;
            };
        }

        DataTable table = new() { Model = model, Presenter = presenter, Culture = DemoData.Culture, Caption = "Customers" };
        if (name.StartsWith("rtl", StringComparison.Ordinal))
        {
            table.FlowDirection = FlowDirection.RightToLeft;
        }

        if (name == "narrow")
        {
            table.Width = 520;
            table.HorizontalAlignment = HorizontalAlignment.Left;
        }

        if (name == "layout")
        {
            table.LayoutKey = "gallery/customers";
            table.LayoutStore = store;
        }

        switch (name)
        {
            case "loading":
                model.State = DataTableLoadState.Loading;
                break;
            case "failed":
                model.State = DataTableLoadState.Failed;
                model.FailureMessage = "The customers could not be loaded. Check the connection and try again.";
                break;
            case "empty":
                break;
            case "cursor":
                model.Reset(DemoData.Make(0, 300));
                model.Summary = "1–300 of 1,204";
                model.Earlier = new DataTableCursor(true, CursorState.Loading);
                model.Later = new DataTableCursor(true, CursorState.Failed);
                break;
            default:
                model.Reset(DemoData.Make(0, 300));
                model.Summary = "1–300 of 1,204";
                model.Later = new DataTableCursor(true, CursorState.Idle);
                break;
        }

        return table;
    }

    internal static async Task Prepare(string name, DataTable table, ListDataTableModel<Customer>? model)
    {
        switch (name)
        {
            case "chosen":
                table.ApplyChoice(["c00002", "c00003", "c00006"], false);
                break;
            case "narrow":
                table.ApplyChoice(["c00001"], false);
                break;
            case "rtlscrolled":
                table.Scroller?.ScrollTo(44, 0, new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));
                await Task.Delay(800);
                break;
            case "scrolled":
                table.Scroller?.ScrollTo(420, 0, new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));
                await Task.Delay(800);
                if (table.HeaderRow?.ChromeLayer is { } headerChrome && table.RowsRepeater?.TryGetElement(0) is DataTableRow first && first.ChromeLayer is { } rowChrome)
                {
                    string Where(FrameworkElement e) => $"x {e.TransformToVisual(table).TransformPoint(new Windows.Foundation.Point(0, 0)).X:0.#}, width {e.ActualWidth:0.#}, translation {e.Translation.X:0.#}";
                    Report.Line($"[scrolled] the scroll view is at {table.Scroller?.HorizontalOffset:0.#}; header row {Where(table.HeaderRow)}; header chrome {Where(headerChrome)}; row {Where(first)}; row chrome {Where(rowChrome)}; header frozen layer {Where(table.HeaderRow.FrozenStart!)}");
                }

                break;
            case "detail":
                foreach (int index in new[] { 1, 4 })
                {
                    if (table.RowsRepeater?.TryGetElement(index) is DataTableRow row)
                    {
                        table.ToggleDetail(row);
                    }
                }

                await Task.Delay(900);
                break;
            case "menu":
                if (table.HeaderRow?.Headers.FirstOrDefault(h => h.GridColumn == 1) is { } heading)
                {
                    table.ShowColumnMenu(heading, null);
                    await Task.Delay(700);
                }

                break;
            case "rowmenu":
                if (table.RowsRepeater?.TryGetElement(2) is DataTableRow target && target.Cells.Count > 1)
                {
                    table.OpenRowMenu(target, target.Cells[1]);
                    await Task.Delay(700);
                }

                break;
            case "layout":
                await ChangeLayoutAsync(table);
                break;
        }

        _ = model;
    }

    // A person's changes to the columns, made by the same calls a pointer and a menu make.
    private static async Task ChangeLayoutAsync(DataTable table)
    {
        DataTableHeaderCell Heading(string key) => table.HeaderRow!.Headers.First(h => table.Model!.Columns[h.ColumnIndex].Key == key);
        DataTableHeaderCell name = Heading("name");
        table.ResizeStarted(name);
        table.ResizeMoved(name, 90);
        table.ResizeEnded(name, true);
        table.HideColumn(table.Model!.Columns.First(c => c.Key == "plan"));
        table.MoveColumn(table.Model!.Columns.First(c => c.Key == "city"), 1);
        table.FreezeUpTo(1);
        await Task.Delay(600);
        table.SaveLayoutNow();
    }

    // What the table made of its model: the numbers a person would check by eye.
    private static void Describe(string tag, DataTable table)
    {
        ColumnLayout? layout = table.CurrentLayout;
        ScrollView? scroll = table.Scroller;
        Report.Line(
            $"[{tag}] row {table.RowHeight:0.##}, header {table.HeaderHeight:0.##}; viewport {scroll?.ViewportWidth:0.#} by {scroll?.ViewportHeight:0.#}, "
            + $"extent {scroll?.ExtentWidth:0.#} by {scroll?.ExtentHeight:0.#}; rows made {table.RowsLayout.LastRealized.Count}; "
            + (layout is null ? "no layout" : $"row width {layout.RowWidth:0.#}, frozen {layout.Fs:0.#} and {layout.Fe:0.#}, gutter {layout.Gutter:0.#}"));
        Report.Line($"[{tag}] columns: " + string.Join(", ", table.Shown.Select(c => $"{c.Column.Key} {c.Placed.Width:0.#}@{c.Placed.X:0.#} {c.Placed.Group}")));
        if (table.RowsRepeater?.TryGetElement(0) is DataTableRow row)
        {
            Report.Line($"[{tag}] first row: " + string.Join(" | ", row.Cells.Select(c => (c.Content as TextBlock)?.Text ?? c.Content?.GetType().Name ?? "-")));
        }
    }

    // The table asks for rows near its ends, and rows that join above the view leave the view where it was.
    private static async Task MoreAsync(DataTable table, ListDataTableModel<Customer> model, string tag)
    {
        List<string> asked = [];
        int earlierMade = 0;
        table.MoreRequested += (_, e) =>
        {
            asked.Add(e.Direction.ToString());
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

        ScrollView? scroll = table.Scroller;
        if (scroll is null)
        {
            return;
        }

        model.Later = new DataTableCursor(true, CursorState.Idle);
        Report.Line($"[{tag}] held {model.RowCount} rows");
        for (int step = 0; step < 3; step++)
        {
            scroll.ScrollTo(0, Math.Max(0, scroll.ExtentHeight - scroll.ViewportHeight), new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));
            await Task.Delay(1500);
            Report.Line($"[{tag}] after going to the end: held {model.RowCount} rows, asked {string.Join(",", asked)}");
        }

        // Rows that join before the first held row, with the view part way down.
        model.Earlier = new DataTableCursor(true, CursorState.Idle);
        scroll.ScrollTo(0, 3000, new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));
        await Task.Delay(1000);
        string key = model.GetRow((int)(scroll.VerticalOffset / table.RowHeight)).Key ?? string.Empty;
        double before = scroll.VerticalOffset;
        int rowsBefore = model.RowCount;
        model.Prepend(DemoData.Make(2000, 50));
        await Task.Delay(1200);
        int at = model.IndexOfKey(key);
        double top = (at * table.RowHeight) - scroll.VerticalOffset;
        Report.Line($"[{tag}] 50 rows joined the start: the row that was at the top of the view, {key}, is now row {at} and stands {top:0.#} from the top of the view; offset {before:0.#} to {scroll.VerticalOffset:0.#}; held {rowsBefore} to {model.RowCount}; asked {string.Join(",", asked)}");
    }

    private static async Task KeysAsync(MainWindow window, DataTable table, string tag)
    {
        List<string> events = [];
        table.ChoiceRequested += (_, e) => events.Add($"choice keys={string.Join(',', e.Keys)} on={e.On} all={e.All} across={e.Across}");
        table.RowInvoked += (_, e) => events.Add("invoked " + e.Key);
        table.SortRequested += (_, e) => events.Add("sort " + e.Column.Key);
        table.EditRequested += (_, e) => events.Add("edit " + e.Key);
        Keys.Foreground(window);
        table.RestoreFocus();
        await Task.Delay(500);
        string Where() => $"active {table.ActiveCell?.RowKey}/{table.ActiveCell?.ColumnKey}, chosen {table.ChosenCount}, said '{table.LastAnnouncement}', sideways {table.Scroller?.HorizontalOffset:0.#}, focus on {FocusName(table)}";
        Report.Line($"[{tag}] start: {Where()}");

        async Task Step(string said, VirtualKey key, params VirtualKey[] with)
        {
            int before = events.Count;
            await Keys.PressAsync(key, with);
            await Task.Delay(350);
            string extra = events.Count > before ? "; events: " + string.Join(" / ", events.Skip(before)) : string.Empty;
            Report.Line($"[{tag}] {said}: {Where()}{extra}");
        }

        await Step("Down", VirtualKey.Down);
        await Step("Down", VirtualKey.Down);
        await Step("Down", VirtualKey.Down);
        await Step("Right", VirtualKey.Right);
        await Step("Right", VirtualKey.Right);
        await Step("Space", VirtualKey.Space);
        await Step("Shift+Down", VirtualKey.Down, VirtualKey.Shift);
        await Step("Shift+Down", VirtualKey.Down, VirtualKey.Shift);
        await Step("Ctrl+A", VirtualKey.A, VirtualKey.Control);
        await Step("Escape", VirtualKey.Escape);
        await Step("End", VirtualKey.End);
        await Step("Left", VirtualKey.Left);
        await Step("Home", VirtualKey.Home);
        await Step("PageDown", VirtualKey.PageDown);
        await Step("Ctrl+End", VirtualKey.End, VirtualKey.Control);
        await Step("Ctrl+Home", VirtualKey.Home, VirtualKey.Control);
        await Step("Enter", VirtualKey.Enter);
        await Step("Up", VirtualKey.Up);
        await Step("Enter on the heading", VirtualKey.Enter);
        await Step("Down", VirtualKey.Down);
        await Step("Space", VirtualKey.Space);
        await Step("Space", VirtualKey.Space);
        await Step("F2 to edit", VirtualKey.F2);
        await Step("G typed to find a name", VirtualKey.G);
        await Step("A typed to go on", VirtualKey.A);
    }

    private static string FocusName(DataTable table)
    {
        object? focused = Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(table.XamlRoot);
        return focused switch
        {
            DataTableCell cell => $"cell (row {cell.Row?.Index}, column {cell.GridColumn})",
            DataTableHeaderCell header => $"heading (column {header.GridColumn})",
            null => "nothing",
            _ => focused.GetType().Name,
        };
    }
}

/// <summary>Keeps a table's layout in memory, as a host would on disk.</summary>
internal sealed class MemoryLayoutStore : IDataTableLayoutStore
{
    private readonly Dictionary<string, string> kept = [];

    /// <summary>What is kept.</summary>
    internal IReadOnlyDictionary<string, string> Kept => kept;

    /// <inheritdoc />
    public bool TryLoad(string key, out string json) => kept.TryGetValue(key, out json!);

    /// <inheritdoc />
    public void Save(string key, string json) => kept[key] = json;
}

/// <summary>Sends real key events to the window, as a person at the keyboard would.</summary>
internal static class Keys
{
    private const uint KeyUp = 2;
    private const uint Extended = 1;

    [DllImport("user32.dll")]
    private static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr window);

    /// <summary>Brings the window to the front so that it gets the keys.</summary>
    /// <param name="window">The window.</param>
    internal static void Foreground(MainWindow window) => SetForegroundWindow(WinRT.Interop.WindowNative.GetWindowHandle(window));

    /// <summary>Presses a key with modifiers held, and lets go.</summary>
    /// <param name="key">The key.</param>
    /// <param name="modifiers">The keys held while it is pressed.</param>
    /// <returns>A task that ends when the key is up.</returns>
    internal static async Task PressAsync(VirtualKey key, params VirtualKey[] modifiers)
    {
        foreach (VirtualKey modifier in modifiers)
        {
            keybd_event((byte)modifier, 0, 0, UIntPtr.Zero);
        }

        await Task.Delay(30);
        uint flags = IsExtended(key) ? Extended : 0;
        keybd_event((byte)key, 0, flags, UIntPtr.Zero);
        await Task.Delay(30);
        keybd_event((byte)key, 0, flags | KeyUp, UIntPtr.Zero);
        await Task.Delay(30);
        foreach (VirtualKey modifier in Enumerable.Reverse(modifiers))
        {
            keybd_event((byte)modifier, 0, KeyUp, UIntPtr.Zero);
        }
    }

    private static bool IsExtended(VirtualKey key) =>
        key is VirtualKey.Left or VirtualKey.Right or VirtualKey.Up or VirtualKey.Down or VirtualKey.Home or VirtualKey.End
            or VirtualKey.PageUp or VirtualKey.PageDown or VirtualKey.Insert or VirtualKey.Delete or VirtualKey.Application;
}
