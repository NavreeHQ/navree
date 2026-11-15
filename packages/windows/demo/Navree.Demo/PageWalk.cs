using System.Globalization;
using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Navree.Controls;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;
using Navree.Shell;
using Windows.Foundation;

namespace Navree.Demo;

/// <summary>
/// What a person meets when an answer from the server arrives while they are in the middle of something: the page that is on the screen must be
/// brought up to date where it stands, not drawn again. Each scenario holds a reference to what the person is using (a text box, the table, a button,
/// a sheet) before the answer, uses it with the real keys and the real pointer, and reads that it is the same object afterwards, with what the person
/// did still in it and the focus where it was. Which scenarios run follows the server: the store (orders), the inbox (a popover), the board (a
/// disclosure, a sheet, cards moved by the keys). What was done is written to <c>page.txt</c>.
/// </summary>
internal sealed class PageWalk(ShellHost host, ShellWindow window, string folder, string? server)
{
    private const uint MoveTo = 0x0001;
    private const uint LeftDown = 0x0002;
    private const uint LeftUp = 0x0004;
    private const uint Wheel = 0x0800;
    private const uint Absolute = 0x8000;
    private const uint KeyUp = 2;
    private const byte TabKey = 0x09;
    private const byte EnterKey = 0x0D;
    private const byte ShiftKey = 0x10;
    private const byte ControlKey = 0x11;
    private const byte AltKey = 0x12;
    private const byte EscapeKey = 0x1B;
    private const byte SpaceKey = 0x20;
    private const byte PageDownKey = 0x22;
    private const byte EndKey = 0x23;
    private const byte DownKey = 0x28;
    private const byte UpKey = 0x26;

    private readonly List<string> log = [];

    [StructLayout(LayoutKind.Sequential)]
    private struct Spot
    {
        public int X;
        public int Y;
    }

