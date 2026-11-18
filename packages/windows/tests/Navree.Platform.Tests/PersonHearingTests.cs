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

    /// <summary>Completes when the system has taken away what it showed: the test says when.</summary>
    internal TaskCompletionSource SystemTakesItAway { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);

    /// <summary>When given, moving the web view's folder on throws it.</summary>
    internal Exception? MovingThrows { get; set; }

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
        ]);

    private void Do(string what)
    {
        lock (gate)
        {
            did.Add(what);
        }
    }
}

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
    public async Task Nobody_is_signed_in_while_the_sign_in_is_up_though_what_is_kept_is_still_kept_for_the_person_before()
    {
        // What waits for a person's eyes (a reminder) is shown by who is signed in now, and kept by whom what is kept is kept for. The
        // two are not the same between a session running out and somebody signing in.
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
            Assert.AreEqual(PersonHeard.Same, people.Hear(Naming(RituKey, "Ritu Sharma", null), mark));
            Assert.AreEqual(ritu, people.Here);

            // Her session runs out overnight and the sign-in is up over her work. What is kept is still hers, and nobody is signed in.
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(NamingNobody(), mark));
            Assert.IsNull(people.Here, "the person whose session ran out is still taken to be signed in: her reminder would be shown to whoever is at the counter");
            Assert.AreEqual(ritu, people.KeptFor, "what is kept stopped being hers when her session ran out: she would not find it when she signs in again");

            // She signs in again.
            Assert.AreEqual(PersonHeard.Same, people.Hear(Naming(RituKey, "Ritu Sharma", null), mark));
            Assert.AreEqual(ritu, people.Here);

            // It runs out again, and Harpreet signs in: from the state that names her, she is the one signed in and what is kept is hers.
            Assert.AreEqual(PersonHeard.Nobody, people.Hear(NamingNobody(), mark));
            Assert.AreEqual(PersonHeard.Another, people.Hear(Naming(HarpreetKey, "Harpreet Kaur", null), mark));
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
            Assert.AreEqual(PersonWatch.MarkOf("Ritu Sharma", "ritu.sharma@example.com"), kept.Read(SessionKeeper.PersonKey), "the mark kept is still the key of the person before");

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
}
