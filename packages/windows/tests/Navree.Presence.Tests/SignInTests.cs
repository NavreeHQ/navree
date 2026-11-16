using System.Net.Sockets;
using System.Text;

namespace Navree.Presence.Tests;

[TestClass]
public sealed class SignInTests
{
    private static readonly DateTimeOffset Now = new(2026, 11, 15, 12, 0, 0, TimeSpan.Zero);

    [TestMethod]
    public void TheCoresWordsForAReturnAreReadAsASchemeALinkOrALoopbackAddress()
    {
        Assert.AreEqual(ReturnKind.Scheme, SignInReturn.Parse("charkha")!.Kind);
        Assert.AreEqual("charkha", SignInReturn.Parse("charkha:/signed-in")!.Scheme);
        Assert.AreEqual("charkha", SignInReturn.Parse("CHARKHA://signed-in")!.Scheme);
        SignInReturn link = SignInReturn.Parse("https://shop.example/auth/done")!;
        Assert.AreEqual(ReturnKind.Https, link.Kind);
        Assert.AreEqual("shop.example", link.Host);
        SignInReturn local = SignInReturn.Parse("http://127.0.0.1:5123/cb")!;
        Assert.AreEqual(ReturnKind.Loopback, local.Kind);
        Assert.AreEqual(5123, local.Port);
    }

    [TestMethod]
    public void AReturnThatIsNotOneThisBuildKnowsIsNothing()
    {
        Assert.IsNull(SignInReturn.Parse(null));
        Assert.IsNull(SignInReturn.Parse(string.Empty));
        Assert.IsNull(SignInReturn.Parse("http://shop.example/cb"));
        Assert.IsNull(SignInReturn.Parse("9bad:/x"));
        Assert.IsNull(SignInReturn.Parse("has space://x"));
        Assert.IsNull(SignInReturn.Parse("charkha:/a b"));
        Assert.IsNull(SignInReturn.Parse("https://user:pass@shop.example/cb"));
        Assert.IsNull(SignInReturn.Parse("a\nb"));
    }

    [TestMethod]
    public void AReturnOnALinkOfTheSiteCannotBeCaughtByThisBuildAndSaysWhy()
    {
        Assert.IsNotNull(SignInReturn.Parse("https://shop.example/cb")!.WhyNot());
        Assert.IsNull(SignInReturn.Parse("charkha")!.WhyNot());
        Assert.IsNull(SignInReturn.Parse("http://localhost:9/cb")!.WhyNot());
    }

    [TestMethod]
    public void AnAddressIsTheReturnWhenItIsOnTheSchemeOrTheSamePortAndPath()
    {
        SignInReturn scheme = SignInReturn.Parse("charkha")!;
        Assert.IsTrue(scheme.Matches("charkha://signed-in?code=1&state=2"));
        Assert.IsTrue(scheme.Matches("CHARKHA:/signed-in"));
        Assert.IsFalse(scheme.Matches("other://signed-in"));
        Assert.IsFalse(scheme.Matches("https://shop.example/"));
        Assert.IsFalse(scheme.Matches(null));
        Assert.IsFalse(scheme.Matches("not an address"));

        SignInReturn loopback = SignInReturn.Parse("http://127.0.0.1:5123/cb")!;
        Assert.IsTrue(loopback.Matches("http://127.0.0.1:5123/cb?code=1"));
        Assert.IsFalse(loopback.Matches("http://127.0.0.1:5124/cb"));
        Assert.IsFalse(loopback.Matches("http://127.0.0.1:5123/other"));
        Assert.IsFalse(loopback.Matches("http://evil.example:5123/cb"));
        Assert.IsFalse(SignInReturn.Parse("https://shop.example/cb")!.Matches("https://shop.example/cb"));
    }

    [TestMethod]
    public void AddressesAreTestedForTheSchemeAndForWhetherTheyMayStartASignIn()
    {
        Assert.IsTrue(SignInRules.IsOnScheme("charkha://x", "charkha"));
        Assert.IsFalse(SignInRules.IsOnScheme("charkhax://x", "charkha"));
        Assert.IsFalse(SignInRules.IsOnScheme("charkha", "charkha"));
        Assert.IsTrue(SignInRules.MayStart("https://idp.example/authorize?client=1"));
        Assert.IsFalse(SignInRules.MayStart("javascript:alert(1)"));
        Assert.IsFalse(SignInRules.MayStart("file:///c:/x"));
        Assert.IsFalse(SignInRules.MayStart("https://user:pw@idp.example/"));
        Assert.IsFalse(SignInRules.MayStart(null));
        Assert.AreEqual("charkha", SignInRules.RegisteredName("CharKha"));
    }

