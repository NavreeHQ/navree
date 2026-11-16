using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Protocol.Tests;

/// <summary>A screen the shell presents as a sheet: the protocol's own examples and the demo's own list of orders, given to the core as the demo gives them.</summary>
[TestClass]
public sealed class SheetRouteTests
{
    [TestMethod]
    public async Task The_demos_list_of_orders_has_a_button_to_a_screen_that_is_pushed_as_a_sheet()
    {
        string corpus = Path.Combine(Repository.Root, "crates", "navree-native", "corpus");
        byte[] config = ClientConfig.Json("https://charkha.example", [], null, new ClientSettings(false, false, 100, "en-IN", null), "10.0", "1.0.0", ["clipboard"], null, false, Undrawable.Kinds);
        List<Effect> effects = [];
        using CoreClient client = new(config, update => effects.AddRange(update.Effects));
        await client.SendAsync(Events.Start);
        Update shell = await client.SendAsync(Events.Response(1, 200, "application/vnd.navree.native+json", null, [], File.ReadAllText(Path.Combine(corpus, "shell.json"))));
        FetchEffect home = shell.Effects.OfType<FetchEffect>().First();
        Update list = await client.SendAsync(Events.Response(home.Request, 200, "application/vnd.navree.native+json", null, [], File.ReadAllText(Path.Combine(Repository.Root, "packages", "windows", "demo", "Navree.Demo", "sheets", "orders.json"))));

        ScreenView? front = list.State.FrontScreen;
        Expect.That(front?.Screen is Models.Screen, "the front screen is " + front?.Content?.GetType().Name + " for " + home.Url);
        Models.Screen tree = (Models.Screen)front!.Screen!;
        Models.ActionNavigate? toSheet = Tree.Nodes(tree).OfType<Models.Button>().Select(button => button.Action).OfType<Models.ActionNavigate>().FirstOrDefault(go => go.Present == Models.Present.Sheet);
        Assert.IsNotNull(toSheet, "no button leads to a sheet");
        Assert.AreEqual("/orders/new", toSheet.Href);

        Update pushed = await client.SendAsync(Events.Act(front.Id, toSheet));
        FetchEffect asked = pushed.Effects.OfType<FetchEffect>().First(fetch => fetch.Url.EndsWith("/orders/new", StringComparison.Ordinal));
        Update opened = await client.SendAsync(Events.Response(asked.Request, 200, "application/vnd.navree.native+json", null, [], File.ReadAllText(Path.Combine(Repository.Root, "packages", "windows", "demo", "Navree.Demo", "sheets", "orders_new.json"))));
        Expect.That(opened.State.Nav.Modals.Count == 1 && opened.State.Nav.Modals[0].Present == Models.Present.Sheet, "no sheet is in front: " + opened.State.Nav.Modals.Count + " modals");
        Assert.AreEqual("New order", (opened.State.FrontScreen?.Screen as Models.Screen)?.Title);
    }
}
