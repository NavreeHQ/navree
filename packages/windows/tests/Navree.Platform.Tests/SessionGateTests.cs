using Navree.Design;
using Navree.Runtime;

namespace Navree.Platform.Tests;

/// <summary>What the core asked for a person is done for that person only, however late the window gets round to it.</summary>
[TestClass]
public sealed class SessionGateTests
{
    private const string App = "Navree.Tests";
    private static readonly Origins Shop = new("https://shop.example");
    private static readonly TimeSpan Short = TimeSpan.FromSeconds(5);

    private static FetchEffect Get(long request, string url) => new(request, "get", url, [], null, 5000, false);

    private static CacheWriteEffect Keep(string key, string value) => new(key, value);

    [TestMethod]
    public async Task A_value_the_core_asked_to_keep_before_it_was_told_of_the_sign_out_is_not_on_disk_when_the_window_carries_it_out_after()
    {
        string folder = Path.Combine(Path.GetTempPath(), "navree-gate-" + Guid.NewGuid().ToString("N"));
        try
        {
            KeptStore kept = new(folder, App);
            CookieStore cookies = new(Shop);
            using SessionKeeper keeper = new(kept, cookies);
            _ = keeper.Open();
            SessionGate gate = new();

            // The core, still the person's who is leaving, asks for a page of theirs to be kept. The window is busy: the update waits its turn.
            CacheWriteEffect late = Keep("page:https://shop.example/orders", "{\"name\":\"Harpreet Kaur, Ludhiana\"}");
            long[] marks = gate.Asked([late]);

            // The person signs out; the core is told, and lets go; the wipe finishes.
            gate.SessionIsEnding();
            WipeReport report = await keeper.EndAsync(
                () => { },
                () =>
                {
                    gate.CoreIsToldTheSessionEnded();
                    return Task.CompletedTask;
                },
                [],
                []).WaitAsync(Short);
            Expect.That(report.Complete, "the wipe finished");

            // Only now does the window get round to the update: what it asks is carried out as the host does it.
            if (gate.MayRun(late, marks[0]))
            {
                keeper.WriteCached(late.Key, late.Value);
            }

            await keeper.DrainAsync().WaitAsync(Short);
            Assert.IsNull(kept.ReadCached(late.Key), "a page of the person who left was kept after they signed out");

            // What the core asks for whoever comes next is kept.
            CacheWriteEffect fresh = Keep("shell", "{\"app\":\"Charkha Supply Co.\"}");
            long[] next = gate.Asked([fresh]);
            Expect.That(gate.MayRun(fresh, next[0]), "a value asked to be kept for whoever comes next is refused");
            keeper.WriteCached(fresh.Key, fresh.Value);
            await keeper.DrainAsync().WaitAsync(Short);
            Assert.AreEqual(fresh.Value, kept.ReadCached(fresh.Key));
        }
        finally
        {
            if (Directory.Exists(folder))
            {
                Directory.Delete(folder, recursive: true);
            }
        }
    }

