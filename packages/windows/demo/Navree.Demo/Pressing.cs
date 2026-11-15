using System.Globalization;
using System.Reflection;
using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Navree.Runtime;
using Navree.Shell;
using Windows.Foundation;

namespace Navree.Demo;

/// <summary>
/// Presses what a person presses in the store demo, with the real pointer and the real keys, and says what the core was asked to
/// send each time: the order number on Orders, a quantity typed into a row of Inventory and its Restock button, and the Filter
/// button of Orders with a box ticked in what it opens. What was done, and what came of it, is written to <c>press.txt</c>.
/// </summary>
internal sealed class Pressing(ShellHost host, ShellWindow window, string folder)
{
    private const uint MoveTo = 0x0001;
    private const uint LeftDown = 0x0002;
    private const uint LeftUp = 0x0004;
    private const uint Absolute = 0x8000;
    private const uint KeyUp = 2;
    private const byte CtrlKey = 0x11;
    private const byte LetterA = 0x41;
    private const byte Digit0 = 0x30;
    private const byte Escape = 0x1B;

    private readonly List<string> log = [];

    [StructLayout(LayoutKind.Sequential)]
    private struct Point32
    {
        public int X;
        public int Y;
    }

    internal async Task RunAsync()
    {
        try
        {
            Directory.CreateDirectory(folder);
            await ReadyAsync();
            window.Size(1280, 1000);
            await SettleAsync();
            Say("== pressing, at 1280 wide");
            await RestockAsync();
            await OrderNumberAsync();
            await PutAwayAsync();
            await FilterAsync();
        }
        catch (Exception problem)
        {
            Say("the pressing failed: " + problem);
        }
        finally
        {
            Say("done");
            File.WriteAllLines(Path.Combine(folder, "press.txt"), log);
            App.Finish(null);
        }
    }

    // The order number is a button in a form: pressing it sends the form's address.
    private async Task OrderNumberAsync()
    {
        await ShowAsync(1);
        Button? number = Everything().OfType<Button>().FirstOrDefault(button => NameOf(button).StartsWith("CS-", StringComparison.Ordinal));
        if (number is null)
        {
            Check("the order number is on the screen", false, "no button is named CS-...");
            return;
        }

        int from = RecordingFetcher.Sent.Count;
        await ClickAsync(number);
        SentRequest? asked = await WaitAsync(from, request => request.Url.Contains("/peek", StringComparison.Ordinal), 6000);
        Check("pressing the order number " + NameOf(number) + " sends its form's address", asked is { Method: "get" } && asked.Url.EndsWith("/orders/200000/peek", StringComparison.Ordinal), Describe(from));
        await ShotAsync("press-order-number", "after the order number was pressed");
    }

    // What the order number opened (the peek is a sheet of the Orders screen) is closed, as a person closes it, before the next step.
    private async Task PutAwayAsync()
    {
        foreach (ScreenView screen in host.State.Screens.Values.ToList())
        {
            foreach (string overlay in screen.Overlays)
            {
                Say("closing the overlay " + overlay + " of screen " + screen.Id.ToString(CultureInfo.InvariantCulture));
                host.Send(Events.Overlay(screen.Id, overlay, false));
            }
        }

        await SettleAsync();
        await Task.Delay(800);
        Say("popups open after the peek was put away: " + VisualTreeHelper.GetOpenPopupsForXamlRoot(window.Surface.XamlRoot).Count);
    }