    [TestMethod]
    public void OneSignInIsOpenAtATimeAndANewOneTakesThePlaceOfAnOlder()
    {
        SignInTracker tracker = new();
        Assert.IsFalse(tracker.IsOpen);
        int first = tracker.Begin(SignInReturn.Parse("charkha")!, Now, TimeSpan.FromMinutes(10));
        int second = tracker.Begin(SignInReturn.Parse("other")!, Now, TimeSpan.FromMinutes(10));
        Assert.AreNotEqual(first, second);
        Assert.AreEqual(SignInOffer.NotTheReturn, tracker.Offer("charkha://x", out _));
        Assert.AreEqual(SignInOffer.Taken, tracker.Offer("other://x", out int ticket));
        Assert.AreEqual(second, ticket);
        Assert.IsFalse(tracker.IsOpen);
    }

    [TestMethod]
    public void AnAddressOfferedWithNoSignInOpenIsNotUsed()
    {
        SignInTracker tracker = new();
        Assert.AreEqual(SignInOffer.NoneOpen, tracker.Offer("charkha://x?code=1", out int ticket));
        Assert.AreEqual(0, ticket);
    }

    [TestMethod]
    public void AReturnIsTakenOnlyOnce()
    {
        SignInTracker tracker = new();
        _ = tracker.Begin(SignInReturn.Parse("charkha")!, Now, TimeSpan.FromMinutes(10));
        Assert.AreEqual(SignInOffer.Taken, tracker.Offer("charkha://x", out _));
        Assert.AreEqual(SignInOffer.NoneOpen, tracker.Offer("charkha://x", out _));
    }

    [TestMethod]
    public void ASignInThatIsNotAnsweredEndsWhenItsTimeIsUp()
    {
        SignInTracker tracker = new();
        int ticket = tracker.Begin(SignInReturn.Parse("charkha")!, Now, TimeSpan.FromMinutes(10));
        Assert.IsNull(tracker.End(Now.AddMinutes(9)));
        Assert.IsTrue(tracker.IsOpen);
        Assert.AreEqual(ticket, tracker.End(Now.AddMinutes(10)));
        Assert.IsFalse(tracker.IsOpen);
        Assert.IsNull(tracker.End(Now.AddMinutes(11)));
    }

    [TestMethod]
    public void ASignInCanBeEndedByForceBeforeItsTime()
    {
        SignInTracker tracker = new();
        int ticket = tracker.Begin(SignInReturn.Parse("charkha")!, Now, TimeSpan.FromMinutes(10));
        Assert.AreEqual(ticket, tracker.End(Now, force: true));
    }

    [TestMethod]
    public void ASignInWhoseOwnReturnHasArrivedIsOverAndItsTimeRunningOutSaysNothing()
    {
        // The browser was sent off with a state; a minute later it comes back with that state and a code.
        SignInTracker tracker = new();
        _ = tracker.Begin(SignInReturn.Parse("charkha://signed-in")!, Now, TimeSpan.FromMinutes(10), SignInRules.QueryValue("https://shop.example/auth/app/sign-in?platform=windows&state=k3xq-7a&code_challenge=abc", "state"));
        Assert.AreEqual(SignInOffer.Taken, tracker.Offer("charkha://signed-in?code=DIWALI24&state=k3xq-7a", out _));
        Assert.IsFalse(tracker.IsOpen, "the sign-in is still open after its own return arrived: a link that arrives now would be taken for its return");

        // Ten minutes after it began its time is up: nothing ends, so nothing more is told to the core.
        Assert.IsNull(tracker.End(Now.AddMinutes(10)), "a sign-in that had finished ended again when its time ran out");
        Assert.IsNull(tracker.End(Now.AddMinutes(30)));
    }

    [TestMethod]
    public void AnAddressThatComesBackTheSameWayWithoutTheSignInsStateLeavesItOpen()
    {
        SignInTracker tracker = new();
        int ticket = tracker.Begin(SignInReturn.Parse("charkha")!, Now, TimeSpan.FromMinutes(10), "k3xq-7a");

        // Another state, no state, and the state as a part of another value: none is this sign-in's return.
        Assert.AreEqual(SignInOffer.NotTheReturn, tracker.Offer("charkha://signed-in?code=DIWALI24&state=another", out _));
        Assert.AreEqual(SignInOffer.NotTheReturn, tracker.Offer("charkha://signed-in?code=DIWALI24", out _));
        Assert.AreEqual(SignInOffer.NotTheReturn, tracker.Offer("charkha://orders/7?note=state=k3xq-7a", out _));
        Assert.AreEqual(SignInOffer.NotTheReturn, tracker.Offer("charkha://signed-in?code=DIWALI24#state=k3xq-7a", out _));
        Assert.IsTrue(tracker.IsOpen);

        // It still ends by itself when its time is up, and that is told once.
        Assert.AreEqual(ticket, tracker.End(Now.AddMinutes(10)));
        Assert.IsNull(tracker.End(Now.AddMinutes(11)));

        // A sign-in begun with no state is known by its way back alone, as before.
        _ = tracker.Begin(SignInReturn.Parse("charkha")!, Now, TimeSpan.FromMinutes(10));
        Assert.AreEqual(SignInOffer.Taken, tracker.Offer("charkha://signed-in?code=DIWALI24", out _));
    }

