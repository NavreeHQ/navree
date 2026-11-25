using System.Collections.Specialized;
using System.Text.Json;
using System.Text.Json.Nodes;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Protocol.Tests;

/// <summary>
/// Signing in through the browser, as the core carries it out for the configuration this client writes: the hand-over starts with a
/// <c>state</c> and an S256 challenge, names the app as a Windows app, and only its own return is exchanged.
/// </summary>
[TestClass]
public sealed class BrowserSignInTests
{
    private const string Native = "application/vnd.navree.native+json";
    private const string Family = "CharkhaSupply_8wekyb3d8bbwe";
    private const string Callback = "charkha:/signed-in";

    // The 48 bytes the client hands the core, and what the core makes of them: the state it sends, and the verifier it keeps until the exchange.
    private const string Bytes = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f30";
    private const string State = "ISIjJCUmJygpKissLS4vMA";
    private const string Challenge = "658WgAyQKf_KhWlXY9I8Os5xARz0DpNUrNgQIF4lD4c";
    private const string Verifier = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA";

    private static readonly string[] Paths = ["/auth/*/redirect", "/api/auth/social/*/redirect"];
    private static readonly string[] WithSession = ["clipboard", "open_url", "auth_session"];
    private static readonly string[] WithoutSession = ["clipboard", "open_url"];

    // The configuration of a packaged app whose scheme is `scheme`, told the device features `caps` and the sign-in addresses `paths`.
    private static byte[] Config(IReadOnlyList<string> caps, string? scheme, IReadOnlyList<string>? paths) =>
        ClientConfig.Json("https://shop.example", [], Family, new ClientSettings(false, false, 100, "en-IN", "Asia/Kolkata"), "10.0.26100.0", "1.0.0", caps, scheme, false, Undrawable.Kinds, signInPaths: paths);

    // The shell as the framework's server writes it for a Windows client: the hand-over's routes, with the callback of the app it lists.
    private static string Shell(string? callback)
    {
        string made = Path.Combine(Repository.Root, "crates", "navree-native-core", "transcripts", "fixtures", "made");
        JsonNode shell = JsonNode.Parse(File.ReadAllText(Path.Combine(made, "shell-app-nobody-windows.json")))!;
        if (callback is not null)
        {
            shell["sign_in"]!["callback"] = callback;
        }

        return shell.ToJsonString();
    }

    private static string Welcome() =>
        File.ReadAllText(Path.Combine(Repository.Root, "crates", "navree-native-core", "transcripts", "fixtures", "made", "welcome.json"));

    [TestMethod]
    public void The_configuration_names_the_sign_in_addresses_only_when_it_is_given_them()
    {
        using JsonDocument told = JsonDocument.Parse(Config(WithSession, "charkha", Paths));
        JsonElement signIn = told.RootElement.GetProperty("sign_in");
        Assert.AreEqual("charkha", signIn.GetProperty("callback").GetString());
        CollectionAssert.AreEqual(Paths, signIn.GetProperty("paths").EnumerateArray().Select(path => path.GetString()).ToArray());
        Assert.AreEqual(Family, told.RootElement.GetProperty("app_id").GetString());

        // A run that cannot catch a return is given none, and says none: the scheme alone stands for the app's origin in a link from outside.
        using JsonDocument untold = JsonDocument.Parse(Config(WithoutSession, "charkha", null));
        Expect.That(!untold.RootElement.GetProperty("sign_in").TryGetProperty("paths", out _), "sign-in addresses are named though none were given");
        using JsonDocument empty = JsonDocument.Parse(Config(WithoutSession, "charkha", []));
        Expect.That(!empty.RootElement.GetProperty("sign_in").TryGetProperty("paths", out _), "an empty list of sign-in addresses is written");
        using JsonDocument none = JsonDocument.Parse(Config(WithoutSession, null, null));
        Expect.That(!none.RootElement.TryGetProperty("sign_in", out _), "a sign-in is described for an app with no scheme and no sign-in address");
    }

