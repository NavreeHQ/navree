namespace Navree.Platform.Tests;

/// <summary>A start that follows a stop of the core begins clean; any other start gives the core what the last run kept.</summary>
[TestClass]
public sealed class StartRulesTests
{
    private const string Snapshot = "{\"screens\":[\"/orders/CS-220480\"]}";

    private static string Folder() => Path.Combine(Path.GetTempPath(), "navree-start-" + Guid.NewGuid().ToString("N"));

    private static void Delete(string folder)
    {
        if (Directory.Exists(folder))
        {
            Directory.Delete(folder, recursive: true);
        }
    }

    [TestMethod]
    public void A_start_gives_the_core_the_snapshot_the_last_run_kept_and_goes_on_giving_it()
    {
        string folder = Folder();
        try
        {
            KeptStore kept = new(folder, "Navree.Tests");
            Assert.IsNull(StartRules.SnapshotToRestore(kept), "a first start has a snapshot");
            kept.Write(StartRules.SnapshotKey, Snapshot);

            // Each start is given it, puts it back, and says so when that is over.
            KeptStore second = new(folder, "Navree.Tests");
            Assert.AreEqual(Snapshot, StartRules.SnapshotToRestore(second));
            StartRules.Restored(second);
            KeptStore third = new(folder, "Navree.Tests");
            Assert.AreEqual(Snapshot, StartRules.SnapshotToRestore(third), "a start used the snapshot up");
            StartRules.Restored(third);
            Assert.IsNull(third.Read(StartRules.RestoringKey));
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public void The_start_after_the_core_stopped_does_not_give_back_the_snapshot_that_was_kept_and_drops_it_one_start_later()
    {
        string folder = Folder();
        try
        {
            // A run kept a snapshot, and then its core stopped: the stop is noted on disk.
            KeptStore stopped = new(folder, "Navree.Tests");
            stopped.Write(StartRules.SnapshotKey, Snapshot);
            stopped.Write(StartRules.StoppedKey, "1");

            // The person presses Restart. The start is clean: the snapshot may be what stopped the core.
            KeptStore restart = new(folder, "Navree.Tests");
            Assert.IsNull(StartRules.SnapshotToRestore(restart), "the restart after a stop was given the snapshot that was kept before the stop");
            Assert.IsNull(restart.Read(StartRules.SnapshotKey), "the snapshot is still where a start takes it from");
            Assert.AreEqual(Snapshot, restart.Read(StartRules.AsideKey), "the snapshot was not kept aside for this one start");
            Assert.IsNull(restart.Read(StartRules.StoppedKey), "the stop is still noted: every start would begin clean for ever");

            // If the app is ended and started again before it kept anything new, that start is clean too, and what was kept aside is gone.
            KeptStore later = new(folder, "Navree.Tests");
            Assert.IsNull(StartRules.SnapshotToRestore(later), "a start was given the snapshot that was set aside");
            Assert.IsNull(later.Read(StartRules.AsideKey), "the snapshot kept aside outlived the one start it was kept for");

            // The app runs on and keeps a snapshot of its own; the next start is given that one.
            later.Write(StartRules.SnapshotKey, "{\"screens\":[\"/\"]}");
            Assert.AreEqual("{\"screens\":[\"/\"]}", StartRules.SnapshotToRestore(new KeptStore(folder, "Navree.Tests")));
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public void A_start_that_ended_while_it_put_the_snapshot_back_is_followed_by_a_clean_start_and_not_by_the_same_snapshot_for_ever()
    {
        string folder = Folder();
        try
        {
            KeptStore first = new(folder, "Navree.Tests");
            first.Write(StartRules.SnapshotKey, Snapshot);

            // The app starts and gives the core the snapshot. The note that it is being put back is on disk before the core has it.
            KeptStore dying = new(folder, "Navree.Tests");
            Assert.AreEqual(Snapshot, StartRules.SnapshotToRestore(dying));
            Assert.IsNotNull(dying.Read(StartRules.RestoringKey), "nothing on disk says a snapshot is being put back");

            // The app ends there, in the middle of it, with nobody told. The next start is clean: the snapshot is not given back.
            KeptStore next = new(folder, "Navree.Tests");
            Assert.IsNull(StartRules.SnapshotToRestore(next), "the start after one that ended while putting the snapshot back was given the same snapshot");
            Assert.IsNull(next.Read(StartRules.SnapshotKey), "the snapshot is still where a start takes it from");
            Assert.AreEqual(Snapshot, next.Read(StartRules.AsideKey), "the snapshot was not kept aside for this one start");
            Assert.IsNull(next.Read(StartRules.RestoringKey), "the note stays: every start would begin clean for ever");

            // The start after that is clean too (nothing was kept since), and what was set aside is gone.
            KeptStore later = new(folder, "Navree.Tests");
            Assert.IsNull(StartRules.SnapshotToRestore(later));
            Assert.IsNull(later.Read(StartRules.AsideKey));
            Assert.IsNull(later.Read(StartRules.RestoringKey), "a start with no snapshot to put back left a note that one is being put back");

            // A snapshot kept by a run that got through its start is given to the next start as usual.
            later.Write(StartRules.SnapshotKey, "{\"screens\":[\"/\"]}");
            KeptStore last = new(folder, "Navree.Tests");
            Assert.AreEqual("{\"screens\":[\"/\"]}", StartRules.SnapshotToRestore(last));
        }
        finally
        {
            Delete(folder);
        }
    }

    [TestMethod]
    public void A_stop_with_no_snapshot_kept_leaves_nothing_aside()
    {
        string folder = Folder();
        try
        {
            KeptStore stopped = new(folder, "Navree.Tests");
            stopped.Write(StartRules.StoppedKey, "1");

            KeptStore restart = new(folder, "Navree.Tests");
            Assert.IsNull(StartRules.SnapshotToRestore(restart));
            Assert.IsNull(restart.Read(StartRules.AsideKey));
            Assert.IsNull(restart.Read(StartRules.StoppedKey));
            Assert.AreEqual(0, restart.Count);
        }
        finally
        {
            Delete(folder);
        }
    }
}
