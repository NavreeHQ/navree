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
    public void The_request_asked_for_just_before_the_core_says_the_session_is_ending_is_its_last_and_no_other_is()
    {
        SessionGate gate = new();

        // A page is loading, from an earlier update.
        FetchEffect page = Get(1, "https://shop.example/orders");
        long loading = gate.Asked([page])[0];

        // One update: the core asks the server to forget this device, says the session is ending, and goes on asking as the person it still holds.
        FetchEffect removal = new(2, "delete", "https://shop.example/_navree/native/devices/this", [], null, 5000, false);
        FetchEffect later = Get(3, "https://shop.example/orders?page=2");
        Effect[] update = [new CancelTimerEffect(4), removal, new SessionEndingEffect(), later];
        long[] marks = gate.Asked(update);
        Expect.That(gate.IsLast(removal.Request), "the request asked for just before the word is not the last");
        Expect.That(!gate.IsLast(page.Request), "a request of an earlier update is let through the sign-out");
        Expect.That(!gate.IsLast(later.Request), "a request asked for after the word is let through the sign-out");

        // The core has not let go: everything in the update is marked as the leaving person's, the word included.
        foreach (long mark in marks)
        {
            Assert.AreEqual(loading, mark, "the core's side moved at the word that the session is ending: it moves when the core lets go");
        }

        // The window carries the update out in order: the request before the word is started, the session's number moves at the word, and
        // what follows the word is refused.
        Expect.That(gate.MayRun(update[1], marks[1]), "the last request is refused");
        Expect.That(gate.MayRun(update[2], marks[2]), "the word that the session is ending is not carried out");
        gate.SessionIsEnding();
        Expect.That(!gate.MayRun(update[3], marks[3]), "a request of the leaving person asked for after the word is started");
        Expect.That(!gate.Live(loading), "the answer to the page that was loading is still told to the core");

        // The last request is still known as such, until it ends; then it is nobody's.
        Expect.That(!gate.Live(marks[1]) && gate.IsLast(removal.Request), "the answer to the last request has no way to the core");
        Expect.That(gate.LastEnded(removal.Request), "the last request ended and was not known as the last");
        Expect.That(!gate.LastEnded(removal.Request), "a request is the last one twice");
        Expect.That(!gate.LastEnded(page.Request), "a request that was never the last is told to the core as if it were");

        // The core lets go: what follows is for whoever comes next.
        Effect[] letting = [new ForgetSessionEffect(), Get(5, "https://shop.example/_navree/native/shell")];
        long[] after = gate.Asked(letting);
        Expect.That(gate.MayRun(letting[1], after[1]), "the start after the sign-out is refused");

        // A last request that never ended is forgotten when the core lets go: its number means nothing to whoever comes next.
        FetchEffect second = new(6, "delete", "https://shop.example/_navree/native/devices/this", [], null, 5000, false);
        _ = gate.Asked([second, new SessionEndingEffect()]);
        Expect.That(gate.IsLast(second.Request), "the last request of the next sign-out is not known");
        _ = gate.Asked([new ForgetSessionEffect()]);
        Expect.That(!gate.IsLast(second.Request), "a last request outlives the core letting go");

        // A word with no request before it lets nothing through.
        _ = gate.Asked([new SessionEndingEffect(), Get(7, "https://shop.example/orders")]);
        Expect.That(!gate.IsLast(7), "a request after the word is let through");
    }

    [TestMethod]
    public void A_state_is_of_someone_leaving_from_the_moment_a_sign_out_begins_until_the_core_gives_one_after_it_let_go()
    {
        // The person signs out in the client. A state the core gave before is shown as theirs until the core has been told.
        SessionGate own = new();
        _ = own.Asked([], out long before);
        Expect.That(!own.OfSomeoneLeaving(before), "a state is of someone leaving though nobody signed out");
        own.SessionIsEnding();
        Expect.That(own.OfSomeoneLeaving(before), "a state given before the sign-out is not that of the person who is leaving");
        _ = own.Asked([Get(1, "https://shop.example/orders")], out long during);
        Expect.That(own.OfSomeoneLeaving(during), "a state the core gives while it has not been told is not that of the person who is leaving");
        own.CoreIsToldTheSessionEnded();
        _ = own.Asked([Get(2, "https://shop.example/_navree/native/shell")], out long after);
        Expect.That(!own.OfSomeoneLeaving(after), "the state the core gives after it was told is still that of the person who left: no web page would load again");
        Expect.That(own.OfSomeoneLeaving(during), "a state of the person who left, shown late, is taken for the next person's");

        // The core ends the session itself, in one update: its state is already that of whoever comes next, when it is shown and after.
        SessionGate core = new();
        _ = core.Asked([Get(1, "https://shop.example/orders")], out long signedIn);
        _ = core.Asked([new ForgetSessionEffect(), Get(2, "https://shop.example/_navree/native/shell")], out long forgotten);
        Expect.That(!core.OfSomeoneLeaving(forgotten), "the state of the update in which the core lets go is taken for the leaving person's before the window has moved");
        core.SessionIsEnding();
        Expect.That(!core.OfSomeoneLeaving(forgotten), "the state of the update in which the core lets go is taken for the leaving person's");
        Expect.That(core.OfSomeoneLeaving(signedIn), "a state of the person who left is taken for the next person's");

        // The core says the session is ending and lets go later: every state in between is the leaving person's.
        SessionGate early = new();
        FetchEffect removal = new(1, "delete", "https://shop.example/_navree/native/devices/this", [], null, 5000, false);
        _ = early.Asked([removal, new SessionEndingEffect()], out long ending);
        Expect.That(!early.OfSomeoneLeaving(ending), "the state is the leaving person's before the window has carried the word out");
        early.SessionIsEnding();
        Expect.That(early.OfSomeoneLeaving(ending), "the state shown while the core waits is not the leaving person's: a web page of theirs would load into the next person's folder");
        _ = early.Asked([], out long waiting);
        Expect.That(early.OfSomeoneLeaving(waiting), "a state the core gives while it waits is not the leaving person's");
        _ = early.Asked([new ForgetSessionEffect()], out long letGo);
        Expect.That(!early.OfSomeoneLeaving(letGo), "the state of the update in which the core lets go is still the leaving person's");
        Expect.That(early.OfSomeoneLeaving(waiting), "a state of the gap, shown late, is taken for the next person's");
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

    [TestMethod]
    public async Task When_the_person_signs_out_in_the_client_while_the_core_waits_for_its_last_answer_both_sign_outs_finish_and_the_app_starts_again()
    {
        string folder = Path.Combine(Path.GetTempPath(), "navree-gate-" + Guid.NewGuid().ToString("N"));
        try
        {
            KeptStore kept = new(folder, App);
            CookieStore cookies = new(Shop);
            using SessionKeeper keeper = new(kept, cookies);
            _ = keeper.Open();
            SessionGate gate = new();

            // Gurleen is signed in.
            cookies.Store("https://shop.example/", ["session=gurleen; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
            keeper.KeepCookies();
            keeper.Write("table-layout:orders", "{\"widths\":[120]}");
            await keeper.DrainAsync().WaitAsync(Short);

            // The core asks the server to forget this device and says the session is ending. The window begins the sign-out at that word;
            // the core lets go when the server has answered.
            FetchEffect removal = new(7, "delete", "https://shop.example/_navree/native/devices/this", [], null, 5000, false);
            long[] ending = gate.Asked([removal, new SessionEndingEffect()]);
            Expect.That(gate.Live(ending[1]), "the word is not of the session now: no sign-out would begin at it");
            gate.SessionIsEnding();
            TaskCompletionSource coreLetsGo = new(TaskCreationOptions.RunContinuationsAsynchronously);
            Task<WipeReport> atTheWord = keeper.EndBeforeCoreAsync(() => { }, coreLetsGo.Task, [], []);

            // Before the server has answered, the person signs out in the client too: a sign-out of the client's own begins, and tells the
            // core. The core, told while it waited, drops its last request, and says in one update that the session is forgotten and
            // that it asks for the shell again.
            gate.SessionIsEnding();
            FetchEffect shell = Get(9, "https://shop.example/_navree/native/shell");
            Effect[] said = [new CancelEffect(removal.Request), new CacheClearEffect(), new ForgetSessionEffect(), shell];
            long[]? marks = null;
            Task<WipeReport> ownSignOut = keeper.EndAsync(
                () => { },
                () =>
                {
                    gate.CoreIsToldTheSessionEnded();
                    marks = gate.Asked(said);

                    // The window carries the word out by telling the sign-out that began at the core's first word: no third one begins.
                    coreLetsGo.SetResult();
                    return Task.CompletedTask;
                },
                [],
                []);

            WipeReport[] reports = await Task.WhenAll(atTheWord, ownSignOut).WaitAsync(Short);
            await keeper.DrainAsync().WaitAsync(Short);
            Expect.That(reports.All(report => report.Complete), "a sign-out did not finish: " + string.Join(", ", reports.SelectMany(report => report.Failed).Select(failed => failed.Key + " (" + failed.Value + ")")));
            Expect.That(!kept.WipeMarked, "the mark stays though both sign-outs finished and the core let go: every start would begin signed out");
            Expect.That(!keeper.Ending, "nothing is kept for whoever comes next");
            Assert.IsNull(kept.Read("table-layout:orders"), "the widths she gave a table are on disk");

            // Both sides have moved twice: what the core asks from its word on is carried out, so the app leaves its start screen.
            Expect.That(marks is not null && gate.MayRun(said[2], marks[2]), "the word that the session is to be forgotten is not carried out");
            Expect.That(marks is not null && gate.MayRun(shell, marks[3]), "the start after the two sign-outs is refused: the app would stay on its start screen until it is started again");
            Expect.That(!gate.IsLast(removal.Request), "the request the core dropped is still somebody's last");

            // Whoever signs in next is kept as usual.
            cookies.Store("https://shop.example/", ["session=jaspreet; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
            keeper.KeepCookies();
            await keeper.DrainAsync().WaitAsync(Short);
            Expect.That(kept.Read(SessionKeeper.CookiesKey)?.Contains("jaspreet", StringComparison.Ordinal) == true, "the next person's cookie is not kept");
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
    public void When_the_cores_word_that_the_session_is_ending_comes_after_the_person_signed_out_in_the_client_it_is_counted_and_begins_nothing()
    {
        SessionGate gate = new();
        long signedIn = gate.Asked([Get(1, "https://shop.example/orders")])[0];

        // The person signs out in the client: the window's side moves. The core has not been told yet.
        gate.SessionIsEnding();

        // A reply that says the person signed out was already with the core. Still holding them, it asks the server to forget this device
        // and says the session is ending.
        FetchEffect removal = new(2, "delete", "https://shop.example/_navree/native/devices/this", [], null, 5000, false);
        Effect[] late = [removal, new SessionEndingEffect()];
        long[] marks = gate.Asked(late, out long waiting);
        Expect.That(!gate.MayRun(removal, marks[0]), "a request of a session that has ended on the window's side is started: it would go out with no cookie");
        Expect.That(gate.MayRun(late[1], marks[1]), "the word that the session is ending is not carried out");
        Expect.That(!gate.Live(marks[1]), "the word is taken for one of the session now: a second sign-out would begin at it");

        // The host begins no sign-out at a word whose mark is not live, and counts the word.
        gate.SessionIsEnding();
        Expect.That(gate.OfSomeoneLeaving(waiting), "the state the core gave with the word is not the leaving person's");

        // The sign-out that is under way tells the core, which was waiting: it drops its request, says the session is forgotten, and asks
        // for the shell again, in one update.
        gate.CoreIsToldTheSessionEnded();
        FetchEffect shell = Get(3, "https://shop.example/_navree/native/shell");
        Effect[] said = [new CancelEffect(removal.Request), new ForgetSessionEffect(), shell];
        long[] after = gate.Asked(said, out long started);
        Expect.That(gate.MayRun(said[1], after[1]), "the word that the session is to be forgotten is not carried out");
        Expect.That(gate.MayRun(shell, after[2]), "the start after the sign-out is refused: the app would stay on its start screen until it is started again");
        Expect.That(!gate.OfSomeoneLeaving(started), "the state of whoever comes next is taken for the leaving person's: no web page would load");
        Expect.That(!gate.IsLast(removal.Request), "the request that was refused is still somebody's last");
        Expect.That(!gate.Live(signedIn), "the answer to a request of the person who left is told to the core");

        // A word that is passed over uncounted leaves the two sides one apart for good, and every request after it refused.
        SessionGate uncounted = new();
        uncounted.SessionIsEnding();
        _ = uncounted.Asked(late);
        uncounted.CoreIsToldTheSessionEnded();
        long[] apart = uncounted.Asked(said);
        Expect.That(!uncounted.MayRun(shell, apart[2]), "the word need not be counted: this test shows nothing");
    }
}
