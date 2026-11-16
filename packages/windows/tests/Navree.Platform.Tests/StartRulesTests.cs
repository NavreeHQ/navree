namespace Navree.Platform.Tests;

/// <summary>A start that follows a stop of the core begins clean; any other start gives the core what the last run kept.</summary>
[TestClass]
public sealed class StartRulesTests
{
    private const string Snapshot = "{\"screens\":[\"/orders/CS-220480\"]}";

    private static string Folder() => Path.Combine(Path.GetTempPath(), "navree-start-" + Guid.NewGuid().ToString("N"));

    [TestMethod]
    public void A_start_gives_the_core_the_snapshot_the_last_run_kept_and_goes_on_giving_it()
    {
        string folder = Folder();
        try
        {
            KeptStore kept = new(folder, "Navree.Tests");
            Assert.IsNull(StartRules.SnapshotToRestore(kept), "a first start has a snapshot");
            kept.Write(StartRules.SnapshotKey, Snapshot);

            Assert.AreEqual(Snapshot, StartRules.SnapshotToRestore(new KeptStore(folder, "Navree.Tests")));
            Assert.AreEqual(Snapshot, StartRules.SnapshotToRestore(new KeptStore(folder, "Navree.Tests")), "a start used the snapshot up");
        }
        finally
        {
            Directory.Delete(folder, recursive: true);
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
            Directory.Delete(folder, recursive: true);
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
            Directory.Delete(folder, recursive: true);
        }
    }
}
