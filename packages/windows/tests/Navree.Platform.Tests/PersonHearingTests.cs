using System.Collections.Concurrent;
using System.Text.Json.Nodes;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Platform.Tests;

/// <summary>What a host gives a change of person, each part written down as it is done.</summary>
internal sealed class RecordedShell
{
    private readonly object gate = new();
    private readonly List<string> did = [];
    private readonly List<string> givenTo = [];
    private int droppedNobodys;

    /// <summary>Completes when the system has taken away what it showed: the test says when.</summary>
    internal TaskCompletionSource SystemTakesItAway { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);

    /// <summary>When given, moving the web view's folder on throws it.</summary>
    internal Exception? MovingThrows { get; set; }

    /// <summary>When given, giving what waits for nobody to a person throws it.</summary>
    internal Exception? GivingThrows { get; set; }

    /// <summary>How many times what waited for nobody was let go of.</summary>
    internal int DroppedNobodys
    {
        get
        {
            lock (gate)
            {
                return droppedNobodys;
            }
        }
    }

    /// <summary>The marks of the people what waited for nobody was given to, in their order. It is no part of a change, so it is not in <see cref="Did"/>.</summary>
    internal string[] GivenTo
    {
        get
        {
            lock (gate)
            {
                return [.. givenTo];
            }
        }
    }

    /// <summary>What was done so far, in its order.</summary>
    internal string[] Did
    {
        get
        {
            lock (gate)
            {
                return [.. did];
            }
        }
    }

    internal PersonChangeParts Parts => new(
        MoveWebOn: () =>
        {
            Do("the web view's folder moves on");
            return MovingThrows is { } failure ? throw failure : "web\\7";
        },
        DropPictures: () => Do("the pictures are let go of"),
        ClearFiles: () =>
        {
            Do("the chosen files begin to go");
            return Task.CompletedTask;
        },
        TakeFromSystem: () =>
        {
            Do("nothing waits to be notified");
            return SystemTakesItAway.Task;
        },
        EmptyWeb: folder =>
        {
            Do("the web views of " + folder + " are closed");
            return Task.CompletedTask;
        },
        OnWindow: step => step(),
        More: () =>
        [
            new WipeStep("the pictures of the pages", () =>
            {
                Do("the pictures of the pages go");
                return Task.CompletedTask;
            }),
        ],
        GiveWaitingTo: mark =>
        {
            lock (gate)
            {
                givenTo.Add(mark);
            }

            if (GivingThrows is { } failure)
            {
                throw failure;
            }
        },
        DropWaitingForNobody: () =>
        {
            lock (gate)
            {
                droppedNobodys++;
            }
        });

    private void Do(string what)
    {
        lock (gate)
        {
            did.Add(what);
        }
    }
}

/// <summary>One step of a conversation replayed through the real core, as the host heard it.</summary>
/// <param name="Named">Whom the state named as signed in; null when it named nobody.</param>
/// <param name="WayInUp">Whether a cover was up over the app.</param>
/// <param name="Heard">What the host heard of who is signed in.</param>
/// <param name="Here">The mark of the person taken to be signed in after it; null for nobody.</param>
/// <param name="TakenAway">How many parts of a change of person had been done when the state had been heard.</param>
internal sealed record HeardStep(string? Named, bool WayInUp, PersonHeard Heard, string? Here, int TakenAway);

/// <summary>
/// The host's part in a change of person, without a window: who a state names is heard before the windows are given the state, what has
/// to go first has gone by then, the rest follows in its order, and the person is told from another by the key the core gives.
/// </summary>
[TestClass]
public sealed class PersonHearingTests
{
    private const string App = "Navree.Tests";
    private const string Waiting = "pending-notifications";
    private const string RituKey = "1c54d473261f6f17cb6193202ce56588716e85757946078ac43d03b22f93a35e";
    private const string HarpreetKey = "38c9705141ffe6c8247000b823439d8248a231f176ef18db39fa1940e1e809cc";
    private static readonly Origins Shop = new("https://shop.example");
    private static readonly TimeSpan Short = TimeSpan.FromSeconds(5);
    private static readonly string[] AtOnce =
    [
        "the web view's folder moves on",
        "the pictures are let go of",
        "the chosen files begin to go",
        "nothing waits to be notified",
    ];

    private static string Folder() => Path.Combine(Path.GetTempPath(), "navree-hearing-" + Guid.NewGuid().ToString("N"));

    private static void Delete(string folder)
    {
        if (Directory.Exists(folder))
        {
            Directory.Delete(folder, recursive: true);
        }
    }

    private static ViewState Naming(string? key, string name, string? email) =>
        ViewState.Empty with { App = new AppPart { Name = "Charkha Supply Co.", User = new Models.User { Name = name, Email = email, Key = key } } };

    private static ViewState NamingNobody() => ViewState.Empty with { App = new AppPart { Name = "Charkha Supply Co." } };

    // A state with a locked cover up over the app (even when a synthetic test leaves a person named in the app's part under it).
    private static ViewState WithTheWayInUp(ViewState state) =>
        state with { Nav = state.Nav with { Front = 4, Modals = [new Modal { Present = Models.Present.Full, Locked = true, Stack = [4] }] } };

