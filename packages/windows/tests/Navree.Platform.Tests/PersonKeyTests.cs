using System.Collections.Concurrent;
using System.Text;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Platform.Tests;

/// <summary>
/// The core makes its key for a person with a secret of this install, so the key cannot be made from a guess of who they are. The client
/// makes that secret once and keeps it. A mark it kept before the install had one is the mark of a plain key: the person it was kept for
/// is still that person when the core first names them by their new key, and nobody else is.
/// </summary>
[TestClass]
public sealed class PersonKeyTests
{
    private const string App = "Navree.Tests";
    private const string Waiting = "pending-notifications";

    // Ravneet's key as the release before made it, and as the core makes it with the secret of an install; and those of another account of
    // the same name and email. They are the keys the core's conversations person-key-steady.json and person-key-was.json record.
    private const string RavneetPlain = "1c54d473261f6f17cb6193202ce56588716e85757946078ac43d03b22f93a35e";
    private const string RavneetKeyed = "k2:17d27bf8db683479c652471e31c96ac7d03e690c41c4f94f56ca99de886e389e";
    private const string OtherPlain = "1bc3dcdc8ebf5d231237007ccc4903b602f64185813b65bc5b6d1f5f798fcab7";
    private const string OtherKeyed = "k2:6bd1a2cdf43aaf682ec4d5da88fc8ca12d82976ffebb660e10e75f894e0c82d7";
    private static readonly Origins Shop = new("https://shop.example");
    private static readonly TimeSpan Short = TimeSpan.FromSeconds(5);

    private static string Folder() => Path.Combine(Path.GetTempPath(), "navree-key-" + Guid.NewGuid().ToString("N"), "kept");

    private static void Delete(string folder)
    {
        string parent = Path.GetDirectoryName(folder)!;
        if (Directory.Exists(parent))
        {
            Directory.Delete(parent, recursive: true);
        }
    }

    private static ViewState Naming(string key, string? was) =>
        ViewState.Empty with
        {
            App = new AppPart
            {
                Name = "Charkha Supply Co.",
                User = new Models.User { Name = "Ravneet Kaur", Email = "ravneet.kaur@example.com", Key = key },
                UserWas = was,
            },
        };

    [TestMethod]
    public void A_mark_kept_of_a_plain_key_is_the_person_whose_plain_key_the_core_says_it_was_and_nobody_elses()
    {
        string kept = PersonWatch.MarkOfKey(RavneetPlain);
        Assert.IsTrue(PersonWatch.IsPlainKeyMark(kept));
        Assert.IsFalse(PersonWatch.IsPlainKeyMark(PersonWatch.MarkOfKey(RavneetKeyed)), "the mark of a key made with the secret is taken for the mark of a plain one");
        Assert.IsFalse(PersonWatch.IsPlainKeyMark(PersonWatch.MarkOf("Ravneet Kaur", "ravneet.kaur@example.com")), "a mark made from a name and an email is taken for the mark of a key");

        // The first start after the update. Her mark is of the key she had; the core names her by the key it makes now, and says what her
        // key was. She is the person what is kept is kept for, and the mark is the mark of the key she has now.
        PersonWatch watch = new(kept);
        Assert.IsTrue(watch.KeepsPlainKey, "a start that keeps the mark of a plain key does not say so");
        Assert.AreEqual(PersonHeard.Same, watch.Hear(RavneetKeyed, "Ravneet Kaur", "ravneet.kaur@example.com", RavneetPlain), "the person the mark was kept for is taken for another when her key is first made with the secret");
        Assert.AreEqual(PersonWatch.MarkOfKey(RavneetKeyed), watch.Named);
        Assert.IsFalse(watch.KeepsPlainKey);
        Assert.AreEqual(PersonHeard.Same, watch.Hear(RavneetKeyed, "Ravneet Kaur", "ravneet.kaur@example.com"));

        // Another account, of the same name and email, at that start: its plain key was another, so it is another person.
        PersonWatch other = new(kept);
        Assert.AreEqual(PersonHeard.Another, other.Hear(OtherKeyed, "Ravneet Kaur", "ravneet.kaur@example.com", OtherPlain), "another account is left what was kept for the first");
        Assert.AreEqual(PersonWatch.MarkOfKey(OtherKeyed), other.Named);

        // A core that does not say what her key was leaves the two marks two people's.
        Assert.AreEqual(PersonHeard.Another, new PersonWatch(kept).Hear(RavneetKeyed, "Ravneet Kaur", "ravneet.kaur@example.com"));
        Assert.AreEqual(PersonHeard.Another, new PersonWatch(kept).Hear(RavneetKeyed, "Ravneet Kaur", "ravneet.kaur@example.com", string.Empty));

        // What the core says a key was holds only a plain mark to a key made with the secret: not a plain key to another plain key, not
        // a mark made from a name and an email, and not one key made with the secret to another.
        Assert.AreEqual(PersonHeard.Another, new PersonWatch(kept).Hear(OtherPlain, "Ravneet Kaur", "ravneet.kaur@example.com", RavneetPlain));
        string byName = PersonWatch.MarkOf("Ravneet Kaur", "ravneet.kaur@example.com");
        Assert.AreEqual(PersonHeard.Another, new PersonWatch(byName).Hear(RavneetKeyed, "Ravneet Kaur", "ravneet.kaur@example.com", RavneetPlain));
        Assert.AreEqual(PersonHeard.Another, new PersonWatch(PersonWatch.MarkOfKey(OtherKeyed)).Hear(RavneetKeyed, "Ravneet Kaur", "ravneet.kaur@example.com", OtherKeyed));

        // A start that found something kept and no mark still takes the first person named for another, whatever the core says.
        Assert.AreEqual(PersonHeard.Another, new PersonWatch(null, somethingKept: true).Hear(RavneetKeyed, "Ravneet Kaur", "ravneet.kaur@example.com", RavneetPlain));

        // After a sign-out nobody is remembered, and whoever is named is the first.
        watch.SignedOut();
        Assert.IsFalse(watch.KeepsPlainKey);
        Assert.AreEqual(PersonHeard.First, watch.Hear(OtherKeyed, "Ravneet Kaur", "ravneet.kaur@example.com", OtherPlain));
    }

