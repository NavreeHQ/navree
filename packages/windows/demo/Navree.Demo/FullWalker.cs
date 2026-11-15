using System.Globalization;
using System.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Navree.Design;
using Navree.Presence;
using Navree.Protocol;
using Navree.Runtime;
using Navree.Shell;

namespace Navree.Demo;

/// <summary>
/// Walks one app through every destination, every screen a link or a row pushes, every sheet, dialog and popover, its settings and its
/// search, at four widths, and pictures each as the screen shows it (the window with its frame, and what is open above it). Beside each
/// picture it writes what assistive technology is given (every control's name and kind) and what looks wrong without looking: a control
/// with no name, a small target, a text that is cut, a part that runs past the window's edge. What was done is written to the log.
/// </summary>
internal sealed class FullWalker(ShellHost host, ShellWindow window, string folder, string app, SystemPlace? place)
{
    private static readonly (string Name, int Width)[] Widths = [("w480", 480), ("w840", 840), ("w1280", 1280), ("w1800", 1800)];
    private readonly List<string> log = [];
    private readonly List<string> faults = [];
    private string prefix = string.Empty;
    private string width = string.Empty;

    internal async Task RunAsync()
    {
        prefix = Prefix();
        try
        {
            Directory.CreateDirectory(folder);
            await ReadyAsync();
            Say("stage " + host.State.Nav.Stage + "; destinations: " + string.Join(" | ", host.State.Nav.Destinations.Select(d => d.Label + " " + d.Href)));
            Say("signed in as: " + (host.State.App?.User?.Name ?? "nobody"));
            foreach ((string name, int px) in Widths)
            {
                // The very wide window is pictured in the light look only.
                if (px > 1280 && prefix != "light-")
                {
                    continue;
                }

                width = name;
                window.Size(px, 980);
                await Settle();
                await Home();
                Say("== " + name + ": navigation destinations " + host.State.Nav.Destinations.Count);
                for (int destination = 0; destination < host.State.Nav.Destinations.Count; destination++)
                {
                    await Destination(destination);
                }

                await Settings();
            }
        }
        catch (Exception problem)
        {
            Say("the walk failed: " + problem);
        }
        finally
        {
            Say("done");
            File.WriteAllLines(Path.Combine(folder, prefix + app + "-walk.txt"), log);
            File.WriteAllLines(Path.Combine(folder, prefix + app + "-faults.txt"), faults);
            App.Finish(place);
        }
    }

    private async Task Destination(int index)
    {
        await Home();
        host.Send(Events.Select(index));
        await Settle();
        string label = "d" + index.ToString(CultureInfo.InvariantCulture);
        await Screen(label, 0);

        ScreenView? front = host.State.FrontScreen;
        if (front is null || front.Screen is not Models.Screen tree)
        {
            return;
        }

        string here = host.State.Nav.Destinations.ElementAtOrDefault(index)?.Href ?? string.Empty;
        List<string> hrefs = [.. Hrefs(tree).Where(href => href != here && href != "/").Distinct().Take(2)];
        Say(label + ": " + hrefs.Count + " links to push: " + string.Join(" ", hrefs));
        int push = 0;
        foreach (string href in hrefs)
        {
            push++;
            if (host.State.FrontScreen is not ScreenView current)
            {
                break;
            }

            host.Send(Events.Act(current.Id, new Models.ActionNavigate { Href = href }));
            await Settle();
            if (host.State.FrontScreen?.Id == current.Id)
            {
                Say(label + " " + href + ": nothing was pushed (the front screen is the same)");
                continue;
            }

            await Screen(label + "-p" + push.ToString(CultureInfo.InvariantCulture), 1);
            await Home();
            host.Send(Events.Select(index));
            await Settle();
        }
    }

    // One screen as it stands: its picture, its overlays one at a time, its search.
    private async Task Screen(string name, int depth)
    {
        await Shot(name);
        ScreenView? front = host.State.FrontScreen;
        if (front is null || front.Screen is not Models.Screen tree)
        {
            return;
        }

        // What a sheet, dialog or popover shows when it is opened by the core's own event is the first thing it holds; the ones that load their
        // content when a link opens them are walked by the checks. They are pictured at the middle width only.
        foreach (Models.Node node in Tree.Nodes(tree).Where(node => width == "w840" && node is Models.Sheet or Models.Dialog or Models.Popover).Take(3))
        {
            string? id = Tree.IdOf(node);
            if (id is null)
            {
                continue;
            }

            host.Send(Events.Overlay(front.Id, id, true));
            await Settle();
            await Task.Delay(500);
            Say(name + ": overlay " + id + " is open; over the window: " + (window.OverTheWindow ?? "nothing"));
            await Shot(name + "-over-" + Safe(id));
            host.Send(Events.Overlay(front.Id, id, false));
            await Settle();
            await Task.Delay(300);
        }

        if (tree.Search is not null && depth == 0 && width == "w840")
        {
            host.Send(Events.Search(front.Id, "a"));
            await Settle();
            await Shot(name + "-search");
            host.Send(Events.Search(front.Id, string.Empty));
            await Settle();
        }
    }

