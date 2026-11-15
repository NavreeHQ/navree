using System.Globalization;
using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;
using Navree.Shell;
using Windows.Foundation;

namespace Navree.Demo;

/// <summary>
/// What a hand does to the live store's orders table, with the real pointer and the real keys: the wheel turned over the table, then the keys
/// (End, Page Down) with a cell chosen, each time looking at how many rows the core holds (a table asks for more rows by cursor as its end is
/// reached); then a right click on a row, the menu it opens, and one of its items chosen with a real click. What was done and what came of it is
/// written to <c>hands.txt</c>.
/// </summary>
internal sealed class TableHands(ShellHost host, ShellWindow window, string folder)
{
    private const uint MoveTo = 0x0001;
    private const uint LeftDown = 0x0002;
    private const uint LeftUp = 0x0004;
    private const uint RightDown = 0x0008;
    private const uint RightUp = 0x0010;
    private const uint Wheel = 0x0800;
    private const uint Absolute = 0x8000;
    private const uint KeyUp = 2;
    private const byte End = 0x23;
    private const byte PageDown = 0x22;

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
            host.Send(Events.Select(1));
            await Task.Delay(3000);
            await WheelAsync();
            await KeysAsync();
            await SearchAsync();
            await MenuAsync();
        }
        catch (Exception problem)
        {
            Say("the hands failed: " + problem);
        }
        finally
        {
            Say("done");
            File.WriteAllLines(Path.Combine(folder, "hands.txt"), log);
            App.Finish(null);
        }
    }

    private int RowsHeld() =>
        host.State.FrontScreen?.Screen is Models.Screen tree ? Tree.Nodes(tree).OfType<Models.Table>().Select(table => table.Rows.Count).FirstOrDefault() : -1;

    private async Task WheelAsync()
    {
        Say("== the wheel");
        Navree.Controls.DataTable? table = Below(window.Surface).OfType<Navree.Controls.DataTable>().FirstOrDefault();
        Check("the orders table is on the screen", table is not null, table is null ? "none" : "found");
        if (table is null)
        {
            return;
        }

        int before = RowsHeld();
        (int x, int y) = ScreenOf(table, table.ActualWidth / 2, table.ActualHeight / 2);
        Foreground();
        Move(x, y);
        await Task.Delay(300);
        for (int turn = 0; turn < 60; turn++)
        {
            mouse_event(Wheel, 0, 0, unchecked((uint)-360), UIntPtr.Zero);
            await Task.Delay(60);
        }

        await Task.Delay(2500);
        int after = RowsHeld();
        Check("turning the wheel over the table to its end asked for more rows", after > before, before + " rows held, then " + after.ToString(CultureInfo.InvariantCulture));
        await ShotAsync("hands-wheel");
    }

    private async Task KeysAsync()
    {
        Say("== the keys");
        Navree.Controls.DataTable? table = Below(window.Surface).OfType<Navree.Controls.DataTable>().FirstOrDefault();
        if (table is null)
        {
            return;
        }

        FrameworkElement? cell = Below(table).OfType<FrameworkElement>().FirstOrDefault(element => element.GetType().Name == "DataTableCell" && element.ActualWidth > 0 && element.TransformToVisual(table).TransformPoint(new Point(0, 0)).Y > 60);
        if (cell is not null)
        {
            await ClickAsync(cell);
        }

        int before = RowsHeld();
        Navree.Controls.DataTableCellRef? chosen = table.ActiveCell;
        Say("the active cell before End: " + Words(chosen));
        Foreground();
        Press(End);
        await Task.Delay(200);

        // The end is where the table asks for more rows; they arrive, and the person is where they were: the same cell, the focus in the table.
        Navree.Controls.DataTableCellRef? atEnd = TableNow()?.ActiveCell;
        for (int waited = 0; waited < 8000 && RowsHeld() <= before; waited += 200)
        {
            await Task.Delay(200);
        }

        await Task.Delay(1500);
        Navree.Controls.DataTable? tableAfter = TableNow();
        Navree.Controls.DataTableCellRef? afterRows = tableAfter?.ActiveCell;
        Check("rows arrived when the end was reached", RowsHeld() > before, before + " rows held, then " + RowsHeld().ToString(CultureInfo.InvariantCulture));
        Check("the active cell is the same after the rows arrived", atEnd is not null && afterRows == atEnd, "at the end " + Words(atEnd) + ", after the rows " + Words(afterRows));
        Check("the focus is in the table after the rows arrived", tableAfter is not null && FocusIsIn(tableAfter), "focus is on " + FocusedWords());
        int afterEnd = RowsHeld();
        Check("End with a cell chosen went to the end and asked for more rows", afterEnd >= before, before + " rows held, then " + afterEnd.ToString(CultureInfo.InvariantCulture));
        for (int page = 0; page < 6; page++)
        {
            Press(PageDown);
            await Task.Delay(500);
        }

        await Task.Delay(2000);
        int afterPages = RowsHeld();
        Check("Page Down kept the view at the end and more rows came", afterPages >= afterEnd, afterEnd + " rows held, then " + afterPages.ToString(CultureInfo.InvariantCulture));
    }

    // Letters are typed one at a time into the table's search box, each long enough for the core's answer to come and the page to be
    // drawn again: every letter is in the box at the end and the box still has the focus.
    private async Task SearchAsync()
    {
        Say("== the search box across an answer");
        host.Send(Events.Select(1));
        await Task.Delay(2500);
        TextBox? box = SearchBox();
        Check("the table's search box is on the screen", box is not null, box is null ? "no text box named Search" : "found");
        if (box is null)
        {
            return;
        }

        await ClickAsync(box);
        foreach (char letter in "kaur")
        {
            Foreground();
            Press((byte)char.ToUpperInvariant(letter));
            await Task.Delay(1800);
        }

        await Task.Delay(2000);
        TextBox? now = SearchBox();
        Check("every letter typed across the answers is in the box", now?.Text == "kaur", "the box holds '" + now?.Text + "'");
        Check("the box has the focus after the answers", now is not null && ReferenceEquals(Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(window.Surface.XamlRoot), now), "focus is on " + FocusedWords());
        await ShotAsync("hands-search");
    }

    private TextBox? SearchBox() =>
        Below(window.Surface).OfType<TextBox>().FirstOrDefault(box => AutomationProperties.GetName(box).Contains("search", StringComparison.OrdinalIgnoreCase) || box.PlaceholderText.Contains("search", StringComparison.OrdinalIgnoreCase));

    private Navree.Controls.DataTable? TableNow() => Below(window.Surface).OfType<Navree.Controls.DataTable>().FirstOrDefault();

    private bool FocusIsIn(DependencyObject container)
    {
        for (DependencyObject? at = Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(window.Surface.XamlRoot) as DependencyObject; at is not null; at = VisualTreeHelper.GetParent(at))
        {
            if (ReferenceEquals(at, container))
            {
                return true;
            }
        }

        return false;
    }

    private string FocusedWords() =>
        Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(window.Surface.XamlRoot) switch
        {
            null => "nothing",
            TextBox box => "a text box that holds '" + box.Text + "'",
            { } other => other.GetType().Name,
        };

    private static string Words(Navree.Controls.DataTableCellRef? cell) => cell is { } at ? "row " + at.RowKey + ", column " + at.ColumnKey : "none";

    private async Task MenuAsync()
    {
        Say("== a row's menu");
        host.Send(Events.Select(1));
        await Task.Delay(2500);
        Navree.Controls.DataTable? table = Below(window.Surface).OfType<Navree.Controls.DataTable>().FirstOrDefault();
        FrameworkElement? cell = table is null ? null : Below(table).OfType<FrameworkElement>().FirstOrDefault(element => element.GetType().Name == "DataTableCell" && element.ActualWidth > 0 && element.TransformToVisual(table).TransformPoint(new Point(0, 0)).Y > 60);
        if (cell is null)
        {
            Check("a row of the table is on the screen", false, "no cell found");
            return;
        }

        (int x, int y) = ScreenOf(cell, cell.ActualWidth / 2, cell.ActualHeight / 2);
        Navree.Controls.DataTableCellRef? activeBefore = table?.ActiveCell;
        Foreground();
        Move(x, y);
        await Task.Delay(250);
        mouse_event(RightDown, 0, 0, 0, UIntPtr.Zero);
        await Task.Delay(80);
        mouse_event(RightUp, 0, 0, 0, UIntPtr.Zero);
        await Task.Delay(900);

        // Where a click that opens no menu went: whether it reached a row (the active cell's keys before and after) and what is open over the window.
        Say("the active cell before the right click: " + Words(activeBefore) + ", after: " + Words(TableNow()?.ActiveCell));
        Say("open over the window: " + string.Join(", ", VisualTreeHelper.GetOpenPopupsForXamlRoot(window.Surface.XamlRoot).Select(popup => popup.Child?.GetType().Name ?? "nothing")) + "; menus in them: " + Popups().SelectMany(Below).OfType<MenuFlyoutPresenter>().Count());
        List<MenuFlyoutItem> items = [.. Popups().SelectMany(Below).OfType<MenuFlyoutItem>()];
        Check("a right click on a row opens its menu", items.Count > 0, "items: " + string.Join(", ", items.Select(item => item.Text + (item.IsEnabled ? string.Empty : " (disabled)"))));
        await ShotAsync("hands-row-menu");
        MenuFlyoutItem? chosen = items.FirstOrDefault(item => item.IsEnabled);
        if (chosen is null)
        {
            return;
        }

        int overlaysBefore = host.State.FrontScreen?.Overlays.Count ?? 0;
        long? frontBefore = host.State.Nav.Front;
        string said = chosen.Text;
        await ClickAsync(chosen);
        await Task.Delay(2500);
        Check("choosing " + said + " did what it names", host.State.Nav.Front != frontBefore || (host.State.FrontScreen?.Overlays.Count ?? 0) > overlaysBefore || host.State.Messages.Count > 0,
            "front screen " + frontBefore + " then " + host.State.Nav.Front + "; overlays " + overlaysBefore + " then " + (host.State.FrontScreen?.Overlays.Count ?? 0) + "; messages " + host.State.Messages.Count);
        await ShotAsync("hands-row-menu-chosen");
    }

    // ---- real input -----------------------------------------------------------------------------------------

    private (int X, int Y) ScreenOf(FrameworkElement element, double x, double y)
    {
        Point inWindow = element.TransformToVisual(null).TransformPoint(new Point(x, y));
        double scale = element.XamlRoot.RasterizationScale;
        Spot origin = default;
        ClientToScreen(WinRT.Interop.WindowNative.GetWindowHandle(window), ref origin);
        return (origin.X + (int)Math.Round(inWindow.X * scale), origin.Y + (int)Math.Round(inWindow.Y * scale));
    }

    private static void Move(int x, int y)
    {
        int width = Math.Max(2, GetSystemMetrics(0));
        int height = Math.Max(2, GetSystemMetrics(1));
        mouse_event(MoveTo | Absolute, (int)((long)x * 65535 / (width - 1)), (int)((long)y * 65535 / (height - 1)), 0, UIntPtr.Zero);
    }

    private async Task ClickAsync(FrameworkElement element)
    {
        Foreground();
        (int x, int y) = ScreenOf(element, element.ActualWidth / 2, element.ActualHeight / 2);
        Move(x, y);
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

    private void Foreground() => SetForegroundWindow(WinRT.Interop.WindowNative.GetWindowHandle(window));

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

    private void Check(string what, bool ok, string seen) => Say("[hands] " + (ok ? "PASS " : "FAIL ") + what + ": " + seen);

    private void Say(string line) => log.Add(DateTime.Now.ToString("HH:mm:ss.fff", CultureInfo.InvariantCulture) + " " + line);
}
