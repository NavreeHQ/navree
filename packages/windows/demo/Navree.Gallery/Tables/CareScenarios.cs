using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Navree.Controls;
using Navree.Design.Tables;
using Windows.System;

namespace Navree.Gallery;

/// <summary>
/// Shows the table keeping the active row and keyboard focus while rows join, leave, are drawn again and are replaced, each case
/// with what the table did written beside a picture of the window.
/// </summary>
internal static class CareScenarios
{
    internal static async Task RunAsync(MainWindow window)
    {
        window.ThemeHost.RequestedTheme = ElementTheme.Light;
        Keys.Foreground(window);
        foreach ((string name, Func<MainWindow, Task> run) in new (string, Func<MainWindow, Task>)[]
        {
            ("focus-far", FocusFarAsync),
            ("focus-removed", FocusRemovedAsync),
            ("focus-replaced", FocusReplacedAsync),
            ("reset-by-key", ResetByKeyAsync),
            ("prepend-top", PrependTopAsync),
            ("new-model-names", NewModelNamesAsync),
            ("refresh-same-count", RefreshSameCountAsync),
            ("detail-open", DetailOpenAsync),
            ("density-kept", DensityKeptAsync),
            ("late-shape", LateShapeAsync),
            ("unloaded", UnloadedAsync),
            ("assistive", AssistiveAsync),
        })
        {
            try
            {
                Report.Line("== care " + name);
                await run(window);
            }
            catch (Exception problem)
            {
                Report.Line($"[care-{name}] failed: {problem}");
            }
        }
    }

    private static async Task<DataTable> ShowAsync(MainWindow window, ListDataTableModel<Customer> model, IDataTablePresenter? presenter = null, Action<DataTable>? before = null)
    {
        DataTable table = new() { Model = model, Presenter = presenter ?? new TextTablePresenter<Customer>(model), Culture = DemoData.Culture, Caption = "Customers", Margin = new Thickness(12) };
        before?.Invoke(table);
        window.StageHost.Child = table;
        await Waiting.LoadedAsync(table);
        await Task.Delay(1200);
        return table;
    }

    private static ListDataTableModel<Customer> Rows(int first, int count)
    {
        ListDataTableModel<Customer> model = DemoData.Model();
        model.Reset(DemoData.Make(first, count));
        model.Summary = $"1–{count} of {count}";
        return model;
    }

    private static string FocusName(DataTable table) =>
        Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(table.XamlRoot) switch
        {
            DataTableCell cell => $"cell of row {cell.Row?.Index} ({cell.Row?.Key}), column {cell.GridColumn}",
            DataTableHeaderCell header => $"heading, column {header.GridColumn}",
            TextBox box => $"text box '{box.Text}'",
            null => "nothing",
            { } other => other.GetType().Name,
        };

    private static string Made(DataTable table, int index) => table.RowsRepeater?.TryGetElement(index) is DataTableRow ? "made" : "not made";