    private async Task Settings()
    {
        await Home();
        window.ShowSettings();
        await Task.Delay(800);
        await Shot("settings");
        window.HideSettings();
        await Task.Delay(300);
    }

    // The window and what is over it, from the screen; beside it, what assistive technology is given and what looks wrong.
    private async Task Shot(string name)
    {
        string file = Path.Combine(folder, prefix + app + "-" + width + "-" + name + ".png");
        try
        {
            window.Activate();
            await Task.Delay(400);
            await ScreenPicture.SaveAsync(file, WindowBox());
            Say("wrote " + Path.GetFileName(file));
        }
        catch (Exception problem)
        {
            Say("no picture " + Path.GetFileName(file) + ": " + problem.Message);
        }

        try
        {
            string audit = Audit(name);
            if (width == "w840" || width == "w480")
            {
                File.WriteAllText(Path.Combine(folder, prefix + app + "-" + width + "-" + name + "-audit.txt"), audit);
            }
        }
        catch (Exception problem)
        {
            Say("no audit of " + name + ": " + problem.Message);
        }

        if (prefix == "light-" && width == "w840")
        {
            await Inspect(Path.Combine(folder, prefix + app + "-" + width + "-" + name + "-ui.txt"));
            if (host.State.FrontScreen?.Screen is Models.Screen outlined)
            {
                File.WriteAllText(Path.Combine(folder, prefix + app + "-" + width + "-" + name + "-nodes.txt"), Outline(outlined));
            }

            try
            {
                if (window.CurrentPage is Microsoft.UI.Xaml.Controls.Panel page && host.State.FrontScreen?.Content is NativeContent)
                {
                    await WindowPicture.SaveAsync(page, Path.Combine(folder, prefix + app + "-" + width + "-" + name + "-page.png"));
                }
            }
            catch (Exception problem)
            {
                Say("no whole-page picture of " + name + ": " + problem.Message);
            }
        }
    }

    private Box WindowBox()
    {
        Windows.Graphics.PointInt32 at = window.AppWindow.Position;
        Windows.Graphics.SizeInt32 size = window.AppWindow.Size;
        Box screen = ScreenCapture.Screen();
        // The window's rectangle holds an invisible border the system keeps for resizing (seven pixels at 96 DPI at the sides and below): left out, so
        // the picture is of the window and not of what is behind it.
        double scale = window.Surface.XamlRoot?.RasterizationScale ?? 1.0;
        int border = (int)Math.Round(7 * scale);
        int left = Math.Max(screen.Left, at.X + border);
        int top = Math.Max(screen.Top, at.Y);
        int right = Math.Min(screen.Right, at.X + size.Width - border);
        int bottom = Math.Min(screen.Bottom, at.Y + size.Height - border);
        return new Box(left, top, right, bottom);
    }

    // ---- what assistive technology is given, and what looks wrong ---------------------------------------

