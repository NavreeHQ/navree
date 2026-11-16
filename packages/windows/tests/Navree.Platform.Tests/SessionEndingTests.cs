using System.Net;
using Navree.Design;
using Navree.Runtime;

namespace Navree.Platform.Tests;

/// <summary>A handler that remembers the cookie a request carried and answers when the test lets it.</summary>
internal sealed class HeldAnswer(Func<HttpResponseMessage> answer) : HttpMessageHandler
{
    private readonly TaskCompletionSource arrived = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private readonly TaskCompletionSource gate = new(TaskCreationOptions.RunContinuationsAsynchronously);

    internal Task Arrived => arrived.Task;

    internal string? Cookie { get; private set; }

    internal void Open() => gate.TrySetResult();

    protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
    {
        Cookie = request.Headers.TryGetValues("Cookie", out IEnumerable<string>? sent) ? string.Join("; ", sent) : null;
        arrived.TrySetResult();
        await gate.Task.WaitAsync(cancellationToken).ConfigureAwait(false);
        return answer();
    }
}

/// <summary>
/// The core says the session is ending before it lets go of the person: it waits for the answer to one last request first. The sign-out
/// begins at that word and not at the answer, and that one request is let through.
/// </summary>
[TestClass]
public sealed class SessionEndingTests
{
    private const string App = "Navree.Tests";
    private const string Orders = "page:https://shop.example/orders";
    private static readonly Origins Shop = new("https://shop.example");
    private static readonly TimeSpan Short = TimeSpan.FromSeconds(5);

    private static string Folder() => Path.Combine(Path.GetTempPath(), "navree-ending-" + Guid.NewGuid().ToString("N"));

    private static void Delete(string folder)
    {
        if (Directory.Exists(folder))
        {
            Directory.Delete(folder, recursive: true);
        }
    }

    // The request the core asks for just before it says the session is ending: the server is to forget what it holds for this device.
    private static FetchEffect Removal() => new(7, "delete", "https://shop.example/_navree/native/devices/this", [], null, 5000, false);

    private static HttpResponseMessage SettingCookie(string cookie)
    {
        HttpResponseMessage answer = new(HttpStatusCode.OK) { Content = new StringContent("{}") };
        answer.Headers.TryAddWithoutValidation("Set-Cookie", cookie);
        return answer;
    }