    private static void Scroll(DataTable table, double offset) =>
        table.Scroller?.ScrollTo(0, offset, new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));

    // The active row far from the view, and rows that join or leave before it, and a row that assistive technology asks for.
    private static async Task FocusFarAsync(MainWindow window)
    {
        ListDataTableModel<Customer> model = Rows(0, 300);
        DataTable table = await ShowAsync(window, model);
        table.FocusRow("c00150");
        await Task.Delay(800);
        Report.Line($"[care-focus-far] focus put on row c00150: {FocusName(table)}");
        Scroll(table, 0);
        await Task.Delay(900);
        Report.Line($"[care-focus-far] the view is at {table.Scroller?.VerticalOffset:0.#}; row 150 is {Made(table, 150)}; focus is on {FocusName(table)}");

        model.Prepend(DemoData.Make(1000, 500));
        await Task.Delay(1500);
        int now = model.IndexOfKey("c00150");
        Report.Line($"[care-focus-far] 500 rows joined above the view: the active row is now {now} and is {Made(table, now)}; the view is at {table.Scroller?.VerticalOffset:0.#}; focus is on {FocusName(table)}");

        table.RealizeRow(5);
        await Task.Delay(100);
        Report.Line($"[care-focus-far] a row was asked for by assistive technology: row 5 is {Made(table, 5)} and the active row is {Made(table, now)}");
        await Task.Delay(900);
        Report.Line($"[care-focus-far] 900 ms later row 5 is {Made(table, 5)} and the active row is {Made(table, now)}; focus is on {FocusName(table)}");

        // Rows leave before the active row but not above the view: the active row is the row it was, and is still made.
        model.Remove("c01003");
        model.Remove("c01004");
        model.Remove("c01005");
        await Task.Delay(1000);
        int after = model.IndexOfKey("c00150");
        Report.Line($"[care-focus-far] three rows before it left: the active row is now {after} ({table.ActiveCell?.RowKey}) and is {Made(table, after)}; focus is on {FocusName(table)}");
        table.ScrollToRow("c00150");
        await Task.Delay(900);
        await Handshake.StepAsync("care-focus-far", "the table scrolled back to the active row after rows joined and left above it");
    }

    // The active row removed with focus on it: focus goes to the row now at its place, the one before when it was the last, then the heading.
    private static async Task FocusRemovedAsync(MainWindow window)
    {
        ListDataTableModel<Customer> model = Rows(0, 6);
        DataTable table = await ShowAsync(window, model);
        table.FocusRow("c00002");
        await Task.Delay(600);
        model.Remove("c00002");
        await Task.Delay(1000);
        Report.Line($"[care-focus-removed] the active row c00002 was removed: active {table.ActiveCell?.RowKey}, focus on {FocusName(table)}, said '{table.LastAnnouncement}'");
        table.FocusRow("c00005");
        await Task.Delay(600);
        model.Remove("c00005");
        await Task.Delay(1000);
        Report.Line($"[care-focus-removed] the last row c00005 was removed: active {table.ActiveCell?.RowKey}, focus on {FocusName(table)}");
        await Handshake.StepAsync("care-focus-removed", "the table after the active row was removed and focus went to the row now at its place");
        foreach (string key in new[] { "c00000", "c00001", "c00003", "c00004" })
        {
            model.Remove(key);
            await Task.Delay(300);
        }

        await Task.Delay(800);
        Report.Line($"[care-focus-removed] no row is left: active {table.ActiveCell?.RowKey ?? "none"}, focus on {FocusName(table)}");
    }

    // A row drawn again with the same key while a person is typing in a field of it: the text and the focus stay.
    private static async Task FocusReplacedAsync(MainWindow window)
    {
        ListDataTableModel<Customer> model = FieldPresenter.Model();
        model.Reset(DemoData.Make(0, 8));
        DataTable table = await ShowAsync(window, model, new FieldPresenter(model));
        Keys.Foreground(window);
        DataTableRow? row = table.RowsRepeater?.TryGetElement(2) as DataTableRow;
        TextBox? box = row?.Cells.Select(c => c.Content).OfType<TextBox>().FirstOrDefault();
        if (box is null)
        {
            Report.Line("[care-focus-replaced] the row 2 has no text box");
            return;
        }

        box.Focus(FocusState.Keyboard);
        await Task.Delay(500);
        box.Text = string.Empty;
        await Keys.PressAsync(VirtualKey.Number7);
        await Keys.PressAsync(VirtualKey.Number5);
        await Task.Delay(300);
        Report.Line($"[care-focus-replaced] typed in the field of row 2: '{box.Text}', focus on {FocusName(table)}");
        Customer old = model.ItemAt(2);
        model.Replace(old with { Name = "Navjot Replaced", City = "Shimla" });
        await Task.Delay(900);
        TextBox? afterBox = (table.RowsRepeater?.TryGetElement(2) as DataTableRow)?.Cells.Select(c => c.Content).OfType<TextBox>().FirstOrDefault();
        string name = (table.RowsRepeater?.TryGetElement(2) as DataTableRow)?.Cells.Select(c => c.Content).OfType<TextBlock>().FirstOrDefault()?.Text ?? "?";
        Report.Line($"[care-focus-replaced] the row was drawn again with the same key: its name now says '{name}', the field still says '{afterBox?.Text}', same field: {ReferenceEquals(box, afterBox)}, focus on {FocusName(table)}");
        await Handshake.StepAsync("care-focus-replaced", "a row drawn again while a person typed in its field");
    }

    // A new set of rows: the active row is the same row by key when the new rows have it, else the first row.
    private static async Task ResetByKeyAsync(MainWindow window)
    {
        ListDataTableModel<Customer> model = Rows(0, 300);
        DataTable table = await ShowAsync(window, model);
        table.FocusRow("c00042");
        await Task.Delay(800);
        model.Reset(DemoData.Make(0, 300).OrderByDescending(c => c.Amount));
        await Task.Delay(1200);
        Report.Line($"[care-reset-by-key] the rows were sorted again: row c00042 is now {model.IndexOfKey("c00042")}; active {table.ActiveCell?.RowKey} at {model.IndexOfKey(table.ActiveCell?.RowKey ?? string.Empty)}; focus on {FocusName(table)}; view at {table.Scroller?.VerticalOffset:0.#}");
        await Handshake.StepAsync("care-reset-by-key", "the table after a new sort, the active row followed by its key");
        model.Reset(DemoData.Make(100, 300));
        await Task.Delay(1200);
        Report.Line($"[care-reset-by-key] rows without c00042: active {table.ActiveCell?.RowKey} (the first row is {model.GetRow(0).Key}); focus on {FocusName(table)}");
    }

    // Rows that join the start of a table whose view is at the very top: the view moves down to the row that was first.
    private static async Task PrependTopAsync(MainWindow window)
    {
        ListDataTableModel<Customer> model = Rows(0, 300);
        DataTable table = await ShowAsync(window, model);
        model.Earlier = new DataTableCursor(true, CursorState.Idle);
        ScrollView? scroll = table.Scroller;
        if (scroll is null)
        {
            return;
        }

        string first = model.GetRow(0).Key ?? string.Empty;
        Report.Line($"[care-prepend-top] the view is at {scroll.VerticalOffset:0.#}; first row {first}");
        model.Prepend(DemoData.Make(1000, 50));
        await Task.Delay(1500);
        int at = model.IndexOfKey(first);
        double top = (at * table.RowHeight) - scroll.VerticalOffset;
        Report.Line($"[care-prepend-top] 50 rows joined the start: the row that was first, {first}, is now row {at} and stands {top:0.#} from the top of the view; offset {scroll.VerticalOffset:0.#}");
        await Handshake.StepAsync("care-prepend-top", "the table after rows joined the start of a view that was at the very top");
    }

    // The end of the rows is near and the host was asked once; a refresh with the same count of rows asks again.
    private static async Task RefreshSameCountAsync(MainWindow window)
    {
        ListDataTableModel<Customer> model = Rows(0, 50);
        model.Later = new DataTableCursor(true, CursorState.Idle);
        List<string> asked = [];
        DataTable table = await ShowAsync(window, model, before: t => t.MoreRequested += (_, e) => asked.Add(e.Direction.ToString()));
        await Task.Delay(1200);
        Report.Line($"[care-refresh-same-count] 50 rows held, the end is near: asked {asked.Count} time(s): {string.Join(",", asked)}");
        model.Reset(DemoData.Make(0, 50));
        await Task.Delay(1500);
        Report.Line($"[care-refresh-same-count] the rows were replaced by 50 others: asked {asked.Count} time(s) in all: {string.Join(",", asked)}; held {model.RowCount}; the view is at {table.Scroller?.VerticalOffset:0.#}");
        await Handshake.StepAsync("care-refresh-same-count", "the table after a refresh with the same count of rows");
    }

    // A row the server sends with its detail open is drawn open, and stays as the person leaves it.
    private static async Task DetailOpenAsync(MainWindow window)
    {
        ListDataTableModel<Customer> model = DemoData.Model();
        model.HasDetailOf = _ => true;
        model.DetailOpenOf = c => c.Id == "c00002";
        model.Reset(DemoData.Make(0, 12));
        TextTablePresenter<Customer> presenter = new(model)
        {
            Detail = c => new TextBlock { Text = $"{c.Name} wrote from {c.City}. Phone {c.Phone}, mail {c.Email}.", TextWrapping = TextWrapping.Wrap },
        };
        DataTable table = await ShowAsync(window, model, presenter);
        await Task.Delay(800);
        DataTableRow? row = table.RowsRepeater?.TryGetElement(2) as DataTableRow;
        Report.Line($"[care-detail-open] row c00002 arrived with its detail open: detail made {row?.DetailContent is not null}, open {table.IsDetailOpen("c00002")}; row c00001 open {table.IsDetailOpen("c00001")}");
        if (row is not null)
        {
            table.ToggleDetail(row);
            await Task.Delay(300);
            model.Replace(model.ItemAt(2));
            await Task.Delay(900);
            Report.Line($"[care-detail-open] the person shut it and the row was drawn again with the detail still said open: open {table.IsDetailOpen("c00002")}");
        }

        await Handshake.StepAsync("care-detail-open", "a row that arrived with its detail open");
    }

    // The density a person chose and kept is the density of the rows from the first draw.
    private static async Task DensityKeptAsync(MainWindow window)
    {
        MemoryLayoutStore store = new();
        Dictionary<string, StoredColumn> none = [];
        store.Save("gallery/density", LayoutText.Serialize(new StoredLayout(LayoutText.CurrentVersion, "comfortable", 1, ["name", "city", "phone", "email", "plan", "amount", "status", "seen", "actions"], none)));
        ListDataTableModel<Customer> plainModel = Rows(0, 40);
        DataTable plain = await ShowAsync(window, plainModel);
        double standard = plain.RowHeight;
        ListDataTableModel<Customer> model = Rows(0, 40);
        DataTable table = await ShowAsync(window, model, before: t =>
        {
            t.LayoutKey = "gallery/density";
            t.LayoutStore = store;
        });
        Report.Line($"[care-density-kept] rows are {standard:0.##} tall with no kept layout and {table.RowHeight:0.##} tall when the person kept comfortable; a row made is {(table.RowsRepeater?.TryGetElement(0) as DataTableRow)?.ActualHeight:0.##} tall");
        await Handshake.StepAsync("care-density-kept", "the table with the density the person kept");
    }

    // A page that arrives later needs a second line and a detail the first rows did not: the rows and the columns follow, the view stays.
    private static async Task LateShapeAsync(MainWindow window)
    {
        ListDataTableModel<Customer> model = DemoData.Model();
        model.HasDetailOf = c => string.CompareOrdinal(c.Id, "c00030") >= 0;
        model.Reset(DemoData.Make(0, 30));
        TextTablePresenter<Customer> presenter = new(model)
        {
            Second = c => string.CompareOrdinal(c.Id, "c00030") >= 0 ? c.Email : null,
            Detail = c => new TextBlock { Text = c.Name + " wrote from " + c.City + ".", TextWrapping = TextWrapping.Wrap },
        };
        DataTable table = await ShowAsync(window, model, presenter);
        ScrollView? scroll = table.Scroller;
        if (scroll is null)
        {
            return;
        }

        Scroll(table, 440);
        await Task.Delay(900);
        double heightBefore = table.RowHeight;
        int topBefore = (int)(scroll.VerticalOffset / heightBefore);
        string key = model.GetRow(topBefore).Key ?? string.Empty;
        double standsBefore = (topBefore * heightBefore) - scroll.VerticalOffset;
        Report.Line($"[care-late-shape] 30 rows: row {heightBefore:0.##} tall, shape {table.Shape}, chevron column {table.HasChevronColumn}; the view is at {scroll.VerticalOffset:0.#} and row {key} stands {standsBefore:0.#} from its top");
        model.Append(DemoData.Make(30, 30));
        await Task.Delay(1800);
        int at = model.IndexOfKey(key);
        double stands = (at * table.RowHeight) - scroll.VerticalOffset;
        Report.Line($"[care-late-shape] 30 more rows with a second line and a detail: row {table.RowHeight:0.##} tall, shape {table.Shape}, chevron column {table.HasChevronColumn}; the view is at {scroll.VerticalOffset:0.#} and row {key} (now {at}) stands {stands:0.#} from its top");
        await Handshake.StepAsync("care-late-shape", "the table after a later page needed a second line and a detail");
    }

    private static async Task<WeakReference> ShowAndDropAsync(MainWindow window, ListDataTableModel<Customer> model)
    {
        DataTable table = new() { Model = model, Presenter = new TextTablePresenter<Customer>(model), Culture = DemoData.Culture, Margin = new Thickness(12) };
        window.StageHost.Child = table;
        await Waiting.LoadedAsync(table);
        await Task.Delay(1000);
        window.StageHost.Child = null;
        return new WeakReference(table);
    }

    // A table out of the tree stops listening to its model and runs nothing; a model that outlives its table does not hold it.
    private static async Task UnloadedAsync(MainWindow window)
    {
        ListDataTableModel<Customer> model = Rows(0, 100);
        WeakReference weak = await ShowAndDropAsync(window, model);
        for (int i = 0; i < 8; i++)
        {
            await Task.Delay(300);
            model.Append(DemoData.Make(100 + (i * 10), 10));
            GC.Collect();
            GC.WaitForPendingFinalizers();
        }

        Report.Line($"[care-unloaded] the table was taken out of the tree and the model changed {8} times since; the table is {(weak.IsAlive ? "still held" : "collected")}");

        ListDataTableModel<Customer> second = Rows(0, 100);
        DataTable table = await ShowAsync(window, second);
        window.StageHost.Child = null;
        await Task.Delay(500);
        second.Append(DemoData.Make(100, 40));
        second.Later = new DataTableCursor(true, CursorState.Idle);
        await Task.Delay(500);
        Report.Line($"[care-unloaded] out of the tree: the model holds {second.RowCount} rows and the table lists {table.RowsList.Count}");
        window.StageHost.Child = table;
        await Waiting.LoadedAsync(table);
        await Task.Delay(1200);
        Report.Line($"[care-unloaded] in the tree again: the model holds {second.RowCount} rows and the table lists {table.RowsList.Count}; rows made {table.RowsLayout.LastRealized.Count}");
        await Handshake.StepAsync("care-unloaded", "the table put back in the tree after its model changed");
    }

    // What assistive technology is told: a client in a process of its own listens to the window's events while the table is used, and the
    // peers are asked what a client would ask. What Narrator says is not heard here.
    private static async Task AssistiveAsync(MainWindow window)
    {
        ListDataTableModel<Customer> model = DemoData.Model();
        model.HasDetailOf = _ => true;
        model.Reset(DemoData.Make(500, 50));
        model.TotalCount = 1204;
        model.Summary = "1–50 of 1,204";
        model.Later = new DataTableCursor(true, CursorState.Idle);
        model.Earlier = new DataTableCursor(true, CursorState.Idle);
        TextTablePresenter<Customer> presenter = new(model)
        {
            Menu = c => new MenuFlyout { Items = { new MenuFlyoutItem { Text = "Open " + c.Name } } },
            Detail = c => new TextBlock { Text = c.Name + " wrote from " + c.City + ".", TextWrapping = TextWrapping.Wrap },
        };
        model.HasMenuOf = _ => true;
        DataTable table = await ShowAsync(window, model, presenter);
        await Task.Delay(500);

        string? folder = Report.Folder;
        string script = Path.Combine(AppContext.BaseDirectory, "Tools", "watch-uia.ps1");
        string log = Path.Combine(folder ?? Path.GetTempPath(), "uia-events.txt");
        string stop = Path.Combine(folder ?? Path.GetTempPath(), "uia-events.stop");
        File.Delete(stop);
        File.Delete(log);
        System.Diagnostics.Process? watcher = null;
        try
        {
            watcher = System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(
                "powershell.exe",
                $"-NoProfile -ExecutionPolicy Bypass -File \"{script}\" -ProcessId {Environment.ProcessId} -Out \"{log}\" -StopFile \"{stop}\"")
            {
                CreateNoWindow = true,
                UseShellExecute = false,
            });
        }
        catch (Exception problem)
        {
            Report.Line("[care-assistive] the listener could not be started: " + problem.Message);
        }

        for (int i = 0; i < 100 && !(File.Exists(log) && File.ReadAllText(log).Contains("handlers added", StringComparison.Ordinal)); i++)
        {
            await Task.Delay(200);
        }

        Report.Line("[care-assistive] listener " + (File.Exists(log) ? "started" : "did not start"));

        // What a client asks of the peers.
        Microsoft.UI.Xaml.Automation.Peers.AutomationPeer tablePeer = Microsoft.UI.Xaml.Automation.Peers.FrameworkElementAutomationPeer.CreatePeerForElement(table);
        DataTableRow? first = table.RowsRepeater?.TryGetElement(3) as DataTableRow;
        if (first is not null)
        {
            Microsoft.UI.Xaml.Automation.Peers.AutomationPeer rowPeer = Microsoft.UI.Xaml.Automation.Peers.FrameworkElementAutomationPeer.CreatePeerForElement(first);
            Report.Line($"[care-assistive] row 3 of 1,204 held 50 with more after and before: position {rowPeer.GetPositionInSet()}, size {rowPeer.GetSizeOfSet()}");
        }

        if (tablePeer is DataTableAutomationPeer grid)
        {
            Report.Line($"[care-assistive] the grid says {grid.RowCount} rows (the rows held) and {grid.ColumnCount} columns");
            foreach ((int r, int c) in new[] { (-1, 0), (0, -1), (50, 0), (0, 99) })
            {
                try
                {
                    grid.GetItem(r, c);
                    Report.Line($"[care-assistive] GetItem({r}, {c}) answered");
                }
                catch (ArgumentOutOfRangeException)
                {
                    Report.Line($"[care-assistive] GetItem({r}, {c}) threw ArgumentOutOfRangeException");
                }
            }

            Report.Line($"[care-assistive] GetItem(20, 1) answered {(grid.GetItem(20, 1) is not null)}; row 20 is {(table.RowsRepeater?.TryGetElement(20) is DataTableRow ? "made" : "not made")}");
        }

        // What a person does, and what the listener hears.
        table.ToggleChoice(2);
        await Task.Delay(500);
        table.ToggleChoice(4);
        await Task.Delay(500);
        table.ToggleChoice(2);
        await Task.Delay(500);
        table.ChooseAllHeld();
        await Task.Delay(700);
        table.ClearChoice();
        await Task.Delay(700);
        if (table.RowsRepeater?.TryGetElement(5) is DataTableRow detailRow)
        {
            table.ToggleDetail(detailRow);
            await Task.Delay(600);
            table.ToggleDetail(detailRow);
            await Task.Delay(600);
        }

        model.Append(DemoData.Make(550, 25));
        await Task.Delay(700);
        Scroll(table, 1200);
        await Task.Delay(900);
        Report.Line($"[care-assistive] last said: '{table.LastAnnouncement}'");
        await Handshake.StepAsync("care-assistive", "the table after the events were raised");

        File.WriteAllText(stop, "stop");
        if (watcher is not null)
        {
            await Task.Run(() => watcher.WaitForExit(8000));
        }

        if (File.Exists(log))
        {
            foreach (string line in File.ReadAllLines(log))
            {
                Report.Line("[care-assistive] heard " + line);
            }
        }
    }

    // A new model: what is typed looks through the new model's names, not the old one's.
    private static async Task NewModelNamesAsync(MainWindow window)
    {
        ListDataTableModel<Customer> first = Rows(0, 120);
        DataTable table = await ShowAsync(window, first);
        Keys.Foreground(window);
        table.FocusRow("c00001");
        await Task.Delay(600);
        await Keys.PressAsync(VirtualKey.H);
        await Task.Delay(900);
        Report.Line($"[care-new-model-names] typed H in the first model: active {table.ActiveCell?.RowKey}");

        ListDataTableModel<Customer> other = DemoData.Model();
        other.Reset(DemoData.Make(500, 90).Select(c => c with { Name = (c.Id.EndsWith('7') ? "Zorawar" : "Navdeep") + " " + c.Id }));
        table.Presenter = new TextTablePresenter<Customer>(other);
        table.Model = other;
        await Task.Delay(1200);
        table.FocusRow("c00500");
        await Task.Delay(600);
        await Keys.PressAsync(VirtualKey.Z);
        await Task.Delay(900);
        string key = table.ActiveCell?.RowKey ?? "none";
        Report.Line($"[care-new-model-names] typed Z in the new model: active {key} named '{(other.IndexOfKey(key) is var at and >= 0 ? other.ItemAt(at).Name : "?")}'");
        await Handshake.StepAsync("care-new-model-names", "the table with another model after typing Z");
    }
}

