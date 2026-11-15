using System.Globalization;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;
using Navree.Shell;

namespace Navree.Demo;

/// <summary>
/// Does what a person does in one of the live demos, and says whether each thing came of it: forward and back, more rows as the end of a
/// list is reached, a search, a form sent empty and what it is told, a message sent and the message that follows, a second window, signing
/// out. Each line is PASS or FAIL with what was seen; the file is <c>checks.txt</c>.
/// </summary>
internal sealed class ChecksWalker(ShellHost host, ShellWindow window, string folder, string app, SystemPlace? place)
{
    private readonly List<string> log = [];

    internal async Task RunAsync()
    {
        try
        {
            Directory.CreateDirectory(folder);
            await ReadyAsync();
            window.Size(1280, 980);
            await SettleAsync();
            Say("== " + app + ": signed in as " + (host.State.App?.User?.Name ?? "nobody") + "; destinations " + string.Join(" | ", host.State.Nav.Destinations.Select(d => d.Label)));
            await Step("forward and back", ForwardAndBack);
            await Step("more rows as the end of a list is reached", MoreRows);
            await Step("a search", Search);
            await Step("a form sent empty", FormErrors);
            if (app == "inbox")
            {
                await Step("a message sent", SendMessage);
            }

            await Step("a second window", SecondWindow);
            await Step("signing out", SignOut);
        }
        catch (Exception problem)
        {
            Say("the checks failed: " + problem);
        }
        finally
        {
            Say("done");
            File.WriteAllLines(Path.Combine(folder, app + "-checks.txt"), log);
            App.Finish(place);
        }
    }

    // ---- the checks ------------------------------------------------------------------------------------------

    private async Task ForwardAndBack()
    {
        for (int destination = 0; destination < host.State.Nav.Destinations.Count && destination < 2; destination++)
        {
            await HomeAsync();
            host.Send(Events.Select(destination));
            await SettleAsync();
            ScreenView? first = host.State.FrontScreen;
            if (first?.Screen is not Models.Screen tree)
            {
                continue;
            }

            string? href = Hrefs(tree).FirstOrDefault(h => h != "/");
            if (href is null)
            {
                Say("destination " + destination + " has no link to push");
                continue;
            }

            host.Send(Events.Act(first.Id, new Models.ActionNavigate { Href = href }));
            await SettleAsync();
            ScreenView? pushed = host.State.FrontScreen;
            Check("destination " + destination + ": " + href + " is pushed", pushed is not null && pushed.Id != first.Id && host.State.Nav.CanGoBack, "front " + first.Id + " then " + pushed?.Id + ", can go back " + host.State.Nav.CanGoBack);
            host.Send(Events.Back);
            await SettleAsync();
            Check("destination " + destination + ": back returns to the first screen", host.State.FrontScreen?.Id == first.Id && !host.State.Nav.CanGoBack, "front " + host.State.FrontScreen?.Id + ", can go back " + host.State.Nav.CanGoBack);
        }
    }

    private async Task MoreRows()
    {
        for (int destination = 0; destination < host.State.Nav.Destinations.Count; destination++)
        {
            await HomeAsync();
            host.Send(Events.Select(destination));
            await SettleAsync();
            if (host.State.FrontScreen is not { Screen: Models.Screen tree } front)
            {
                continue;
            }

            string? id = Collections(tree).Where(c => c.More).Select(c => c.Id).FirstOrDefault();
            if (id is null)
            {
                continue;
            }

            int before = Counted(tree, id);
            host.Send(Events.More(front.Id, id));
            await SettleAsync();
            await Task.Delay(800);
            int after = host.State.FrontScreen?.Screen is Models.Screen now ? Counted(now, id) : -1;
            Check("destination " + destination + ": asking for more of " + id + " adds rows", after > before, before + " rows then " + after);
            return;
        }

        Say("no list with more to come was found");
    }