    private string Audit(string name)
    {
        StringBuilder text = new();
        FrameworkElement root = window.Surface;
        int nameless = 0;
        int small = 0;
        int trimmed = 0;
        int beyond = 0;
        double rootWidth = root.ActualWidth;
        List<UIElement> roots = [root];
        if (root.XamlRoot is not null)
        {
            foreach (Popup popup in VisualTreeHelper.GetOpenPopupsForXamlRoot(root.XamlRoot))
            {
                if (popup.Child is not null)
                {
                    roots.Add(popup.Child);
                }
            }
        }

        foreach (UIElement start in roots)
        {
            Visit(start, 0);
        }

        string summary = name + " at " + width + ": " + nameless + " controls with no name, " + small + " targets under 24 px, " + trimmed + " texts cut, " + beyond + " parts past the window's edge";
        Say(summary);
        if (nameless + small + trimmed + beyond > 0)
        {
            faults.Add(summary);
        }

        return text.ToString();

        void Visit(DependencyObject at, int depth)
        {
            if (at is FrameworkElement element && element.Visibility == Visibility.Visible && element.ActualWidth > 0 && element.ActualHeight > 0)
            {
                bool control = element is ButtonBase or TextBox or PasswordBox or ComboBox or Slider or ToggleSwitch or ListViewItem or NavigationViewItem or AutoSuggestBox or CalendarDatePicker or DatePicker or HyperlinkButton or SelectorBarItem or MenuFlyoutItem;
                if (control)
                {
                    string words = Words(element);
                    Windows.Foundation.Rect box = Place(element, root);
                    text.AppendLine(new string(' ', Math.Min(depth, 12)) + element.GetType().Name + " \"" + words + "\" " + Fmt(box));
                    if (words.Length == 0)
                    {
                        nameless++;
                        text.AppendLine("    ^ NO NAME");
                    }

                    if ((element is ButtonBase or ComboBox or ToggleSwitch or ListViewItem) && (element.ActualWidth < 24 || element.ActualHeight < 24))
                    {
                        small++;
                        text.AppendLine("    ^ SMALL TARGET " + Fmt(box));
                    }

                    if (rootWidth > 0 && (box.Right > rootWidth + 1 || box.Left < -1))
                    {
                        beyond++;
                        text.AppendLine("    ^ PAST THE EDGE " + Fmt(box));
                    }
                }
                else if (element is TextBlock block && block.IsTextTrimmed)
                {
                    trimmed++;
                    text.AppendLine(new string(' ', Math.Min(depth, 12)) + "TextBlock CUT \"" + Truncate(block.Text) + "\" " + Fmt(Place(element, root)));
                }
            }

            int count = VisualTreeHelper.GetChildrenCount(at);
            for (int child = 0; child < count; child++)
            {
                Visit(VisualTreeHelper.GetChild(at, child), depth + 1);
            }
        }
    }

    private static string Words(FrameworkElement element)
    {
        string named = AutomationProperties.GetName(element);
        if (!string.IsNullOrEmpty(named))
        {
            return named;
        }

        try
        {
            AutomationPeer? peer = FrameworkElementAutomationPeer.FromElement(element);
            string? fromPeer = peer?.GetName();
            if (!string.IsNullOrEmpty(fromPeer))
            {
                return fromPeer;
            }
        }
        catch (Exception)
        {
            // A peer that cannot be asked has no name to read.
        }

        return string.Empty;
    }

    private static Windows.Foundation.Rect Place(FrameworkElement element, FrameworkElement root)
    {
        try
        {
            Windows.Foundation.Point at = element.TransformToVisual(root).TransformPoint(new Windows.Foundation.Point(0, 0));
            return new Windows.Foundation.Rect(at.X, at.Y, element.ActualWidth, element.ActualHeight);
        }
        catch (Exception)
        {
            return new Windows.Foundation.Rect(0, 0, element.ActualWidth, element.ActualHeight);
        }
    }

    private static string Fmt(Windows.Foundation.Rect box) =>
        string.Create(CultureInfo.InvariantCulture, $"[{box.Left:0},{box.Top:0} {box.Width:0}x{box.Height:0}]");

    private static string Truncate(string text) => text.Length > 60 ? text[..60] + "..." : text;

    // What the system's own tool reads from outside the app.
    private async Task Inspect(string file)
    {
        try
        {
            System.Diagnostics.ProcessStartInfo info = new("winapp", "ui inspect -a " + Environment.ProcessId.ToString(CultureInfo.InvariantCulture) + " --depth 14")
            {
                RedirectStandardOutput = true,
                UseShellExecute = false,
                CreateNoWindow = true,
            };
            using System.Diagnostics.Process? tool = System.Diagnostics.Process.Start(info);
            if (tool is null)
            {
                return;
            }

            string output = await tool.StandardOutput.ReadToEndAsync();
            await tool.WaitForExitAsync();
            File.WriteAllText(file, System.Text.RegularExpressions.Regex.Replace(output, "\u001b\\[[0-9;]*m", string.Empty));
        }
        catch (Exception problem)
        {
            Say("no accessibility tree: " + problem.Message);
        }
    }

    // ---- reading the screen ------------------------------------------------------------------------------