    [TestMethod]
    public async Task A_sign_out_whose_start_throws_is_carried_to_its_end_so_the_core_is_told_and_whoever_comes_next_can_use_the_app()
    {
        string folder = Path.Combine(Path.GetTempPath(), "navree-gate-" + Guid.NewGuid().ToString("N"));
        try
        {
            KeptStore kept = new(folder, App);
            CookieStore cookies = new(Shop);
            using SessionKeeper keeper = new(kept, cookies);
            _ = keeper.Open();
            SessionGate gate = new();

            // Gurpreet is signed in: his cookie in memory and on disk, a page of his in the cache, the widths he gave a table.
            cookies.Store("https://shop.example/", ["session=gurpreet; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
            keeper.KeepCookies();
            keeper.WriteCached("page:https://shop.example/orders", "{\"name\":\"Gurpreet Singh, Amritsar\"}");
            keeper.Write("table-layout:orders", "{\"widths\":[120]}");
            await keeper.DrainAsync().WaitAsync(Short);

            // He signs out, and stopping the work under way throws: something that waited on a request threw as the request was cancelled.
            gate.SessionIsEnding();
            bool told = false;
            bool webEmptied = false;
            bool filesCleared = false;
            WipeReport report = await keeper.EndAsync(
                () => throw new InvalidOperationException("a request threw as it was cancelled"),
                () =>
                {
                    told = true;
                    gate.CoreIsToldTheSessionEnded();
                    return Task.CompletedTask;
                },
                [new WipeStep("what the web view keeps", () =>
                {
                    webEmptied = true;
                    return Task.CompletedTask;
                })],
                [new WipeStep("the files chosen for forms", () =>
                {
                    filesCleared = true;
                    return Task.CompletedTask;
                })]).WaitAsync(Short);
            await keeper.DrainAsync().WaitAsync(Short);

            // The sign-out went on to its end: the core was told, every step ran, and everything of his is gone.
            Expect.That(told, "the core was never told the session ended: everything it asks from now on is refused");
            Expect.That(webEmptied && filesCleared, "the steps of the sign-out did not run");
            Assert.AreEqual(0, cookies.Count, "his cookie is in memory: " + cookies.Export());
            Assert.IsNull(kept.Read(SessionKeeper.CookiesKey), "his cookie is on disk");
            Assert.IsNull(kept.ReadCached("page:https://shop.example/orders"), "a page of his can be read by whoever comes next");
            Assert.IsNull(kept.Read("table-layout:orders"), "the widths he gave a table are on disk");

            // What threw is said, so the person is told, and the mark stays so that the next start finishes the sign-out once more.
            Expect.That(!report.Complete, "a sign-out whose start threw is said to have finished");
            Expect.That(report.Failed.Any(failed => failed.Key == "the work under way"), "what threw is not named: " + string.Join(", ", report.Failed.Select(failed => failed.Key)));
            Expect.That(kept.WipeMarked, "the mark went though the sign-out did not finish");

            // The app is not left shut. What the core asks for whoever comes next is done, and what is theirs is kept.
            Expect.That(!keeper.Ending, "nothing is kept again in this run");
            Expect.That(!keeper.Wiping, "the web view is kept off the network for the rest of this run");
            FetchEffect shell = Get(1, "https://shop.example/_navree/native/shell");
            Expect.That(gate.MayRun(shell, gate.Asked([shell])[0]), "the start after the sign-out is refused: the app would never show its sign-in");
            cookies.Store("https://shop.example/", ["session=simran; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
            keeper.KeepCookies();
            CacheWriteEffect fresh = Keep("shell", "{\"app\":\"Charkha Supply Co.\"}");
            if (gate.MayRun(fresh, gate.Asked([fresh])[0]))
            {
                keeper.WriteCached(fresh.Key, fresh.Value);
            }

            await keeper.DrainAsync().WaitAsync(Short);
            Expect.That(kept.Read(SessionKeeper.CookiesKey)?.Contains("simran", StringComparison.Ordinal) == true, "the next person's cookie is not kept");
            Assert.AreEqual(fresh.Value, kept.ReadCached(fresh.Key), "what the core asks to keep for whoever comes next is not kept");
        }
        finally
        {
            if (Directory.Exists(folder))
            {
                Directory.Delete(folder, recursive: true);
            }
        }
    }

    [TestMethod]
    public void What_the_core_of_the_person_who_is_leaving_starts_during_the_sign_out_is_not_started_and_the_start_that_follows_is()
    {
        SessionGate gate = new();
        long before = gate.Asked([Get(1, "https://shop.example/orders")])[0];
        Expect.That(gate.Live(before), "a request is refused though nobody signed out");

        // The sign-out begins on the window's thread. A timer of the person who is leaving fires in the core, which asks for a request, a
        // stream, another timer and a notification: none is for whoever comes next.
        gate.SessionIsEnding();
        Effect[] leaving =
        [
            Get(2, "https://shop.example/orders?page=2"),
            new StreamOpenEffect(3, "https://shop.example/live", []),
            new TimerEffect(4, 30000),
            Keep("snapshot", "{\"typed\":\"Simran's draft\"}"),
            new CopyEffect("4111 1111 1111 1111", true),
        ];
        long[] marks = gate.Asked(leaving);
        for (int at = 0; at < leaving.Length; at++)
        {
            Expect.That(!gate.MayRun(leaving[at], marks[at]), leaving[at].GetType().Name + " of the person who is leaving is carried out during the sign-out");
        }

        // The request that was under way before is told to nobody when it ends.
        Expect.That(!gate.Live(before), "the answer to a request of the person who left is still told to the core");

        // What stops something, reads or reports is carried out whoever asked.
        Effect[] always = [new CancelEffect(1), new CancelTimerEffect(4), new StreamCloseEffect(3), new CacheReadEffect("shell"), new CacheWriteEffect("snapshot", null), new ReportEffect("sign_in", "x")];
        long[] alwaysMarks = gate.Asked(always);
        for (int at = 0; at < always.Length; at++)
        {
            Expect.That(gate.MayRun(always[at], alwaysMarks[at]), always[at].GetType().Name + " is refused");
        }

        // The core is told, on its own thread, and starts again from the shell: that request is for whoever comes next.
        gate.CoreIsToldTheSessionEnded();
        FetchEffect shell = Get(5, "https://shop.example/_navree/native/shell");
        long start = gate.Asked([shell])[0];
        Expect.That(gate.MayRun(shell, start), "the start after a sign-out is refused: the app would never show its sign-in");
        Expect.That(!gate.MayRun(leaving[0], marks[0]), "a request of the person who left is started after the core let go");
    }

    [TestMethod]
    public void When_the_core_ends_the_session_itself_what_it_asks_after_saying_so_is_for_whoever_comes_next()
    {
        SessionGate gate = new();

        // One update of the core: a request still for the person who is signed in, then the word that the session is to be forgotten, then the
        // shell asked for again.
        Effect[] update = [Get(1, "https://shop.example/orders"), new ForgetSessionEffect(), Get(2, "https://shop.example/_navree/native/shell")];
        long[] marks = gate.Asked(update);

        // The window carries them out in order: the first while the session is still the one now, then the end of the session begins.
        Expect.That(gate.MayRun(update[0], marks[0]), "what was asked before the session ended is refused before it ended");
        Expect.That(gate.MayRun(update[1], marks[1]), "the word that the session is to be forgotten is not carried out");
        gate.SessionIsEnding();
        Expect.That(gate.MayRun(update[2], marks[2]), "the shell asked for after the session was forgotten is refused");
        Expect.That(!gate.Live(marks[0]), "the answer to a request of the person who left is told to the core");

        // A second sign-out, by the person this time, pairs with its own word to the core.
        gate.SessionIsEnding();
        long during = gate.Asked([Get(3, "https://shop.example/orders")])[0];
        Expect.That(!gate.Live(during), "a request made while the second sign-out runs is started");
        gate.CoreIsToldTheSessionEnded();
        long after = gate.Asked([Get(4, "https://shop.example/_navree/native/shell")])[0];
        Expect.That(gate.Live(after), "the start after the second sign-out is refused");
    }
}
