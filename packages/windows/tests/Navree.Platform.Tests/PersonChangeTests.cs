using Navree.Design;

namespace Navree.Platform.Tests;

/// <summary>
/// Another person signs in with no sign-out between: a session ran out, or a server ended it and said nothing of it to the client. The core
/// only has its cache emptied. What the client keeps of its own for the person before goes, and the cookies, which are already those of the
/// person who is here now, stay.
/// </summary>
[TestClass]
public sealed class PersonChangeTests
{
    private const string App = "Navree.Tests";
    private const string Orders = "page:https://shop.example/orders";
    private const string Waiting = "pending-notifications";
    private const string RitusReminder = "[{\"id\":\"dr-malhotra\",\"title\":\"Your appointment with Dr. Malhotra, Model Town, 10:30\"}]";
    private static readonly Origins Shop = new("https://shop.example");
    private static readonly TimeSpan Short = TimeSpan.FromSeconds(5);
    private static readonly string Ritu = PersonWatch.MarkOf("Ritu Sharma", "ritu.sharma@example.com");
    private static readonly string Harpreet = PersonWatch.MarkOf("Harpreet Kaur", "harpreet.kaur@example.com");

    private static string Folder() => Path.Combine(Path.GetTempPath(), "navree-person-" + Guid.NewGuid().ToString("N"));

    private static void Delete(string folder)
    {
        if (Directory.Exists(folder))
        {
            Directory.Delete(folder, recursive: true);
        }
    }

