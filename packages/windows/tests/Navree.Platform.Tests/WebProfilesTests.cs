namespace Navree.Platform.Tests;

/// <summary>Folders that are only names in memory, some of which refuse to be deleted or made.</summary>
internal sealed class FakeWebFolders : IWebFolders
{
    internal HashSet<string> There { get; } = new(StringComparer.Ordinal);

    internal HashSet<string> Held { get; } = new(StringComparer.Ordinal);

    internal bool NothingCanBeMade { get; set; }

    internal bool MakingThrows { get; set; }

    internal List<string> Asked { get; } = [];

    public IReadOnlyList<string> Names(string root) =>
        [.. There.Where(path => Path.GetDirectoryName(path) == root).Select(path => Path.GetFileName(path)).Order(StringComparer.Ordinal)];

    public bool Make(string path)
    {
        if (MakingThrows)
        {
            throw new InvalidOperationException("the disk threw");
        }

        if (NothingCanBeMade)
        {
            return false;
        }

        _ = There.Add(path);
        return true;
    }

    public bool Delete(string path, TimeSpan limit)
    {
        Asked.Add(path);
        if (Held.Contains(path))
        {
            return false;
        }

        _ = There.Remove(path);
        return true;
    }
}

/// <summary>A web view whose data could not be removed is never used for the next person.</summary>
[TestClass]
public sealed class WebProfilesTests
{
    private static readonly string Root = Path.Combine(Path.GetTempPath(), "navree-web-profiles");

    private static string In(string name) => Path.Combine(Root, name);

    [TestMethod]
    public void The_folder_moves_on_whatever_making_the_new_one_or_telling_of_it_throws_and_no_web_page_is_shown_then()
    {
        FakeWebFolders disk = new();
        List<string> said = [];
        WebProfiles profiles = new(Root, disk, what =>
        {
            said.Add(what);
            throw new InvalidOperationException("whoever listens threw");
        });
        Assert.IsTrue(profiles.Open(lastSessionEnded: false));
        disk.There.Add(In("1"));

        // The person signs out, and making the folder of whoever comes next throws; so does whoever is told of it.
        disk.MakingThrows = true;
        string ended = profiles.MoveOn();
        Assert.AreEqual(In("1"), ended, "the end of the session was not given the folder to empty");
        Assert.IsNull(profiles.Folder, "a web page may be shown though the folder of whoever comes next is not there");
        Assert.AreEqual(1, said.Count);

        // The folder of the person who left is emptied and deleted as usual, and the sign-out is not finished: the next start goes on in a new folder.
        Assert.IsFalse(profiles.Finish(ended, taken: true));
        Assert.IsFalse(disk.There.Contains(In("1")), "the folder of the person who left is still on disk though nothing held it");
        Assert.IsNull(profiles.Folder);
    }

    [TestMethod]
    public void Each_session_keeps_its_web_data_in_a_folder_of_its_own_and_the_next_start_goes_on_in_the_last()
    {
        FakeWebFolders disk = new();
        WebProfiles first = new(Root, disk);
        Assert.IsTrue(first.Open(lastSessionEnded: false));
        Assert.AreEqual(In("1"), first.Folder);

        // The web view made its folder. The person signs out, and the folder is deleted while it is being emptied.
        disk.There.Add(In("1"));
        string ended = first.MoveOn();
        Assert.AreEqual(In("1"), ended);
        Assert.AreEqual(In("2"), first.Folder, "whoever comes next is given the folder of the person who left");
        Assert.IsTrue(first.Finish(ended, taken: true));
        Assert.AreEqual(In("2"), first.Folder);
        CollectionAssert.AreEqual(new[] { In("2") }, disk.There.ToArray(), "the folder of the session that ended is still on disk though nothing held it");

        // Another sign-out in the same run.
        string second = first.MoveOn();
        Assert.AreEqual(In("2"), second);
        Assert.IsTrue(first.Finish(second, taken: true));
        Assert.AreEqual(In("3"), first.Folder);

        // The app is started again with nobody having signed out since: the same session, the same folder.
        WebProfiles next = new(Root, disk);
        Assert.IsTrue(next.Open(lastSessionEnded: false));
        Assert.AreEqual(In("3"), next.Folder);
        CollectionAssert.AreEqual(new[] { In("3") }, disk.There.ToArray(), "folders of sessions that ended are still on disk");
    }

