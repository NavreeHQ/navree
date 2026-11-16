namespace Navree.Platform.Tests;

/// <summary>Folders that are only names in memory, some of which refuse to be deleted or made.</summary>
internal sealed class FakeWebFolders : IWebFolders
{
    internal HashSet<string> There { get; } = new(StringComparer.Ordinal);

    internal HashSet<string> Held { get; } = new(StringComparer.Ordinal);

    internal bool NothingCanBeMade { get; set; }

    internal List<string> Asked { get; } = [];

    public IReadOnlyList<string> Names(string root) =>
        [.. There.Where(path => Path.GetDirectoryName(path) == root).Select(path => Path.GetFileName(path)).Order(StringComparer.Ordinal)];

    public bool Make(string path)
    {
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
    public void Each_session_keeps_its_web_data_in_a_folder_of_its_own_and_the_next_start_goes_on_in_the_last()
    {
        FakeWebFolders disk = new();
        WebProfiles first = new(Root, disk);
        Assert.IsTrue(first.Open(lastSessionEnded: false));
        Assert.AreEqual(In("1"), first.Folder);

        // The web view made its folder; the person signs out, and what it kept was taken away.
        disk.There.Add(In("1"));
        disk.There.Remove(In("1"));
        Assert.IsTrue(first.End(taken: true));
        Assert.AreEqual(In("2"), first.Folder, "whoever comes next is given the folder of the person who left");

        // Another sign-out in the same run.
        Assert.IsTrue(first.End(taken: true));
        Assert.AreEqual(In("3"), first.Folder);

        // The app is started again with nobody having signed out since: the same session, the same folder.
        WebProfiles next = new(Root, disk);
        Assert.IsTrue(next.Open(lastSessionEnded: false));
        Assert.AreEqual(In("3"), next.Folder);
        CollectionAssert.AreEqual(new[] { In("3") }, disk.There.ToArray(), "folders of sessions that ended are still on disk");
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
        Assert.IsFalse(run.End(taken: false), "a sign-out that left the web view's data on disk was said to be finished");
        Assert.IsNull(run.Folder, "a web page can still be shown in the run whose sign-out left the web view's data");
        Expect.That(said.Any(line => line.Contains("no web page is shown until the app is started again", StringComparison.Ordinal)), "nothing was said: " + string.Join(" | ", said));

        // The same run, a second sign-out: still nothing is shown, and it is still not finished.
        Assert.IsFalse(run.End(taken: true));
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

        // The browser behind the folder is still running, so the folder cannot go yet; its profile was cleared through a page.
        disk.Held.Add(In("1"));
        Assert.IsTrue(run.End(taken: true));
        Assert.AreEqual(In("2"), run.Folder, "the cleared folder of the person who left is used for whoever comes next");
        CollectionAssert.AreEqual(new[] { "1" }, run.Left.ToArray());

        // At the next sign-out the browser has ended, and the left-over folder goes with the one of that session.
        disk.Held.Clear();
        disk.There.Add(In("2"));
        Assert.IsTrue(run.End(taken: true));
        Assert.AreEqual(In("3"), run.Folder);
        CollectionAssert.AreEqual(new[] { In("3") }, disk.There.ToArray());
    }

    [TestMethod]
    public void When_no_new_folder_can_be_made_nothing_is_shown_and_the_sign_out_is_not_finished()
    {
        FakeWebFolders disk = new() { NothingCanBeMade = true };
        disk.There.Add(In("2"));

        // At a sign-out.
        WebProfiles run = new(Root, disk);
        Assert.IsTrue(run.Open(lastSessionEnded: false));
        Assert.AreEqual(In("2"), run.Folder);
        Assert.IsFalse(run.End(taken: true));
        Assert.IsNull(run.Folder);

        // At a start that finishes a sign-out.
        WebProfiles start = new(Root, disk);
        Assert.IsFalse(start.Open(lastSessionEnded: true), "the start went on though whoever comes next has no folder of their own");
        Assert.IsNull(start.Folder);
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
    public void The_person_is_told_when_a_sign_out_did_not_finish_and_only_then()
    {
        Expect.That(SignOutWords.Unfinished(new WipeReport(["the cookies"], [new KeyValuePair<string, string>("what the web view keeps", "IOException")])), "a wipe with a failed step is not told");
        Expect.That(!SignOutWords.Unfinished(new WipeReport(["the cookies", "what the web view keeps"], [])), "a finished wipe is told as unfinished");
        Expect.That(!SignOutWords.Unfinished(null), "no report is told as unfinished");
        StringAssert.Contains(SignOutWords.UnfinishedHelp, "Restart");
    }
}