    // Ritu is signed in at the shop's counter in Ludhiana: her cookie in memory and on disk, her mark beside it, the widths she gave a
    // table, a reminder that waits for tomorrow morning, a snapshot of where she was, and a page of hers in the core's cache.
    private static async Task RituSignedInAsync(SessionKeeper keeper, CookieStore cookies)
    {
        cookies.Store("https://shop.example/", ["session=ritu; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
        keeper.KeepCookies();
        keeper.KeepPerson(Ritu);
        keeper.Write("table-layout:orders", "{\"widths\":[240,180]}");
        keeper.Write(Waiting, RitusReminder);
        keeper.Write("snapshot", "{\"typed\":\"Ritu's draft\"}");
        keeper.WriteCached(Orders, "{\"name\":\"Ritu Sharma, Ludhiana\"}");
        await keeper.DrainAsync().WaitAsync(Short);
    }

    // Her session has run out, and Harpreet signs in through the app's own form: the cookie in the store is Harpreet's from then on.
    private static void HarpreetSignsIn(SessionKeeper keeper, CookieStore cookies)
    {
        cookies.Store("https://shop.example/", ["session=harpreet; Path=/; Max-Age=3600"], DateTimeOffset.UtcNow);
        keeper.KeepCookies();
    }

    [TestMethod]
    public void The_first_person_named_is_remembered_and_only_another_one_with_no_sign_out_between_is_a_change()
    {
        PersonWatch watch = new();
        Assert.AreEqual(PersonHeard.Nobody, watch.Hear(null, null), "a state that names nobody is taken for a person");
        Assert.IsNull(watch.Named);

        Assert.AreEqual(PersonHeard.First, watch.Hear("Ritu Sharma", "ritu.sharma@example.com"));
        Assert.AreEqual(Ritu, watch.Named);
        Assert.AreEqual(PersonHeard.Same, watch.Hear("Ritu Sharma", "ritu.sharma@example.com"));

        // Her session runs out: the shell names nobody. She signs in again and finds everything as it was.
        Assert.AreEqual(PersonHeard.Nobody, watch.Hear(null, null));
        Assert.AreEqual(Ritu, watch.Named, "a state that names nobody made the watch forget who was here");
        Assert.AreEqual(PersonHeard.Same, watch.Hear("Ritu Sharma", "ritu.sharma@example.com"), "the same person signing in again is taken for another");

        // It runs out again over lunch, and this time Harpreet signs in. That is said once.
        Assert.AreEqual(PersonHeard.Nobody, watch.Hear(null, null));
        Assert.AreEqual(PersonHeard.Another, watch.Hear("Harpreet Kaur", "harpreet.kaur@example.com"), "another person with no sign-out between is not seen");
        Assert.AreEqual(Harpreet, watch.Named);
        Assert.AreEqual(PersonHeard.Same, watch.Hear("Harpreet Kaur", "harpreet.kaur@example.com"), "one change of person is said twice");

        // Harpreet signs out: the sign-out takes everything away itself, so Ritu, who signs in after it, is the first again.
        watch.SignedOut();
        Assert.IsNull(watch.Named);
        Assert.AreEqual(PersonHeard.First, watch.Hear("Ritu Sharma", "ritu.sharma@example.com"), "a person who signs in after a sign-out is taken for a change with no sign-out between");
    }

    [TestMethod]
    public void Two_people_are_told_apart_by_name_and_email_and_the_mark_says_nothing_of_either()
    {
        // The same name with another email, with none, and with an empty one are four people.
        string[] marks =
        [
            PersonWatch.MarkOf("Gurpreet Singh", "gurpreet.singh@example.com"),
            PersonWatch.MarkOf("Gurpreet Singh", "gurpreet.s@example.com"),
            PersonWatch.MarkOf("Gurpreet Singh", null),
            PersonWatch.MarkOf("Gurpreet Singh", string.Empty),
            PersonWatch.MarkOf("Gurpreet Singh+gurpreet.singh@example.com", null),
            PersonWatch.MarkOf("Gurpreet", " Singh"),
        ];
        Assert.AreEqual(marks.Length, marks.Distinct(StringComparer.Ordinal).Count(), "two people have one mark");
        Assert.AreEqual(marks[0], PersonWatch.MarkOf("Gurpreet Singh", "gurpreet.singh@example.com"), "one person has two marks");
        foreach (string mark in marks)
        {
            Expect.That(!mark.Contains("Gurpreet", StringComparison.OrdinalIgnoreCase) && !mark.Contains("example", StringComparison.OrdinalIgnoreCase), "the mark says who it is the mark of");
        }

        PersonWatch watch = new();
        Assert.AreEqual(PersonHeard.First, watch.Hear("Gurpreet Singh", null));
        Assert.AreEqual(PersonHeard.Same, watch.Hear("Gurpreet Singh", null));
        Assert.AreEqual(PersonHeard.Another, watch.Hear("Gurpreet Singh", "gurpreet.singh@example.com"));
    }

    [TestMethod]
    public void A_start_that_remembers_whom_the_last_run_kept_for_sees_another_person_named_as_a_change()
    {
        // The last run ended with Ritu named. Her session ran out while the app was closed.
        Assert.AreEqual(PersonHeard.Another, new PersonWatch(Ritu).Hear("Harpreet Kaur", "harpreet.kaur@example.com"), "a change of person across a restart is not seen");
        Assert.AreEqual(PersonHeard.Same, new PersonWatch(Ritu).Hear("Ritu Sharma", "ritu.sharma@example.com"), "the same person at the next start is taken for another");

        // Nothing was kept (a first start, or one after a sign-out): whoever is named is the first.
        Assert.AreEqual(PersonHeard.First, new PersonWatch(null).Hear("Harpreet Kaur", "harpreet.kaur@example.com"));
        Assert.AreEqual(PersonHeard.First, new PersonWatch(string.Empty).Hear("Harpreet Kaur", "harpreet.kaur@example.com"));
    }

    [TestMethod]
    public async Task Another_person_signing_in_with_no_sign_out_between_takes_what_was_kept_for_the_person_before_and_leaves_the_cookies()
    {
        string folder = Folder();
        try
        {
            KeptStore kept = new(folder, App);
            CookieStore cookies = new(Shop);
            using (SessionKeeper keeper = new(kept, cookies))
            {
                _ = keeper.Open();
                Assert.IsNull(keeper.KeptPerson(), "a first start remembers somebody");
                await RituSignedInAsync(keeper, cookies);
                Assert.AreEqual(Ritu, kept.Read(SessionKeeper.PersonKey), "the mark of the person signed in is not kept");

                HarpreetSignsIn(keeper, cookies);

                // The core names Harpreet, and the host carries the change out with its own steps.
                int webEmptied = 0;
                int systemEmptied = 0;
                Task<WipeReport> changing = keeper.PersonChangedAsync(
                    Harpreet,
                    [
                        new WipeStep("what the web view keeps", () =>
                        {
                            _ = Interlocked.Increment(ref webEmptied);
                            return Task.CompletedTask;
                        }),
                        new WipeStep("the notifications, the count and the jump list", () =>
                        {
                            _ = Interlocked.Increment(ref systemEmptied);
                            return Task.CompletedTask;
                        }),
                    ]);

                // What the core asks to keep for Harpreet in the same update, and a reminder she is given a moment later, are hers and stay.
                keeper.WriteCached("boot", "{\"app\":\"Charkha Supply Co.\"}");
                keeper.Write(Waiting, "[{\"id\":\"stock-count\",\"title\":\"Stock count at 6\"}]");
                WipeReport report = await changing.WaitAsync(Short);
                await keeper.DrainAsync().WaitAsync(Short);

                Expect.That(report.Complete, "the change of person did not finish: " + string.Join(", ", report.Failed.Select(failed => failed.Key + " (" + failed.Value + ")")));
                CollectionAssert.AreEqual(
                    new[] { "what was kept for the person before", "what the web view keeps", "the notifications, the count and the jump list" },
                    report.Done.ToArray(),
                    "the steps did not each run once, in their order");
                Assert.AreEqual(1, Volatile.Read(ref webEmptied));
                Assert.AreEqual(1, Volatile.Read(ref systemEmptied));

                // Nothing the client kept for Ritu is left for Harpreet.
                Assert.IsNull(kept.Read("table-layout:orders"), "the widths Ritu gave a table are kept for Harpreet");
                Assert.IsNull(kept.Read("snapshot"), "Ritu's snapshot is kept for Harpreet");
                Expect.That(kept.Read(Waiting)?.Contains("dr-malhotra", StringComparison.Ordinal) != true, "Ritu's reminder still waits, to be shown to Harpreet");

                // What is Harpreet's stays: her cookie, in memory and on disk, and what was kept for her since the change began.
                Assert.AreEqual("session=harpreet", cookies.Header("https://shop.example/", DateTimeOffset.UtcNow), "the change of person took Harpreet's cookie out of memory");
                Expect.That(kept.Read(SessionKeeper.CookiesKey)?.Contains("harpreet", StringComparison.Ordinal) == true, "Harpreet's cookie file went: she would be signed out at the next start");
                Expect.That(kept.Read(Waiting)?.Contains("stock-count", StringComparison.Ordinal) == true, "a reminder Harpreet was given as she signed in went with Ritu's");
                Assert.AreEqual("{\"app\":\"Charkha Supply Co.\"}", kept.ReadCached("boot"), "what the core kept for Harpreet went");

                // The core's cache is the core's to empty, which it asks for itself in the same update: the change of person leaves it alone.
                Assert.IsNotNull(kept.ReadCached(Orders), "the change of person emptied the core's cache");

                // The mark is Harpreet's now, and no sign-out was begun.
                Assert.AreEqual(Harpreet, kept.Read(SessionKeeper.PersonKey), "the mark kept is not that of the person who is here now: the next start would do the change over");
                Expect.That(!kept.WipeMarked, "a change of person left the mark of a sign-out: the next start would begin signed out");
                Expect.That(!keeper.Ending && !keeper.Wiping, "a change of person is taken for a sign-out under way");
            }

            // The next start is Harpreet's: signed in, remembered, and nothing of Ritu's is read.
            KeptStore keptNext = new(folder, App);
            CookieStore cookiesNext = new(Shop);
            using SessionKeeper next = new(keptNext, cookiesNext);
            Expect.That(!next.Open(), "a change of person was taken for an unfinished sign-out");
            Assert.AreEqual("session=harpreet", cookiesNext.Header("https://shop.example/", DateTimeOffset.UtcNow), "the next start came up signed out though nobody signed out");
            Assert.AreEqual(PersonHeard.Same, new PersonWatch(next.KeptPerson()).Hear("Harpreet Kaur", "harpreet.kaur@example.com"), "the next start takes Harpreet for another person");
            Assert.IsNull(keptNext.Read("table-layout:orders"));
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task A_change_of_person_that_did_not_finish_is_done_over_at_the_next_start()
    {
        string folder = Folder();
        try
        {
            KeptStore kept = new(folder, App);
            CookieStore cookies = new(Shop);
            using (SessionKeeper keeper = new(kept, cookies))
            {
                _ = keeper.Open();
                await RituSignedInAsync(keeper, cookies);
                HarpreetSignsIn(keeper, cookies);

                // The system would not empty the jump list: a step fails.
                WipeReport report = await keeper.PersonChangedAsync(
                    Harpreet,
                    [new WipeStep("the notifications, the count and the jump list", () => throw new InvalidOperationException("the jump list could not be emptied"))]).WaitAsync(Short);
                Expect.That(!report.Complete, "a change of person whose step failed is said to have finished");
                Expect.That(report.Failed.Any(failed => failed.Key == "the notifications, the count and the jump list"), "what failed is not named: " + string.Join(", ", report.Failed.Select(failed => failed.Key)));

                // The core goes on naming Harpreet for the rest of the run. The mark kept stays Ritu's: nothing says the change is over.
                keeper.KeepPerson(Harpreet);
                await keeper.DrainAsync().WaitAsync(Short);
                Assert.AreEqual(Ritu, kept.Read(SessionKeeper.PersonKey), "the mark of the person who is here now was written though the change did not finish");

                // What could be taken away was: Ritu's reminder does not wait for Harpreet in this run.
                Assert.IsNull(kept.Read(Waiting), "what was kept for Ritu stayed because another step failed");
            }

            // The next start remembers Ritu and hears Harpreet named: the change is done over, and this time it finishes.
            KeptStore keptNext = new(folder, App);
            CookieStore cookiesNext = new(Shop);
            using SessionKeeper next = new(keptNext, cookiesNext);
            _ = next.Open();
            Assert.AreEqual(PersonHeard.Another, new PersonWatch(next.KeptPerson()).Hear("Harpreet Kaur", "harpreet.kaur@example.com"), "the next start does not see the change that was left unfinished");
            bool ranAgain = false;
            WipeReport again = await next.PersonChangedAsync(
                Harpreet,
                [new WipeStep("the notifications, the count and the jump list", () =>
                {
                    ranAgain = true;
                    return Task.CompletedTask;
                })]).WaitAsync(Short);
            await next.DrainAsync().WaitAsync(Short);
            Expect.That(again.Complete && ranAgain, "the change was not done over");
            Assert.AreEqual(Harpreet, keptNext.Read(SessionKeeper.PersonKey));
            Assert.AreEqual("session=harpreet", cookiesNext.Header("https://shop.example/", DateTimeOffset.UtcNow), "doing the change over signed Harpreet out");
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task An_app_ended_in_the_middle_of_a_change_of_person_does_it_over_at_its_next_start()
    {
        string folder = Folder();
        TaskCompletionSource browserEnds = new(TaskCreationOptions.RunContinuationsAsynchronously);
        Task<WipeReport>? changing = null;
        SessionKeeper? ended = null;
        try
        {
            KeptStore kept = new(folder, App);
            CookieStore cookies = new(Shop);
            ended = new SessionKeeper(kept, cookies);
            _ = ended.Open();
            await RituSignedInAsync(ended, cookies);
            HarpreetSignsIn(ended, cookies);

            // Emptying what the web view kept for Ritu takes long (its browser is slow to end), and the app is ended while it does.
            changing = ended.PersonChangedAsync(Harpreet, [new WipeStep("what the web view keeps", () => browserEnds.Task)]);
            await ended.DrainAsync().WaitAsync(Short);
            Expect.That(!changing.IsCompleted, "the step that takes long did not hold the change back: this test shows nothing");
            Assert.AreEqual(Ritu, kept.Read(SessionKeeper.PersonKey), "the mark of the person who is here now was written before the change finished");

            // The next start still remembers Ritu, so Harpreet named is a change of person, and it is done from the beginning.
            KeptStore keptNext = new(folder, App);
            CookieStore cookiesNext = new(Shop);
            using SessionKeeper next = new(keptNext, cookiesNext);
            Expect.That(!next.Open(), "a change of person cut short was taken for an unfinished sign-out");
            Assert.AreEqual(PersonHeard.Another, new PersonWatch(next.KeptPerson()).Hear("Harpreet Kaur", "harpreet.kaur@example.com"), "the next start does not see the change that was cut short");
            Assert.AreEqual("session=harpreet", cookiesNext.Header("https://shop.example/", DateTimeOffset.UtcNow));
        }
        finally
        {
            // The run that was ended is let go of, so that nothing of it is running when the folder is deleted.
            _ = browserEnds.TrySetResult();
            if (changing is not null)
            {
                _ = await Task.WhenAny(changing, Task.Delay(Short));
            }

            ended?.Dispose();
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task A_sign_out_takes_the_mark_of_the_person_and_one_that_could_not_be_written_during_it_is_written_the_next_time_they_are_named()
    {
        string folder = Folder();
        try
        {
            KeptStore kept = new(folder, App);
            CookieStore cookies = new(Shop);
            using SessionKeeper keeper = new(kept, cookies);
            _ = keeper.Open();
            await RituSignedInAsync(keeper, cookies);

            // Ritu signs out. Emptying what the web view keeps takes a while, so the sign-out is still under way when the core, which has
            // let go of her, names Harpreet, who signed in at once.
            TaskCompletionSource webEmptied = new(TaskCreationOptions.RunContinuationsAsynchronously);
            Task<WipeReport> ending = keeper.EndAsync(() => { }, () => Task.CompletedTask, [new WipeStep("what the web view keeps", () => webEmptied.Task)], []);
            Expect.That(keeper.Ending, "the sign-out is not under way: this test shows nothing");
            keeper.KeepPerson(Harpreet);
            await keeper.DrainAsync().WaitAsync(Short);
            Expect.That(kept.Read(SessionKeeper.PersonKey) != Harpreet, "a mark was written while a sign-out was under way");

            webEmptied.SetResult();
            WipeReport report = await ending.WaitAsync(Short);
            await keeper.DrainAsync().WaitAsync(Short);
            Expect.That(report.Complete, "the sign-out did not finish: " + string.Join(", ", report.Failed.Select(failed => failed.Key)));
            Assert.IsNull(kept.Read(SessionKeeper.PersonKey), "the mark of the person who signed out is still kept");

            // The core names Harpreet again with its next state: now her mark is written.
            keeper.KeepPerson(Harpreet);
            await keeper.DrainAsync().WaitAsync(Short);
            Assert.AreEqual(Harpreet, kept.Read(SessionKeeper.PersonKey), "the mark that could not be written during the sign-out was never written");
        }
        finally
        {
            Delete(folder);
        }
    }
}