    [TestMethod]
    public async Task A_link_to_a_sign_in_address_starts_the_hand_over_and_only_its_own_return_is_exchanged()
    {
        using CoreClient client = new(Config(WithSession, "charkha", Paths), _ => { });
        await client.SendAsync(Events.Start);
        await client.SendAsync(Events.Response(1, 200, Native, null, [], Shell(Callback)));
        Update shown = await client.SendAsync(Events.Response(2, 200, Native, null, [], Welcome()));
        long screen = shown.State.Nav.Front ?? throw new AssertFailedException("no screen is in front");

        // A link to a provider's redirect is never fetched: the core asks for random bytes, and then for the browser.
        Update tapped = await client.SendAsync(Events.Act(screen, new Models.ActionNavigate { Href = "/auth/google/redirect" }));
        Expect.That(!tapped.Effects.OfType<FetchEffect>().Any(), "the sign-in address was fetched");
        RandomEffect random = tapped.Effects.OfType<RandomEffect>().Single();
        Assert.AreEqual(48, random.Bytes);
        Update opened = await client.SendAsync(Events.Random(random.Request, Bytes));
        SignInEffect session = opened.Effects.OfType<SignInEffect>().Single();

        // It comes back on the callback the shell named, which is on the app's own scheme, and shares the browser's own session.
        Assert.AreEqual(Callback, session.Callback);
        Assert.IsFalse(session.Ephemeral);
        Uri start = new(session.Url);
        Assert.AreEqual("https://shop.example/auth/app/sign-in", start.GetLeftPart(UriPartial.Path));
        NameValueCollection said = System.Web.HttpUtility.ParseQueryString(start.Query);
        Assert.AreEqual("windows", said["platform"]);
        Assert.AreEqual(Family, said["app"]);
        Assert.AreEqual(Challenge, said["code_challenge"]);
        Assert.AreEqual("S256", said["code_challenge_method"]);
        Assert.AreEqual(State, said["state"]);
        Assert.AreEqual(Callback, said["redirect_uri"]);
        Expect.That(!session.Url.Contains(Verifier, StringComparison.Ordinal), "the verifier left the core with the start address");

        // Another state, no state, another address on the scheme and an address of the site: none is this sign-in's return.
        string[] notItsOwn =
        [
            Callback + "?code=OTHER00000&state=not-the-state",
            Callback + "?code=OTHER00000",
            "charkha://signed-in?code=OTHER00000&state=" + State,
            "charkha:/orders?code=OTHER00000&state=" + State,
            "https://shop.example/signed-in?code=OTHER00000&state=" + State,
        ];
        for (int at = 0; at < notItsOwn.Length; at++)
        {
            Update refused = await client.SendAsync(Events.SignInDone(notItsOwn[at]));
            Expect.That(!refused.Effects.OfType<FetchEffect>().Any(), "return " + at + " that is not this sign-in's was exchanged or opened");
            Expect.That(refused.Effects.OfType<ReportEffect>().Any(report => report.What == "sign_in"), "return " + at + " was not said to be refused");
        }

        // Its own return: the code goes to the exchange route with the verifier, naming the app as a Windows app, with the session's token.
        Update back = await client.SendAsync(Events.SignInDone(Callback + "?code=Tq7uX0b2c9&state=" + State));
        FetchEffect exchange = back.Effects.OfType<FetchEffect>().Single();
        Assert.AreEqual("post", exchange.Method);
        Assert.AreEqual("https://shop.example/api/auth/app/exchange", exchange.Url);
        Expect.That(exchange.Xsrf, "the exchange does not carry the session's token");
        JsonBody body = exchange.Body as JsonBody ?? throw new AssertFailedException("the exchange is not posted as JSON: " + exchange.Body?.GetType().Name);
        using JsonDocument posted = JsonDocument.Parse(body.Text);
        Assert.AreEqual("windows", posted.RootElement.GetProperty("platform").GetString());
        Assert.AreEqual(Family, posted.RootElement.GetProperty("app").GetString());
        Assert.AreEqual("Tq7uX0b2c9", posted.RootElement.GetProperty("code").GetString());
        Assert.AreEqual(Verifier, posted.RootElement.GetProperty("code_verifier").GetString());

        // No sign-in is open any more: the same return, handed back again or as a link from outside, is neither exchanged nor opened as a page.
        Update again = await client.SendAsync(Events.SignInDone(Callback + "?code=Tq7uX0b2c9&state=" + State));
        Expect.That(!again.Effects.OfType<FetchEffect>().Any(), "a return that came twice was exchanged again");
        Update late = await client.SendAsync(Events.OpenLink(Callback + "?code=Tq7uX0b2c9&state=" + State));
        Expect.That(!late.Effects.OfType<FetchEffect>().Any(), "a return with no sign-in open was exchanged or opened as a page");
    }