    private async Task Search()
    {
        for (int destination = 0; destination < host.State.Nav.Destinations.Count; destination++)
        {
            await HomeAsync();
            host.Send(Events.Select(destination));
            await SettleAsync();
            if (host.State.FrontScreen is not { Screen: Models.Screen tree } front)
            {
                continue;
            }

            if (tree.Search is not null)
            {
                int was = Rows(tree);
                host.Send(Events.Search(front.Id, "kaur"));
                await SettleAsync();
                await Task.Delay(800);
                int is1 = host.State.FrontScreen?.Screen is Models.Screen now1 ? Rows(now1) : -1;
                Check("destination " + destination + ": the screen's search narrows what is shown", is1 != was, was + " parts then " + is1);
                host.Send(Events.Search(front.Id, string.Empty));
                await SettleAsync();
                return;
            }

            foreach (Models.Form form in Tree.Nodes(tree).OfType<Models.Form>())
            {
                Models.Field? field = Tree.FieldsOf(form).FirstOrDefault(f => f.Input is Models.TextInput { Kind: Models.TextKind.Search } || f.Name is "q" or "search" or "query");
                if (field is null)
                {
                    continue;
                }

                int was = Rows(tree);
                host.Send(Events.Field(front.Id, Tree.FormId(form), field.Id ?? field.Name, new TextValue("kaur")));
                await SettleAsync();
                if (SubmitOf(form) is Models.Button submit)
                {
                    Act(front.Id, submit, Tree.FormId(form));
                }

                await SettleAsync();
                await Task.Delay(1200);
                int is2 = host.State.FrontScreen?.Screen is Models.Screen now2 ? Rows(now2) : -1;
                Check("destination " + destination + ": typing in " + field.Name + " narrows what is shown", is2 != was, was + " parts then " + is2);
                host.Send(Events.Field(front.Id, Tree.FormId(form), field.Id ?? field.Name, new TextValue(string.Empty)));
                await SettleAsync();
                return;
            }
        }

        Say("no search was found");
    }

    private async Task FormErrors()
    {
        for (int destination = 0; destination < host.State.Nav.Destinations.Count; destination++)
        {
            await HomeAsync();
            host.Send(Events.Select(destination));
            await SettleAsync();
            if (host.State.FrontScreen is not { Screen: Models.Screen tree } front)
            {
                continue;
            }

            foreach (Models.Form form in Tree.Nodes(tree).OfType<Models.Form>())
            {
                IReadOnlyList<Models.Field> fields = Tree.FieldsOf(form);
                if (!fields.Any(f => f.Required) || SubmitOf(form) is not Models.Button submit)
                {
                    continue;
                }

                string formId = Tree.FormId(form);
                foreach (Models.Field field in fields.Where(f => f.Input is Models.TextInput))
                {
                    host.Send(Events.Field(front.Id, formId, field.Id ?? field.Name, new TextValue(string.Empty)));
                }

                await SettleAsync();
                Act(front.Id, submit, formId);
                await SettleAsync();
                await Task.Delay(1000);
                FormView? view = host.State.FrontScreen?.Forms.GetValueOrDefault(formId);
                int errors = view?.Fields.Values.Sum(f => f.Errors.Count) ?? 0;
                Check("destination " + destination + ": " + formId + " sent empty is told what is wrong", errors > 0, errors + " errors under its fields; messages waiting " + host.State.Messages.Count);
                return;
            }
        }

        Say("no form with a required field was found");
    }

    private async Task SendMessage()
    {
        await HomeAsync();
        host.Send(Events.Select(0));
        await SettleAsync();
        if (host.State.FrontScreen is not { Screen: Models.Screen list } listed)
        {
            return;
        }

        string? href = Hrefs(list).FirstOrDefault(h => h.StartsWith("/inbox/", StringComparison.Ordinal));
        if (href is null)
        {
            Say("no conversation to open");
            return;
        }

        host.Send(Events.Act(listed.Id, new Models.ActionNavigate { Href = href }));
        await SettleAsync();
        await Task.Delay(1500);
        if (host.State.FrontScreen is not { Screen: Models.Screen thread } front)
        {
            return;
        }

        Models.Form? composer = Tree.Nodes(thread).OfType<Models.Form>().FirstOrDefault(f => Tree.FormId(f) == "composer");
        Models.Button? send = composer is null ? null : SubmitOf(composer);
        Check("the open conversation has its composer and a Send button", composer is not null && send is not null, "form " + (composer is not null) + ", button " + (send is not null));
        if (composer is null || send is null)
        {
            return;
        }

        const string Phrase = "Thanks, this is looked into today (sent from the Windows client)";
        host.Send(Events.Field(front.Id, "composer", "body", new TextValue(Phrase)));
        await SettleAsync();
        Act(front.Id, send, "composer");
        await SettleAsync();
        await Task.Delay(2500);
        bool shown = host.State.FrontScreen?.Screen is Models.Screen after && Said(after, Phrase);
        Check("the message sent is in the thread", shown, shown ? "found in the screen's tree" : "not found; messages waiting " + host.State.Messages.Count);
    }