    // A quantity is typed into the field of a row, and the row's Restock button is pressed: the form's fields go with it.
    private async Task RestockAsync()
    {
        await ShowAsync(2);
        TextBox? quantity = Everything().OfType<TextBox>().FirstOrDefault(box => NameOf(box).StartsWith("Units of Chakla belan", StringComparison.Ordinal));
        Button? restock = quantity is null ? null : RestockBesides(quantity);
        if (quantity is null || restock is null)
        {
            Check("a row of Inventory has its quantity field and its Restock button", false, "field " + (quantity is not null) + ", button " + (restock is not null));
            return;
        }

        Say("the field held " + quantity.Text + " before it was typed into");
        await ClickAsync(quantity);
        Foreground();
        ScreenView? before = host.State.FrontScreen;
        FrameworkElement? page = window.CurrentPage;
        lastSeen = before;
        lastPage = page;
        drawn = Private(Private(window, "front"), "shown") as ScreenView;
        drawnWidth = Private(Private(window, "front"), "shownWidth");
        Say("the page was drawn for width " + drawnWidth + "; the window's width is " + Private(window, "width") + "; the drawing's context is " + (Private(Private(window, "front"), "<Context>k__BackingField") is null ? "none" : "there"));
        host.StateChanged += OnState;
        await KeyAsync(CtrlKey, LetterA);
        await Task.Delay(300);
        await KeyAsync(0, (byte)(Digit0 + 6));
        await Task.Delay(700);
        Say("after the 6: the field holds '" + quantity.Text + "', it is " + (quantity.XamlRoot is null ? "off the screen" : "still on it") + ", the page is " + (ReferenceEquals(page, window.CurrentPage) ? "the same" : "a new one") + ", focus is on " + FocusedWords());
        Compare(before, host.State.FrontScreen);
        TextBox? again = Everything().OfType<TextBox>().FirstOrDefault(box => NameOf(box).StartsWith("Units of Chakla belan", StringComparison.Ordinal));
        Say("the field is " + (ReferenceEquals(again, quantity) ? "the same element" : "a new element") + " and holds '" + again?.Text + "'");
        await KeyAsync(0, Digit0);
        await Task.Delay(700);
        host.StateChanged -= OnState;
        again = Everything().OfType<TextBox>().FirstOrDefault(box => NameOf(box).StartsWith("Units of Chakla belan", StringComparison.Ordinal));
        Check("the quantity typed is in the field", again?.Text == "60", "the field holds '" + again?.Text + "'; focus is on " + FocusedWords());
        await ShotAsync("press-restock-typed", "60 typed into the quantity of the first row");

        restock = again is null ? null : RestockBesides(again);
        if (restock is null)
        {
            Check("the Restock button is on the screen after the typing", false, "no button");
            return;
        }

        int from = RecordingFetcher.Sent.Count;
        await ClickAsync(restock);
        SentRequest? asked = await WaitAsync(from, request => request.Method == "post", 6000);
        string body = asked is null ? string.Empty : Uri.UnescapeDataString(asked.Body);
        Check(
            "pressing Restock sends the form with the quantity typed",
            asked is not null && asked.Url.EndsWith("/inventory/18/restock", StringComparison.Ordinal) && body.Contains("quantity", StringComparison.Ordinal) && body.Contains("60", StringComparison.Ordinal),
            Describe(from));
        await ShotAsync("press-restock-pressed", "after Restock was pressed");
    }