    [TestMethod]
    public async Task The_person_named_by_her_new_key_keeps_what_was_kept_and_her_reminders_wait_under_the_mark_she_has_now()
    {
        string folder = Folder();
        string before = PersonWatch.MarkOfKey(RavneetPlain);
        string now = PersonWatch.MarkOfKey(RavneetKeyed);
        try
        {
            // The release before: Ravneet is signed in at the counter in Ludhiana, her mark made from her plain key, a snapshot of where
            // she was and a reminder that waits.
            using (SessionKeeper earlier = new(new KeptStore(folder, App), new CookieStore(Shop)))
            {
                _ = earlier.Open();
                earlier.KeepPerson(before);
                earlier.Write("snapshot", "{\"typed\":\"Ravneet's draft\"}");
                earlier.Write(Waiting, "[{\"id\":\"dr-malhotra\",\"title\":\"Your appointment with Dr. Malhotra, Model Town, 10:30\"}]");
                await earlier.DrainAsync().WaitAsync(Short);
            }

            // The first start after the update.
            KeptStore kept = new(folder, App);
            using SessionKeeper keeper = new(kept, new CookieStore(Shop));
            _ = keeper.Open();
            string? keptFor = keeper.KeptPerson();
            Assert.AreEqual(before, keptFor);
            Assert.IsTrue(PersonWatch.IsPlainKeyMark(keptFor!), "the start does not ask the core what a person's key was");
            SessionGate gate = new();
            RecordedShell shell = new();
            List<(string Before, string After)> moved = [];
            ConcurrentQueue<string> problems = new();
            PersonChange people = new(
                new PersonWatch(keptFor, keeper.KeepsOfAPerson()),
                gate,
                keeper,
                shell.Parts with { MoveWaiting = (had, has) => moved.Add((had, has)) },
                problems.Enqueue);

            // The core names her by the key it makes with the install's secret, and says what her key was.
            _ = gate.Asked([], out long mark);
            Assert.AreEqual(PersonHeard.Same, people.Hear(Naming(RavneetKeyed, RavneetPlain), mark));
            Assert.AreEqual(0, shell.Did.Length, "something of hers was taken away: " + string.Join("; ", shell.Did));
            Assert.IsNull(people.Last, "a change of person began for the person what is kept is kept for");
            Assert.AreEqual(now, people.KeptFor);
            Assert.AreEqual(now, people.Here);
            CollectionAssert.AreEqual(new[] { (before, now) }, moved, "what waited under the mark she had is not kept under the mark she has now");
            Assert.AreEqual(0, shell.GivenTo.Length, "she was given what waited for nobody, as a first person is");

            // The mark of her new key is written in place of the one before, and what was kept for her is still there.
            await keeper.DrainAsync().WaitAsync(Short);
            Assert.AreEqual(now, kept.Read(SessionKeeper.PersonKey));
            Assert.AreEqual("{\"typed\":\"Ravneet's draft\"}", kept.Read("snapshot"));
            Assert.IsNotNull(kept.Read(Waiting));

            // The states after say her key and nothing more: she is the same, and nothing is moved again.
            _ = gate.Asked([], out mark);
            Assert.AreEqual(PersonHeard.Same, people.Hear(Naming(RavneetKeyed, null), mark));
            Assert.AreEqual(1, moved.Count);
            Assert.AreEqual(0, problems.Count, string.Join("; ", problems));
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public async Task Another_account_named_at_the_first_start_after_the_update_is_another_person_and_what_was_kept_goes()
    {
        string folder = Folder();
        string before = PersonWatch.MarkOfKey(RavneetPlain);
        try
        {
            using (SessionKeeper earlier = new(new KeptStore(folder, App), new CookieStore(Shop)))
            {
                _ = earlier.Open();
                earlier.KeepPerson(before);
                earlier.Write("snapshot", "{\"typed\":\"Ravneet's draft\"}");
                await earlier.DrainAsync().WaitAsync(Short);
            }

            KeptStore kept = new(folder, App);
            using SessionKeeper keeper = new(kept, new CookieStore(Shop));
            _ = keeper.Open();
            SessionGate gate = new();
            RecordedShell shell = new();
            List<(string Before, string After)> moved = [];
            PersonChange people = new(
                new PersonWatch(keeper.KeptPerson(), keeper.KeepsOfAPerson()),
                gate,
                keeper,
                shell.Parts with { MoveWaiting = (had, has) => moved.Add((had, has)) },
                _ => { });

            // Her session ran out, and another account of the same name signs in: the core says that account's own plain key.
            _ = gate.Asked([], out long mark);
            Assert.AreEqual(PersonHeard.Another, people.Hear(Naming(OtherKeyed, OtherPlain), mark));
            Assert.AreEqual(0, moved.Count, "what waited for her was kept for another account");
            Assert.IsNotNull(people.Last, "no change of person began");
            shell.SystemTakesItAway.SetResult();
            WipeReport report = await people.Last!.WaitAsync(Short);
            Assert.IsTrue(report.Complete, string.Join("; ", report.Failed.Select(failed => failed.Key)));
            await keeper.DrainAsync().WaitAsync(Short);
            Assert.IsNull(kept.Read("snapshot"), "her snapshot was left to another account");
            Assert.AreEqual(PersonWatch.MarkOfKey(OtherKeyed), kept.Read(SessionKeeper.PersonKey));
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public void The_secret_of_an_install_is_made_once_and_is_the_same_at_every_start_also_after_a_sign_out()
    {
        string folder = Folder();
        string elsewhere = Folder();
        try
        {
            string made = InstallSecret.Open(folder, App);
            Assert.AreEqual(2 * InstallSecret.Bytes, made.Length);
            Assert.IsTrue(InstallSecret.Reads(made), "the secret made is not one the core takes");
            Assert.AreEqual(made, InstallSecret.Open(folder, App), "the next start made another secret");

            // It stands beside the folder of what is kept, protected, and a sign-out, which empties that folder, leaves it.
            string file = InstallSecret.FileBeside(folder);
            Assert.IsTrue(File.Exists(file));
            Assert.IsFalse(file.StartsWith(Path.GetFullPath(folder) + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase), "the secret is kept where a sign-out empties");
            string written = Encoding.Latin1.GetString(File.ReadAllBytes(file));
            Assert.IsFalse(written.Contains(made, StringComparison.OrdinalIgnoreCase), "the secret is written as it is");
            KeptStore kept = new(folder, App);
            kept.Write("snapshot", "{}");
            kept.Clear();
            Assert.AreEqual(made, InstallSecret.Open(folder, App), "a sign-out took the install's secret away");

            // Another install has another, and another app's name does not open this one's: it makes its own in its place.
            Assert.AreNotEqual(made, InstallSecret.Open(elsewhere, App), "two installs have one secret");
            string anothers = InstallSecret.Open(folder, "Another.App");
            Assert.AreNotEqual(made, anothers);
            Assert.AreEqual(anothers, InstallSecret.Open(folder, "Another.App"));

            // A file that does not read is replaced, and what replaces it stays.
            File.WriteAllBytes(file, [1, 2, 3, 4]);
            string again = InstallSecret.Open(folder, App);
            Assert.IsTrue(InstallSecret.Reads(again));
            Assert.AreNotEqual(made, again);
            Assert.AreEqual(again, InstallSecret.Open(folder, App));

            // What the core takes: hex, two characters a byte, of 32 bytes or more.
            Assert.IsFalse(InstallSecret.Reads(null));
            Assert.IsFalse(InstallSecret.Reads(made[..^2]), "31 bytes are taken for a secret");
            Assert.IsFalse(InstallSecret.Reads(made + "0"), "half a byte is taken for a secret");
            Assert.IsFalse(InstallSecret.Reads("zz" + made[2..]));
            Assert.IsTrue(InstallSecret.Reads(made + "0a"));
        }
        finally
        {
            Delete(folder);
            Delete(elsewhere);
        }
    }
}