    private static IEnumerable<string> Hrefs(Models.Screen tree)
    {
        foreach (Models.Node node in Tree.Nodes(tree))
        {
            if (node.GetType().GetProperty("Action")?.GetValue(node) is Models.ActionNavigate go && go.Href.StartsWith('/') && !go.Href.StartsWith("//", StringComparison.Ordinal))
            {
                yield return go.Href;
            }
        }
    }

    // What the protocol's tree holds that a person can use: each part with its id, its words and what it does.
    private static string Outline(Models.Screen tree)
    {
        StringBuilder text = new();
        text.AppendLine("screen " + tree.GetType().Name + (tree.Search is null ? string.Empty : " with a search"));
        foreach (Models.Node node in Tree.Nodes(tree))
        {
            string kind = node.GetType().Name;
            if (kind is "Stack" or "Grid" or "Divider" or "Spacer" or "Icon" or "NodeUnknown")
            {
                continue;
            }

            StringBuilder line = new();
            line.Append(kind);
            foreach (string name in new[] { "Id", "Name", "Label", "Title", "Text", "Kind", "Href" })
            {
                if (node.GetType().GetProperty(name)?.GetValue(node) is { } value && value.ToString() is { Length: > 0 } said)
                {
                    line.Append(' ').Append(name).Append('=').Append(said.Length > 48 ? said[..48] + "..." : said);
                }
            }

            if (node.GetType().GetProperty("Action")?.GetValue(node) is Models.Action action)
            {
                line.Append(" action=").Append(action.GetType().Name);
                if (action is Models.ActionNavigate go)
                {
                    line.Append(' ').Append(go.Href);
                }
            }

            if (node.GetType().GetProperty("Ask")?.GetValue(node) is not null)
            {
                line.Append(" asks");
            }

            text.AppendLine(line.ToString());
        }

        return text.ToString();
    }

    private static string Safe(string id)
    {
        StringBuilder text = new();
        foreach (char letter in id)
        {
            text.Append(char.IsAsciiLetterOrDigit(letter) ? letter : '-');
        }

        return text.Length > 24 ? text.ToString(0, 24) : text.ToString();
    }

    private static string Prefix()
    {
        string[] arguments = Environment.GetCommandLineArgs();
        string theme = Array.IndexOf(arguments, "--dark") >= 0 ? "dark" : "light";
        return theme + (Array.IndexOf(arguments, "--rtl") >= 0 ? "-rtl" : string.Empty) + "-";
    }

    private async Task ReadyAsync()
    {
        for (int round = 0; round < 120; round++)
        {
            ViewState state = await host.SettledAsync();
            if (state.Nav.Stage == Stage.Ready)
            {
                await Task.Delay(1500);
                return;
            }

            await Task.Delay(250);
        }

        Say("the app did not get ready: " + host.State.Nav.Stage + " " + host.State.Nav.Failure);
        await Reach();
    }

    // What a plain request from this process to the server says, so a failure to get ready can be told apart from a refusal by the client.
    private async Task Reach()
    {
        string[] arguments = Environment.GetCommandLineArgs();
        int at = Array.IndexOf(arguments, "--server");
        if (at < 0 || at + 1 >= arguments.Length)
        {
            return;
        }

        try
        {
            using HttpClient client = new() { Timeout = TimeSpan.FromSeconds(10) };
            using HttpResponseMessage reply = await client.GetAsync(arguments[at + 1] + "/_navree/native/shell");
            Say("a plain request to " + arguments[at + 1] + " answered " + (int)reply.StatusCode);
        }
        catch (Exception problem)
        {
            Say("a plain request to " + arguments[at + 1] + " failed: " + problem.GetType().Name + ": " + problem.Message + " / " + problem.InnerException?.Message);
        }
    }

    // The core has nothing left to do and nothing is loading: two calm moments in a row.
    private async Task Settle()
    {
        int calm = 0;
        for (int round = 0; round < 80 && calm < 2; round++)
        {
            ViewState state = await host.SettledAsync();
            await Task.Delay(250);
            bool busy = state.Nav.Stage != Stage.Ready || state.Screens.Values.Any(screen => screen.Loading || screen.Refreshing);
            calm = busy ? 0 : calm + 1;
        }
    }

    // Back to the first screen of each destination.
    private async Task Home()
    {
        for (int step = 0; step < 8 && host.State.Nav.CanGoBack; step++)
        {
            host.Send(Events.Back);
            await Settle();
        }
    }

    private void Say(string line) => log.Add(DateTime.Now.ToString("HH:mm:ss.fff", CultureInfo.InvariantCulture) + " " + width + " " + line);
}