    // The Filter button opens the query's other fields; a box ticked in them asks for the rows again with the filter.
    private async Task FilterAsync()
    {
        await ShowAsync(1);
        Button? filter = Everything().OfType<Button>().FirstOrDefault(button => button.Content is string text && text == "Filter");
        if (filter is null)
        {
            Check("the Filter button is on the screen", false, "no button says Filter");
            return;
        }

        if (filter.Flyout is { } flyout)
        {
            flyout.Opened += (_, _) => Say("the flyout opened");
            flyout.Closed += (_, _) => Say("the flyout closed");
        }

        Say("overlays now: " + string.Join(",", host.State.FrontScreen?.Overlays ?? []) + "; popups open: " + VisualTreeHelper.GetOpenPopupsForXamlRoot(window.Surface.XamlRoot).Count);
        await ClickAsync(filter);
        await Task.Delay(800);
        Say("popups open: " + VisualTreeHelper.GetOpenPopupsForXamlRoot(window.Surface.XamlRoot).Count + "; focus is on " + FocusedWords());
        CheckBox? open = InPopups().OfType<CheckBox>().FirstOrDefault(box => box.Content is string text && text == "Open");
        Check("the Filter button opens a flyout with the Status choices", open is not null, open is null ? "no box says Open in an open popup" : "found");
        await ShotAsync("press-filter-open", "the flyout of the Filter button");
        if (open is null)
        {
            return;
        }

        int from = RecordingFetcher.Sent.Count;
        await ClickAsync(open);
        SentRequest? asked = await WaitAsync(from, request => Uri.UnescapeDataString(request.Url).Contains("filter[status][in]=open", StringComparison.Ordinal), 6000);
        Check("ticking Open sends the query with the filter", asked is { Method: "get" }, Describe(from));

        // The screen is drawn again when the rows come; the flyout is there again, with the box still ticked, for the next box.
        await Task.Delay(1500);
        CheckBox? again = InPopups().OfType<CheckBox>().FirstOrDefault(box => box.Content is string text && text == "Open");
        Check("the flyout is open again, with Open ticked, after the screen was drawn again", again is { IsChecked: true }, again is null ? "the flyout is gone" : "Open is " + again.IsChecked);
        await ShotAsync("press-filter-ticked", "Open ticked in the flyout");
        CheckBox? packed = InPopups().OfType<CheckBox>().FirstOrDefault(box => box.Content is string text && text == "Packed");
        if (packed is null)
        {
            return;
        }

        from = RecordingFetcher.Sent.Count;
        await ClickAsync(packed);
        asked = await WaitAsync(from, request => Uri.UnescapeDataString(request.Url).Contains("filter[fulfilment][in]=packed", StringComparison.Ordinal), 6000);
        Check("ticking Packed too asks with both filters", asked is not null && Uri.UnescapeDataString(asked.Url).Contains("filter[status][in]=open", StringComparison.Ordinal), Describe(from));
        await ShotAsync("press-filter-two", "Open and Packed ticked");
    }

    // The Restock button of the row the field is in: the first container above the field that holds one.
    private static Button? RestockBesides(DependencyObject field)
    {
        for (DependencyObject? at = VisualTreeHelper.GetParent(field); at is not null; at = VisualTreeHelper.GetParent(at))
        {
            Button? found = Below(at).OfType<Button>().FirstOrDefault(button => NameOf(button) == "Restock");
            if (found is not null)
            {
                return found;
            }
        }

        return null;
    }

    private async Task ShowAsync(int destination)
    {
        for (int step = 0; step < 6 && host.State.Nav.CanGoBack; step++)
        {
            host.Send(Events.Back);
            await SettleAsync();
        }

        host.Send(Events.Select(destination));
        await SettleAsync();
        if (host.State.FrontScreen is { } front)
        {
            foreach (string overlay in front.Overlays)
            {
                Say("closing the overlay " + overlay + " left open by the step before");
                host.Send(Events.Overlay(front.Id, overlay, false));
            }

            await SettleAsync();
        }

        await Task.Delay(1200);
        for (int waited = 0; waited < 9000 && VisualTreeHelper.GetOpenPopupsForXamlRoot(window.Surface.XamlRoot).Count > 0; waited += 100)
        {
            await Task.Delay(100);
        }

        if (VisualTreeHelper.GetOpenPopupsForXamlRoot(window.Surface.XamlRoot).Count > 0)
        {
            Foreground();
            await KeyAsync(0, Escape);
            await Task.Delay(400);
        }

        Say("popups open when the screen is ready: " + VisualTreeHelper.GetOpenPopupsForXamlRoot(window.Surface.XamlRoot).Count);
        Foreground();
    }

    private string FocusedWords() =>
        Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(window.Surface.XamlRoot) switch
        {
            null => "nothing",
            TextBox box => "a text box that holds '" + box.Text + "'",
            { } other => other.GetType().Name,
        };

    // What of a screen's state is another object after something was done: the parts a screen is drawn again for.
    private void Compare(ScreenView? before, ScreenView? after) =>
        Say("what the screen is drawn again for, changed by the typing: " + Differences(before, after));