    private async Task SecondWindow()
    {
        await HomeAsync();
        host.Send(Events.Select(0));
        await SettleAsync();
        bool opened = place?.OpenWindow() ?? false;
        await Task.Delay(2500);
        Check("a second window opens over the one core", opened, "OpenWindow answered " + opened);
        if (opened)
        {
            await ScreenPicture.SaveAsync(Path.Combine(folder, app + "-two-windows.png"));
        }
    }

    private async Task SignOut()
    {
        string? before = host.State.App?.User?.Name;
        bool ended = false;
        host.SessionEnded += _ => ended = true;
        host.Send(Events.SignedOut);
        await Task.Delay(3000);
        await SettleAsync();
        Check("signing out wipes the session", ended, "the wipe report " + (ended ? "came" : "did not come") + "; before: " + before + ", after: " + (host.State.App?.User?.Name ?? "nobody") + ", stage " + host.State.Nav.Stage + ", screens held " + host.State.Screens.Count);
    }

    // ---- reading the screen -----------------------------------------------------------------------------------

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

    // The lists and tables of a screen, by id, with whether more come by cursor.
    private static IEnumerable<(string Id, int Count, bool More)> Collections(Models.Screen tree)
    {
        foreach (Models.Node node in Tree.Nodes(tree))
        {
            switch (node)
            {
                case Models.List { Id: string id, More: not null } list:
                    yield return (id, list.Children.Count, true);
                    break;
                case Models.Table { Id: string tableId } table:
                    yield return (tableId, table.Rows.Count, true);
                    break;
                default:
                    break;
            }
        }
    }

    private static int Counted(Models.Screen tree, string id) =>
        Collections(tree).Where(c => c.Id == id).Select(c => c.Count).FirstOrDefault();

    private static int Rows(Models.Screen tree) => Tree.Nodes(tree).Count();

    private static Models.Button? SubmitOf(Models.Form form)
    {
        List<Models.Button> found = [];

        void Visit(Models.Node node)
        {
            if (node is Models.Button { Action: Models.ActionSubmit } button)
            {
                found.Add(button);
            }

            foreach (Models.Node child in Tree.ChildNodes(node))
            {
                Visit(child);
            }
        }

        foreach (Models.Node child in form.Children)
        {
            Visit(child);
        }

        return found.FirstOrDefault();
    }

    private void Act(long screen, Models.Button button, string form)
    {
        if (button.Action is not null)
        {
            host.Send(Events.Act(screen, button.Action, form, null, null, null, button.Ask, button.Id));
        }
    }

    private static bool Said(Models.Screen tree, string phrase)
    {
        foreach (Models.Node node in Tree.Nodes(tree))
        {
            foreach (System.Reflection.PropertyInfo property in node.GetType().GetProperties())
            {
                if (property.PropertyType == typeof(string) && property.GetValue(node) is string text && text.Contains(phrase, StringComparison.Ordinal))
                {
                    return true;
                }
            }
        }

        return false;
    }

    // ---- the walk's own parts ----------------------------------------------------------------------------------

    private async Task Step(string what, Func<Task> run)
    {
        try
        {
            Say("== " + what);
            await run();
        }
        catch (Exception problem)
        {
            Say("[check] FAIL " + what + " stopped: " + problem.GetType().Name + ": " + problem.Message);
        }
    }

    private void Check(string what, bool ok, string seen) => Say("[check] " + (ok ? "PASS " : "FAIL ") + what + ": " + seen);

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
    }

    private async Task SettleAsync()
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

    private async Task HomeAsync()
    {
        for (int step = 0; step < 8 && host.State.Nav.CanGoBack; step++)
        {
            host.Send(Events.Back);
            await SettleAsync();
        }
    }

    private void Say(string line) => log.Add(DateTime.Now.ToString("HH:mm:ss.fff", CultureInfo.InvariantCulture) + " " + line);
}
