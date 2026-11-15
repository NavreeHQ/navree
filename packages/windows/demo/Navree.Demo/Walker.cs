using Microsoft.UI.Xaml;
using Navree.Protocol;
using Navree.Runtime;
using Navree.Shell;

namespace Navree.Demo;

/// <summary>
/// Walks the demo through its screens at three widths and takes a picture of the window's content at each: a narrow
/// window, a middle one and a wide one. The picture is of what the app draws (the system's own frame is not in it).
/// What was done, and anything that went wrong, is written to <c>walk.txt</c> beside the pictures.
/// </summary>
internal sealed class Walker(ShellHost host, ShellWindow window, string folder, SystemPlace? place = null)
{
    private static readonly (string Name, int Width)[] Widths = [("narrow", 480), ("middle", 840), ("wide", 1280)];
    private readonly List<string> log = [];

    internal async Task RunAsync()
    {
        string prefix = Prefix();
        try
        {
            Directory.CreateDirectory(folder);
            await ReadyAsync();
            foreach ((string name, int width) in Widths)
            {
                window.Size(width, 1000);
                await SettleAsync();
                await HomeAsync();

                host.Send(Events.Select(1));
                await Shot(prefix, name, "orders");

                if (host.State.Nav.Front is long front)
                {
                    host.Send(Events.Act(front, new Models.ActionNavigate { Href = "/orders/1" }));
                    await Shot(prefix, name, "order");
                    await TryDialogAsync(name);
                }

                await HomeAsync();
                host.Send(Events.Select(2));
                await Shot(prefix, name, "inventory");

                host.Send(Events.Select(0));
                await Shot(prefix, name, "dashboard");
            }
        }
        catch (Exception problem)
        {
            Say("the walk failed: " + problem);
        }
        finally
        {
            Say("done");
            File.WriteAllLines(Path.Combine(folder, prefix + "walk.txt"), log);
            App.Finish(place);
        }
    }

    // The order's Refund button opens a dialog of the page's own: it is over the window, and closing it is told to the core.
    // (A dialog is drawn by the system above the window's content, so the pictures of the content do not hold it; the log says what was over the window.)
    private async Task TryDialogAsync(string width)
    {
        if (host.State.FrontScreen is not ScreenView order || order.Screen is not Models.Screen tree)
        {
            return;
        }

        Models.Button? refund = Navree.Design.Tree.Nodes(tree).OfType<Models.Button>().FirstOrDefault(button => button.Label == "Refund");
        if (refund?.Action is null)
        {
            Say("no Refund button on the order");
            return;
        }

        host.Send(Events.Act(order.Id, refund.Action, null, null, null, null, refund.Ask, refund.Id));
        await SettleAsync();
        await Task.Delay(600);
        Say(width + ": after Refund, overlays " + string.Join(",", host.State.FrontScreen?.Overlays ?? []) + "; over the window: " + (window.OverTheWindow ?? "nothing"));
        host.Send(Events.Overlay(order.Id, "refund", false));
        await SettleAsync();
        await Task.Delay(300);
        Say(width + ": after closing it, overlays " + string.Join(",", host.State.FrontScreen?.Overlays ?? []) + "; over the window: " + (window.OverTheWindow ?? "nothing"));
    }

    // What a screen reader is given, as the system's own tool reads it from outside the app (when the tool is on the machine).
    private async Task InspectAsync(string file)
    {
        try
        {
            System.Diagnostics.ProcessStartInfo info = new("winapp", "ui inspect -a " + Environment.ProcessId.ToString(System.Globalization.CultureInfo.InvariantCulture) + " --depth 14")
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

            string text = await tool.StandardOutput.ReadToEndAsync();
            await tool.WaitForExitAsync();
            File.WriteAllText(file, System.Text.RegularExpressions.Regex.Replace(text, "\u001b\\[[0-9;]*m", string.Empty));
            Say("wrote " + Path.GetFileName(file));
        }
        catch (Exception problem)
        {
            Say("no accessibility tree: " + problem.Message);
        }
    }

    private static string Prefix()
    {
        string[] arguments = Environment.GetCommandLineArgs();
        string theme = Array.IndexOf(arguments, "--dark") >= 0 ? "dark" : "light";
        return theme + (Array.IndexOf(arguments, "--rtl") >= 0 ? "-rtl" : string.Empty) + "-";
    }

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

    // The core has nothing left to do and nothing is loading: two calm moments in a row.
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

    // Back to the first screen of each destination.
    private async Task HomeAsync()
    {
        for (int step = 0; step < 6 && host.State.Nav.CanGoBack; step++)
        {
            host.Send(Events.Back);
            await SettleAsync();
        }
    }

    private async Task Shot(string prefix, string width, string screen)
    {
        await SettleAsync();
        string file = Path.Combine(folder, prefix + width + "-" + screen + ".png");
        if (host.State.FrontScreen?.Content is WebContent)
        {
            // The system's web view draws on a surface of its own that a picture of the content cannot hold: the pictures
            // taken from outside the app are the ones that show it.
            Say("no picture of " + Path.GetFileName(file) + ": the screen is a web page");
            return;
        }

        try
        {
            await WindowPicture.SaveAsync((Microsoft.UI.Xaml.Controls.Panel)window.Surface, file);
            Say("wrote " + Path.GetFileName(file));
            if (prefix == "light-")
            {
                await InspectAsync(Path.Combine(folder, prefix + width + "-" + screen + "-ui.txt"));
            }

            if (window.CurrentPage is Microsoft.UI.Xaml.Controls.Panel page)
            {
                string whole = Path.Combine(folder, prefix + width + "-" + screen + "-page.png");
                await WindowPicture.SaveAsync(page, whole);
                Say("wrote " + Path.GetFileName(whole) + " (" + (int)page.ActualWidth + "x" + (int)page.ActualHeight + ")");
            }
        }
        catch (Exception problem)
        {
            Say("no picture " + Path.GetFileName(file) + ": " + problem.Message);
        }
    }

    private void Say(string line) => log.Add(DateTime.Now.ToString("HH:mm:ss.fff", System.Globalization.CultureInfo.InvariantCulture) + " " + line);
}