    // Jasleen is signed in: her cookie in memory and on disk, a page of hers in the cache, the widths she gave a table.
    private static async Task SignInAsync(SessionKeeper keeper, CookieStore cookies)
    {
        cookies.Store("https://shop.example/", ["session=jasleen; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
        keeper.KeepCookies();
        keeper.WriteCached(Orders, "{\"name\":\"Jasleen Kaur, Patiala\"}");
        keeper.Write("table-layout:orders", "{\"widths\":[120]}");
        await keeper.DrainAsync().WaitAsync(Short);
    }

    [TestMethod]
    public async Task A_request_holds_the_cookies_of_the_moment_it_was_made_whatever_becomes_of_them_before_it_is_sent()
    {
        CookieStore cookies = new(Shop);
        cookies.Store("https://shop.example/", ["session=jasleen; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
        HeldAnswer server = new(() => SettingCookie("session=jasleen-again; Path=/; Max-Age=3600"));
        using HttpFetcher fetcher = new(Shop, cookies, server);

        // The request is made, and the very next thing done where it was made is the first act of a sign-out.
        Task<FetchResult> asking = fetcher.FetchAsync(Removal(), CancellationToken.None);
        cookies.Clear();

        await server.Arrived.WaitAsync(Short);
        Assert.AreEqual("session=jasleen", server.Cookie, "a request made before the cookies went was sent without them");
        server.Open();
        FetchResult answer = await asking.WaitAsync(Short);
        Assert.AreEqual(200, answer.Status);
        Assert.AreEqual(0, cookies.Count, "the answer put a cookie back after the cookies went: " + cookies.Export());
    }

    [TestMethod]
    public async Task When_the_core_says_the_session_is_ending_the_sign_out_begins_at_once_and_the_last_request_still_goes_out_with_its_cookie()
    {
        string folder = Folder();
        try
        {
            KeptStore kept = new(folder, App);
            CookieStore cookies = new(Shop);
            using SessionKeeper keeper = new(kept, cookies);
            _ = keeper.Open();
            SessionGate gate = new();
            await SignInAsync(keeper, cookies);

            HeldAnswer server = new(() => SettingCookie("session=jasleen-again; Path=/; Max-Age=3600"));
            using HttpFetcher fetcher = new(Shop, cookies, server);

            // One update of the core: it asks the server to forget this device, and says the session is ending. It lets go when that is answered.
            FetchEffect removal = Removal();
            Effect[] update = [removal, new SessionEndingEffect()];
            long[] marks = gate.Asked(update);

            // The window carries them out in order, as the host does. The request first: it is made here and now, so it holds her cookie.
            using CancellationTokenSource work = new();
            Expect.That(gate.MayRun(update[0], marks[0]), "her last request is refused");
            Expect.That(gate.IsLast(removal.Request), "her last request is not known as such: the sign-out would cancel it");
            Task<FetchResult> removing = Delivery.FetchAsync(fetcher, removal, work.Token);

            // Then the word that the session is ending: the session's number moves on and the sign-out begins, with nothing waited for.
            Expect.That(gate.MayRun(update[1], marks[1]), "the word that the session is ending is not carried out");
            gate.SessionIsEnding();
            TaskCompletionSource coreLetsGo = new(TaskCreationOptions.RunContinuationsAsynchronously);
            TaskCompletionSource stepsRan = new(TaskCreationOptions.RunContinuationsAsynchronously);
            int windowsEmptied = 0;
            Task<WipeReport> ending = keeper.EndBeforeCoreAsync(
                () =>
                {
                    // Every request under way is cancelled but her last: the host asks the gate which that is.
                    if (!gate.IsLast(removal.Request))
                    {
                        work.Cancel();
                    }
                },
                coreLetsGo.Task,
                [new WipeStep("what the web view keeps", () => Task.CompletedTask)],
                [new WipeStep("the files chosen for forms", () =>
                {
                    _ = Interlocked.Increment(ref windowsEmptied);
                    _ = stepsRan.TrySetResult();
                    return Task.CompletedTask;
                })],
                [new WipeStep("the files chosen for forms", () =>
                {
                    _ = Interlocked.Increment(ref windowsEmptied);
                    return Task.CompletedTask;
                })]);

            // From that line on, before anything is awaited: the mark is on disk and her cookie is out of memory.
            Expect.That(kept.WipeMarked, "no mark is on disk when the sign-out has begun: an app ended now would finish nothing at its next start");
            Assert.AreEqual(0, cookies.Count, "her cookie is still in memory");

            // The server has the request and has not answered. Everything of hers is gone all the same.
            await server.Arrived.WaitAsync(Short);
            await stepsRan.Task.WaitAsync(Short);
            await keeper.DrainAsync().WaitAsync(Short);
            Expect.That(!removing.IsCompleted, "the request was answered before the test let it be");
            Assert.IsNull(kept.Read(SessionKeeper.CookiesKey), "her cookie file is on disk while the core waits for the answer");
            Assert.IsNull(kept.ReadCached(Orders), "a page of hers is on disk while the core waits for the answer");
            Assert.IsNull(kept.Read("table-layout:orders"), "the widths she gave a table are on disk while the core waits for the answer");
            Assert.AreEqual(1, Volatile.Read(ref windowsEmptied), "what is the windows' own did not go at once");
            Expect.That(kept.WipeMarked, "the mark went before the core let go");
            Expect.That(!ending.IsCompleted, "the sign-out is said to be over though the core has not let go");

            // While the core has not let go it is still hers: what it asks to start or keep is refused, and nothing is written.
            Expect.That(keeper.Ending, "something may be written while the core has not let go");
            CacheWriteEffect late = new(Orders, "{\"name\":\"Jasleen Kaur, Patiala\"}");
            Expect.That(!gate.MayRun(late, gate.Asked([late])[0]), "a page of hers is kept again while the core waits");
            FetchEffect more = new(8, "get", "https://shop.example/orders?page=2", [], null, 5000, false);
            Expect.That(!gate.MayRun(more, gate.Asked([more])[0]), "another request of hers is started while the core waits");
            Expect.That(!gate.IsLast(more.Request), "a request asked for after the word is let through like the last one");
            keeper.Write("snapshot", "{\"typed\":\"Jasleen's draft\"}");
            await keeper.DrainAsync().WaitAsync(Short);
            Assert.IsNull(kept.Read("snapshot"), "a snapshot of hers was written while the core waits");

            // Her last request went out with her cookie and was not cancelled. Its answer is told to the core, and keeps nothing.
            Assert.AreEqual("session=jasleen", server.Cookie, "her last request went out without her cookie: the server cannot know whose device to forget");
            Expect.That(!work.IsCancellationRequested, "her last request was cancelled when the sign-out began");
            server.Open();
            FetchResult answer = await removing.WaitAsync(Short);
            Assert.AreEqual(200, answer.Status, "her last request failed: " + answer.Error);
            Expect.That(!gate.Live(marks[0]), "the session her last request was asked in is still the one now");
            Expect.That(gate.LastEnded(removal.Request), "the answer to her last request is told to nobody: the core would wait for it for ever");
            Expect.That(!gate.IsLast(removal.Request), "a request that has ended is still somebody's last");
            keeper.KeepCookies();
            await keeper.DrainAsync().WaitAsync(Short);
            Assert.AreEqual(0, cookies.Count, "the answer to her last request put a cookie back: " + cookies.Export());
            Assert.IsNull(kept.Read(SessionKeeper.CookiesKey), "the answer to her last request put a cookie on disk");

            // The core lets go: the word that the session is to be forgotten, and the shell asked for again, in one update. The window
            // carries the word out by telling the sign-out that began; no other begins, and the session's number does not move again.
            FetchEffect shell = new(9, "get", "https://shop.example/_navree/native/shell", [], null, 5000, false);
            Effect[] letting = [new ForgetSessionEffect(), shell];
            long[] after = gate.Asked(letting);
            Expect.That(gate.MayRun(letting[0], after[0]), "the word that the session is to be forgotten is not carried out");
            coreLetsGo.SetResult();
            Expect.That(gate.MayRun(shell, after[1]), "the start after the sign-out is refused: the app would never show its sign-in");

            WipeReport report = await ending.WaitAsync(Short);
            Expect.That(report.Complete, "the sign-out did not finish: " + string.Join(", ", report.Failed.Select(failed => failed.Key + " (" + failed.Value + ")")));
            Expect.That(!kept.WipeMarked, "the mark stays though the sign-out finished and the core let go: every start would begin signed out");
            Expect.That(!keeper.Ending, "nothing is kept for whoever comes next");
            Assert.AreEqual(2, Volatile.Read(ref windowsEmptied), "what her screens gained while the core waited is not taken away when it lets go");

            // Whoever signs in next is kept as usual.
            cookies.Store("https://shop.example/", ["session=manpreet; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
            keeper.KeepCookies();
            await keeper.DrainAsync().WaitAsync(Short);
            Expect.That(kept.Read(SessionKeeper.CookiesKey)?.Contains("manpreet", StringComparison.Ordinal) == true, "the next person's cookie is not kept");
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task An_app_ended_while_the_core_waits_for_the_last_answer_finishes_the_sign_out_at_its_next_start()
    {
        string folder = Folder();
        TaskCompletionSource coreLetsGo = new(TaskCreationOptions.RunContinuationsAsynchronously);
        TaskCompletionSource browserEnds = new(TaskCreationOptions.RunContinuationsAsynchronously);
        Task<WipeReport>? ending = null;
        SessionKeeper? ended = null;
        try
        {
            KeptStore kept = new(folder, App);
            CookieStore cookies = new(Shop);
            ended = new SessionKeeper(kept, cookies);
            _ = ended.Open();
            await SignInAsync(ended, cookies);

            // The core says the session is ending. Emptying what the web view keeps takes long (its browser is slow to end), so what comes
            // after it has not run when the app is ended.
            SessionGate gate = new();
            _ = gate.Asked([Removal(), new SessionEndingEffect()]);
            gate.SessionIsEnding();
            ending = ended.EndBeforeCoreAsync(() => { }, coreLetsGo.Task, [new WipeStep("what the web view keeps", () => browserEnds.Task)], [], []);
            Expect.That(kept.WipeMarked, "no mark is on disk when the core has said the session is ending");

            // The cookie file goes first and at once; the rest of what is hers is still on disk behind the web view's step.
            for (int round = 0; round < 200 && kept.Read(SessionKeeper.CookiesKey) is not null; round++)
            {
                await Task.Delay(25);
            }

            Assert.IsNull(kept.Read(SessionKeeper.CookiesKey), "the cookie file did not go at once");
            Assert.IsNotNull(kept.Read("table-layout:orders"), "the step that takes long did not hold the rest back: this test shows nothing");

            // The app is ended here, while the core still waits for the answer: the core never lets go, and the steps left never run. The
            // next start finds the mark and finishes the sign-out before it reads anything.
            KeptStore keptNext = new(folder, App);
            CookieStore cookiesNext = new(Shop);
            using SessionKeeper next = new(keptNext, cookiesNext);
            bool finished = next.Open();
            Expect.That(finished, "the next start does not say it finished a sign-out");
            Assert.IsNull(cookiesNext.Header("https://shop.example/", DateTimeOffset.UtcNow), "the next start carries a cookie of the person who left");
            Assert.IsNull(keptNext.ReadCached(Orders), "the next start kept a page of the person who left");
            Assert.IsNull(keptNext.Read("table-layout:orders"), "the next start kept the widths the person who left gave a table");
            Assert.AreEqual(0, Directory.GetFiles(folder, "*.kept").Length + Directory.GetFiles(folder, "*.page").Length);
            Expect.That(!keptNext.WipeMarked, "the mark stays though the next start finished the sign-out");
        }
        finally
        {
            // The run that was ended is let go of, so that nothing of it is running when the folder is deleted.
            _ = browserEnds.TrySetResult();
            _ = coreLetsGo.TrySetResult();
            if (ending is not null)
            {
                _ = await Task.WhenAny(ending, Task.Delay(Short));
            }

            ended?.Dispose();
            Delete(folder);
        }
    }
}
