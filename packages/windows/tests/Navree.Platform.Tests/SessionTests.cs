using System.Net;
using Navree.Design;
using Navree.Runtime;

namespace Navree.Platform.Tests;

/// <summary>A handler whose answer waits until the test lets it go.</summary>
internal sealed class Held(Func<HttpResponseMessage> answer) : HttpMessageHandler
{
    private readonly TaskCompletionSource arrived = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private readonly TaskCompletionSource gate = new(TaskCreationOptions.RunContinuationsAsynchronously);

    internal Task Arrived => arrived.Task;

    internal void Open() => gate.TrySetResult();

    protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
    {
        arrived.TrySetResult();
        await gate.Task.ConfigureAwait(false);
        return answer();
    }
}

/// <summary>Signing out ends a session for good, whatever else is going on and whatever happens while it ends.</summary>
[TestClass]
public sealed class SessionTests
{
    private const string App = "Navree.Tests";
    private static readonly Origins Shop = new("https://shop.example");
    private static readonly TimeSpan Short = TimeSpan.FromSeconds(5);

    private static string Folder() => Path.Combine(Path.GetTempPath(), "navree-session-" + Guid.NewGuid().ToString("N"));

    private static FetchEffect Get(string url) => new(1, "get", url, [], null, 5000, false);

    private static HttpResponseMessage SettingCookie(string cookie)
    {
        HttpResponseMessage answer = new(HttpStatusCode.OK) { Content = new StringContent("{}") };
        answer.Headers.TryAddWithoutValidation("Set-Cookie", cookie);
        return answer;
    }

    // A keeper as one start of the app makes it, on a folder that outlives it.
    private static (SessionKeeper Keeper, CookieStore Cookies, KeptStore Kept, bool Finished) Start(string folder)
    {
        KeptStore kept = new(folder, App);
        CookieStore cookies = new(Shop);
        SessionKeeper keeper = new(kept, cookies);
        bool finished = keeper.Open();
        return (keeper, cookies, kept, finished);
    }

