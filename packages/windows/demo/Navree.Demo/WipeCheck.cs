using System.Globalization;
using Navree.Platform;
using Navree.Protocol;
using Navree.Runtime;
using Navree.Shell;

namespace Navree.Demo;

/// <summary>
/// Signing out empties what the web view keeps, read from the web view. A web page that writes a cookie and a stored value is
/// shown, shown again (its reload says it finds both), left for a screen that is not a web page, and the person signs out
/// there with the answer a server gives; the app then starts again and shows its web page once more, which says what it finds:
/// nothing. The file is <c>wipe.txt</c>; each line is PASS or FAIL with what was seen.
/// </summary>
internal sealed class WipeCheck(ShellHost host, ShellWindow window, string folder, string webFolder, SystemPlace? place)
{
    private readonly List<string> log = [];

    internal async Task RunAsync()
    {
        try
        {
            Directory.CreateDirectory(folder);
            window.Size(1280, 980);
            await Task.Delay(500);
            await Run();
        }
        catch (Exception problem)
        {
            Say("[check] FAIL the wipe check stopped: " + problem.GetType().Name + ": " + problem.Message);
        }
        finally
        {
            Say("done");
            File.WriteAllLines(Path.Combine(folder, "wipe.txt"), log);
            App.Finish(place);
        }
    }

    private async Task Run()
    {
        // The app's front page is the probe's page, in the web view.
        bool first = await Until(() => WipeProbe.Reports >= 1 && host.State.FrontScreen?.Content is WebContent);
        Check("the web page is shown and says what it held when it began", first, first ? "cookies '" + WipeProbe.Said(0).Cookie + "', stored '" + WipeProbe.Said(0).Stored + "'" : "no report came; front " + host.State.FrontScreen?.Content?.GetType().Name);
        if (!first)
        {
            return;
        }

        Check("it began with nothing", WipeProbe.Said(0) == (string.Empty, string.Empty), "cookies '" + WipeProbe.Said(0).Cookie + "', stored '" + WipeProbe.Said(0).Stored + "'");
        Say("web folder before: " + Describe());

        // The page is loaded again: it finds what it wrote.
        long screen = host.State.Nav.Front ?? 0;
        host.Send(Events.Refresh(screen));
        bool again = await Until(() => WipeProbe.Reports >= 2);
        (string cookie, string stored) = again ? WipeProbe.Said(1) : (string.Empty, string.Empty);
        Check("loaded again it finds its cookie and its stored value", again && cookie.Contains("navree_probe=1", StringComparison.Ordinal) && stored == "1", again ? "cookies '" + cookie + "', stored '" + stored + "'" : "no second report came");

        // The control: the core is told the client ended the session, which starts the app again but does not empty the web view (the client's
        // wipe does). The web page then finds what it kept, so the empty finding after the real wipe below is the wipe's doing.
        host.Send(Events.SignedOut);
        bool restarted = await Until(() => WipeProbe.Reports >= 3);
        (string kept, string keptStored) = restarted ? WipeProbe.Said(2) : (string.Empty, string.Empty);
        Check("the control: a restart without the wipe leaves what the web page kept", restarted && kept.Contains("navree_probe=1", StringComparison.Ordinal) && keptStored == "1", restarted ? "cookies '" + kept + "', stored '" + keptStored + "'" : "no report came after the restart");
        await Settle();

        // The web page is left for a screen that is not a web page, shown again, and left once more: the person signs out from a native screen.
        for (int visit = 0; visit < 2; visit++)
        {
            host.Send(Events.Select(1));
            await Settle();
            await Task.Delay(1500);
            host.Send(Events.Select(0));
            await Settle();
            await Task.Delay(1500);
        }

        host.Send(Events.Select(1));
        await Settle();
        bool native = host.State.FrontScreen?.Content is not WebContent;
        Check("the person is on a screen that is not a web page", native, "front " + host.State.FrontScreen?.Content?.GetType().Name);

        // Signing out, with the answer a server gives.
        int reports = WipeProbe.Reports;
        WipeReport? report = null;
        host.SessionEnded += ended => report = ended;
        host.Send(Events.Act(host.State.Nav.Front ?? 0, new Models.ActionRequest { Href = "/sign-out", Method = Models.Method.Post }));
        bool ended = await Until(() => report is not null);
        Check("the wipe ran and every step of it finished", ended && report!.Complete && report.Done.Count >= 3, ended ? "done: " + string.Join(", ", report!.Done) + "; failed: " + string.Join(", ", report.Failed.Select(f => f.Key + " (" + f.Value + ")")) : "no report came");
        Say("web folder after: " + Describe());

        // The app starts again and shows its web page: it finds nothing of the session.
        bool second = await Until(() => WipeProbe.Reports > reports);
        (string cookieAfter, string storedAfter) = second ? WipeProbe.Said(reports) : ("?", "?");
        Check("the web page shown after signing out finds nothing of the session", second && cookieAfter.Length == 0 && storedAfter.Length == 0, second ? "cookies '" + cookieAfter + "', stored '" + storedAfter + "'" : "no report came; front " + host.State.FrontScreen?.Content?.GetType().Name);
    }

    private string Describe() =>
        Directory.Exists(webFolder)
            ? Directory.GetFiles(webFolder, "*", SearchOption.AllDirectories).Length.ToString(CultureInfo.InvariantCulture) + " files"
            : "not there";

    private static async Task<bool> Until(Func<bool> condition)
    {
        for (int round = 0; round < 240; round++)
        {
            if (condition())
            {
                return true;
            }

            await Task.Delay(250);
        }

        return false;
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
