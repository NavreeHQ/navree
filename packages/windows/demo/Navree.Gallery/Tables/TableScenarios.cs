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
    private static readonly string[] Standard = ["standard", "chosen", "sorted", "loading", "empty", "failed", "scrolled", "keys"];

    /// <summary>The scenarios a run of "table" covers.</summary>
    internal static IReadOnlyList<string> All => Standard;

    /// <summary>Runs scenarios one after the other, each in light and in dark.</summary>
    /// <param name="window">The window.</param>
    /// <param name="names">The scenarios.</param>
    /// <returns>A task that ends when all are done.</returns>
    internal static async Task RunAsync(MainWindow window, IEnumerable<string> names)
    {
        foreach (string name in names)
        {
            foreach (ElementTheme theme in new[] { ElementTheme.Light, ElementTheme.Dark })
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
        DataTable table = Build(name, out ListDataTableModel<Customer>? model);
        table.Margin = new Thickness(12);
        window.StageHost.Child = table;
        await Waiting.LoadedAsync(table);
        await Task.Delay(1200);
        await Prepare(name, table, model);
        await Task.Delay(500);
        Describe(tag, table);
        if (name == "keys")
        {
            await KeysAsync(window, table, tag);
        }

        await Handshake.StepAsync("table-" + tag, "the " + name + " table in " + theme);
    }

    private static DataTable Build(string name, out ListDataTableModel<Customer>? model)
    {
        model = DemoData.Model(sorted: name == "sorted" ? "amount" : null, descending: true);
        TextTablePresenter<Customer> presenter = new(model);
        DataTable table = new() { Model = model, Presenter = presenter, Culture = DemoData.Culture, Caption = "Customers" };
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
            default:
                model.Reset(DemoData.Make(0, 300));
                model.Summary = "1–300 of 1,204";
                model.Later = new DataTableCursor(true, CursorState.Idle);
                break;
        }

        return table;
    }

    private static async Task Prepare(string name, DataTable table, ListDataTableModel<Customer>? model)
    {
        switch (name)
        {
            case "chosen":
                table.ApplyChoice(["c00002", "c00003", "c00006"], false);
                break;
            case "scrolled":
                table.Scroller?.ScrollTo(420, 0, new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));
                await Task.Delay(800);
                break;
        }

        _ = model;
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

    private static async Task KeysAsync(MainWindow window, DataTable table, string tag)
    {
        List<string> events = [];
        table.ChoiceRequested += (_, e) => events.Add($"choice keys={string.Join(',', e.Keys)} on={e.On} all={e.All} across={e.Across}");
        table.RowInvoked += (_, e) => events.Add("invoked " + e.Key);
        table.SortRequested += (_, e) => events.Add("sort " + e.Column.Key);
        Keys.Foreground(window);
        table.RestoreFocus();
        await Task.Delay(500);
        string Where() => $"active {table.ActiveCell?.RowKey}/{table.ActiveCell?.ColumnKey}, chosen {table.ChosenCount}, focus on {FocusName(table)}";
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
            or VirtualKey.PageUp or VirtualKey.PageDown or VirtualKey.Insert or VirtualKey.Delete;
}