/// <summary>Draws a table whose second column is a text box, as a form in a table does.</summary>
internal sealed class FieldPresenter(ListDataTableModel<Customer> model) : IDataTablePresenter
{
    internal static ListDataTableModel<Customer> Model() => new(
        [
            new DataTableColumn<Customer>("name", "Name", c => c.Name, ColumnKind.Title, Role: ColumnRole.Title),
            new DataTableColumn<Customer>("city", "City", c => c.City, ColumnKind.Words),
            new DataTableColumn<Customer>("note", "Quantity", c => c.Email, ColumnKind.Words),
        ],
        c => c.Id);

    public UIElement CreateCell(DataTableColumn column, RowShape shape) =>
        column.Key == "note" ? new TextBox { VerticalAlignment = VerticalAlignment.Center, MinWidth = 60 } : new TextBlock { VerticalAlignment = VerticalAlignment.Center };

    public void BindCell(UIElement cell, in DataTableRecord record, int columnIndex)
    {
        string text = CellText(record, columnIndex);
        switch (cell)
        {
            case TextBox box:
                box.Text = text;
                break;
            case TextBlock block:
                block.Text = text;
                break;
        }
    }

    public void ClearCell(UIElement cell)
    {
        if (cell is TextBox box)
        {
            box.Text = string.Empty;
        }
    }

    public string CellText(in DataTableRecord record, int columnIndex) =>
        record.Data is Customer item ? model.Source[columnIndex].Text(item) : string.Empty;

    public string? SecondLine(in DataTableRecord record) => null;

    public string RowName(in DataTableRecord record) => CellText(record, 0);

    public int ControlCount(in DataTableRecord record, int columnIndex) => 0;

    public UIElement? CreateDetail(in DataTableRecord record) => null;

    public Microsoft.UI.Xaml.Controls.Primitives.FlyoutBase? CreateRowMenu(in DataTableRecord record) => null;
}