    [TestMethod]
    public async Task The_folder_moves_on_before_anything_is_emptied_so_a_clear_that_never_returns_leaves_no_page_in_the_old_folder()
    {
        FakeWebFolders disk = new();
        WebProfiles run = new(Root, disk);
        Assert.IsTrue(run.Open(lastSessionEnded: false));
        string old = run.Folder!;
        disk.There.Add(old);
        disk.Held.Add(old);

        // The sign-out begins: the folder moves on in its first line, before any step runs.
        string ended = run.MoveOn();
        Assert.AreEqual(old, ended);
        Assert.AreNotEqual(old, run.Folder, "the folder of the person who is leaving is still the one a web page would be given");
        Expect.That(disk.There.Contains(run.Folder!), "the folder of whoever comes next is not on disk");

        // The web step's clear never returns. The wipe gives the step up at its limit and goes on, as the sign-out does.
        TaskCompletionSource never = new(TaskCreationOptions.RunContinuationsAsynchronously);
        Task? step = null;
        async Task EmptyAsync()
        {
            await never.Task;
            _ = run.Finish(ended, taken: false);
        }

        WipeReport report = await Wipe.RunAsync([new WipeStep("what the web view keeps", () => step = EmptyAsync())], TimeSpan.FromMilliseconds(200));
        Expect.That(!report.Complete, "a web step that never returned is said to have finished");
        for (int waited = 0; waited < 250 && step is null; waited++)
        {
            await Task.Delay(20);
        }

        Assert.IsNotNull(step, "the web step was never started");

        // A web page is asked for now, while the old folder is still being emptied: it is given the new folder, never the old.
        Assert.IsNotNull(run.Folder);
        Assert.AreNotEqual(old, run.Folder, "a web page asked for during the wipe is opened in the folder of the person who left");

        // Much later the clear gives up with nothing cleared and the folder still held: from then on no page is shown; still never the old.
        never.SetResult();
        await step!.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.IsNull(run.Folder);
    }

    [TestMethod]
    public void When_no_new_folder_can_be_made_a_web_page_asked_for_during_the_wipe_is_refused_and_never_given_the_old_folder()
    {
        FakeWebFolders disk = new();
        disk.There.Add(In("2"));
        WebProfiles run = new(Root, disk);
        Assert.IsTrue(run.Open(lastSessionEnded: false));
        Assert.AreEqual(In("2"), run.Folder);

        disk.NothingCanBeMade = true;
        string ended = run.MoveOn();
        Assert.AreEqual(In("2"), ended);
        Assert.IsNull(run.Folder, "with no folder for whoever comes next, a web page is still given a folder");

        // The old folder was emptied and deleted; the sign-out is still not finished, so the next start makes the new folder first.
        Assert.IsFalse(run.Finish(ended, taken: true), "a sign-out that left whoever comes next no folder of their own is said to be finished");
        Assert.IsNull(run.Folder);

        // At a start that finishes a sign-out.
        WebProfiles start = new(Root, disk);
        Assert.IsFalse(start.Open(lastSessionEnded: true), "the start went on though whoever comes next has no folder of their own");
        Assert.IsNull(start.Folder);
    }

    [TestMethod]
    public void When_what_the_web_view_kept_can_be_neither_cleared_nor_deleted_no_web_page_is_shown_in_that_run_and_the_next_start_uses_a_new_folder()
    {
        FakeWebFolders disk = new();
        List<string> said = [];
        WebProfiles run = new(Root, disk, said.Add);
        Assert.IsTrue(run.Open(lastSessionEnded: false));
        disk.There.Add(In("1"));

        // Something still holds a file of the folder: it cannot be deleted, and no web view was there to clear it.
        disk.Held.Add(In("1"));
        string ended = run.MoveOn();
        Assert.IsFalse(run.Finish(ended, taken: false), "a sign-out that left the web view's data on disk was said to be finished");
        Assert.IsNull(run.Folder, "a web page can still be shown in the run whose sign-out left the web view's data");
        Expect.That(said.Any(line => line.Contains("no web page is shown until the app is started again", StringComparison.Ordinal)), "nothing was said: " + string.Join(" | ", said));

        // The same run, a second sign-out: still nothing is shown, and it is still not finished.
        string again = run.MoveOn();
        Assert.AreNotEqual(In("1"), again);
        Assert.IsFalse(run.Finish(again, taken: true));
        Assert.IsNull(run.Folder);

        // The next start finds the sign-out unfinished. The folder is still held: the start goes on, in a folder the old profile was never in.
        said.Clear();
        WebProfiles next = new(Root, disk, said.Add);
        Assert.IsTrue(next.Open(lastSessionEnded: true), "a folder that cannot be deleted kept the app from going on");
        Assert.IsNotNull(next.Folder);
        Assert.AreNotEqual(In("1"), next.Folder, "the folder of the person who left is used again");
        Expect.That(disk.There.Contains(next.Folder!), "the new folder is not on disk, so a later start would not know it");
        CollectionAssert.AreEqual(new[] { "1" }, next.Left.ToArray());
        Expect.That(said.Any(line => line.Contains("(1)", StringComparison.Ordinal) && line.Contains("not used again", StringComparison.Ordinal)), "the folder left behind was not said: " + string.Join(" | ", said));

        // A later start, with nobody having signed out: the same new folder, and the old one is tried again and goes once it is let go.
        string session = next.Folder!;
        WebProfiles later = new(Root, disk);
        Assert.IsTrue(later.Open(lastSessionEnded: false));
        Assert.AreEqual(session, later.Folder);
        Expect.That(disk.There.Contains(In("1")), "the held folder went though it was held");
        disk.Held.Clear();
        WebProfiles last = new(Root, disk);
        Assert.IsTrue(last.Open(lastSessionEnded: false));
        Assert.AreEqual(session, last.Folder);
        CollectionAssert.AreEqual(new[] { session }, disk.There.ToArray(), "the old folder is still on disk after it was let go");
        Assert.AreEqual(0, last.Left.Count);
    }

