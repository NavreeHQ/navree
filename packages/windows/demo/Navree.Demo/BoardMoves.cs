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
/// Moves a card of the live board three ways with the real keys and the real pointer, and says where the card stood each time, as the core
/// holds it: by its handle's keys (Alt, Shift and an arrow), by the handle's menu (a real click on its item), and by dragging it with the pointer
/// into another lane. The board is then asked again, so what the server kept is told too. What was done is written to <c>moves.txt</c>.
/// </summary>
internal sealed partial class BoardMoves(ShellHost host, ShellWindow window, string folder)
{
    private const uint MoveTo = 0x0001;
    private const uint LeftDown = 0x0002;
    private const uint LeftUp = 0x0004;
    private const uint Absolute = 0x8000;
    private const uint KeyUp = 2;
    private const byte AltKey = 0x12;
    private const byte ShiftKey = 0x10;
    private const byte Down = 0x28;
    private const byte Right = 0x27;

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
            await SettleAsync();
            await Task.Delay(1500);
            await KeysAsync();
            await MenuAsync();
            await DragAsync();
            await PersistAsync();
            await SheetsAsync();
        }
        catch (Exception problem)
        {
            Say("the moves failed: " + problem);
        }
        finally
        {
            Say("done");
            File.WriteAllLines(Path.Combine(folder, "moves.txt"), log);
            App.Finish(null);
        }
    }

    // ---- the three ways ---------------------------------------------------------------------------------------

    private async Task KeysAsync()
    {
        Say("== by the keys of the handle");
        (string Lane, int Index, string Id, string Title)? card = Pick("backlog", 0);
        if (card is not { } first)
        {
            Check("a card is in the first lane", false, "none found");
            return;
        }

        Button? handle = HandleOf(first.Title);
        Check("the card has its handle", handle is not null, "the handle named for " + first.Title);
        if (handle is null)
        {
            return;
        }

        Foreground();
        _ = handle.Focus(FocusState.Keyboard);
        await Task.Delay(500);
        await ChordAsync(Down);
        (string Lane, int Index)? now = await WaitAsync(first.Id, first.Lane, 1);
        Say("after Down the focus is on " + Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(window.Surface.XamlRoot)?.GetType().Name + " '" + (Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(window.Surface.XamlRoot) is DependencyObject focused ? AutomationProperties.GetName(focused) : string.Empty) + "'");
        Check("Alt, Shift and Down moved the card one place down", now is { Lane: "backlog", Index: 1 }, Said(first.Id));
        await ShotAsync("moves-keys-down");

        // The screen was drawn again for the move: the handle has the focus back, so the keys go on moving the same card.
        await Task.Delay(800);
        await ChordAsync(Right);
        (string Lane, int Index)? across = await WaitAsync(first.Id, "ready", null);
        Check("Alt, Shift and Right moved the card into the next lane", across is { Lane: "ready" }, Said(first.Id));
        await ShotAsync("moves-keys-right");
    }

    private async Task MenuAsync()
    {
        Say("== by the menu of the handle");
        (string Lane, int Index, string Id, string Title)? card = Pick("ready", 0);
        if (card is not { } picked)
        {
            Check("a card is in the second lane", false, "none found");
            return;
        }

        Button? handle = HandleOf(picked.Title);
        if (handle is null)
        {
            Check("the card has its handle", false, "none");
            return;
        }

        await ClickAsync(handle);
        await Task.Delay(600);
        MenuFlyoutItem? item = Popups().SelectMany(Below).OfType<MenuFlyoutItem>().FirstOrDefault(entry => entry.Text == "Move to Backlog");
        Check("the handle's menu offers Move to Backlog", item is not null && item.IsEnabled, item is null ? "no such item; items: " + string.Join(", ", Popups().SelectMany(Below).OfType<MenuFlyoutItem>().Select(entry => entry.Text)) : "enabled " + item.IsEnabled);
        await ShotAsync("moves-menu");
        if (item is null)
        {
            return;
        }

        await ClickAsync(item);
        (string Lane, int Index)? now = await WaitAsync(picked.Id, "backlog", null);
        Check("choosing Move to Backlog moved the card there", now is { Lane: "backlog" }, Said(picked.Id));
    }

    private async Task DragAsync()
    {
        Say("== by dragging");
        (string Lane, int Index, string Id, string Title)? card = Pick("backlog", 2);
        if (card is not { } picked)
        {
            Check("a card is in the first lane to drag", false, "none found");
            return;
        }

        // The steps before left the page scrolled to the card they moved, so the lane it is dragged to may be above the window: the page is
        // taken back to its top, and the positions are read after that.
        Button? beforeScroll = HandleOf(picked.Title);
        ScrollViewer? page = null;
        for (DependencyObject? at = beforeScroll is null ? null : VisualTreeHelper.GetParent(beforeScroll); at is not null; at = VisualTreeHelper.GetParent(at))
        {
            if (at is ScrollViewer found)
            {
                page = found;
            }
        }

        if (page is not null)
        {
            _ = page.ChangeView(null, 0, null, true);
            await Task.Delay(800);
            await SettleAsync();
        }

        Button? handle = HandleOf(picked.Title);
        FrameworkElement? cardElement = handle is null ? null : Up(handle);
        (string Lane, int Index, string Id, string Title)? target = Pick("review", 0);
        Button? targetHandle = target is { } there ? HandleOf(there.Title) : null;
        FrameworkElement? targetCard = targetHandle is null ? null : Up(targetHandle);
        if (cardElement is null || targetCard is null)
        {
            Check("the card and the lane it is dragged to are on the screen", false, "card " + (cardElement is not null) + ", target " + (targetCard is not null));
            return;
        }

        // Pressed at the card's lower left, where the words and the buttons are not, and let go on the middle of a card of another lane.
        (int fromX, int fromY) = ScreenOf(cardElement, 12, cardElement.ActualHeight - 6);
        (int toX, int toY) = ScreenOf(targetCard, targetCard.ActualWidth / 2, targetCard.ActualHeight * 0.75);
        Foreground();
        Absolute1(fromX, fromY);
        await Task.Delay(300);
        mouse_event(LeftDown, 0, 0, 0, UIntPtr.Zero);
        await Task.Delay(200);
        for (int step = 1; step <= 20; step++)
        {
            Absolute1(fromX + ((toX - fromX) * step / 20), fromY + ((toY - fromY) * step / 20));
            await Task.Delay(40);
        }

        await Task.Delay(400);
        await ShotAsync("moves-drag-over");
        mouse_event(LeftUp, 0, 0, 0, UIntPtr.Zero);
        (string Lane, int Index)? now = await WaitAsync(picked.Id, "review", null);
        Check("dragging the card into the third lane moved it there", now is { Lane: "review" }, Said(picked.Id));
        await ShotAsync("moves-drag-dropped");
    }

    // What the server kept: the board is asked for again, and the card is looked for.
    private async Task PersistAsync()
    {
        Say("== what the server kept");
        if (host.State.Nav.Front is long front)
        {
            host.Send(Events.Refresh(front));
        }

        await Task.Delay(1500);
        await SettleAsync();
        foreach (string id in moved)
        {
            Say("the card " + id + " stands " + Said(id) + " after the board was asked for again");
        }
    }

    private readonly List<string> moved = [];

    // ---- what the core holds ----------------------------------------------------------------------------------

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
        string id = row.Id ?? row.Key ?? row.Title;
        if (!moved.Contains(id))
        {
            moved.Add(id);
        }

        return (lane, index, id, row.Title);
    }

    private (string Lane, int Index)? Where(string id)
    {
        if (host.State.FrontScreen?.Screen is not Models.Screen tree)
        {
            return null;
        }

        foreach (Models.List list in Tree.Nodes(tree).OfType<Models.List>().Where(candidate => candidate.Reorder is not null))
        {
            List<Models.Row> rows = [.. list.Children.OfType<Models.Row>()];
            int at = rows.FindIndex(row => (row.Id ?? row.Key ?? row.Title) == id);
            if (at >= 0)
            {
                return (list.Reorder!.Key ?? list.Id ?? string.Empty, at);
            }
        }

        return null;
    }

    private string Said(string id) => Where(id) is { } there ? "in " + there.Lane + " at place " + there.Index.ToString(CultureInfo.InvariantCulture) : "nowhere in the lists held";

    private async Task<(string Lane, int Index)?> WaitAsync(string id, string lane, int? index)
    {
        for (int waited = 0; waited < 6000; waited += 200)
        {
            await Task.Delay(200);
            if (Where(id) is { } there && there.Lane == lane && (index is null || there.Index == index))
            {
                return there;
            }
        }

        return Where(id);
    }

    // ---- what is on the screen --------------------------------------------------------------------------------

    private Button? HandleOf(string title) =>
        Below(window.Surface).OfType<Button>().FirstOrDefault(button => AutomationProperties.GetName(button) == "Move " + title);

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

    // ---- real input -------------------------------------------------------------------------------------------

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

    private void Check(string what, bool ok, string seen) => Say("[moves] " + (ok ? "PASS " : "FAIL ") + what + ": " + seen);

    private void Say(string line) => log.Add(DateTime.Now.ToString("HH:mm:ss.fff", CultureInfo.InvariantCulture) + " " + line);
}