    private static string Differences(ScreenView? before, ScreenView? after)
    {
        if (before is null || after is null)
        {
            return before is null && after is null ? "no screen" : "a screen came or went";
        }

        List<string> changed = [];
        if (!ReferenceEquals(before.Content, after.Content))
        {
            changed.Add("content");
        }

        if (!ReferenceEquals(before.States, after.States))
        {
            changed.Add("states");
        }

        if (!ReferenceEquals(before.Overlays, after.Overlays))
        {
            changed.Add("overlays");
        }

        if (!ReferenceEquals(before.Regions, after.Regions))
        {
            changed.Add("regions");
        }

        if (!ReferenceEquals(before.Lists, after.Lists))
        {
            changed.Add("lists");
        }

        if (!ReferenceEquals(before.Forms, after.Forms))
        {
            changed.Add("forms");
        }

        if (before.Loading != after.Loading || before.Refreshing != after.Refreshing)
        {
            changed.Add("loading");
        }

        if (before.Id != after.Id)
        {
            changed.Add("which screen");
        }

        return changed.Count == 0 ? "nothing" : string.Join(", ", changed);
    }

    private ScreenView? lastSeen;
    private FrameworkElement? lastPage;
    private ScreenView? drawn;
    private object? drawnWidth;
    private int updates;