    // A person signed in on an earlier start: their cookie and a page of theirs are on disk.
    private static void SignedInBefore(string folder)
    {
        (SessionKeeper keeper, CookieStore cookies, _, _) = Start(folder);
        using (keeper)
        {
            cookies.Store("https://shop.example/", ["session=harpreet; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
            keeper.KeepCookies();
            keeper.Write("page:https://shop.example/orders", "{\"name\":\"Harpreet Kaur, Ludhiana\"}");
            keeper.DrainAsync().Wait(Short);
        }
    }

    private static void Delete(string folder)
    {
        if (Directory.Exists(folder))
        {
            Directory.Delete(folder, recursive: true);
        }
    }

    [TestMethod]
    public async Task An_answer_that_lands_after_sign_out_began_leaves_no_cookie_in_memory_or_on_disk()
    {
        string folder = Folder();
        try
        {
            (SessionKeeper keeper, CookieStore cookies, KeptStore kept, _) = Start(folder);
            using (keeper)
            {
                cookies.Store("https://shop.example/", ["session=harpreet; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
                keeper.KeepCookies();

                // A list page is loading when the person signs out, and its answer renews the session cookie.
                Held server = new(() => SettingCookie("session=harpreet-renewed; Path=/; Max-Age=3600"));
                using HttpFetcher fetcher = new(Shop, cookies, server);
                Task<FetchResult> loading = fetcher.FetchAsync(Get("https://shop.example/orders"), CancellationToken.None);
                await server.Arrived.WaitAsync(Short);

                WipeReport report = await keeper.EndAsync(() => { }, null, [], []).WaitAsync(Short);
                server.Open();
                _ = await loading.WaitAsync(Short);
                keeper.KeepCookies();
                await keeper.DrainAsync().WaitAsync(Short);

                Expect.That(report.Complete, "the wipe finished");
                Assert.AreEqual(0, cookies.Count, "the cookie store holds a cookie after sign-out: " + cookies.Export());
                Assert.IsNull(kept.Read(SessionKeeper.CookiesKey), "a cookie is on disk after sign-out");
            }

            (SessionKeeper next, CookieStore nextCookies, _, _) = Start(folder);
            using (next)
            {
                Assert.IsNull(nextCookies.Header("https://shop.example/", DateTimeOffset.UtcNow), "the next start carries a cookie of the person who left");
            }
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task A_write_queued_before_sign_out_is_not_on_disk_after_it()
    {
        string folder = Folder();
        try
        {
            (SessionKeeper keeper, _, KeptStore kept, _) = Start(folder);
            using (keeper)
            {
                // The disk is busy (a read the core asked for is being answered) when a page of the person is given to it, and then they sign out.
                ManualResetEventSlim busy = new(false);
                keeper.Read("page:https://shop.example/", _ => busy.Wait(Short));
                keeper.Write("page:https://shop.example/orders", "{\"name\":\"Gurpreet Singh, Amritsar\"}");

                Task<WipeReport> ending = keeper.EndAsync(() => { }, null, [], []);
                await Task.Delay(100);
                busy.Set();
                _ = await ending.WaitAsync(Short);
                await keeper.DrainAsync().WaitAsync(Short);

                Assert.IsNull(kept.Read("page:https://shop.example/orders"), "a page of the person who left is on disk after sign-out");
            }
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task No_snapshot_is_written_from_the_start_of_the_wipe_until_the_core_has_ended_the_session()
    {
        string folder = Folder();
        try
        {
            (SessionKeeper keeper, _, KeptStore kept, _) = Start(folder);
            using (keeper)
            {
                // The snapshot timer fires, and the window is closed to the notification area, while the session is ending.
                long? stampedBefore = keeper.Stamp();
                WipeReport report = await keeper.EndAsync(
                    () => { },
                    () =>
                    {
                        keeper.Write("snapshot", "{\"typed\":\"Simran's draft\"}");
                        keeper.Write(stampedBefore, "snapshot", "{\"typed\":\"Simran's earlier draft\"}");
                        return Task.CompletedTask;
                    },
                    [new WipeStep("the window's own", () =>
                    {
                        keeper.Write("snapshot", "{\"typed\":\"Simran's draft, later\"}");
                        return Task.CompletedTask;
                    })],
                    []).WaitAsync(Short);
                keeper.Write(stampedBefore, "snapshot", "{\"typed\":\"Simran's draft, last\"}");
                await keeper.DrainAsync().WaitAsync(Short);

                Expect.That(report.Complete, "the wipe finished");
                Assert.IsNull(kept.Read("snapshot"), "a snapshot of the person who left is on disk: " + kept.Read("snapshot"));
            }
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public void A_sign_out_cut_short_after_it_began_is_finished_at_the_next_start_before_anything_is_read()
    {
        string folder = Folder();
        try
        {
            SignedInBefore(folder);
            (SessionKeeper keeper, _, _, _) = Start(folder);
            using (keeper)
            {
                // The process ends at the first thing the sign-out does (the lid is closed, the app is killed).
                Assert.ThrowsExactly<InvalidOperationException>(() =>
                    keeper.EndAsync(() => throw new InvalidOperationException("the process ended here"), null, [], []).GetAwaiter().GetResult());
            }

            (SessionKeeper next, CookieStore cookies, KeptStore kept, bool finished) = Start(folder);
            using (next)
            {
                Assert.IsNull(cookies.Header("https://shop.example/", DateTimeOffset.UtcNow), "the next start took the cookie of the person who left");
                Assert.IsNull(kept.Read("page:https://shop.example/orders"), "the next start kept a page of the person who left");
                Expect.That(finished, "the next start says it finished a sign-out");
            }
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task A_kept_file_another_program_holds_does_not_stop_the_start_that_finishes_a_sign_out_and_is_never_read_again()
    {
        string folder = Folder();
        try
        {
            SignedInBefore(folder);
            (SessionKeeper keeper, _, _, _) = Start(folder);
            using (keeper)
            {
                // The process ends at the first thing the sign-out does: the mark is on disk, and so is everything of the person.
                Assert.ThrowsExactly<InvalidOperationException>(() =>
                    keeper.EndAsync(() => throw new InvalidOperationException("the process ended here"), null, [], []).GetAwaiter().GetResult());
            }

            // Another program (a scanner, an indexer) holds every kept file open in a way that does not let it be deleted.
            string[] files = Directory.GetFiles(folder, "*.kept");
            Assert.AreEqual(2, files.Length, "the cookie file and the page of the person who left are on disk");
            List<FileStream> held = [.. files.Select(file => new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.Read))];
            try
            {
                (SessionKeeper next, CookieStore cookies, KeptStore kept, bool finished) = Start(folder);
                using (next)
                {
                    Expect.That(finished, "the start does not say it began from an unfinished sign-out");
                    Assert.AreEqual(2, next.SetAsideAtStart, "the files that could not be deleted are not counted as set aside");
                    Assert.IsNull(cookies.Header("https://shop.example/", DateTimeOffset.UtcNow), "the start took the cookie of the person who left from a file it could not delete");
                    Assert.IsNull(kept.Read(SessionKeeper.CookiesKey), "the cookie file of the person who left is still read");
                    Assert.IsNull(kept.Read("page:https://shop.example/orders"), "a page of the person who left is still read");
                    Expect.That(!kept.WipeMarked, "the mark stays though everything is gone or set aside: every start would begin signed out");
                    Assert.AreEqual(2, Directory.GetFiles(folder, "*.kept").Length, "the held files went though they were held");
                }

                // A start while the files are still held: nothing of them is read, and nothing stops it.
                (SessionKeeper stillHeld, CookieStore cookiesThen, KeptStore keptThen, bool again) = Start(folder);
                using (stillHeld)
                {
                    Expect.That(!again, "a finished sign-out is finished again");
                    Assert.IsNull(cookiesThen.Header("https://shop.example/", DateTimeOffset.UtcNow), "a later start took the cookie of the person who left");
                    Assert.IsNull(keptThen.Read("page:https://shop.example/orders"));
                    Assert.AreEqual(2, stillHeld.SetAsideAtStart);
                }
            }
            finally
            {
                foreach (FileStream file in held)
                {
                    file.Dispose();
                }
            }

            // The other program has let go: the next start deletes what was set aside, and whoever signs in is kept as usual.
            (SessionKeeper last, CookieStore lastCookies, KeptStore lastKept, _) = Start(folder);
            using (last)
            {
                Assert.AreEqual(0, last.SetAsideAtStart);
                Assert.AreEqual(0, Directory.GetFiles(folder, "*.kept").Length, "the files set aside are still on disk after they were let go");
                lastCookies.Store("https://shop.example/", ["session=manpreet; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
                last.KeepCookies();
                await last.DrainAsync().WaitAsync(Short);
                Expect.That(lastKept.Read(SessionKeeper.CookiesKey)?.Contains("manpreet", StringComparison.Ordinal) == true, "the next person's cookie is not kept");
            }
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public void Clearing_deletes_every_file_it_can_and_says_when_one_is_still_there()
    {
        string folder = Folder();
        try
        {
            KeptStore kept = new(folder, App);
            kept.Write("a", "1");
            kept.Write("b", "2");
            kept.Write("c", "3");
            string[] files = Directory.GetFiles(folder, "*.kept");
            using (FileStream one = new(files[1], FileMode.Open, FileAccess.Read, FileShare.Read))
            {
                _ = Assert.ThrowsExactly<IOException>(kept.Clear, "a clear that left a file says it finished");
                Assert.AreEqual(1, Directory.GetFiles(folder, "*.kept").Length, "the clear stopped at the file it could not delete and left the others");
            }

            kept.Clear();
            Assert.AreEqual(0, Directory.GetFiles(folder, "*.kept").Length);
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task A_wipe_whose_step_fails_has_taken_the_cookie_file_first_and_is_finished_again_at_the_next_start()
    {
        string folder = Folder();
        try
        {
            SignedInBefore(folder);
            (SessionKeeper keeper, _, KeptStore kept, _) = Start(folder);
            bool cookieFileGoneFirst = false;
            using (keeper)
            {
                WipeReport report = await keeper.EndAsync(
                    () => { },
                    null,
                    [new WipeStep("what the web view keeps", () =>
                    {
                        cookieFileGoneFirst = kept.Read(SessionKeeper.CookiesKey) is null;
                        throw new IOException("the folder is held");
                    })],
                    []).WaitAsync(Short);
                Expect.That(!report.Complete, "the step that failed is reported");
            }

            Expect.That(cookieFileGoneFirst, "the cookie file was still on disk when the next step ran");
            (SessionKeeper next, CookieStore cookies, _, bool finished) = Start(folder);
            using (next)
            {
                Assert.IsNull(cookies.Header("https://shop.example/", DateTimeOffset.UtcNow), "the next start took the cookie of the person who left");
                Expect.That(finished, "a wipe that did not finish is finished again at the next start");
            }

            (SessionKeeper third, _, _, bool again) = Start(folder);
            using (third)
            {
                Expect.That(!again, "once finished, the start after it has nothing to finish");
            }
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task The_notifications_waiting_for_the_person_who_left_are_gone_at_the_next_start_and_their_step_runs_after_the_kept_store()
    {
        string folder = Folder();
        try
        {
            SignedInBefore(folder);
            (SessionKeeper keeper, _, KeptStore kept, _) = Start(folder);
            using (keeper)
            {
                keeper.Write("pending-notifications", "[{\"id\":\"shipped\",\"title\":\"Order for Kavita Rani, Jalandhar has shipped\"}]");
                await keeper.DrainAsync().WaitAsync(Short);
                bool keptEmptyBefore = false;
                WipeReport report = await keeper.EndAsync(
                    () => { },
                    null,
                    [],
                    [new WipeStep("the notifications, the count and the jump list", () =>
                    {
                        keptEmptyBefore = kept.Read("pending-notifications") is null;
                        throw new InvalidOperationException("the system would not take them away");
                    })]).WaitAsync(Short);

                Expect.That(keptEmptyBefore, "the waiting notifications were still kept when the step ran");
                Expect.That(report.Failed.Any(failed => failed.Key == "the notifications, the count and the jump list"), "a step that could not take them away is reported by name");
            }

            (SessionKeeper next, _, KeptStore nextKept, bool finished) = Start(folder);
            using (next)
            {
                Expect.That(finished, "the next start finishes the sign-out again");
                Assert.IsNull(nextKept.Read("pending-notifications"), "the next start would notify for the person who left");
            }
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task The_web_view_is_held_off_the_network_only_until_what_could_rebuild_the_session_is_gone()
    {
        string folder = Folder();
        try
        {
            (SessionKeeper keeper, _, _, _) = Start(folder);
            using (keeper)
            {
                bool heldDuringWebStep = false;
                bool heldAfter = true;
                bool heldWhileTellingCore = true;
                WipeReport report = await keeper.EndAsync(
                    () => Expect.That(keeper.Wiping, "the web view may reach the network as the sign-out begins"),
                    () =>
                    {
                        heldWhileTellingCore = keeper.Wiping;
                        return Task.CompletedTask;
                    },
                    [new WipeStep("what the web view keeps", () =>
                    {
                        heldDuringWebStep = keeper.Wiping;
                        return Task.CompletedTask;
                    })],
                    [new WipeStep("the pictures", () =>
                    {
                        heldAfter = keeper.Wiping;
                        return Task.CompletedTask;
                    })]).WaitAsync(Short);

                Expect.That(report.Complete, "the wipe finished");
                Expect.That(heldDuringWebStep, "the web view may reach the network while what it keeps is being cleared");
                Expect.That(!heldWhileTellingCore, "a page of whoever comes next is held off the network while the core is told");
                Expect.That(!heldAfter, "a page of whoever comes next is held off the network after the web view's data is gone");
                Expect.That(!keeper.Wiping, "held off the network after the sign-out");
            }
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task What_is_written_after_the_core_ended_the_session_is_kept()
    {
        string folder = Folder();
        try
        {
            (SessionKeeper keeper, CookieStore cookies, KeptStore kept, _) = Start(folder);
            using (keeper)
            {
                WipeReport report = await keeper.EndAsync(() => { }, () => Task.CompletedTask, [], []).WaitAsync(Short);
                Expect.That(report.Complete, "the wipe finished");

                // The next person signs in.
                cookies.Store("https://shop.example/", ["session=manpreet; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
                keeper.KeepCookies();
                keeper.Write("table-layout:orders", "{\"widths\":[120]}");
                await keeper.DrainAsync().WaitAsync(Short);

                Assert.AreEqual("{\"widths\":[120]}", kept.Read("table-layout:orders"));
                Expect.That(kept.Read(SessionKeeper.CookiesKey)?.Contains("manpreet", StringComparison.Ordinal) == true, "the next person's cookie is kept");
            }

            (SessionKeeper next, CookieStore nextCookies, _, bool finished) = Start(folder);
            using (next)
            {
                Expect.That(!finished, "a finished sign-out leaves nothing to finish");
                Assert.AreEqual("session=manpreet", nextCookies.Header("https://shop.example/", DateTimeOffset.UtcNow));
            }
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task What_the_core_asks_to_be_thrown_away_goes_and_the_person_is_still_signed_in_at_the_next_start()
    {
        string folder = Folder();
        try
        {
            (SessionKeeper keeper, CookieStore cookies, KeptStore kept, _) = Start(folder);
            using (keeper)
            {
                cookies.Store("https://shop.example/", ["session=harpreet; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
                keeper.KeepCookies();
                keeper.Write("page:https://shop.example/orders", "{\"name\":\"Harpreet Kaur, Ludhiana\"}");
                await keeper.DrainAsync().WaitAsync(Short);
                Expect.That(kept.Read(SessionKeeper.CookiesKey) is not null, "the cookie was on disk before the clear");

                // The app was updated, so the core asks for everything it kept to be thrown away. Nobody signed out, and no cookie changes afterwards.
                keeper.ClearKept();
                keeper.KeepCookies();
                await keeper.DrainAsync().WaitAsync(Short);

                Assert.IsNull(kept.Read("page:https://shop.example/orders"), "a page kept before the clear is still on disk");
                Expect.That(
                    kept.Read(SessionKeeper.CookiesKey)?.Contains("harpreet", StringComparison.Ordinal) == true,
                    "the cookies of the person who is still signed in are not on disk after the clear");
            }

            (SessionKeeper next, CookieStore nextCookies, _, bool finished) = Start(folder);
            using (next)
            {
                Expect.That(!finished, "a clear the core asked for was taken for a sign-out");
                Assert.AreEqual("session=harpreet", nextCookies.Header("https://shop.example/", DateTimeOffset.UtcNow), "the next start came up signed out though nobody signed out");
            }
        }
        finally
        {
            Delete(folder);
        }
    }
}
