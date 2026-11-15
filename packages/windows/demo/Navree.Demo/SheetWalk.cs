using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Navree.Design;
using Navree.Platform;
using Navree.Protocol;
using Navree.Runtime;
using Navree.Shell;

namespace Navree.Demo;

/// <summary>
/// Answers the core's requests from the protocol's own examples (the shell, the list of orders that has a button leading to a screen the
/// shell presents as a sheet, and that screen): the demo servers present no route as a sheet, and this is the only way a screen that is
/// pushed as a sheet is reached.
/// </summary>
internal sealed class CorpusFetcher(string corpus) : IFetcher
{
    public Task<FetchResult> FetchAsync(FetchEffect effect, CancellationToken cancel)
    {
        string path = new Uri(effect.Url).AbsolutePath;
        string? file = path.EndsWith("/_navree/native/shell", StringComparison.Ordinal) ? "shell.json"
            : path == "/orders/new" ? "screen-form.json"
            : path is "/" or "/orders" ? "screen-list.json"
            : null;
        return Task.FromResult(effect.Method == "get" && file is not null && File.Exists(Path.Combine(corpus, file))
            ? new FetchResult(200, "application/vnd.navree.native+json", null, [], File.ReadAllText(Path.Combine(corpus, file)), null)
            : new FetchResult(404, "text/plain", null, [], string.Empty, null));
    }

    /// <summary>The folder of the protocol's examples, found above the program.</summary>
    internal static string? Folder()
    {
        DirectoryInfo? at = new(AppContext.BaseDirectory);
        while (at is not null)
        {
            string candidate = Path.Combine(at.FullName, "crates", "navree-native", "corpus");
            if (Directory.Exists(candidate))
            {
                return candidate;
            }

            at = at.Parent;
        }

        return null;
    }
}

/// <summary>
/// A screen that the shell presents as a sheet is pushed at three widths, and what is shown is looked at: the sheet is open, and what the
/// screen holds (its title and its first field) is in the window and not an empty box with a title and a Close button. Each is pictured. The
/// file is <c>sheet.txt</c>.
/// </summary>
internal sealed class SheetWalk(ShellHost host, ShellWindow window, string folder, SystemPlace? place)
{
    private readonly List<string> log = [];

    internal async Task RunAsync()
    {
        try
        {
            Directory.CreateDirectory(folder);
            await Ready();
            foreach (int width in new[] { 480, 840, 1280 })
            {
                await One(width);
            }
        }
        catch (Exception problem)
        {
            Say("[check] FAIL the sheet walk stopped: " + problem.GetType().Name + ": " + problem.Message);
        }
        finally
        {
            Say("done");
            File.WriteAllLines(Path.Combine(folder, "sheet.txt"), log);
            App.Finish(place);
        }
    }

    private async Task One(int width)
    {
        window.Size(width, 980);
        await Settle();
        Say("== " + width.ToString(CultureInfo.InvariantCulture) + " wide");
        ScreenView? list = host.State.FrontScreen;
        Models.ActionNavigate? toSheet = list?.Screen is Models.Screen tree
            ? Tree.Nodes(tree).OfType<Models.Button>().Select(button => button.Action).OfType<Models.ActionNavigate>().FirstOrDefault(go => go.Present == Models.Present.Sheet)
            : null;
        Check(width + ": the list has a button that leads to a sheet", list is not null && toSheet is not null, toSheet is null ? "no such button on " + (list?.Url ?? "no screen") : toSheet.Href);
        if (list is null || toSheet is null)
        {
            return;
        }

        host.Send(Events.Act(list.Id, toSheet));
        await Settle();
        await Task.Delay(1500);
        bool open = host.State.Nav.Modals.Count > 0;
        Check(width + ": the screen is presented as a sheet", open, open ? "a modal is in front, " + host.State.Nav.Modals[0].Present : "no modal in front");
        string words = DrawnText.Everything(window);
        Check(width + ": the sheet shows the screen's title", words.Contains("New customer", StringComparison.Ordinal), "\"New customer\" looked for in the words of the window and what is open over it");
        Check(width + ": the sheet shows the screen's first field", words.Contains("Name", StringComparison.Ordinal), "\"Name\" looked for in the words of the window and what is open over it");
        await ScreenPicture.SaveAsync(Path.Combine(folder, "sheet-w" + width.ToString(CultureInfo.InvariantCulture) + ".png"));
        if (host.State.Nav.Modals.Count > 0 && host.State.Nav.Front is long front)
        {
            host.Send(Events.Dismissed(front));
            await Settle();
        }
    }

    private async Task Ready()
    {
        for (int round = 0; round < 120 && host.State.Nav.Stage != Stage.Ready; round++)
        {
            await Task.Delay(250);
        }

        await Task.Delay(1500);
    }

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

    private void Check(string what, bool ok, string seen) => Say("[check] " + (ok ? "PASS " : "FAIL ") + what + ": " + seen);

    private void Say(string line) => log.Add(DateTime.Now.ToString("HH:mm:ss.fff", CultureInfo.InvariantCulture) + " " + line);
}
