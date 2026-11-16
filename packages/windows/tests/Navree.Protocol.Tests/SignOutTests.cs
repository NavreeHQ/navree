using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Protocol.Tests;

/// <summary>The reply of a server that signs the person out makes the core ask the client to forget the session.</summary>
[TestClass]
public sealed class SignOutTests
{
    // The reply the demo gives to a request to sign out: a session that ends and a message that says so.
    private const string SignedOut = """{"navree":1,"build":"152bd84abf4d","vocab":0,"ops":[{"op":"signed_out"},{"op":"toast","text":"You are signed out"}]}""";

    [TestMethod]
    public async Task A_reply_that_ends_the_session_asks_the_client_to_forget_it_and_the_app_starts_again()
    {
        string made = Path.Combine(Repository.Root, "crates", "navree-native-core", "transcripts", "fixtures");
        string shell = File.ReadAllText(Path.Combine(made, "store", "shell.json"));
        string page = File.ReadAllText(Path.Combine(made, "made", "account.json"));
        byte[] config = ClientConfig.Json("https://shop.example", [], null, new ClientSettings(false, false, 100, "en-IN", null), "10.0", "1.0.0", ["clipboard"], null, false, Undrawable.Kinds);
        List<Effect> effects = [];
        using CoreClient client = new(config, update => effects.AddRange(update.Effects));
        await client.SendAsync(Events.Start);
        await client.SendAsync(Events.Response(1, 200, "application/vnd.navree.native+json", null, [], shell));
        Update shown = await client.SendAsync(Events.Response(2, 200, "application/vnd.navree.native+json", null, [], page));
        long screen = shown.State.Nav.Front ?? throw new AssertFailedException("no screen is in front");

        effects.Clear();
        Update asked = await client.SendAsync(Events.Act(screen, new Models.ActionRequest { Href = "/sign-out", Method = Models.Method.Post }));
        FetchEffect request = asked.Effects.OfType<FetchEffect>().Single();
        Assert.AreEqual("post", request.Method);
        StringAssert.EndsWith(request.Url, "/sign-out");

        effects.Clear();
        Update answered = await client.SendAsync(Events.Response(request.Request, 200, "application/vnd.navree.native+json", null, [], SignedOut));
        Expect.That(effects.Any(effect => effect is ForgetSessionEffect), "the client was not asked to forget the session: " + string.Join(", ", effects.Select(effect => effect is ReportEffect r ? "report " + r.What + " " + r.Detail : effect.GetType().Name)));
        Expect.That(effects.Any(effect => effect is FetchEffect fetch && fetch.Url.EndsWith("/_navree/native/shell", StringComparison.Ordinal)), "the app did not start again from its shell");
        Assert.AreEqual(Stage.Starting, answered.State.Nav.Stage);
    }
}