    [DllImport("user32.dll")]
    private static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);

    [DllImport("user32.dll")]
    private static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);

    [DllImport("user32.dll")]
    private static extern bool ClientToScreen(IntPtr window, ref Spot spot);

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll")]
    private static extern int GetSystemMetrics(int index);

    internal async Task RunAsync()
    {
        try
        {
            Directory.CreateDirectory(folder);
            for (int round = 0; round < 120 && host.State.Nav.Stage != Stage.Ready; round++)
            {
                await Task.Delay(250);
            }

            window.Size(1280, 1000);
            await Task.Delay(1500);
            string port = server is null ? string.Empty : new Uri(server).Port.ToString(CultureInfo.InvariantCulture);
            Say("the server is " + server + " (port " + port + ")");
            await KindsAsync();
            switch (port)
            {
                case "8095":
                    await StoreAsync();
                    break;
                case "8096":
                    await InboxAsync();
                    break;
                case "8097":
                    await BoardAsync();
                    break;
                default:
                    Say("no scenario is written for this server");
                    break;
            }
        }
        catch (Exception problem)
        {
            Say("the walk failed: " + problem);
        }
        finally
        {
            Say("done");
            File.WriteAllLines(Path.Combine(folder, "page.txt"), log);
            App.Finish(null);
        }
    }

    // How many parts of each kind the first destinations of this app draw: what the kinds that are brought up to date in place are chosen by.
    private async Task KindsAsync()
    {
        Dictionary<string, int> all = [];
        for (int destination = 0; destination < 5; destination++)
        {
            host.Send(Events.Select(destination));
            await Task.Delay(2500);
            if (host.State.FrontScreen?.Screen is not Models.Screen tree)
            {
                continue;
            }

            Dictionary<string, int> here = [];
            foreach (Models.Node node in Tree.Nodes(tree))
            {
                string kind = node.GetType().Name;
                here[kind] = here.GetValueOrDefault(kind) + 1;
                all[kind] = all.GetValueOrDefault(kind) + 1;
            }

            Say("destination " + destination + " (" + tree.Title + ") holds " + string.Join(", ", here.OrderByDescending(pair => pair.Value).Select(pair => pair.Key + " " + pair.Value)));
        }

        Say("all of them together: " + string.Join(", ", all.OrderByDescending(pair => pair.Value).Select(pair => pair.Key + " " + pair.Value)));
    }

    // ================================================================================================================
    // The store: the orders table
    // ================================================================================================================

    private async Task StoreAsync()
    {
        // The search goes last: the letters stay in the box and in the core, and the table that follows would hold fewer rows.
        await GuardedAsync("the keys", KeysAsync);
        await GuardedAsync("the wheel", WheelAsync);
        await GuardedAsync("coming back to a screen", ReturnAsync);
        await GuardedAsync("the search box", SearchAsync);
    }

    // One scenario that throws loses only itself.
    private async Task GuardedAsync(string name, Func<Task> scenario)
    {
        try
        {
            await scenario();
        }
        catch (Exception problem)
        {
            Say("[page] FAIL " + name + " could not be walked: " + problem.Message);
        }
    }

    // The orders table with its first rows only: the destination chosen and the screen asked for again.
    private async Task FreshOrdersAsync()
    {
        host.Send(Events.Select(1));
        await Task.Delay(2500);
        if (host.State.FrontScreen is { } front)
        {
            host.Send(Events.Refresh(front.Id));
            for (int waited = 0; waited < 9000 && RowsHeld() > 60; waited += 200)
            {
                await Task.Delay(200);
            }
        }

        await Task.Delay(2000);
    }

    // ---- lines 1 and 9: letters typed into the table's search box across the answers ---------------------------

    private async Task SearchAsync()
    {
        Say("== letters typed into the search box of the orders table, one at a time, across the core's answers");
        await FreshOrdersAsync();
        TextBox? before = SearchBox();
        DataTable? table = TableNow();
        Check("the search box and the table are on the screen", before is not null && table is not null, before is null ? "no search box" : "found");
        if (before is null || table is null)
        {
            return;
        }

        long screen = host.State.Nav.Front ?? 0;
        int mark = PageReports.Total;
        int cellsBefore = PageReports.CellsDrawn;
        await ClickAsync(before);
        foreach (char letter in "kaur")
        {
            Foreground();
            Press((byte)char.ToUpperInvariant(letter));
            await Task.Delay(1800);
            TextBox? during = SearchBox();
            Say("after '" + letter + "' the box holds '" + during?.Text + "', the core holds '" + CoreSearch() + "', the focus is on " + FocusedWords() + ", the box is the first one: " + ReferenceEquals(before, during));
        }

        await Task.Delay(2000);
        TextBox? after = SearchBox();
        Check("every letter typed across the answers is in the box", after?.Text == "kaur", "the box holds '" + after?.Text + "'");
        Check("the box is the same object as before the first key", ReferenceEquals(before, after), "same: " + ReferenceEquals(before, after));
        Check("the box has the focus after the answers", after is not null && ReferenceEquals(FocusManager.GetFocusedElement(window.Surface.XamlRoot), after), "focus is on " + FocusedWords());
        Check("the caret is at the end of the words", after is { SelectionStart: 4, SelectionLength: 0 }, "caret at " + after?.SelectionStart + ", selection " + after?.SelectionLength);
        Check("the table is the same object as before the first key", ReferenceEquals(table, TableNow()), "same: " + ReferenceEquals(table, TableNow()));
        await ShotAsync("page-search");

        // Line 9: what the answers drew.
        List<PageReport> passes = [.. PageReports.All.TakeLast(Math.Max(0, PageReports.Total - mark)).Where(report => report.Screen == screen)];
        PageReport? first = PageReports.FirstDrawings.GetValueOrDefault(screen);
        foreach (PageReport pass in passes)
        {
            Say("an answer: " + pass.Kind + ", drew " + pass.Rendered + " parts" + (pass.Kinds.Length > 0 ? " (" + pass.Kinds + ")" : string.Empty) + ", " + pass.Stood + " stood, " + pass.Updated + " brought up to date, " + pass.Replaced + " drawn again, " + pass.Lazy + " nodes drawn by the table's rows meanwhile");
        }

        foreach (string note in PageReports.Notes.TakeLast(12))
        {
            Say("why a part was drawn again: " + note);
        }

        int drawn = passes.Sum(pass => pass.Rendered);
        int whole = passes.Count(pass => pass.Kind == "whole");
        Say("the page's first drawing drew " + (first?.Rendered.ToString(CultureInfo.InvariantCulture) ?? "?") + " parts (" + first?.Kinds + ")");
        Say("the table's rows drew " + (PageReports.CellsDrawn - cellsBefore).ToString(CultureInfo.InvariantCulture) + " nodes in all since the first letter, which is the rows that came into view");
        Check("a search answer drew no part of the page again, only the table's rows", passes.Count > 0 && drawn == 0 && whole == 0, passes.Count + " answers, " + drawn + " parts drawn by them, " + whole + " of them drew the page whole");
    }

    private string CoreSearch() =>
        host.State.FrontScreen?.Forms.Values.SelectMany(form => form.Fields).Where(pair => pair.Value.Value is TextValue && pair.Key.Contains("search", StringComparison.OrdinalIgnoreCase)).Select(pair => pair.Key + "=" + ((TextValue)pair.Value.Value).Text).FirstOrDefault() ?? "nothing";

    // ---- line 2 (and why End brought no rows): the keys in the table -------------------------------------------

    private async Task KeysAsync()
    {
        Say("== the keys in the orders table: what each does, and what the table does when rows arrive");
        await FreshOrdersAsync();
        DataTable? table = TableNow();
        if (table is null)
        {
            Check("the orders table is on the screen", false, "none");
            return;
        }

        Describe("at the start", table);
        FrameworkElement? cell = FirstCell(table);
        if (cell is not null)
        {
            await ClickAsync(cell);
        }

        Describe("after a click on the first cell", table);

        // What the keys do, one after the other, each looked at before the next.
        Foreground();
        Press(EndKey);
        await Task.Delay(700);
        Describe("after End", table);
        Foreground();
        Press(PageDownKey);
        await Task.Delay(700);
        Describe("after Page Down", table);

        // Control and End is the last row of the grid; the rows held are 50, so the end is where more rows are asked for.
        await FreshOrdersAsync();
        table = TableNow()!;
        cell = FirstCell(table);
        if (cell is not null)
        {
            await ClickAsync(cell);
        }

        int before = RowsHeld();
        string? lastKey = LastRowKey();
        DataTableCellRef? lastBefore = table.ActiveCell;
        Foreground();
        keybd_event(ControlKey, 0, 0, UIntPtr.Zero);
        await Task.Delay(60);
        Press(EndKey);
        await Task.Delay(60);
        keybd_event(ControlKey, 0, KeyUp, UIntPtr.Zero);
        for (int waited = 0; waited < 9000 && RowsHeld() <= before; waited += 20)
        {
            await Task.Delay(20);
            lastBefore = table.ActiveCell ?? lastBefore;
        }

        Check("Control and End put the active cell on the last row held", lastBefore is { } reached && reached.RowKey == lastKey, "the last row is " + lastKey + ", the active cell was " + Words(lastBefore));

        Describe("after Control and End, as the rows arrived", table);
        await Task.Delay(1800);
        DataTable? tableAfter = TableNow();
        DataTableCellRef? afterRows = tableAfter?.ActiveCell;
        Describe("a little after the rows arrived", tableAfter ?? table);
        Check("rows arrived when the end was reached with Control and End", RowsHeld() > before, before + " rows held, then " + RowsHeld().ToString(CultureInfo.InvariantCulture));
        Check("the active cell is the same after the rows arrived", lastBefore is not null && afterRows == lastBefore, "before " + Words(lastBefore) + ", after " + Words(afterRows));
        Check("the focus is in the table after the rows arrived", tableAfter is not null && FocusIsIn(tableAfter), "focus is on " + FocusedWords());
        Check("the table is the same object after the rows arrived", ReferenceEquals(table, tableAfter), "same: " + ReferenceEquals(table, tableAfter));

        // Page Down from the top of fresh rows, until the table asks for more.
        await FreshOrdersAsync();
        table = TableNow()!;
        cell = FirstCell(table);
        if (cell is not null)
        {
            await ClickAsync(cell);
        }

        before = RowsHeld();
        DataTableCellRef? started = table.ActiveCell;
        lastBefore = started;
        int presses = 0;
        for (; presses < 14 && RowsHeld() <= before; presses++)
        {
            Foreground();
            Press(PageDownKey);
            for (int waited = 0; waited < 450 && RowsHeld() <= before; waited += 20)
            {
                await Task.Delay(20);
                lastBefore = table.ActiveCell ?? lastBefore;
            }
        }

        for (int waited = 0; waited < 5000 && RowsHeld() <= before; waited += 100)
        {
            await Task.Delay(100);
        }

        await Task.Delay(1800);
        tableAfter = TableNow();
        afterRows = tableAfter?.ActiveCell;
        Describe("after " + presses + " Page Down presses, a little after rows arrived", tableAfter ?? table);
        Check("Page Down brought more rows than before", RowsHeld() > before, before + " rows held, then " + RowsHeld().ToString(CultureInfo.InvariantCulture) + " after " + presses + " presses");
        Check("the active cell moved down with Page Down and is the same after the rows arrived", started is not null && lastBefore != started && afterRows == lastBefore, "started at " + Words(started) + ", before the rows " + Words(lastBefore) + ", after " + Words(afterRows));
        Check("the focus is in the table after Page Down brought rows", tableAfter is not null && FocusIsIn(tableAfter), "focus is on " + FocusedWords());
        Check("the table is the same object after Page Down brought rows", ReferenceEquals(table, tableAfter), "same: " + ReferenceEquals(table, tableAfter));
    }

    private void Describe(string when, DataTable table)
    {
        ScrollView? view = ScrollOf(table);
        Say(when + ": " + RowsHeld() + " rows held by the core, active cell " + Words(table.ActiveCell) + ", focus on " + FocusedWords()
            + (view is null ? string.Empty : ", view at " + view.VerticalOffset.ToString("0", CultureInfo.InvariantCulture) + " of " + view.ExtentHeight.ToString("0", CultureInfo.InvariantCulture) + " with a viewport of " + view.ViewportHeight.ToString("0", CultureInfo.InvariantCulture)));
    }

    // ---- line 3: the wheel to the end of the table, and on --------------------------------------------------------

    private async Task WheelAsync()
    {
        Say("== the wheel turned over the table, to its end and on");
        await FreshOrdersAsync();
        DataTable? table = TableNow();
        ScrollView? view = table is null ? null : ScrollOf(table);
        if (table is null || view is null)
        {
            Check("the orders table is on the screen", false, "none");
            return;
        }

        (int x, int y) = ScreenOf(table, table.ActualWidth / 2, table.ActualHeight / 2);
        Foreground();
        Absolute1(x, y);
        await Task.Delay(400);
        List<(int Rows, double Offset)> seen = [];
        int arrivals = 0;
        int rowsBefore = RowsHeld();
        for (int turn = 0; turn < 150 && RowsHeld() < 200; turn++)
        {
            mouse_event(Wheel, 0, 0, unchecked((uint)-360), UIntPtr.Zero);
            await Task.Delay(60);
            int rows = RowsHeld();
            if (rows > rowsBefore)
            {
                arrivals++;
                rowsBefore = rows;
            }

            seen.Add((rows, view.VerticalOffset));
        }

        await Task.Delay(2500);
        int total = RowsHeld();
        double highest = 0;
        double mostLost = 0;
        foreach ((int _, double offset) in seen)
        {
            highest = Math.Max(highest, offset);
            mostLost = Math.Max(mostLost, highest - offset);
        }

        Say("the wheel was turned " + seen.Count + " times; the core holds " + total + " rows; " + arrivals + " arrivals; the view reached " + highest.ToString("0", CultureInfo.InvariantCulture) + " and lost at most " + mostLost.ToString("0", CultureInfo.InvariantCulture) + " after reaching it");
        Check("rows kept arriving as the end was reached", arrivals >= 3 && total >= 200, arrivals + " arrivals, " + total + " rows");
        Check("the view did not jump when rows arrived, and it did scroll", mostLost < 60 && highest > 1000, "at most " + mostLost.ToString("0", CultureInfo.InvariantCulture) + " px lost after reaching " + highest.ToString("0", CultureInfo.InvariantCulture));
        Check("the table is the same object after the rows arrived", ReferenceEquals(table, TableNow()), "same: " + ReferenceEquals(table, TableNow()));
        await ShotAsync("page-wheel");
    }

    // ---- line 8: a screen left and come back to stands where it was ------------------------------------------

    private async Task ReturnAsync()
    {
        Say("== a screen left and come back to");
        for (int destination = 0; destination < 4; destination++)
        {
            host.Send(Events.Select(destination));
            await Task.Delay(2500);
            ScrollViewer? page = PageScroller();
            if (page is null || page.ScrollableHeight < 300)
            {
                Say("destination " + destination + " does not scroll (" + (page?.ScrollableHeight ?? 0).ToString("0", CultureInfo.InvariantCulture) + " px): not used");
                continue;
            }

            _ = page.ChangeView(null, 300, null, true);
            await Task.Delay(800);
            double was = page.VerticalOffset;
            host.Send(Events.Select(destination == 1 ? 0 : 1));
            await Task.Delay(2500);
            host.Send(Events.Select(destination));
            await Task.Delay(2500);
            double now = PageScroller()?.VerticalOffset ?? -1;
            Check("a screen left and come back to stands where it was", Math.Abs(now - was) < 5, "destination " + destination + ": at " + was.ToString("0", CultureInfo.InvariantCulture) + " before, at " + now.ToString("0", CultureInfo.InvariantCulture) + " after");
            return;
        }

        Check("a screen that scrolls was found to leave and come back to", false, "none of the first four destinations scrolls");
    }

    private ScrollViewer? PageScroller()
    {
        FrameworkElement? page = window.CurrentPage;
        for (DependencyObject? at = page; at is not null; at = VisualTreeHelper.GetParent(at))
        {
            if (at is ScrollViewer scroller)
            {
                return scroller;
            }
        }

        return null;
    }

    // ================================================================================================================
    // The inbox: a popover opened and closed with Escape (line 5)
    // ================================================================================================================

    private async Task InboxAsync()
    {
        host.Send(Events.Select(0));
        await SettleAsync();
        await Task.Delay(1500);
        if (host.State.FrontScreen?.Screen is Models.Screen list
            && Tree.Nodes(list).Select(node => node.GetType().GetProperty("Action")?.GetValue(node)).OfType<Models.ActionNavigate>().FirstOrDefault(go => go.Href.StartsWith("/inbox/", StringComparison.Ordinal)) is { } open)
        {
            host.Send(Events.Act(host.State.Nav.Front, open));
        }

        await Task.Delay(4000);
        foreach (string label in new[] { "Snooze", "Saved replies" })
        {
            await PopoverAsync(label);
        }
    }

    private async Task PopoverAsync(string label)
    {
        Say("== " + label);
        Button? button = ButtonNamed(label);
        Check("the button " + label + " is on the screen", button is not null, button is null ? "not found" : "found");
        if (button is null)
        {
            return;
        }

        int reports = PageReports.Total;
        await ClickAsync(button);
        await Task.Delay(900);
        bool open = VisualTreeHelper.GetOpenPopupsForXamlRoot(window.Surface.XamlRoot).Any(popup => popup.Child is not null && Below(popup.Child).Any(part => part is FlyoutPresenter or ContentDialog));
        Check(label + " opened", open, "a flyout or a dialog is open: " + open);
        Foreground();
        Press(EscapeKey);
        for (int waited = 0; waited < 5000 && host.State.FrontScreen?.Overlays.Count > 0; waited += 100)
        {
            await Task.Delay(100);
        }

        await Task.Delay(900);
        Say("the core's answers since the press: " + (PageReports.Total - reports) + " passes over the page; overlays held: " + (host.State.FrontScreen?.Overlays.Count ?? 0));
        foreach (PageReport pass in PageReports.All.TakeLast(Math.Max(0, PageReports.Total - reports)))
        {
            Say("a pass: " + pass.Kind + ", drew " + pass.Rendered + " (" + pass.Kinds + "), " + pass.Stood + " stood, " + pass.Updated + " updated, " + pass.Replaced + " drawn again");
        }

        foreach (string note in PageReports.Notes.TakeLast(6))
        {
            Say("why a part was drawn again: " + note);
        }
        bool closed = !VisualTreeHelper.GetOpenPopupsForXamlRoot(window.Surface.XamlRoot).Any(popup => popup.Child is not null && Below(popup.Child).Any(part => part is FlyoutPresenter or ContentDialog));
        Check("Escape took " + label + " away", closed, closed ? "nothing is open" : "something is still open");
        Button? again = ButtonNamed(label);
        object? focused = FocusManager.GetFocusedElement(window.Surface.XamlRoot);
        Check("the focus is on the button that opened it", ReferenceEquals(focused, button), "focus is on " + (focused?.GetType().Name ?? "nothing") + " '" + (focused is DependencyObject element ? Name(element) : string.Empty) + "'");
        Check("the button is the same object as before it was pressed", ReferenceEquals(button, again), "same: " + ReferenceEquals(button, again));

        // The core's answer has come by now; after it the button still stands and still has the focus.
        await Task.Delay(1500);
        Check("after the core's answer the focus is still on that button", ReferenceEquals(FocusManager.GetFocusedElement(window.Surface.XamlRoot), button), "focus is on " + FocusedWords() + "; the core holds overlays: " + string.Join(",", host.State.FrontScreen?.Overlays ?? []));
    }

    private Button? ButtonNamed(string label) =>
        Below(window.Surface).OfType<Button>().FirstOrDefault(candidate => candidate.ActualWidth > 0 && (AutomationProperties.GetName(candidate) == label || candidate.Content as string == label));

    // ================================================================================================================
    // The board: a disclosure (line 7), a sheet sent with a wrong value (line 6), a card moved and another pressed (line 10)
    // ================================================================================================================

    private async Task BoardAsync()
    {
        host.Send(Events.Select(0));
        await SettleAsync();
        await Task.Delay(1500);
        await DisclosureAsync();
        host.Send(Events.Select(0));
        await SettleAsync();
        await Task.Delay(1500);
        await SheetAsync();
        host.Send(Events.Select(0));
        await SettleAsync();
        await Task.Delay(1500);
        await StaleAsync();
    }

    private async Task DisclosureAsync()
    {
        Say("== a disclosure opened and closed with Space");
        Expander? first = Below(window.Surface).OfType<Expander>().FirstOrDefault(candidate => candidate.ActualHeight > 0);
        Check("a disclosure is on the board screen", first is not null, first is null ? "none" : "found: " + (first.Header as string));
        if (first is null)
        {
            return;
        }

        string header = first.Header as string ?? string.Empty;
        ToggleButton? toggle = Below(first).OfType<ToggleButton>().FirstOrDefault();
        Foreground();
        _ = (toggle as Control)?.Focus(FocusState.Keyboard);
        await Task.Delay(500);
        bool was = first.IsExpanded;
        await PressAsync(SpaceKey);
        await SettleAsync();
        await Task.Delay(1500);
        Expander? after = Below(window.Surface).OfType<Expander>().FirstOrDefault(candidate => candidate.Header as string == header && candidate.ActualHeight > 0);
        Check("Space opened the disclosure", after is not null && after.IsExpanded != was, "was " + was + ", now " + after?.IsExpanded);
        Check("the disclosure is the same object after it was opened", ReferenceEquals(first, after), "same: " + ReferenceEquals(first, after));
        Check("the focus is still on the disclosure after it was opened", Chain(FocusManager.GetFocusedElement(window.Surface.XamlRoot) as DependencyObject).Any(part => ReferenceEquals(part, first)), "focus is on " + FocusedWords());
        await PressAsync(SpaceKey);
        await SettleAsync();
        await Task.Delay(1500);
        Check("Space shut the disclosure again", after is not null && after.IsExpanded == was, "now " + after?.IsExpanded + ", began " + was);
        Check("the focus is still on the disclosure after it was closed", Chain(FocusManager.GetFocusedElement(window.Surface.XamlRoot) as DependencyObject).Any(part => ReferenceEquals(part, first)), "focus is on " + FocusedWords());
    }

    private async Task SheetAsync()
    {
        Say("== a comment sent empty inside the card's sheet");
        (string Lane, int Index, string Id, string Title)? card = Pick("backlog", 0);
        Button? handle = card is { } picked ? HandleOf(picked.Title) : null;
        FrameworkElement? cardElement = handle is null ? null : Up(handle);
        FrameworkElement? title = card is not { } named || cardElement is null
            ? null
            : Below(cardElement).OfType<FrameworkElement>().FirstOrDefault(element => element is Button or TextBlock && element != handle && (element is TextBlock block ? block.Text == named.Title : Name(element) == named.Title || Name(element).Contains(named.Title, StringComparison.Ordinal)));
        Check("a card with a title to press is on the board", title is not null, title is null ? "none found" : "found");
        if (title is null)
        {
            return;
        }

        await ClickAsync(title);
        FrameworkElement? sheet = null;
        TextBox? comment = null;
        for (int waited = 0; waited < 9000 && comment is null; waited += 250)
        {
            await Task.Delay(250);
            sheet = SheetOnScreen();
            comment = sheet is null ? null : Below(sheet).OfType<TextBox>().FirstOrDefault(box => Name(box).Contains("comment", StringComparison.OrdinalIgnoreCase) || box.PlaceholderText.Contains("note", StringComparison.OrdinalIgnoreCase));
        }

        Check("pressing the card opens a sheet with its comment field", comment is not null, comment is null ? "no sheet with a comment box" : "found in " + sheet!.GetType().Name);
        if (comment is null || sheet is null)
        {
            return;
        }

        Button? send = Below(sheet).OfType<Button>().FirstOrDefault(button => Name(button).Contains("comment", StringComparison.OrdinalIgnoreCase) || Name(button).Contains("send", StringComparison.OrdinalIgnoreCase) || Name(button).Contains("post", StringComparison.OrdinalIgnoreCase));
        Check("the sheet has the button that sends the comment", send is not null, send is null ? "none" : Name(send));
        if (send is null)
        {
            return;
        }

        await ClickAsync(comment);
        Foreground();
        for (int step = 0; step < 6 && !ReferenceEquals(FocusManager.GetFocusedElement(window.Surface.XamlRoot), send); step++)
        {
            await PressAsync(TabKey);
        }

        bool reached = ReferenceEquals(FocusManager.GetFocusedElement(window.Surface.XamlRoot), send);
        if (reached)
        {
            await PressAsync(EnterKey);
        }
        else
        {
            await ClickAsync(send);
        }

        Focus? asked = null;
        long? olderSerial = host.State.FrontScreen?.Focus?.Serial;
        for (int sample = 0; sample < 150; sample++)
        {
            if (host.State.FrontScreen?.Focus is { } request && request.Serial != olderSerial)
            {
                asked ??= request;
            }

            await Task.Delay(15);
        }

        await SettleAsync();
        await Task.Delay(1500);
        FrameworkElement? after = SheetOnScreen();
        string? error = host.State.FrontScreen?.Forms.Values.SelectMany(form => form.Fields.Values).SelectMany(field => field.Errors).FirstOrDefault();
        Check("the server refused the empty comment with words for the field", error is not null, error is null ? "no field holds an error" : "\"" + error + "\"");
        Check("the sheet stayed open (the same dialog, or the one pane)", after is not null && ReferenceEquals(after, sheet), "sheet before " + sheet.GetType().Name + ", after " + after?.GetType().Name + ", same: " + ReferenceEquals(after, sheet));
        Check("the error stands at its field in the sheet", error is not null && after is not null && DrawnText.Of(after).Contains(error, StringComparison.Ordinal), error is null ? "no error" : "\"" + error + "\" looked for in the sheet");
        Check("the comment box is the same object after the answer", after is not null && Below(after).OfType<TextBox>().Any(box => ReferenceEquals(box, comment)), "same box found: " + (after is not null && Below(after).OfType<TextBox>().Any(box => ReferenceEquals(box, comment))));
        Check("the core asked for the focus to be put on the field in error", asked is not null, asked is null ? "no request was seen" : "target " + asked.Target);
        if (asked is not null)
        {
            object? focused = FocusManager.GetFocusedElement(window.Surface.XamlRoot);
            bool onField = focused is TextBox box && after is not null && Below(after).Contains(box) && Chain(box).Any(part => AutomationProperties.GetAutomationId(part) == asked.Target);
            Check("the field the core named has the focus", onField, "the target is " + asked.Target + "; focus is on " + FocusedWords());
        }

        await ShotAsync("page-sheet-error");
        if (host.State.FrontScreen is { } front)
        {
            foreach (string overlay in front.Overlays.ToList())
            {
                host.Send(Events.Overlay(front.Id, overlay, false));
            }

            await SettleAsync();
        }
    }

    // What is open over the board as a sheet: the side pane in the window, or the dialog in a popup.
    private FrameworkElement? SheetOnScreen()
    {
        FrameworkElement? pane = Below(window.Surface).OfType<FrameworkElement>().FirstOrDefault(element => element.GetType().Name == "SidePane" && element.ActualWidth > 0);
        return pane ?? Popups().SelectMany(Below).OfType<ContentDialog>().FirstOrDefault();
    }

    // ---- line 10: a card moved with the keys, then a button of another card that stood ---------------------------

    private async Task StaleAsync()
    {
        Say("== a card moved with the keys, then another card's handle pressed");
        (string Lane, int Index, string Id, string Title)? a = Pick("backlog", 0);
        (string Lane, int Index, string Id, string Title)? b = Pick("backlog", 1);
        if (a is not { } first || b is not { } second)
        {
            Check("two cards are in the first lane", false, "none found");
            return;
        }

        List<string> start = Order("backlog");
        Say("the first lane holds " + string.Join(", ", start.Take(5)) + (start.Count > 5 ? " ..." : string.Empty));
        Button? handleA = HandleOf(first.Title);
        Button? handleB = HandleOf(second.Title);
        FrameworkElement? cardB = handleB is null ? null : Up(handleB);
        if (handleA is null || handleB is null || cardB is null)
        {
            Check("the two cards have their handles", false, "a " + (handleA is not null) + ", b " + (handleB is not null));
            return;
        }

        // The first card goes one place down with the keys; the second card stands and is now first.
        Foreground();
        _ = handleA.Focus(FocusState.Keyboard);
        await Task.Delay(500);
        await ChordAsync(DownKey);
        await WaitOrderAsync("backlog", 1, first.Id);
        List<string> moved = Order("backlog");
        Say("after the first card went down the lane holds " + string.Join(", ", moved.Take(5)));
        Check("the first card went one place down", moved.IndexOf(first.Id) == 1, "it is at " + moved.IndexOf(first.Id));
        await Task.Delay(1000);

        // The second card is the same object as before; its handle is pressed with the keys: it goes one place down in the board as it is now.
        Button? handleBNow = HandleOf(second.Title);
        FrameworkElement? cardBNow = handleBNow is null ? null : Up(handleBNow);
        Check("the second card stood across the move of the first", ReferenceEquals(cardB, cardBNow) && ReferenceEquals(handleB, handleBNow), "card same: " + ReferenceEquals(cardB, cardBNow) + ", handle same: " + ReferenceEquals(handleB, handleBNow));
        Foreground();
        _ = (handleBNow ?? handleB).Focus(FocusState.Keyboard);
        await Task.Delay(500);
        await ChordAsync(DownKey);
        await WaitOrderAsync("backlog", 1, second.Id);
        List<string> end = Order("backlog");
        Say("after the second card's handle was pressed the lane holds " + string.Join(", ", end.Take(5)));
        Check("the lane is where it began after the two moves", end.Take(5).SequenceEqual(start.Take(5)), "began " + string.Join(", ", start.Take(5)) + ", now " + string.Join(", ", end.Take(5)));
        Check("the second card went one place down in the board as it is now, not as it was", end.IndexOf(second.Id) == moved.IndexOf(second.Id) + 1 && end.IndexOf(first.Id) == moved.IndexOf(first.Id) - 1, "the lane held " + string.Join(", ", moved.Take(4)) + " and now holds " + string.Join(", ", end.Take(4)));
    }

    private (string Lane, int Index, string Id, string Title)? Pick(string lane, int index)
    {
        if (host.State.FrontScreen?.Screen is not Models.Screen tree)
        {
            return null;
        }

        Models.List? list = Tree.Nodes(tree).OfType<Models.List>().FirstOrDefault(candidate => candidate.Reorder?.Key == lane);
        List<Models.Row> rows = list is null ? [] : [.. list.Children.OfType<Models.Row>()];
        if (index >= rows.Count)
        {
            return null;
        }

        Models.Row row = rows[index];
        return (lane, index, row.Id ?? row.Key ?? row.Title, row.Title);
    }

    private List<string> Order(string lane)
    {
        if (host.State.FrontScreen?.Screen is not Models.Screen tree)
        {
            return [];
        }

        Models.List? list = Tree.Nodes(tree).OfType<Models.List>().FirstOrDefault(candidate => candidate.Reorder?.Key == lane);
        return list is null ? [] : [.. list.Children.OfType<Models.Row>().Select(row => row.Id ?? row.Key ?? row.Title)];
    }

    private async Task WaitOrderAsync(string lane, int index, string id)
    {
        for (int waited = 0; waited < 6000; waited += 200)
        {
            await Task.Delay(200);
            if (Order(lane).IndexOf(id) == index)
            {
                break;
            }
        }
    }

    private Button? HandleOf(string title) =>
        Below(window.Surface).OfType<Button>().FirstOrDefault(button => button.ActualWidth > 0 && AutomationProperties.GetName(button) == "Move " + title);

    private static FrameworkElement? Up(DependencyObject from)
    {
        for (DependencyObject? at = VisualTreeHelper.GetParent(from); at is not null; at = VisualTreeHelper.GetParent(at))
        {
            if (at is Microsoft.UI.Xaml.Controls.Border { CanDrag: true } card)
            {
                return card;
            }
        }

        return null;
    }

    // ================================================================================================================
    // What is on the screen, and what the core holds
    // ================================================================================================================

    private string? LastRowKey() =>
        host.State.FrontScreen?.Screen is Models.Screen tree ? Tree.Nodes(tree).OfType<Models.Table>().Select(table => table.Rows.LastOrDefault()?.Id).FirstOrDefault() : null;

    private int RowsHeld() =>
        host.State.FrontScreen?.Screen is Models.Screen tree ? Tree.Nodes(tree).OfType<Models.Table>().Select(table => table.Rows.Count).FirstOrDefault() : -1;

    private TextBox? SearchBox() =>
        Below(window.Surface).OfType<TextBox>().FirstOrDefault(box => box.ActualWidth > 0 && (AutomationProperties.GetName(box).Contains("search", StringComparison.OrdinalIgnoreCase) || box.PlaceholderText.Contains("search", StringComparison.OrdinalIgnoreCase)));

    private DataTable? TableNow() => Below(window.Surface).OfType<DataTable>().FirstOrDefault();

    private static FrameworkElement? FirstCell(DataTable table) =>
        Below(table).OfType<FrameworkElement>()
            .Where(element => element.GetType().Name == "DataTableCell" && element.ActualWidth > 0 && element.TransformToVisual(table).TransformPoint(new Point(0, 0)).Y is > 60 and < 400)
            .OrderBy(element => element.TransformToVisual(table).TransformPoint(new Point(0, 0)).Y)
            .FirstOrDefault();

    private bool FocusIsIn(DependencyObject container) =>
        Chain(FocusManager.GetFocusedElement(window.Surface.XamlRoot) as DependencyObject).Any(part => ReferenceEquals(part, container));

    private string FocusedWords() =>
        FocusManager.GetFocusedElement(window.Surface.XamlRoot) switch
        {
            null => "nothing",
            TextBox box => "a text box that holds '" + box.Text + "'",
            Button button => "a button named '" + Name(button) + "'",
            { } other => other.GetType().Name + (other is DependencyObject element && Name(element).Length > 0 ? " '" + Name(element) + "'" : string.Empty),
        };

    private static ScrollView? ScrollOf(DataTable table) => Below(table).OfType<ScrollView>().FirstOrDefault();

    private static string Words(DataTableCellRef? cell) => cell is { } at ? "row " + at.RowKey + ", column " + at.ColumnKey : "none";

    private static string Name(DependencyObject element) => AutomationProperties.GetName(element) ?? string.Empty;

    private static IEnumerable<DependencyObject> Chain(DependencyObject? from)
    {
        for (DependencyObject? at = from; at is not null; at = VisualTreeHelper.GetParent(at))
        {
            yield return at;
        }
    }

    private IEnumerable<DependencyObject> Popups()
    {
        foreach (Popup popup in VisualTreeHelper.GetOpenPopupsForXamlRoot(window.Surface.XamlRoot))
        {
            if (popup.Child is { } child)
            {
                yield return child;
            }
        }
    }

    private static IEnumerable<DependencyObject> Below(DependencyObject root)
    {
        yield return root;
        int count = VisualTreeHelper.GetChildrenCount(root);
        for (int at = 0; at < count; at++)
        {
            foreach (DependencyObject inner in Below(VisualTreeHelper.GetChild(root, at)))
            {
                yield return inner;
            }
        }
    }

    // ================================================================================================================
    // Real input
    // ================================================================================================================

    private (int X, int Y) ScreenOf(FrameworkElement element, double x, double y)
    {
        Point inWindow = element.TransformToVisual(null).TransformPoint(new Point(x, y));
        double scale = element.XamlRoot.RasterizationScale;
        Spot origin = default;
        ClientToScreen(WinRT.Interop.WindowNative.GetWindowHandle(window), ref origin);
        return (origin.X + (int)Math.Round(inWindow.X * scale), origin.Y + (int)Math.Round(inWindow.Y * scale));
    }

    private static void Absolute1(int x, int y)
    {
        int width = Math.Max(2, GetSystemMetrics(0));
        int height = Math.Max(2, GetSystemMetrics(1));
        mouse_event(MoveTo | Absolute, (int)((long)x * 65535 / (width - 1)), (int)((long)y * 65535 / (height - 1)), 0, UIntPtr.Zero);
    }

    private async Task ClickAsync(FrameworkElement element)
    {
        Foreground();
        (int x, int y) = ScreenOf(element, element.ActualWidth / 2, element.ActualHeight / 2);
        Absolute1(x, y);
        await Task.Delay(250);
        mouse_event(LeftDown, 0, 0, 0, UIntPtr.Zero);
        await Task.Delay(80);
        mouse_event(LeftUp, 0, 0, 0, UIntPtr.Zero);
        await Task.Delay(500);
    }

    private static void Press(byte key)
    {
        keybd_event(key, 0, 0, UIntPtr.Zero);
        keybd_event(key, 0, KeyUp, UIntPtr.Zero);
    }

    private async Task PressAsync(byte key)
    {
        Foreground();
        keybd_event(key, 0, 0, UIntPtr.Zero);
        await Task.Delay(50);
        keybd_event(key, 0, KeyUp, UIntPtr.Zero);
        await Task.Delay(120);
    }

    private static async Task ChordAsync(byte arrow)
    {
        keybd_event(AltKey, 0, 0, UIntPtr.Zero);
        keybd_event(ShiftKey, 0, 0, UIntPtr.Zero);
        await Task.Delay(60);
        keybd_event(arrow, 0, 0, UIntPtr.Zero);
        await Task.Delay(60);
        keybd_event(arrow, 0, KeyUp, UIntPtr.Zero);
        await Task.Delay(60);
        keybd_event(ShiftKey, 0, KeyUp, UIntPtr.Zero);
        keybd_event(AltKey, 0, KeyUp, UIntPtr.Zero);
    }

    private void Foreground() => SetForegroundWindow(WinRT.Interop.WindowNative.GetWindowHandle(window));

    private async Task ShotAsync(string name)
    {
        try
        {
            await ScreenPicture.SaveAsync(Path.Combine(folder, name + ".png"));
        }
        catch (Exception problem)
        {
            Say("no picture " + name + ": " + problem.Message);
        }
    }

    private async Task SettleAsync()
    {
        int calm = 0;
        for (int round = 0; round < 80 && calm < 2; round++)
        {
            ViewState state = await host.SettledAsync();
            await Task.Delay(250);
            calm = state.Screens.Values.Any(screen => screen.Loading || screen.Refreshing) ? 0 : calm + 1;
        }
    }

    private void Check(string what, bool ok, string seen) => Say("[page] " + (ok ? "PASS " : "FAIL ") + what + ": " + seen);

    private void Say(string line) => log.Add(DateTime.Now.ToString("HH:mm:ss.fff", CultureInfo.InvariantCulture) + " " + line);
}