    [TestMethod]
    public void AValueOfAnAddressesQueryIsReadAsTheCoreReadsIt()
    {
        Assert.AreEqual("k3xq-7a", SignInRules.QueryValue("charkha://signed-in?code=1&state=k3xq-7a", "state"));
        Assert.AreEqual("first", SignInRules.QueryValue("charkha://signed-in?state=first&state=second", "state"));
        Assert.AreEqual("a b/c", SignInRules.QueryValue("charkha://signed-in?state=a%20b%2Fc", "state"));
        Assert.AreEqual("a b", SignInRules.QueryValue("charkha://signed-in?state=a+b", "state"));
        Assert.AreEqual(string.Empty, SignInRules.QueryValue("charkha://signed-in?state", "state"));
        Assert.IsNull(SignInRules.QueryValue("charkha://signed-in?code=1#state=x", "state"));
        Assert.IsNull(SignInRules.QueryValue("charkha://signed-in", "state"));
        Assert.IsNull(SignInRules.QueryValue(null, "state"));

        Assert.IsTrue(SignInRules.Same("k3xq-7a", "k3xq-7a"));
        Assert.IsFalse(SignInRules.Same("k3xq-7a", "k3xq-7b"));
        Assert.IsFalse(SignInRules.Same("k3xq-7a", "k3xq"));
        Assert.IsFalse(SignInRules.Same("k3xq-7a", null));
    }

    [TestMethod]
    public void TheRequestLineOfALoopbackReturnMustBeAPlainGetOfAPath()
    {
        Assert.AreEqual("/cb?code=1&state=2", LoopbackReturn.Target("GET /cb?code=1&state=2 HTTP/1.1"));
        Assert.IsNull(LoopbackReturn.Target("POST /cb HTTP/1.1"));
        Assert.IsNull(LoopbackReturn.Target("GET http://evil.example/cb HTTP/1.1"));
        Assert.IsNull(LoopbackReturn.Target("GET //evil.example HTTP/1.1"));
        Assert.IsNull(LoopbackReturn.Target("GET /cb HTTP/2"));
        Assert.IsNull(LoopbackReturn.Target("GET /cb"));
        Assert.IsNull(LoopbackReturn.Target(string.Empty));
    }

    [TestMethod]
    public async Task ALoopbackReturnTakesOneRequestOnTheMachineItselfAndAnswersIt()
    {
        using LoopbackReturn listening = LoopbackReturn.Start();
        Assert.AreNotEqual(0, listening.Port);
        using CancellationTokenSource limit = new(TimeSpan.FromSeconds(20));
        Task<string?> waiting = listening.WaitAsync(limit.Token);

        using TcpClient browser = new();
        await browser.ConnectAsync(System.Net.IPAddress.Loopback, listening.Port, limit.Token);
        NetworkStream stream = browser.GetStream();
        await stream.WriteAsync(Encoding.ASCII.GetBytes("GET /cb?code=abc&state=xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"), limit.Token);
        byte[] buffer = new byte[2048];
        int read = await stream.ReadAsync(buffer, limit.Token);
        string answer = Encoding.UTF8.GetString(buffer, 0, read);

        string? address = await waiting;
        Assert.AreEqual("http://127.0.0.1:" + listening.Port + "/cb?code=abc&state=xyz", address);
        Expect.That(answer.StartsWith("HTTP/1.1 200", StringComparison.Ordinal), "the browser is answered");
        Expect.That(answer.Contains("no-store", StringComparison.Ordinal), "the answer is not kept");
        Assert.IsTrue(SignInReturn.Parse("http://127.0.0.1:" + listening.Port + "/cb")!.Matches(address));
    }

    [TestMethod]
    public async Task ALoopbackReturnThatIsNotAskedForARequestEndsWhenTheWaitIsCancelled()
    {
        using LoopbackReturn listening = LoopbackReturn.Start();
        using CancellationTokenSource limit = new();
        Task<string?> waiting = listening.WaitAsync(limit.Token);
        await limit.CancelAsync();
        Assert.IsNull(await waiting);
    }
}