    [TestMethod]
    public async Task While_a_sign_in_is_open_the_core_tells_a_link_to_a_page_from_an_address_on_the_callback()
    {
        using CoreClient client = new(Config(WithSession, "charkha", Paths), _ => { });
        await client.SendAsync(Events.Start);
        await client.SendAsync(Events.Response(1, 200, Native, null, [], Shell(Callback)));
        Update shown = await client.SendAsync(Events.Response(2, 200, Native, null, [], Welcome()));
        long screen = shown.State.Nav.Front ?? throw new AssertFailedException("no screen is in front");
        Update tapped = await client.SendAsync(Events.Act(screen, new Models.ActionNavigate { Href = "/auth/google/redirect" }));
        RandomEffect random = tapped.Effects.OfType<RandomEffect>().Single();
        Update opened = await client.SendAsync(Events.Random(random.Request, Bytes));
        Expect.That(opened.Effects.OfType<SignInEffect>().Any(), "no sign-in was started");

        // What the client hands on as a link from outside while its sign-in is open. An address on the callback that is not the sign-in's
        // return is held to the sign-in's state by the core all the same: it is refused, and nothing is exchanged or opened.
        foreach (string planted in new[] { Callback + "?code=OTHER00000&state=not-the-state", Callback + "?code=OTHER00000" })
        {
            Update refused = await client.SendAsync(Events.OpenLink(planted));
            Expect.That(!refused.Effects.OfType<FetchEffect>().Any(), "an address on the callback that is not the sign-in's return was exchanged or opened");
            Expect.That(refused.Effects.OfType<ReportEffect>().Any(report => report.What == "sign_in"), "an address on the callback was not said to be refused");
        }

        // A link to a page of the app is no return: the core says nothing of a sign-in for it, and exchanges nothing.
        Update link = await client.SendAsync(Events.OpenLink("charkha://orders/7"));
        Expect.That(!link.Effects.OfType<ReportEffect>().Any(report => report.What == "sign_in"), "a link to a page of the app was taken for a sign-in's return");
        Expect.That(!link.Effects.OfType<FetchEffect>().Any(IsExchange), "a link to a page of the app was exchanged");

        // The sign-in is still open: its own return is exchanged.
        Update back = await client.SendAsync(Events.SignInDone(Callback + "?code=Tq7uX0b2c9&state=" + State));
        Expect.That(back.Effects.OfType<FetchEffect>().Any(IsExchange), "the sign-in's own return was not exchanged after a link to a page arrived");
    }

    private static bool IsExchange(FetchEffect fetch) => fetch.Url.EndsWith("/api/auth/app/exchange", StringComparison.Ordinal);

    [TestMethod]
    public async Task A_callback_that_is_not_on_the_apps_own_scheme_starts_no_sign_in()
    {
        // A shell that names a way back the app cannot be sure is its own: another scheme, a port of this machine, a link of another site.
        foreach (string callback in new[] { "other:/signed-in", "http://127.0.0.1:5123/signed-in", "https://elsewhere.example/app/signed-in" })
        {
            using CoreClient client = new(Config(WithSession, "charkha", Paths), _ => { });
            await client.SendAsync(Events.Start);
            await client.SendAsync(Events.Response(1, 200, Native, null, [], Shell(callback)));
            Update shown = await client.SendAsync(Events.Response(2, 200, Native, null, [], Welcome()));
            long screen = shown.State.Nav.Front ?? throw new AssertFailedException("no screen is in front");
            Update tapped = await client.SendAsync(Events.Act(screen, new Models.ActionNavigate { Href = "/auth/google/redirect" }));
            RandomEffect random = tapped.Effects.OfType<RandomEffect>().Single();
            Update opened = await client.SendAsync(Events.Random(random.Request, Bytes));
            Expect.That(!opened.Effects.OfType<SignInEffect>().Any(), "a sign-in was started that would come back on " + callback);
            Expect.That(opened.Effects.OfType<ReportEffect>().Any(report => report.What == "sign_in"), "nothing says why the sign-in did not start");
        }
    }

    [TestMethod]
    public async Task Without_the_sign_in_addresses_a_link_to_one_starts_no_sign_in_in_the_browser()
    {
        // A run that cannot catch a return tells the core neither a sign-in session nor the addresses: the link is an ordinary link of the app.
        using CoreClient client = new(Config(WithoutSession, null, null), _ => { });
        await client.SendAsync(Events.Start);
        await client.SendAsync(Events.Response(1, 200, Native, null, [], Shell(Callback)));
        Update shown = await client.SendAsync(Events.Response(2, 200, Native, null, [], Welcome()));
        long screen = shown.State.Nav.Front ?? throw new AssertFailedException("no screen is in front");
        Update tapped = await client.SendAsync(Events.Act(screen, new Models.ActionNavigate { Href = "/auth/google/redirect" }));
        Expect.That(!tapped.Effects.OfType<RandomEffect>().Any(), "the core began a hand-over for a run that cannot catch its return");
        Expect.That(!tapped.Effects.OfType<SignInEffect>().Any(), "a sign-in in the browser was started");
    }
}