    [TestMethod]
    public async Task Who_a_state_names_is_heard_before_the_windows_are_given_it_and_what_must_go_first_has_gone_by_then()
    {
        string folder = Folder();
        try
        {
            KeptStore kept = new(folder, App);
            CookieStore cookies = new(Shop);
            using SessionKeeper keeper = new(kept, cookies);
            _ = keeper.Open();
            SessionGate gate = new();
            RecordedShell shell = new();
            ConcurrentQueue<string> problems = new();
            PersonChange people = new(new PersonWatch(keeper.KeptPerson(), keeper.KeepsOfAPerson()), gate, keeper, shell.Parts, problems.Enqueue);

            // As the host hands a state on: who it names is heard, and then the windows are given it. What a window finds done when it
            // is given the state is written down with it.
            List<string> order = [];
            void Deliver(ViewState state)
            {
                _ = gate.Asked([], out long mark);
                Delivery.Show(
                    state,
                    heard => order.Add("heard " + people.Hear(heard, mark)),
                    shown =>
                    {
                        string[] done = shell.Did;
                        string found = done.Length == 0 ? "nothing taken away" : AtOnce.All(done.Contains) ? "what must go first gone" : "only " + string.Join("; ", done);
                        order.Add("shown " + (shown.App?.User?.Name ?? "nobody") + " with " + found);
                    },
                    problems.Enqueue);
            }

            // Ritu is signed in at the counter, and a reminder of hers waits.
            cookies.Store("https://shop.example/", ["session=ritu; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
            keeper.KeepCookies();
            Deliver(Naming(RituKey, "Ritu Sharma", "ritu.sharma@example.com"));
            keeper.Write(Waiting, "[{\"id\":\"dr-malhotra\",\"title\":\"Your appointment with Dr. Malhotra, Model Town, 10:30\"}]");
            await keeper.DrainAsync().WaitAsync(Short);
            Assert.AreEqual(PersonWatch.MarkOfKey(RituKey), kept.Read(SessionKeeper.PersonKey), "the first person named is not written down as the one what is kept is kept for");

            // Her session runs out over lunch: the shell names nobody. Nothing is taken away, since she may sign in again.
            Deliver(NamingNobody());

            // Harpreet signs in through the app's own form, and the shell names her.
            cookies.Store("https://shop.example/", ["session=harpreet; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
            keeper.KeepCookies();
            Deliver(Naming(HarpreetKey, "Harpreet Kaur", "harpreet.kaur@example.com"));
            CollectionAssert.AreEqual(
                new[]
                {
                    "heard First", "shown Ritu Sharma with nothing taken away",
                    "heard Nobody", "shown nobody with nothing taken away",
                    "heard Another", "shown Harpreet Kaur with what must go first gone",
                },
                order,
                "the windows were given a state before who it names was heard, or before what must go first had gone");
            CollectionAssert.AreEqual(AtOnce, shell.Did.Take(4).ToArray(), "what must be so before the windows see the new person was not done at once, in its order");

            // What is kept for her from the moment her state is shown is hers, and stays: what was kept for Ritu had its turn first.
            keeper.Write(Waiting, "[{\"id\":\"stock-count\",\"title\":\"Stock count at 6\"}]");
            await keeper.DrainAsync().WaitAsync(Short);
            Expect.That(kept.Read(Waiting)?.Contains("stock-count", StringComparison.Ordinal) == true, "a reminder Harpreet was given as she signed in went with Ritu's");
            Expect.That(kept.Read(Waiting)?.Contains("dr-malhotra", StringComparison.Ordinal) != true, "Ritu's reminder still waits, to be shown to Harpreet");

            // The system has not taken away what it showed yet: the change is not over, and the mark kept is still Ritu's.
            Expect.That(people.Last is { IsCompleted: false }, "the change is over though a part of it is not: this test shows nothing");
            Assert.AreEqual(PersonWatch.MarkOfKey(RituKey), kept.Read(SessionKeeper.PersonKey), "the mark of the person who is here now was written before the change finished");

            // It does, and the rest follows, each part once and in its order.
            shell.SystemTakesItAway.SetResult();
            WipeReport report = await people.Last!.WaitAsync(Short);
            await keeper.DrainAsync().WaitAsync(Short);
            Expect.That(report.Complete, "the change did not finish: " + string.Join(", ", report.Failed.Select(failed => failed.Key + " (" + failed.Value + ")")));
            CollectionAssert.AreEqual(
                new[]
                {
                    "what was kept for the person before",
                    "the web view's folder",
                    "the pictures",
                    "what the web view keeps",
                    "the files chosen for forms",
                    "the notifications, the count and the jump list",
                    "the pictures of the pages",
                },
                report.Done.ToArray(),
                "the parts did not each run once, in their order");
            string[] everything = [.. AtOnce, "the web views of web\\7 are closed", "the pictures of the pages go"];
            CollectionAssert.AreEqual(everything, shell.Did, "a part ran twice, or the web views of the folder that ended were not closed");
            Assert.AreEqual(PersonWatch.MarkOfKey(HarpreetKey), kept.Read(SessionKeeper.PersonKey), "the mark is not that of the person who is here now though the change finished");
            Assert.AreEqual("session=harpreet", cookies.Header("https://shop.example/", DateTimeOffset.UtcNow), "the change of person took her cookie");
            Assert.AreEqual(0, problems.Count, string.Join(" | ", problems));

            // The core goes on naming her: nothing more is taken away.
            Deliver(Naming(HarpreetKey, "Harpreet Kaur", "harpreet.kaur@example.com"));
            Assert.AreEqual("heard Same", order[^2]);
            Assert.AreEqual(6, shell.Did.Length, "the same person named again took something away");
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task A_part_that_throws_is_said_and_the_parts_after_it_still_run_and_the_mark_of_the_person_before_stays()
    {
        string folder = Folder();
        try
        {
            KeptStore kept = new(folder, App);
            CookieStore cookies = new(Shop);
            using SessionKeeper keeper = new(kept, cookies);
            _ = keeper.Open();
            SessionGate gate = new();
            RecordedShell shell = new() { MovingThrows = new IOException("Ritu Sharma's folder would not move") };
            ConcurrentQueue<string> problems = new();
            PersonChange people = new(new PersonWatch(), gate, keeper, shell.Parts, problems.Enqueue);
            _ = gate.Asked([], out long mark);

            Assert.AreEqual(PersonHeard.First, people.Hear(Naming(RituKey, "Ritu Sharma", null), mark));
            Assert.AreEqual(PersonHeard.Another, people.Hear(Naming(HarpreetKey, "Harpreet Kaur", null), mark));
            CollectionAssert.AreEqual(AtOnce, shell.Did, "the parts after the one that threw were not done");

            shell.SystemTakesItAway.SetResult();
            WipeReport report = await people.Last!.WaitAsync(Short);
            await keeper.DrainAsync().WaitAsync(Short);
            Expect.That(!report.Complete, "a change whose part threw is said to have finished");
            CollectionAssert.AreEqual(new[] { "the web view's folder" }, report.Failed.Select(failed => failed.Key).ToArray(), "what failed is not named, or more failed");
            Expect.That(report.Done.Contains("the pictures of the pages"), "the parts after the one that failed did not run");
            Expect.That(!shell.Did.Any(part => part.Contains("are closed", StringComparison.Ordinal)), "web views were closed for a folder that did not end");

            // It is said, by what failed and with nothing of a person; and the mark kept stays that of the person before.
            Assert.AreEqual(1, problems.Count, string.Join(" | ", problems));
            Expect.That(problems.TryPeek(out string? said) && said.Contains("the web view's folder", StringComparison.Ordinal) && !said.Contains("Ritu", StringComparison.Ordinal), "what is said names a person, or not what failed");
            Assert.AreEqual(PersonWatch.MarkOfKey(RituKey), kept.Read(SessionKeeper.PersonKey), "the mark of the person who is here now was written though the change did not finish");

            // The core goes on naming Harpreet for the rest of the run: the mark still stays, so the next start does the change over.
            Assert.AreEqual(PersonHeard.Same, people.Hear(Naming(HarpreetKey, "Harpreet Kaur", null), mark));
            await keeper.DrainAsync().WaitAsync(Short);
            Assert.AreEqual(PersonWatch.MarkOfKey(RituKey), kept.Read(SessionKeeper.PersonKey));
            Assert.AreEqual(PersonHeard.Another, new PersonWatch(kept.Read(SessionKeeper.PersonKey)).Hear(HarpreetKey, "Harpreet Kaur", null), "the next start does not do the change over");
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public void The_state_of_a_person_who_is_signing_out_is_not_heard_and_whoever_is_named_after_the_sign_out_is_the_first()
    {
        string folder = Folder();
        try
        {
            using SessionKeeper keeper = new(new KeptStore(folder, App), new CookieStore(Shop));
            _ = keeper.Open();
            SessionGate gate = new();
            RecordedShell shell = new();
            PersonChange people = new(new PersonWatch(), gate, keeper, shell.Parts, _ => { });

            _ = gate.Asked([], out long signedIn);
            Assert.AreEqual(PersonHeard.First, people.Hear(Naming(RituKey, "Ritu Sharma", null), signedIn));

            // She signs out in the client: the window's side moves, and the host says so. A state the core gave before it was told still
            // names her, and is shown while she leaves: it says nothing of who comes next.
            gate.SessionIsEnding();
            people.SignedOut();
            _ = gate.Asked([], out long leaving);
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(Naming(RituKey, "Ritu Sharma", null), leaving), "the state of a person who is signing out is heard: whoever comes next would be taken for another person");

            // The core is told, and Harpreet signs in: the sign-out took everything away, so she is the first, and nothing more goes.
            gate.CoreIsToldTheSessionEnded();
            _ = gate.Asked([], out long next);
            Assert.AreEqual(PersonHeard.First, people.Hear(Naming(HarpreetKey, "Harpreet Kaur", null), next), "a person who signs in after a sign-out is taken for a change with no sign-out between");
            Assert.IsNull(people.Last, "a change of person began after a sign-out");
            Assert.AreEqual(0, shell.Did.Length);
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task Nobody_is_signed_in_while_the_way_in_is_up_though_the_state_goes_on_naming_the_person_whose_session_ended()
    {
        // What waits for a person's eyes (a reminder) is shown by who is signed in now, and kept by whom what is kept is kept for. The
        // two are not the same between a session running out and somebody signing in. In that time the core's state goes on naming
        // the person whose session it was, under the way in it put up: the way in says that nobody is signed in, and the name does not.
        string folder = Folder();
        try
        {
            KeptStore kept = new(folder, App);
            using SessionKeeper keeper = new(kept, new CookieStore(Shop));
            _ = keeper.Open();
            keeper.KeepPerson(PersonWatch.MarkOfKey(RituKey));
            await keeper.DrainAsync().WaitAsync(Short);
            string ritu = PersonWatch.MarkOfKey(RituKey);
            string harpreet = PersonWatch.MarkOfKey(HarpreetKey);

            // The app is started. The last run kept Ritu's mark, and no state has been heard: nobody is known to be signed in.
            SessionGate gate = new();
            RecordedShell shell = new();
            PersonChange people = new(new PersonWatch(keeper.KeptPerson(), keeper.KeepsOfAPerson()), gate, keeper, shell.Parts, _ => { });
            Assert.AreEqual(ritu, people.KeptFor, "the start does not say whom what is kept is kept for");
            Assert.IsNull(people.Here, "somebody is taken to be signed in before any state was heard: her reminder would be shown to whoever started the app");

            // The device is offline: the shell kept from the last run is drawn, and it names nobody.
            _ = gate.Asked([], out long mark);
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(NamingNobody(), mark));
            Assert.IsNull(people.Here);
            Assert.AreEqual(ritu, people.KeptFor);

            // The shell answers and names her: she is the one signed in.
            ViewState ritus = Naming(RituKey, "Ritu Sharma", null);
            Assert.AreEqual(PersonHeard.Same, people.Hear(ritus, mark));
            Assert.AreEqual(ritu, people.Here);

            // Even if a synthetic state still named her under a locked cover, the client's own cover check keeps Here null.
            ViewState wayInUp = WithTheWayInUp(ritus);
            Expect.That(wayInUp.App?.User?.Key == RituKey && PersonChange.WayInIsUp(wayInUp), "the state does not name her under the way in: this test shows nothing");
            Assert.AreEqual(PersonHeard.Same, people.Hear(wayInUp, mark));
            Assert.IsNull(people.Here, "the person whose session ran out is taken to be signed in while the way in is up: her reminder would be shown to whoever is at the counter");
            Assert.AreEqual(ritu, people.KeptFor, "what is kept stopped being hers when her session ran out: she would not find it when she signs in again");

            // The sign-in page answers, and somebody types into it: each state is the same in this, for as long as the way in is up.
            Assert.AreEqual(PersonHeard.Same, people.Hear(wayInUp, mark));
            Assert.IsNull(people.Here, "the person whose session ran out is signed in again by a later state under the way in");

            // Somebody signs in: the core holds a locked waiting cover with nobody named until the shell answers.
            ViewState waitingForShell = WithTheWayInUp(NamingNobody());
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(waitingForShell, mark));
            Assert.IsNull(people.Here, "somebody is taken to be signed in on the waiting cover before the shell said who signed in");
            Assert.AreEqual(ritu, people.KeptFor);

            // While the shell is slow or retries on the waiting cover, nobody is signed in, however many states pass.
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(waitingForShell with { Nav = waitingForShell.Nav with { Failure = Failure.Offline } }, mark));
            Assert.IsNull(people.Here, "a retry state on the waiting cover signed her in");

            // The shell answers, takes the waiting cover away, and names her: it was she who signed in.
            ViewState ritusAgain = Naming(RituKey, "Ritu Sharma", null);
            Assert.AreEqual(PersonHeard.Same, people.Hear(ritusAgain, mark));
            Assert.AreEqual(ritu, people.Here, "she signed in again, the shell named her, and she is not taken to be signed in: what was held for her would never be shown");

            // It runs out again, and the way in is up with nobody named: nothing changes.
            ViewState nobodyUnder = WithTheWayInUp(NamingNobody());
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(nobodyUnder, mark));
            Assert.IsNull(people.Here);
            Assert.AreEqual(ritu, people.KeptFor);

            // Harpreet signs in. A state that names her while a way in is still up is a change of person all the same, and she is not
            // taken to be signed in while it is up.
            ViewState harpreetsUnder = WithTheWayInUp(Naming(HarpreetKey, "Harpreet Kaur", null));
            Assert.AreEqual(PersonHeard.Another, people.Hear(harpreetsUnder, mark), "a change of person is not seen while a way in is up");
            Assert.IsNull(people.Here, "a person named under a way in is taken to be signed in");
            Assert.AreEqual(harpreet, people.KeptFor);
            CollectionAssert.AreEqual(AtOnce, shell.Did.Take(4).ToArray(), "what must go first at a change of person did not, because a way in was up");

            // The waiting cover stays up with nobody named, and then the shell answers naming Harpreet: from that state she is the one signed in.
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(nobodyUnder, mark));
            Assert.IsNull(people.Here);
            Assert.AreEqual(PersonHeard.Same, people.Hear(Naming(HarpreetKey, "Harpreet Kaur", null), mark));
            Assert.AreEqual(harpreet, people.Here);
            Assert.AreEqual(harpreet, people.KeptFor);
            shell.SystemTakesItAway.SetResult();
            _ = await people.Last!.WaitAsync(Short);

            // She signs out in the client. From its first moment nobody is signed in and nothing is kept for anybody, and the state that
            // still names her while she leaves changes neither.
            gate.SessionIsEnding();
            people.SignedOut();
            Assert.IsNull(people.Here);
            Assert.IsNull(people.KeptFor);
            _ = gate.Asked([], out long leaving);
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(Naming(HarpreetKey, "Harpreet Kaur", null), leaving));
            Assert.IsNull(people.Here, "a person who is signing out is taken to be signed in");
            Assert.IsNull(people.KeptFor);

            // A start that read no mark, though something is kept, can say of nobody that what is kept is theirs: a reminder that is a
            // person's is not kept by it.
            PersonChange unknown = new(new PersonWatch(null, somethingKept: true), new SessionGate(), keeper, new RecordedShell().Parts, _ => { });
            Assert.IsNull(unknown.KeptFor, "a start with no mark says whom what is kept is kept for");
            Assert.IsNull(unknown.Here);
        }
        finally
        {
            Delete(folder);
        }
    }

    // Replays the core's conversation `session-ended.json` through the real core, each state heard as the host hears it: Ravneet Kaur is
    // signed in, an answer says her session is over, the way in is put up over her work, somebody signs in at it and the shell is asked
    // who; and, when it was she, her session later ends once more. The server of the conversation names nobody by id, so the core has
    // nothing to ask the shell again for until somebody has signed in. With `anotherSignsIn`, the shell's answer after the way in has
    // gone names Manpreet Kaur, and the conversation is left there.
    private static async Task<List<HeardStep>> PlaySessionEndedAsync(PersonChange people, SessionGate gate, RecordedShell shell, bool anotherSignsIn)
    {
        string transcripts = Path.Combine(Repository.Root, "crates", "navree-native-core", "transcripts");
        JsonNode transcript = JsonNode.Parse(await File.ReadAllTextAsync(Path.Combine(transcripts, "session-ended.json")))!;
        JsonObject config = JsonNode.Parse("""{"origin":"https://shop.example","client":{"platform":"android","form":"phone","vocab":0,"text_percent":100}}""")!.AsObject();
        if (transcript["config"] is JsonObject overrides)
        {
            foreach (KeyValuePair<string, JsonNode?> over in overrides)
            {
                config[over.Key] = over.Value?.DeepClone();
            }
        }

        List<HeardStep> seen = [];
        using CoreClient client = new(System.Text.Encoding.UTF8.GetBytes(config.ToJsonString()), _ => { });
        foreach (JsonNode? step in transcript["steps"]!.AsArray())
        {
            JsonObject send = step!["send"]!.DeepClone().AsObject();
            bool anotherNamed = false;
            if (step["body"] is JsonNode body)
            {
                string text = await File.ReadAllTextAsync(Path.Combine(transcripts, "fixtures", body.GetValue<string>()));

                // The first shell to answer while the waiting cover is up after the sign-in says who signed in at it.
                if (anotherSignsIn && body.GetValue<string>() == "made/shell-routes.json" && seen.Count > 0 && seen[^1].WayInUp)
                {
                    string hers = text;
                    text = text.Replace("Ravneet Kaur", "Manpreet Kaur", StringComparison.Ordinal).Replace("ravneet.kaur@example.com", "manpreet.kaur@example.com", StringComparison.Ordinal);
                    Expect.That(text != hers, "the shell of the conversation does not name Ravneet Kaur: this test shows nothing");
                    anotherNamed = true;
                }

                send[send["t"]!.GetValue<string>() == "cache" ? "value" : "body"] = text;
            }

            Update update = await client.SendAsync(send.ToJsonString());
            _ = gate.Asked(update.Effects, out long mark);

            // As the host does: who the state names is heard before any window is given it, and what has been taken away by then is
            // what a window finds taken away.
            PersonHeard heard = people.Hear(update.State, mark);
            seen.Add(new HeardStep(update.State.App?.User?.Name, PersonChange.WayInIsUp(update.State), heard, people.Here, shell.Did.Length));
            if (anotherNamed)
            {
                break;
            }
        }

        return seen;
    }

    private static string Told(List<HeardStep> seen) =>
        string.Join(", ", seen.Select((step, at) => at + " " + (step.Named ?? "nobody") + (step.WayInUp ? " way-in" : string.Empty) + (step.Here is null ? " -" : " here")));

    [TestMethod]
    public async Task In_the_cores_own_conversation_of_a_session_that_ended_nobody_is_signed_in_from_the_way_in_going_up_until_the_shell_has_answered_after_it_went()
    {
        string folder = Folder();
        try
        {
            using SessionKeeper keeper = new(new KeptStore(folder, App), new CookieStore(Shop));
            _ = keeper.Open();
            SessionGate gate = new();
            RecordedShell shell = new();
            PersonChange people = new(new PersonWatch(), gate, keeper, shell.Parts, _ => { });
            List<HeardStep> seen = await PlaySessionEndedAsync(people, gate, shell, anotherSignsIn: false);

            // The 401 puts the locked sign-in cover up and clears app.user; after the sign-in redirect, the locked waiting cover stays up
            // until the shell asked after the sign-in answers at step `gone`.
            int up = seen.FindIndex(step => step.WayInUp);
            int gone = up < 0 ? -1 : seen.FindIndex(up, step => !step.WayInUp);
            int again = gone < 0 ? -1 : seen.FindIndex(gone, step => step.WayInUp);
            Expect.That(up > 0 && gone > up + 1 && again > gone, "the conversation does not put the way in up, hold it through the waiting cover until the shell answers, and put it up again: this test shows nothing (" + Told(seen) + ")");
            string? her = seen[up - 1].Here;
            Assert.IsNotNull(her, "nobody was signed in before the session ended: this test shows nothing (" + Told(seen) + ")");

            // From the 401 through the sign-in form and the post-sign-in waiting cover, the core names nobody and keeps the locked cover up,
            // so nobody is signed in at any step before the shell answers.
            for (int at = up; at < gone; at++)
            {
                Assert.IsTrue(seen[at].WayInUp, "step " + at + ": the locked cover went down before the shell answered (" + Told(seen) + ")");
                Assert.IsNull(seen[at].Named, "step " + at + ": the core still named someone under the sign-in or waiting cover (" + Told(seen) + ")");
                Assert.IsNull(seen[at].Here, "step " + at + ": the person whose session ended is taken to be signed in while the cover is up (" + Told(seen) + ")");
            }

            // The shell answers at `gone`: the waiting cover goes and she is named again, until her session ends a second time.
            Assert.AreEqual("Ravneet Kaur", seen[gone].Named, "the shell's answer did not name Ravneet Kaur (" + Told(seen) + ")");
            for (int at = gone; at < again; at++)
            {
                Assert.AreEqual(her, seen[at].Here, "step " + at + ": she signed in again, the shell named her, and she is not taken to be signed in, so what was held for her would not be shown (" + Told(seen) + ")");
            }

            // It ends again, with a form being sent: from that answer on, nobody is named and nobody is signed in.
            Assert.IsNull(seen[again].Named, "the core still named her under the second way in: " + Told(seen));
            Assert.IsNull(seen[again].Here, "the second end of her session left her taken to be signed in (" + Told(seen) + ")");
            Expect.That(seen.All(step => !step.WayInUp || step.Here is null), "somebody is taken to be signed in while a way in is up: " + Told(seen));

            // She was one person throughout: nothing was taken away, and what is kept is still kept for her.
            Assert.IsNull(people.Last, "the end of her session was taken for a change of person");
            Assert.AreEqual(0, shell.Did.Length);
            Assert.AreEqual(her, people.KeptFor);
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task In_the_cores_own_conversation_another_person_who_signs_in_at_the_way_in_is_never_taken_for_the_one_whose_session_ended()
    {
        string folder = Folder();
        try
        {
            using SessionKeeper keeper = new(new KeptStore(folder, App), new CookieStore(Shop));
            _ = keeper.Open();
            SessionGate gate = new();
            RecordedShell shell = new();
            PersonChange people = new(new PersonWatch(), gate, keeper, shell.Parts, _ => { });
            List<HeardStep> seen = await PlaySessionEndedAsync(people, gate, shell, anotherSignsIn: true);

            int up = seen.FindIndex(step => step.WayInUp);
            Expect.That(up > 0 && seen.Count > up + 2 && !seen[^1].WayInUp, "the conversation does not put the way in up, hold the waiting cover and then have the shell answer: this test shows nothing (" + Told(seen) + ")");
            string? ravneet = seen[up - 1].Here;
            Assert.IsNotNull(ravneet, "nobody was signed in before the session ended: this test shows nothing (" + Told(seen) + ")");

            // From the answer that says Ravneet's session is over, through Manpreet's sign-in and the waiting cover while the shell is on its way:
            // nobody is named, the cover stays locked, and nobody is signed in. So nothing that was held for her is shown in any of these.
            for (int at = up; at < seen.Count - 1; at++)
            {
                Assert.IsTrue(seen[at].WayInUp, "step " + at + ": the locked cover went down before the shell answered (" + Told(seen) + ")");
                Assert.IsNull(seen[at].Named, "step " + at + ": someone was named under the cover before the shell answered (" + Told(seen) + ")");
                Assert.IsNull(seen[at].Here, "step " + at + ": somebody is taken to be signed in before the shell has said who signed in at the way in, so what was held for Ravneet would be shown to Manpreet (" + Told(seen) + ")");
                Assert.AreEqual(0, seen[at].TakenAway, "step " + at + ": something was taken away before anybody else was named (" + Told(seen) + ")");
            }

            // The shell answers, and names Manpreet: another person. What must go first has gone when his state has been heard, before
            // any window is given it, and only from that state is anybody signed in: he.
            HeardStep his = seen[^1];
            Assert.AreEqual("Manpreet Kaur", his.Named, "the shell after the way in did not name the other person: this test shows nothing (" + Told(seen) + ")");
            Assert.AreEqual(PersonHeard.Another, his.Heard, "another person who signed in at the way in is taken for the one whose session ended: everything kept for her is left to him");
            Expect.That(his.TakenAway >= AtOnce.Length, "what waited for the person before had not gone when the state that names the other person was heard");
            CollectionAssert.AreEqual(AtOnce, shell.Did.Take(4).ToArray(), "what must go first at the change of person did not, in its order");
            Assert.IsNotNull(his.Here, "the person the shell named after the way in is not taken to be signed in");
            Assert.AreNotEqual(ravneet, his.Here);
            Assert.AreEqual(his.Here, people.KeptFor);

            // At no step from the way in going up was Ravneet the one signed in.
            Expect.That(seen.Skip(up).All(step => step.Here != ravneet), "the person whose session ended was taken to be signed in after the way in went up: " + Told(seen));

            shell.SystemTakesItAway.SetResult();
            WipeReport report = await people.Last!.WaitAsync(Short);
            Expect.That(report.Complete, "the change of person did not finish: " + string.Join(", ", report.Failed.Select(failed => failed.Key)));
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task Another_person_who_signs_in_at_the_way_in_is_shown_nothing_that_was_held_for_the_person_before()
    {
        string folder = Folder();
        try
        {
            using SessionKeeper keeper = new(new KeptStore(folder, App), new CookieStore(Shop));
            _ = keeper.Open();
            string ritu = PersonWatch.MarkOfKey(RituKey);
            string harpreet = PersonWatch.MarkOfKey(HarpreetKey);
            SessionGate gate = new();
            RecordedShell shell = new();
            PersonChange people = new(new PersonWatch(), gate, keeper, shell.Parts, _ => { });
            _ = gate.Asked([], out long mark);

            // As the host hands a state on. What a window finds when it is given one is written down: who is signed in then, which is
            // whose reminders may be shown, and what has been taken away.
            List<string> order = [];
            void Deliver(string what, ViewState state) =>
                Delivery.Show(
                    state,
                    heard => order.Add(what + ": heard " + people.Hear(heard, mark)),
                    shown =>
                    {
                        string who = people.Here is null ? "nobody" : people.Here == ritu ? "Ritu" : people.Here == harpreet ? "Harpreet" : "somebody else";
                        string[] done = shell.Did;
                        string gone = done.Length == 0 ? "nothing taken away" : AtOnce.All(done.Contains) ? "what must go first gone" : "only " + string.Join("; ", done);
                        order.Add(what + ": shown with " + who + " signed in and " + gone);
                    },
                    _ => { });

            // Ritu is signed in at the counter in Ludhiana, and leaves the app open overnight.
            ViewState ritus = Naming(RituKey, "Ritu Sharma", null);
            Deliver("Ritu's shell", ritus);

            // At ten past two a part of her page is fetched again, and the answer says her session is over: the way in goes up and nobody is named.
            ViewState covered = WithTheWayInUp(NamingNobody());
            Deliver("the way in goes up", covered);

            // At twenty past nine Harpreet signs in at the way in. The waiting cover stays locked with nobody named until the shell answers.
            Deliver("the waiting cover", covered);
            Deliver("the waiting cover retries", covered with { Nav = covered.Nav with { Failure = Failure.Offline } });
            Deliver("every part is read again", covered);

            // The shell answers, and names Harpreet.
            Deliver("Harpreet's shell", Naming(HarpreetKey, "Harpreet Kaur", null));

            CollectionAssert.AreEqual(
                new[]
                {
                    "Ritu's shell: heard First", "Ritu's shell: shown with Ritu signed in and nothing taken away",
                    "the way in goes up: heard Nobody", "the way in goes up: shown with nobody signed in and nothing taken away",
                    "the waiting cover: heard Nobody", "the waiting cover: shown with nobody signed in and nothing taken away",
                    "the waiting cover retries: heard Nobody", "the waiting cover retries: shown with nobody signed in and nothing taken away",
                    "every part is read again: heard Nobody", "every part is read again: shown with nobody signed in and nothing taken away",
                    "Harpreet's shell: heard Another", "Harpreet's shell: shown with Harpreet signed in and what must go first gone",
                },
                order,
                "the person whose session ended was taken to be signed in after the way in went up, or what waited for her had not gone when the windows were given the state that names the other person");

            shell.SystemTakesItAway.SetResult();
            WipeReport report = await people.Last!.WaitAsync(Short);
            Expect.That(report.Complete, "the change of person did not finish: " + string.Join(", ", report.Failed.Select(failed => failed.Key)));

            // When the shell asked for after a sign-in never answers, the waiting cover stays up with nobody named, however many states pass:
            // what was held stays held, and is not shown to whoever signed in.
            SessionGate otherGate = new();
            _ = otherGate.Asked([], out long otherMark);
            PersonChange unanswered = new(new PersonWatch(), otherGate, keeper, new RecordedShell().Parts, _ => { });
            Assert.AreEqual(PersonHeard.First, unanswered.Hear(ritus, otherMark));
            Assert.AreEqual(ritu, unanswered.Here);
            Assert.AreEqual(PersonHeard.Nobody, unanswered.Hear(covered, otherMark));
            for (int state = 0; state < 40; state++)
            {
                Assert.AreEqual(PersonHeard.Nobody, unanswered.Hear(covered, otherMark));
                Assert.IsNull(unanswered.Here, "state " + state + " on the waiting cover, with no answer from the shell, signed in the person whose session ended");
            }
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task A_person_who_becomes_the_one_signed_in_with_no_change_of_person_is_given_what_waited_for_nobody_before_the_windows_are_given_the_state()
    {
        string folder = Folder();
        try
        {
            KeptStore kept = new(folder, App);
            using SessionKeeper keeper = new(kept, new CookieStore(Shop));
            _ = keeper.Open();
            string ritu = PersonWatch.MarkOfKey(RituKey);
            string harpreet = PersonWatch.MarkOfKey(HarpreetKey);
            SessionGate gate = new();
            RecordedShell shell = new();
            ConcurrentQueue<string> problems = new();
            PersonChange people = new(new PersonWatch(keeper.KeptPerson(), keeper.KeepsOfAPerson()), gate, keeper, shell.Parts, problems.Enqueue);
            _ = gate.Asked([], out long mark);

            // Ritu signs in at the counter after somebody signed out. Her orders page is on screen before the shell's answer names her,
            // and she asks to be reminded of a call at five: nobody has been named, so the reminder is nobody's.
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(NamingNobody(), mark));
            Assert.IsNull(people.KeptFor, "somebody is named before the shell said so: this test shows nothing");
            Assert.AreEqual(0, shell.GivenTo.Length, "what waits for nobody was given away though nobody was named");

            // The shell names her, the first since the sign-out: what waited for nobody is hers, and it is given before the windows are
            // given the state that names her.
            List<string> order = [];
            Delivery.Show(
                Naming(RituKey, "Ritu Sharma", null),
                heard => order.Add("heard " + people.Hear(heard, mark) + " and given to " + shell.GivenTo.Length),
                shown => order.Add("shown " + shown.App?.User?.Name + " with " + shell.GivenTo.Length + " given"),
                problems.Enqueue);
            CollectionAssert.AreEqual(new[] { "heard First and given to 1", "shown Ritu Sharma with 1 given" }, order, "what waited for nobody was not given to the first person named before the windows were given the state");
            CollectionAssert.AreEqual(new[] { ritu }, shell.GivenTo, "what waited for nobody stays nobody's after the first person was named: it would be shown to whoever is there, at every later start");

            // Named again while she is signed in: nothing more is given.
            ViewState ritus = Naming(RituKey, "Ritu Sharma", null);
            Assert.AreEqual(PersonHeard.Same, people.Hear(ritus, mark));
            Assert.AreEqual(1, shell.GivenTo.Length, "what waits for nobody was given again to a person who was signed in already");

            // Her session ends, and the way in goes up: anything that waited for nobody goes when the cover rises, and when she signs in
            // again (the person before, Same) she is NEVER given what waited for nobody — it is let go of before the windows see the state.
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(WithTheWayInUp(NamingNobody()), mark));
            Assert.IsNull(people.Here, "somebody is taken to be signed in before the shell said who signed in at the way in");
            Assert.AreEqual(ritu, people.KeptFor);
            Assert.AreEqual(1, shell.GivenTo.Length, "what waits for nobody was given to somebody while nobody was signed in");
            Assert.AreEqual(1, shell.DroppedNobodys, "what waited for nobody was not dropped when the cover went up after a person was kept");

            // The shell answers, and names her: the same person. What waited for nobody is dropped, never given to the person before.
            order.Clear();
            Delivery.Show(
                Naming(RituKey, "Ritu Sharma", null),
                heard => order.Add("heard " + people.Hear(heard, mark) + " and dropped " + shell.DroppedNobodys),
                shown => order.Add("shown " + shown.App?.User?.Name + " with " + shell.GivenTo.Length + " given"),
                problems.Enqueue);
            CollectionAssert.AreEqual(
                new[] { "heard Same and dropped 2", "shown Ritu Sharma with 1 given" },
                order);
            CollectionAssert.AreEqual(new[] { ritu }, shell.GivenTo, "what waited for nobody was given to the person before");

            // The same when the state named nobody with no cover for a newcomer's whole session, and then names Ritu again: dropped, never given.
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(NamingNobody(), mark));
            Assert.AreEqual(1, shell.GivenTo.Length);
            ViewState ritusBack = Naming(RituKey, "Ritu Sharma", null);
            Assert.AreEqual(PersonHeard.Same, people.Hear(ritusBack, mark));
            Assert.AreEqual(1, shell.GivenTo.Length, "she is the person before, and was given what a newcomer may have asked for while nobody was named");
            Assert.AreEqual(3, shell.DroppedNobodys);

            // It ends again. This time Harpreet signs in at the way in, and the shell names her: another person.
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(WithTheWayInUp(NamingNobody()), mark));
            Assert.IsNull(people.Here);
            Assert.AreEqual(PersonHeard.Another, people.Hear(Naming(HarpreetKey, "Harpreet Kaur", null), mark));
            Assert.AreEqual(harpreet, people.Here);
            Assert.AreEqual(1, shell.GivenTo.Length, "a person who signed in with no sign-out between was given what waited: a change of person takes it away");
            CollectionAssert.AreEqual(AtOnce, shell.Did.Take(4).ToArray(), "the change of person did not take away what waited");
            shell.SystemTakesItAway.SetResult();
            _ = await people.Last!.WaitAsync(Short);

            // She signs out, and Ritu signs in after her: Ritu is the first again (nobody was kept before her), so she is given what waited for nobody since.
            gate.SessionIsEnding();
            people.SignedOut();
            gate.CoreIsToldTheSessionEnded();
            _ = gate.Asked([], out long next);
            Assert.AreEqual(PersonHeard.First, people.Hear(Naming(RituKey, "Ritu Sharma", null), next));
            CollectionAssert.AreEqual(new[] { ritu, ritu }, shell.GivenTo);
            Assert.AreEqual(0, problems.Count, string.Join(" | ", problems));

            // A start that read her mark hears her as the same person (the person before): she is never given what waited for nobody; it is dropped instead.
            RecordedShell known = new();
            PersonChange started = new(new PersonWatch(ritu), gate, keeper, known.Parts, _ => { });
            Assert.AreEqual(0, known.GivenTo.Length);
            Assert.AreEqual(PersonHeard.Same, started.Hear(Naming(RituKey, "Ritu Sharma", null), next));
            Assert.AreEqual(0, known.GivenTo.Length, "a start that read the person before's mark gave her what waited for nobody");
            Assert.AreEqual(1, known.DroppedNobodys);
            Assert.AreEqual(PersonHeard.Same, started.Hear(Naming(RituKey, "Ritu Sharma", null), next));
            Assert.AreEqual(1, known.DroppedNobodys);

            // When the giving fails, it is said with nothing of the person, and she is still written down as the one what is kept is
            // kept for.
            KeptStore otherKept = new(Path.Combine(folder, "other"), App);
            using SessionKeeper other = new(otherKept, new CookieStore(Shop));
            _ = other.Open();
            RecordedShell failing = new() { GivingThrows = new IOException("Harpreet Kaur's reminders could not be written") };
            ConcurrentQueue<string> said = new();
            SessionGate otherGate = new();
            _ = otherGate.Asked([], out long otherMark);
            Assert.AreEqual(PersonHeard.First, new PersonChange(new PersonWatch(), otherGate, other, failing.Parts, said.Enqueue).Hear(Naming(HarpreetKey, "Harpreet Kaur", null), otherMark));
            await other.DrainAsync().WaitAsync(Short);
            Assert.AreEqual(1, said.Count, string.Join(" | ", said));
            Expect.That(said.TryPeek(out string? line) && line.Contains("could not be given", StringComparison.Ordinal) && !line.Contains("Harpreet", StringComparison.Ordinal), "what is said names a person, or not what failed");
            Assert.AreEqual(harpreet, otherKept.Read(SessionKeeper.PersonKey), "the mark of the first person named was not written because giving them what waited failed");
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task A_person_the_core_names_with_no_key_is_said_once_and_is_not_the_person_a_key_was_kept_for()
    {
        string folder = Folder();
        try
        {
            KeptStore kept = new(folder, App);
            using SessionKeeper keeper = new(kept, new CookieStore(Shop));
            _ = keeper.Open();
            keeper.KeepPerson(PersonWatch.MarkOfKey(RituKey));
            await keeper.DrainAsync().WaitAsync(Short);

            // The last run kept Ritu's key. A core that gives no key stands beside the client now, and the person it names is shown with
            // her name and her email.
            SessionGate gate = new();
            RecordedShell shell = new();
            ConcurrentQueue<string> problems = new();
            PersonChange people = new(new PersonWatch(keeper.KeptPerson(), keeper.KeepsOfAPerson()), gate, keeper, shell.Parts, problems.Enqueue);
            _ = gate.Asked([], out long mark);
            Assert.AreEqual(
                PersonHeard.Another,
                people.Hear(Naming(null, "Ritu Sharma", "ritu.sharma@example.com"), mark),
                "a person named with no key is taken for the one whose key was kept: what was kept stays for whoever signed in");
            CollectionAssert.AreEqual(AtOnce, shell.Did, "what must go first for a person who cannot be told to be the one before did not go");

            // It is said, once, and with nothing of the person.
            Assert.AreEqual(PersonHeard.Same, people.Hear(Naming(null, "Ritu Sharma", "ritu.sharma@example.com"), mark));
            Assert.AreEqual(PersonHeard.Same, people.Hear(Naming(string.Empty, "Ritu Sharma", "ritu.sharma@example.com"), mark));
            Assert.AreEqual(1, problems.Count, string.Join(" | ", problems));
            Expect.That(
                problems.TryPeek(out string? said) && said.Contains("no key", StringComparison.Ordinal) && !said.Contains("Ritu", StringComparison.Ordinal) && !said.Contains("example", StringComparison.Ordinal),
                "what is said names a person, or does not say that no key was given");

            shell.SystemTakesItAway.SetResult();
            WipeReport report = await people.Last!.WaitAsync(Short);
            await keeper.DrainAsync().WaitAsync(Short);
            Expect.That(report.Complete, "the change did not finish: " + string.Join(", ", report.Failed.Select(failed => failed.Key)));
            Assert.AreEqual(PersonWatch.MarkOf("Ritu Sharma", "ritu.sharma@example.com"), kept.Read(SessionKeeper.PersonKey), "the mark kept is not that of the person named now, made from the name and email");

            // A core that gives a key says nothing.
            ConcurrentQueue<string> none = new();
            SessionGate otherGate = new();
            PersonChange keyed = new(new PersonWatch(), otherGate, keeper, new RecordedShell().Parts, none.Enqueue);
            _ = otherGate.Asked([], out long otherMark);
            Assert.AreEqual(PersonHeard.First, keyed.Hear(Naming(HarpreetKey, "Harpreet Kaur", null), otherMark));
            Assert.AreEqual(0, none.Count, string.Join(" | ", none));
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task Two_accounts_of_one_name_in_the_cores_own_conversation_are_a_change_of_person()
    {
        // The core's conversation `keep-person.json`: two people of one name and no email, one after the other on one device, whom the
        // server names by two ids. It is replayed through the real core, and each state is heard as the host hears it.
        string transcripts = Path.Combine(Repository.Root, "crates", "navree-native-core", "transcripts");
        JsonNode transcript = JsonNode.Parse(await File.ReadAllTextAsync(Path.Combine(transcripts, "keep-person.json")))!;
        JsonObject config = JsonNode.Parse("""{"origin":"https://shop.example","client":{"platform":"android","form":"phone","vocab":0,"text_percent":100}}""")!.AsObject();
        foreach (KeyValuePair<string, JsonNode?> over in transcript["config"]!.AsObject())
        {
            config[over.Key] = over.Value?.DeepClone();
        }

        string folder = Folder();
        try
        {
            using SessionKeeper keeper = new(new KeptStore(folder, App), new CookieStore(Shop));
            _ = keeper.Open();
            SessionGate gate = new();
            RecordedShell shell = new();
            PersonChange people = new(new PersonWatch(), gate, keeper, shell.Parts, _ => { });
            List<string> heard = [];
            List<string?> keys = [];
            using CoreClient client = new(System.Text.Encoding.UTF8.GetBytes(config.ToJsonString()), _ => { });

            // The first launch of the conversation: the first Harpreet Singh, and then, in the same launch, the other.
            foreach (JsonNode? step in transcript["steps"]!.AsArray().Take(7))
            {
                JsonObject send = step!["send"]!.DeepClone().AsObject();
                if (step["body"] is JsonNode body)
                {
                    string text = await File.ReadAllTextAsync(Path.Combine(transcripts, "fixtures", body.GetValue<string>()));
                    send[send["t"]!.GetValue<string>() == "cache" ? "value" : "body"] = text;
                }

                Update update = await client.SendAsync(send.ToJsonString());
                _ = gate.Asked(update.Effects, out long mark);
                heard.Add(people.Hear(update.State, mark).ToString());
                keys.Add(update.State.App?.User?.Key);
            }

            string?[] given = [.. keys.Where(key => key is not null).Distinct()];
            Assert.AreEqual(2, given.Length, "the core did not give each of the two accounts a key of its own: " + string.Join(", ", keys.Select(key => key ?? "none")));
            Assert.AreEqual("1bc3dcdc8ebf5d231237007ccc4903b602f64185813b65bc5b6d1f5f798fcab7", given[0], "the first account's key is not the one the conversation writes down");
            Assert.AreEqual("9cc5ffc2817cc3c911c721e28112cc30ae527d56ce46fbc784f9207caf606c75", given[1], "the second account's key is not the one the conversation writes down");

            // The shell's first answer names the first of them; the answer in step 7 names the other, with the same name and no email.
            Assert.AreEqual("First", heard[2], "the first Harpreet Singh was not heard as the first person: " + string.Join(", ", heard));
            Assert.AreEqual("Another", heard[6], "the second Harpreet Singh is taken for the first: everything kept for the first is left to him (" + string.Join(", ", heard) + ")");
            Assert.AreEqual(1, heard.Count(said => said == "Another"), "the change was heard more than once: " + string.Join(", ", heard));
            CollectionAssert.AreEqual(AtOnce, shell.Did.Take(4).ToArray(), "what must go first for the second Harpreet Singh did not");

            shell.SystemTakesItAway.SetResult();
            WipeReport report = await people.Last!.WaitAsync(Short);
            Expect.That(report.Complete, "the change did not finish: " + string.Join(", ", report.Failed.Select(failed => failed.Key)));
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task NobodyIsHereFromASessionsEndWithNoCoverUntilTheShellAnswers()
    {
        // When a session ends without a cover (the shell answers with nobody after a session expired, or an ordinary sign-in page is
        // navigated to without a locked modal), nobody is taken to be signed in across every screen and redirect until the shell answers
        // again and names who signed in.
        string transcripts = Path.Combine(Repository.Root, "crates", "navree-native-core", "transcripts");
        string signedInShell = await File.ReadAllTextAsync(Path.Combine(transcripts, "fixtures", "store", "shell.json"));
        string homeBody = await File.ReadAllTextAsync(Path.Combine(transcripts, "fixtures", "made", "home.json"));
        string signInPage = await File.ReadAllTextAsync(Path.Combine(transcripts, "fixtures", "made", "sign-in.json"));
        string otherShell = await File.ReadAllTextAsync(Path.Combine(transcripts, "fixtures", "made", "shell-other.json"));

        // A shell with the same destinations and no signed-in user: what a server returns when a session expired with no cover.
        JsonObject anonymousShellNode = JsonNode.Parse(signedInShell)!.AsObject();
        _ = anonymousShellNode.Remove("user");
        string anonymousShell = anonymousShellNode.ToJsonString();

        string folder = Folder();
        try
        {
            using SessionKeeper keeper = new(new KeptStore(folder, App), new CookieStore(Shop));
            _ = keeper.Open();
            SessionGate gate = new();
            RecordedShell shell = new();
            PersonChange people = new(new PersonWatch(), gate, keeper, shell.Parts, _ => { });
            byte[] config = System.Text.Encoding.UTF8.GetBytes("""{"origin":"https://shop.example","client":{"platform":"windows","form":"desktop","vocab":0,"text_percent":100}}""");
            using CoreClient client = new(config, _ => { });

            async Task<Update> StepAsync(string eventJson)
            {
                Update update = await client.SendAsync(eventJson);
                _ = gate.Asked(update.Effects, out long mark);
                _ = people.Hear(update.State, mark);
                return update;
            }

            // Ravneet Kaur starts signed in by id.
            Update started = await StepAsync(Events.Start);
            long firstShell = started.Effects.OfType<FetchEffect>().Single().Request;
            Update named = await StepAsync(Events.Response(firstShell, 200, "application/vnd.navree.native+json", null, [new KeyValuePair<string, string>("x-navree-person", "0193a4b2-0000-7000-8000-00000000ra01")], signedInShell));
            string? ravneet = people.Here;
            Assert.IsNotNull(ravneet, "Ravneet Kaur was not taken to be signed in after her shell answered");
            long firstHome = named.Effects.OfType<FetchEffect>().Single().Request;
            _ = await StepAsync(Events.Response(firstHome, 200, "application/vnd.navree.native+json", null, [new KeyValuePair<string, string>("x-navree-person", "0193a4b2-0000-7000-8000-00000000ra01")], homeBody));

            // The app comes back after her session expired: the shell answers with nobody and puts up no cover.
            Update rechecked = await StepAsync(Events.Start);
            long secondShell = rechecked.Effects.OfType<FetchEffect>().Single().Request;
            Update endedNoCover = await StepAsync(Events.Response(secondShell, 200, "application/vnd.navree.native+json", null, [], anonymousShell));
            Assert.IsFalse(PersonChange.WayInIsUp(endedNoCover.State), "the session's end put up a cover: this test is for an end with no cover");
            Assert.IsNull(endedNoCover.State.App?.User, "the core still named a person after the shell answered with nobody");
            Assert.IsNull(people.Here, "somebody is still taken to be signed in after a session's end with no cover");
            Assert.AreEqual(ravneet, people.KeptFor, "what was kept stopped being kept for Ravneet before another person was named");

            // Whoever is at the desk opens the ordinary sign-in page (not a cover) and posts it: still no cover, and Here stays null
            // through every step until the shell answers.
            long anonHome = endedNoCover.Effects.OfType<FetchEffect>().Single().Request;
            Update redirected = await StepAsync(Events.Response(anonHome, 302, null, "/login", [], string.Empty));
            Assert.IsFalse(PersonChange.WayInIsUp(redirected.State));
            Assert.IsNull(people.Here);

            long loginGet = redirected.Effects.OfType<FetchEffect>().Single().Request;
            Update loginShown = await StepAsync(Events.Response(loginGet, 200, "application/vnd.navree.native+json", null, [], signInPage));
            Assert.IsFalse(PersonChange.WayInIsUp(loginShown.State));
            Assert.IsNull(people.Here);

            // Harpreet Singh finishes signing in and the shell answers naming him: only now is somebody Here, after Ravneet's data went.
            Update askAfterSignIn = await StepAsync(Events.Start);
            Assert.IsNull(people.Here, "asking for the shell signed somebody in before the shell answered");
            long thirdShell = askAfterSignIn.Effects.OfType<FetchEffect>().Single().Request;
            _ = await StepAsync(Events.Response(thirdShell, 200, "application/vnd.navree.native+json", null, [new KeyValuePair<string, string>("x-navree-person", "0193a4b2-0000-7000-8000-00000000hs02")], otherShell));
            Assert.IsNotNull(people.Here);
            Assert.AreNotEqual(ravneet, people.Here);
            CollectionAssert.AreEqual(AtOnce, shell.Did.Take(4).ToArray());

            shell.SystemTakesItAway.SetResult();
            WipeReport report = await people.Last!.WaitAsync(Short);
            Expect.That(report.Complete, "the change of person did not finish");
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task Six_new_core_conversations_keep_person_hearing_and_changes_sound()
    {
        // Replays person-key-steady, person-key-other-id, session-ended-outside, sign-in-cover, sign-in-another-person, and sign-in-shell-fails
        // through PersonChange as the host hears each update.
        string[] names =
        [
            "person-key-steady.json",
            "person-key-other-id.json",
            "session-ended-outside.json",
            "sign-in-cover.json",
            "sign-in-another-person.json",
            "sign-in-shell-fails.json",
        ];

        string transcripts = Path.Combine(Repository.Root, "crates", "navree-native-core", "transcripts");
        foreach (string name in names)
        {
            string folder = Folder();
            try
            {
                JsonNode transcript = JsonNode.Parse(await File.ReadAllTextAsync(Path.Combine(transcripts, name)))!;
                JsonObject config = JsonNode.Parse("""{"origin":"https://shop.example","client":{"platform":"android","form":"phone","vocab":0,"text_percent":100}}""")!.AsObject();
                if (transcript["config"] is JsonObject overrides)
                {
                    foreach (KeyValuePair<string, JsonNode?> over in overrides)
                    {
                        config[over.Key] = over.Value?.DeepClone();
                    }
                }

                using SessionKeeper keeper = new(new KeptStore(folder, App), new CookieStore(Shop));
                _ = keeper.Open();
                SessionGate gate = new();
                RecordedShell shell = new();
                shell.SystemTakesItAway.SetResult();
                PersonChange people = new(new PersonWatch(), gate, keeper, shell.Parts, _ => { });
                CoreClient? client = new(System.Text.Encoding.UTF8.GetBytes(config.ToJsonString()), _ => { });
                try
                {
                    List<PersonHeard> heardSteps = [];
                    foreach (JsonNode? step in transcript["steps"]!.AsArray())
                    {
                        if (step!["restart"]?.GetValue<bool>() == true)
                        {
                            if (people.Last is not null)
                            {
                                _ = await people.Last.WaitAsync(Short);
                            }

                            await keeper.DrainAsync().WaitAsync(Short);
                            client.Dispose();
                            client = new CoreClient(System.Text.Encoding.UTF8.GetBytes(config.ToJsonString()), _ => { });
                            gate = new SessionGate();
                            shell = new RecordedShell();
                            shell.SystemTakesItAway.SetResult();
                            people = new PersonChange(new PersonWatch(keeper.KeptPerson(), keeper.KeepsOfAPerson()), gate, keeper, shell.Parts, _ => { });
                        }

                        JsonObject send = step["send"]!.DeepClone().AsObject();
                        if (step["body"] is JsonNode body)
                        {
                            string text = await File.ReadAllTextAsync(Path.Combine(transcripts, "fixtures", body.GetValue<string>()));
                            send[send["t"]!.GetValue<string>() == "cache" ? "value" : "body"] = text;
                        }

                        Update update = await client.SendAsync(send.ToJsonString());
                        _ = gate.Asked(update.Effects, out long mark);
                        PersonHeard heard = people.Hear(update.State, mark);
                        heardSteps.Add(heard);

                        if (PersonChange.WayInIsUp(update.State) || update.State.App?.User is null)
                        {
                            Assert.IsNull(people.Here, name + ": somebody was taken to be Here while a cover was up or nobody was named");
                        }
                    }

                    if (name is "person-key-other-id.json")
                    {
                        Assert.AreEqual(2, heardSteps.Count(h => h == PersonHeard.Another), name + ": each transition between distinct accounts or id modes must be heard as Another");
                    }
                    else if (name is "sign-in-another-person.json")
                    {
                        Assert.AreEqual(1, heardSteps.Count(h => h == PersonHeard.Another), name + ": signing in as another person must trigger PersonHeard.Another");
                    }
                    else if (name is "sign-in-cover.json" or "sign-in-shell-fails.json" or "session-ended-outside.json")
                    {
                        Assert.AreEqual(0, heardSteps.Count(h => h == PersonHeard.Another), name + ": the same person returning after a cover must not trigger a wipe");
                        Assert.IsNotNull(people.Here, name + ": the returning person was not Here after the shell answered");
                    }

                    if (people.Last is not null)
                    {
                        WipeReport report = await people.Last.WaitAsync(Short);
                        Expect.That(report.Complete, name + ": a change of person did not finish");
                    }
                }
                finally
                {
                    client?.Dispose();
                }
            }
            finally
            {
                Delete(folder);
            }
        }
    }

    [TestMethod]
    public void AReminderAskedWhileNobodyIsNamedNeverGoesToThePersonBeforeInPersonChange()
    {
        // Simran Kaur is signed in at the counter, then her session ends and a newcomer works for their whole session without ever being
        // named (app.user stays null throughout). Reminders asked during that unnamed session are owned by nobody (Here == null), dropped
        // when the newcomer's session ends at a cover, and dropped again if Simran returns — never given to Simran.
        string folder = Folder();
        try
        {
            using SessionKeeper keeper = new(new KeptStore(folder, App), new CookieStore(Shop));
            _ = keeper.Open();
            SessionGate gate = new();
            List<(string Id, string? Owner)> waiting = [];
            PersonChangeParts parts = new(
                MoveWebOn: () => "web\\1",
                DropPictures: () => { },
                ClearFiles: () => Task.CompletedTask,
                TakeFromSystem: () =>
                {
                    waiting.Clear();
                    return Task.CompletedTask;
                },
                EmptyWeb: _ => Task.CompletedTask,
                OnWindow: step => step(),
                More: () => [],
                GiveWaitingTo: owner =>
                {
                    for (int i = 0; i < waiting.Count; i++)
                    {
                        if (waiting[i].Owner is null)
                        {
                            waiting[i] = (waiting[i].Id, owner);
                        }
                    }
                },
                DropWaitingForNobody: () => waiting.RemoveAll(item => item.Owner is null));

            PersonChange people = new(new PersonWatch(), gate, keeper, parts, _ => { });
            _ = gate.Asked([], out long mark);
            string simran = PersonWatch.MarkOfKey(RituKey);

            // Simran signs in and schedules her own reminder.
            Assert.AreEqual(PersonHeard.First, people.Hear(Naming(RituKey, "Simran Kaur", "simran.kaur@example.com"), mark));
            waiting.Add(("simrans-own", people.Here));
            Assert.AreEqual(simran, waiting.Single().Owner);

            // Her session expires without naming anyone; a newcomer uses the app for their whole session with nobody named and asks for
            // two reminders.
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(NamingNobody(), mark));
            Assert.IsNull(people.Here);
            Assert.AreEqual(simran, people.KeptFor);
            waiting.Add(("newcomer-one", people.Here));
            waiting.Add(("newcomer-two", people.Here));
            Assert.IsTrue(waiting.Where(item => item.Id.StartsWith("newcomer", StringComparison.Ordinal)).All(item => item.Owner is null));

            // Case 1: Simran signs back in directly with no cover between — the newcomer's reminders are dropped, never given to Simran,
            // while Simran's own reminder stays hers.
            Assert.AreEqual(PersonHeard.Same, people.Hear(Naming(RituKey, "Simran Kaur", "simran.kaur@example.com"), mark));
            CollectionAssert.AreEqual(new[] { ("simrans-own", (string?)simran) }, waiting);

            // Case 2: Unnamed newcomer asks for another reminder, then their session ends at a cover — dropped as soon as the cover goes up,
            // before Simran even finishes signing in at the cover.
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(NamingNobody(), mark));
            waiting.Add(("newcomer-three", people.Here));
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(WithTheWayInUp(NamingNobody()), mark));
            CollectionAssert.AreEqual(new[] { ("simrans-own", (string?)simran) }, waiting);
        }
        finally
        {
            Delete(folder);
        }
    }
}