    [TestMethod]
    public void A_start_that_finishes_a_sign_out_never_uses_the_folder_of_the_run_before_even_when_it_was_the_only_one()
    {
        FakeWebFolders disk = new();
        disk.There.Add(In("4"));
        disk.Held.Add(In("4"));

        WebProfiles start = new(Root, disk);
        Assert.IsTrue(start.Open(lastSessionEnded: true));
        Assert.AreEqual(In("5"), start.Folder);
        CollectionAssert.AreEqual(new[] { "4" }, start.Left.ToArray());

        // Without a sign-out to finish, the same disk gives the folder that is there.
        FakeWebFolders same = new();
        same.There.Add(In("4"));
        WebProfiles plain = new(Root, same);
        Assert.IsTrue(plain.Open(lastSessionEnded: false));
        Assert.AreEqual(In("4"), plain.Folder);
    }

    [TestMethod]
    public void A_profile_cleared_through_a_web_view_is_a_finished_sign_out_though_its_folder_goes_later()
    {
        FakeWebFolders disk = new();
        WebProfiles run = new(Root, disk);
        Assert.IsTrue(run.Open(lastSessionEnded: false));
        disk.There.Add(In("1"));

        // The browser behind the folder is still ending, so the folder cannot go yet; its profile was cleared through a page.
        disk.Held.Add(In("1"));
        string ended = run.MoveOn();
        Assert.IsTrue(run.Finish(ended, taken: true));
        Assert.AreEqual(In("2"), run.Folder, "the cleared folder of the person who left is used for whoever comes next");
        CollectionAssert.AreEqual(new[] { "1" }, run.Left.ToArray());

        // At the next sign-out the browser has ended, and the left-over folder goes with the one of that session.
        disk.Held.Clear();
        string second = run.MoveOn();
        Assert.IsTrue(run.Finish(second, taken: true));
        Assert.AreEqual(In("3"), run.Folder);
        CollectionAssert.AreEqual(new[] { In("3") }, disk.There.ToArray());
    }

    [TestMethod]
    public void What_an_earlier_build_kept_directly_in_the_web_folder_is_deleted_and_never_used()
    {
        FakeWebFolders disk = new();
        disk.There.Add(In("EBWebView"));
        disk.There.Add(In("01"));
        disk.There.Add(In("-3"));

        WebProfiles start = new(Root, disk);
        Assert.IsTrue(start.Open(lastSessionEnded: false));
        Assert.AreEqual(In("1"), start.Folder, "a folder that is not a session's own number was taken for the session's");
        Assert.AreEqual(0, disk.There.Count, "what an earlier build kept is still on disk: " + string.Join(", ", disk.There));
    }

    [TestMethod]
    public void A_wipe_with_a_step_that_failed_is_an_unfinished_sign_out_and_a_wipe_whose_steps_all_finished_is_not()
    {
        Expect.That(SignOutWords.Unfinished(new WipeReport(["the cookies"], [new KeyValuePair<string, string>("what the web view keeps", "IOException")])), "a wipe with a failed step is not taken for unfinished");
        Expect.That(!SignOutWords.Unfinished(new WipeReport(["the cookies", "what the web view keeps"], [])), "a finished wipe is taken for unfinished");
        Expect.That(!SignOutWords.Unfinished(null), "no report is taken for an unfinished sign-out");
        StringAssert.Contains(SignOutWords.UnfinishedHelp, "Restart");
    }
}
