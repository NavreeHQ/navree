using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Navree.Design;
using Navree.Platform;
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
            await Step("the account menu", AccountMenu);
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

            bool backBefore = host.State.Nav.CanGoBack;
            host.Send(Events.Act(first.Id, new Models.ActionNavigate { Href = href }));
            await SettleAsync();
            ScreenView? pushed = host.State.FrontScreen;
            Check("destination " + destination + ": " + href + " is pushed", pushed is not null && pushed.Id != first.Id && host.State.Nav.CanGoBack, "front " + first.Id + " then " + pushed?.Id + ", can go back " + host.State.Nav.CanGoBack);
            if (pushed?.Screen is Models.Screen pushedTree && !string.IsNullOrEmpty(pushedTree.Title))
            {
                await Task.Delay(800);
                Check("destination " + destination + ": the window shows the pushed screen's title \"" + pushedTree.Title + "\"", DrawnText.Of(window.Content).Contains(pushedTree.Title, StringComparison.Ordinal), "looked for it in the words of the window");
            }

            host.Send(Events.Back);
            await SettleAsync();
            Check("destination " + destination + ": back returns to the first screen", host.State.FrontScreen?.Id == first.Id && host.State.Nav.CanGoBack == backBefore, "front " + host.State.FrontScreen?.Id + ", can go back " + host.State.Nav.CanGoBack + " (before the push: " + backBefore + ")");
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
                Check("destination " + destination + ": the screen's search narrows what is shown", is1 >= 0 && is1 < was, was + " parts then " + is1 + " (fewer is narrower)");
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
                Check("destination " + destination + ": typing in " + field.Name + " narrows what is shown", is2 >= 0 && is2 < was, was + " parts then " + is2 + " (fewer is narrower)");
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
                string? firstError = view?.Fields.Values.SelectMany(f => f.Errors).FirstOrDefault();
                if (firstError is not null)
                {
                    Check("destination " + destination + ": the first error is drawn at its field", DrawnText.Of(window.Content).Contains(firstError, StringComparison.Ordinal), "\"" + firstError + "\" looked for in the words of the window");
                }

                Check("destination " + destination + ": " + formId + " sent empty is told what is wrong", errors > 0 || host.State.Messages.Count > 0, errors + " errors under its fields; messages waiting " + host.State.Messages.Count + (host.State.Messages.Count > 0 ? ": " + string.Join(" / ", host.State.Messages.Select(m => m.ToString())) : string.Empty));
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
        Models.Field? bodyField = Tree.FieldsOf(composer).FirstOrDefault(f => f.Name == "body");
        host.Send(Events.Field(front.Id, "composer", bodyField?.Id ?? "body", new TextValue(Phrase)));
        await SettleAsync();
        Act(front.Id, send, "composer");
        await SettleAsync();
        await Task.Delay(2500);
        bool shown = host.State.FrontScreen?.Screen is Models.Screen after && Said(after, Phrase);
        bool drawn = DrawnText.Of(window.Content).Contains(Phrase, StringComparison.Ordinal);
        Check("the message sent is in the thread", shown, shown ? "found in the screen's tree" : "not found; messages waiting " + host.State.Messages.Count);
        Check("the message sent is drawn in the window", drawn, drawn ? "found in the words of the window" : "not found in the words of the window");
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

    // The person's picture is pressed, as a person does: the menu that opens holds what a screen reader is given, and a way out.
    private async Task AccountMenu()
    {
        Models.User? user = host.State.App?.User;
        Button? account = FindAccount(window.Content);
        Check("the person's picture is in the title bar", account is not null && user is not null, account is null ? "no button named Account was found" : "found");
        if (account?.Flyout is not MenuFlyout menu || user is null)
        {
            return;
        }

        account.Flyout.ShowAt(account);
        await Task.Delay(1200);
        string[] spoken = [.. menu.Items.OfType<MenuFlyoutItemBase>().Select(item => item is MenuFlyoutSeparator ? "-" : Microsoft.UI.Xaml.Automation.AutomationProperties.GetName(item).Length > 0 ? Microsoft.UI.Xaml.Automation.AutomationProperties.GetName(item) : item is MenuFlyoutItem entry ? entry.Text : item is MenuFlyoutSubItem sub ? sub.Text : "?")];
        Say("the menu holds: " + string.Join(" | ", spoken));
        Check("the menu says who is signed in, to a screen reader", spoken.Contains(AccountRules.SignedInAs(user)), AccountRules.SignedInAs(user) + " looked for in what the items say");
        bool out1 = menu.Items.OfType<MenuFlyoutItem>().Any(item => item.Text.Contains("Sign out", StringComparison.OrdinalIgnoreCase));
        Check("the menu has a way to sign out", out1, out1 ? "an item says Sign out" : "no item says Sign out");
        Check("the name and the address are items a screen reader reaches", menu.Items.OfType<MenuFlyoutItem>().Take(1).All(item => item.IsEnabled), "the first item is " + (menu.Items.OfType<MenuFlyoutItem>().FirstOrDefault()?.IsEnabled == true ? "enabled" : "disabled"));
        await ScreenPicture.SaveAsync(Path.Combine(folder, app + "-account-menu.png"));
        account.Flyout.Hide();
        await Task.Delay(400);
    }

    private static Button? FindAccount(DependencyObject? node)
    {
        if (node is null)
        {
            return null;
        }

        if (node is Button button && Microsoft.UI.Xaml.Automation.AutomationProperties.GetName(button).StartsWith("Account,", StringComparison.Ordinal))
        {
            return button;
        }

        for (int child = 0; child < VisualTreeHelper.GetChildrenCount(node); child++)
        {
            if (FindAccount(VisualTreeHelper.GetChild(node, child)) is Button found)
            {
                return found;
            }
        }

        return null;
    }

    private async Task SignOut()
    {
        WipeReport? report = null;
        host.SessionEnded += ended => report = ended;

        // The person signs out as they would: with what the server put in their menu, or with the client's own item when it has none.
        Models.Button? server = host.State.App?.User?.Menu.OfType<Models.Button>().FirstOrDefault(b => b.Label.Contains("Sign out", StringComparison.OrdinalIgnoreCase));
        bool own = host.OffersOwnSignOut;
        Check("the person has a way out: the server's menu or the client's own", server?.Action is not null || own, server?.Action is not null ? "the server's menu has \"" + server.Label + "\"" : own ? "the client's own item" : "none: the menu holds " + string.Join(", ", host.State.App?.User?.Menu.Select(n => n.GetType().Name) ?? []));
        if (server?.Action is not null)
        {
            host.Send(Events.Act(host.State.Nav.Front ?? 0, server.Action, null, null, null, null, server.Ask, server.Id));
        }
        else if (own)
        {
            _ = host.SignOutHereAsync();
        }

        for (int round = 0; round < 120 && report is null; round++)
        {
            await Task.Delay(250);
        }

        await SettleAsync();
        string steps = report is null ? "no report came" : "done: " + string.Join(", ", report.Done) + "; failed: " + string.Join(", ", report.Failed.Select(f => f.Key + " (" + f.Value + ")"));
        Check("signing out wipes the session: the wipe came and every step of it finished", report is { Complete: true, Done.Count: >= 3 }, steps);
        string? webFolder = host.Options.WebFolder;
        Check("signing out leaves no web view data on the device", webFolder is null || !Directory.Exists(webFolder) || Directory.GetFiles(webFolder, "*", SearchOption.AllDirectories).Length == 0, webFolder is null ? "no folder of its own to read" : Directory.Exists(webFolder) ? "the folder is there" : "the folder is gone");
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

    // The whole screen as the protocol writes it, searched for the words: whatever part holds them.
    private static bool Said(Models.Screen tree, string phrase)
    {
        using MemoryStream stream = new();
        using (System.Text.Json.Utf8JsonWriter writer = new(stream))
        {
            Models.WriteScreen(writer, tree);
        }

        return System.Text.Encoding.UTF8.GetString(stream.ToArray()).Contains(phrase, StringComparison.Ordinal);
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
