using System.Globalization;
using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;
using Navree.Shell;
using Windows.Foundation;

namespace Navree.Demo;

/// <summary>
/// Opens a conversation of the live inbox and presses, with the real pointer, the buttons that open its popovers: the popover must come up as a
/// flyout beside the button that opened it (not as a dialog in the middle), Escape must take it away, the core must be told, and the focus must be
/// back on the button. What was seen is written to <c>popover.txt</c>.
/// </summary>
internal sealed class PopoverCheck(ShellHost host, ShellWindow window, string folder)
{
    private const uint MoveTo = 0x0001;
    private const uint LeftDown = 0x0002;
    private const uint LeftUp = 0x0004;
    private const uint Absolute = 0x8000;
    private const uint KeyUp = 2;
    private const byte Escape = 0x1B;

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

            window.Size(1000, 900);
            await Task.Delay(1500);
            if (host.State.FrontScreen?.Screen is Models.Screen list
                && Tree.Nodes(list).Select(node => node.GetType().GetProperty("Action")?.GetValue(node)).OfType<Models.ActionNavigate>().FirstOrDefault(go => go.Href.StartsWith("/inbox/", StringComparison.Ordinal)) is { } open)
            {
                host.Send(Events.Act(host.State.Nav.Front, open));
            }

            await Task.Delay(4000);
            foreach (string label in new[] { "Snooze", "Saved replies" })
            {
                await OneAsync(label);
            }
        }
        catch (Exception problem)
        {
            Say("the check failed: " + problem);
        }
        finally
        {
            Say("done");
            File.WriteAllLines(Path.Combine(folder, "popover.txt"), log);
            App.Finish(null);
        }
    }

    private async Task OneAsync(string label)
    {
        Say("== " + label);
        Button? button = Below(window.Surface).OfType<Button>().FirstOrDefault(candidate => AutomationProperties.GetName(candidate) == label || candidate.Content as string == label);
        Check("the button " + label + " is on the screen", button is not null, button is null ? "not found" : "found");
        if (button is null)
        {
            return;
        }

        await ClickAsync(button);
        await Task.Delay(900);
        Popup[] open = [.. VisualTreeHelper.GetOpenPopupsForXamlRoot(window.Surface.XamlRoot)];
        bool dialog = open.Any(popup => popup.Child is not null && Below(popup.Child).OfType<ContentDialog>().Any());
        FlyoutPresenter? presenter = open.SelectMany(popup => popup.Child is null ? [] : Below(popup.Child)).OfType<FlyoutPresenter>().FirstOrDefault();
        Check(label + " opens as a flyout, not as a dialog", presenter is not null && !dialog, "flyout " + (presenter is not null) + ", dialog " + dialog + ", overlays " + string.Join(",", host.State.FrontScreen?.Overlays ?? []));
        if (presenter is not null)
        {
            // The two rectangles in one space, the button's own, so that no window, popup or scale difference can come between them: the flyout's
            // is the presenter's box carried into the button's space, and the gap is the distance between their nearest edges (none when they touch).
            double gap;
            string said;

            // The page is drawn again when the flyout opens, so the button that was pressed may be a control that has left the window: the
            // button that stands there now, by the same name, is the one the flyout is measured against.
            Button? standing = Below(window.Surface).OfType<Button>().FirstOrDefault(candidate => candidate.ActualWidth > 0 && (AutomationProperties.GetName(candidate) == label || candidate.Content as string == label));
            button = standing ?? button;
            try
            {
                Rect flyout = presenter.TransformToVisual(button).TransformBounds(new Rect(0, 0, presenter.ActualWidth, presenter.ActualHeight));
                Rect own = new(0, 0, button.ActualWidth, button.ActualHeight);
                double across = Math.Max(0, Math.Max(flyout.Left - own.Right, own.Left - flyout.Right));
                double down = Math.Max(0, Math.Max(flyout.Top - own.Bottom, own.Top - flyout.Bottom));
                gap = Math.Sqrt((across * across) + (down * down));
                Rect inWindowFlyout = presenter.TransformToVisual(null).TransformBounds(new Rect(0, 0, presenter.ActualWidth, presenter.ActualHeight));
                Rect inWindowButton = button.TransformToVisual(null).TransformBounds(own);
                said = "the flyout " + Box(flyout) + " and the button " + Box(own) + " in the button's space; in the window's space " + Box(inWindowFlyout) + " and " + Box(inWindowButton) + "; scale " + button.XamlRoot.RasterizationScale.ToString("0.00", CultureInfo.InvariantCulture) + "; the nearest edges are " + gap.ToString("0", CultureInfo.InvariantCulture) + " px apart";
            }
            catch (Exception problem)
            {
                gap = double.MaxValue;
                said = "the two could not be put in one space: " + problem.GetType().Name;
            }

            Check(label + " stands beside the button that opened it", gap < 40, said);
        }

        await ShotAsync("popover-" + label.Replace(' ', '-'));
        SetForegroundWindow(WinRT.Interop.WindowNative.GetWindowHandle(window));
        keybd_event(Escape, 0, 0, UIntPtr.Zero);
        keybd_event(Escape, 0, KeyUp, UIntPtr.Zero);
        await Task.Delay(900);
        bool closed = !VisualTreeHelper.GetOpenPopupsForXamlRoot(window.Surface.XamlRoot).Any(popup => popup.Child is not null && Below(popup.Child).Any(part => part is FlyoutPresenter or ContentDialog));
        Check("Escape takes " + label + " away", closed, closed ? "no flyout and no dialog is open" : "a flyout or a dialog is still open");
        Check("the core was told it is closed", !(host.State.FrontScreen?.Overlays.Count > 0), "overlays now: " + string.Join(",", host.State.FrontScreen?.Overlays ?? []));
        object? focused = FocusManager.GetFocusedElement(window.Surface.XamlRoot);
        Check("the focus is back on the button", ReferenceEquals(focused, button) || (focused is FrameworkElement element && AutomationProperties.GetName(element) == label), "focus is on " + (focused?.GetType().Name ?? "nothing"));
    }

    private static string Box(Rect box) =>
        string.Create(CultureInfo.InvariantCulture, $"[{box.Left:0},{box.Top:0} {box.Width:0}x{box.Height:0}]");

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

    private async Task ClickAsync(FrameworkElement element)
    {
        SetForegroundWindow(WinRT.Interop.WindowNative.GetWindowHandle(window));
        Point inWindow = element.TransformToVisual(null).TransformPoint(new Point(element.ActualWidth / 2, element.ActualHeight / 2));
        double scale = element.XamlRoot.RasterizationScale;
        Spot origin = default;
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

    private void Check(string what, bool ok, string seen) => Say("[popover] " + (ok ? "PASS " : "FAIL ") + what + ": " + seen);

    private void Say(string line) => log.Add(DateTime.Now.ToString("HH:mm:ss.fff", CultureInfo.InvariantCulture) + " " + line);
}
