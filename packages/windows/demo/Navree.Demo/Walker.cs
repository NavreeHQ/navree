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
internal sealed class Walker(ShellHost host, ShellWindow window, string folder)
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
                window.Size(width, 2600);
                await SettleAsync();
                await HomeAsync();

                host.Send(Events.Select(1));
                await Shot(prefix, name, "orders");

                if (host.State.Nav.Front is long front)
                {
                    host.Send(Events.Act(front, new Models.ActionNavigate { Href = "/orders/1" }));
                    await Shot(prefix, name, "order");
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
            Application.Current.Exit();
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
        try
        {
            await WindowPicture.SaveAsync((Microsoft.UI.Xaml.Controls.Panel)window.Surface, file);
            Say("wrote " + Path.GetFileName(file));
        }
        catch (Exception problem)
        {
            Say("no picture " + Path.GetFileName(file) + ": " + problem.Message);
        }
    }

    private void Say(string line) => log.Add(DateTime.Now.ToString("HH:mm:ss.fff", System.Globalization.CultureInfo.InvariantCulture) + " " + line);
}