    // A field of an object that is not ours to read, read for a diagnosis.
    private static object? Private(object? target, string name) =>
        target?.GetType().GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)?.GetValue(target);

    // Each update of the view state, and whether the window drew the page again for it (its own handler has run by now).
    private void OnState(ViewState state)
    {
        FrameworkElement? page = window.CurrentPage;
        Say("update " + (++updates).ToString(CultureInfo.InvariantCulture) + ": stage " + state.Nav.Stage + ", changed " + Differences(lastSeen, state.FrontScreen) + ", the page is " + (ReferenceEquals(page, lastPage) ? "the same" : "new"));
        if (drawn is { } was && state.FrontScreen is { } now && updates == 1)
        {
            Say("  against what was drawn: same screen " + (was.Id == now.Id) + ", content " + ReferenceEquals(was.Content, now.Content) + ", states " + ReferenceEquals(was.States, now.States) + ", overlays " + ReferenceEquals(was.Overlays, now.Overlays) + ", regions " + ReferenceEquals(was.Regions, now.Regions) + ", lists " + ReferenceEquals(was.Lists, now.Lists) + ", loading " + (was.Loading == now.Loading) + ", refreshing " + (was.Refreshing == now.Refreshing) + ", failure " + Equals(was.Failure, now.Failure) + ", width " + Equals(drawnWidth, Private(window, "width")));
        }

        lastSeen = state.FrontScreen;
        lastPage = page;
    }

    // ---- what is on the screen -------------------------------------------------------------------------

    private IEnumerable<DependencyObject> Everything() => Below(window.Surface);

    private static IEnumerable<DependencyObject> Below(DependencyObject root)
    {
        int count = VisualTreeHelper.GetChildrenCount(root);
        for (int at = 0; at < count; at++)
        {
            DependencyObject child = VisualTreeHelper.GetChild(root, at);
            yield return child;
            foreach (DependencyObject inner in Below(child))
            {
                yield return inner;
            }
        }
    }

    private IEnumerable<DependencyObject> InPopups()
    {
        foreach (Popup popup in VisualTreeHelper.GetOpenPopupsForXamlRoot(window.Surface.XamlRoot))
        {
            if (popup.Child is { } child)
            {
                yield return child;
                foreach (DependencyObject inner in Below(child))
                {
                    yield return inner;
                }
            }
        }
    }

    private static string NameOf(DependencyObject element) => AutomationProperties.GetName(element) ?? string.Empty;

    // ---- the pointer and the keys ----------------------------------------------------------------------

    [DllImport("user32.dll")]
    private static extern int GetSystemMetrics(int index);

    [DllImport("user32.dll")]
    private static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);

    [DllImport("user32.dll")]
    private static extern bool ClientToScreen(IntPtr handle, ref Point32 point);

    [DllImport("user32.dll")]
    private static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr handle);

    private void Foreground() => SetForegroundWindow(WinRT.Interop.WindowNative.GetWindowHandle(window));

    // The middle of an element, on the screen, as the pointer is moved there: an absolute move over the whole screen.
    private async Task ClickAsync(FrameworkElement element)
    {
        Foreground();
        Point inWindow = element.TransformToVisual(null).TransformPoint(new Point(element.ActualWidth / 2, element.ActualHeight / 2));
        double scale = element.XamlRoot.RasterizationScale;
        Point32 origin = default;
        ClientToScreen(WinRT.Interop.WindowNative.GetWindowHandle(window), ref origin);
        int x = origin.X + (int)Math.Round(inWindow.X * scale);
        int y = origin.Y + (int)Math.Round(inWindow.Y * scale);
        int width = Math.Max(2, GetSystemMetrics(0));
        int height = Math.Max(2, GetSystemMetrics(1));
        mouse_event(MoveTo | Absolute, (int)((long)x * 65535 / (width - 1)), (int)((long)y * 65535 / (height - 1)), 0, UIntPtr.Zero);
        await Task.Delay(250);
        mouse_event(LeftDown, 0, 0, 0, UIntPtr.Zero);
        await Task.Delay(80);
        mouse_event(LeftUp, 0, 0, 0, UIntPtr.Zero);
        await Task.Delay(500);
        Say("pressed " + element.GetType().Name + " '" + NameOf(element) + "' at " + x.ToString(CultureInfo.InvariantCulture) + "," + y.ToString(CultureInfo.InvariantCulture));
    }

    private static async Task KeyAsync(byte modifier, byte key)
    {
        if (modifier != 0)
        {
            keybd_event(modifier, 0, 0, UIntPtr.Zero);
        }

        await Task.Delay(40);
        keybd_event(key, 0, 0, UIntPtr.Zero);
        await Task.Delay(40);
        keybd_event(key, 0, KeyUp, UIntPtr.Zero);
        await Task.Delay(40);
        if (modifier != 0)
        {
            keybd_event(modifier, 0, KeyUp, UIntPtr.Zero);
        }
    }

    // ---- what the core was asked ------------------------------------------------------------------------

    private static async Task<SentRequest?> WaitAsync(int from, Func<SentRequest, bool> match, int milliseconds)
    {
        for (int waited = 0; waited < milliseconds; waited += 100)
        {
            SentRequest? found = RecordingFetcher.Sent.Skip(from).FirstOrDefault(match);
            if (found is not null)
            {
                return found;
            }

            await Task.Delay(100);
        }

        return null;
    }

    private static string Describe(int from)
    {
        IReadOnlyList<SentRequest> asked = [.. RecordingFetcher.Sent.Skip(from)];
        return asked.Count == 0
            ? "the core asked for nothing"
            : "the core asked: " + string.Join("; ", asked.Select(request => request.Method.ToUpperInvariant() + " " + Uri.UnescapeDataString(request.Url) + (request.Body.Length == 0 ? string.Empty : " body [" + Uri.UnescapeDataString(request.Body) + "]")));
    }

    private void Check(string what, bool ok, string detail) => Say("[press] " + (ok ? "PASS" : "FAIL") + " " + what + ": " + detail);

    // ---- the app --------------------------------------------------------------------------------------------

    private async Task ReadyAsync()
    {
        for (int round = 0; round < 60; round++)
        {
            ViewState state = await host.SettledAsync();
            if (state.Nav.Stage == Stage.Ready)
            {
                return;
            }

            await Task.Delay(250);
        }

        Say("the app did not get ready");
    }

    private async Task SettleAsync()
    {
        int calm = 0;
        for (int round = 0; round < 60 && calm < 2; round++)
        {
            ViewState state = await host.SettledAsync();
            await Task.Delay(250);
            bool busy = state.Nav.Stage != Stage.Ready || state.Screens.Values.Any(screen => screen.Loading || screen.Refreshing);
            calm = busy ? 0 : calm + 1;
        }
    }

    // A picture is asked for by a file, as the gallery asks: whoever runs the demo takes it and answers with another.
    private async Task ShotAsync(string id, string details)
    {
        File.WriteAllText(Path.Combine(folder, id + ".ready"), details);
        string taken = Path.Combine(folder, id + ".taken");
        for (int round = 0; round < 200 && !File.Exists(taken); round++)
        {
            await Task.Delay(100);
        }
    }

    private void Say(string line) => log.Add(DateTime.Now.ToString("HH:mm:ss.fff", CultureInfo.InvariantCulture) + " " + line);
}
