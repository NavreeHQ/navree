using Navree.Design.Tables;

namespace Navree.Controls.Tests.Controls;

[TestClass]
public sealed class LayoutKeeperTests
{
    private sealed class FakeStore : IDataTableLayoutStore
    {
        internal List<(string Key, string Json)> Saved { get; } = [];

        public bool TryLoad(string key, out string json)
        {
            json = string.Empty;
            return false;
        }

        public void Save(string key, string json) => Saved.Add((key, json));
    }

    private static StoredLayout Layout(int count) =>
        new(LayoutText.CurrentVersion, "standard", 1, ["a", "b"], new Dictionary<string, StoredColumn> { ["a"] = new("words", 120, null, count) });

    [TestMethod]
    public void A_table_that_never_got_a_width_writes_nothing_when_it_is_unloaded()
    {
        FakeStore store = new();
        LayoutKeeper keeper = new();
        Assert.IsFalse(keeper.TrySave(store, "customers", Layout));
        keeper.Changed();
        Assert.IsFalse(keeper.TrySave(store, "customers", Layout), "a layout that was never read is not written over what was kept");
        Assert.AreEqual(0, store.Saved.Count);
    }

    [TestMethod]
    public void A_layout_that_was_read_and_not_changed_writes_nothing()
    {
        FakeStore store = new();
        LayoutKeeper keeper = new();
        Assert.IsTrue(keeper.ShouldRead("customers", ["a", "b"]));
        Assert.IsFalse(keeper.TrySave(store, "customers", Layout));
        Assert.AreEqual(0, store.Saved.Count);
    }

    [TestMethod]
    public void A_change_after_the_layout_was_read_is_written_once()
    {
        FakeStore store = new();
        LayoutKeeper keeper = new();
        keeper.ShouldRead("customers", ["a", "b"]);
        keeper.Resume(4);
        keeper.Changed();
        Assert.IsTrue(keeper.TrySave(store, "customers", Layout));
        Assert.IsFalse(keeper.TrySave(store, "customers", Layout), "the same change is not written twice");
        Assert.AreEqual(1, store.Saved.Count);
        Assert.AreEqual("customers", store.Saved[0].Key);
        Assert.AreEqual(5, keeper.Counter, "the count goes on from the count that was kept");
        Assert.AreEqual(5, LayoutText.Parse(store.Saved[0].Json)!.Columns["a"].Seen);
    }

    [TestMethod]
    public void A_key_and_a_set_of_columns_are_read_once_and_again_when_the_columns_change()
    {
        LayoutKeeper keeper = new();
        Assert.IsTrue(keeper.ShouldRead("customers", ["a", "b"]));
        Assert.IsFalse(keeper.ShouldRead("customers", ["a", "b"]));
        Assert.IsTrue(keeper.ShouldRead("customers", ["a", "b", "c"]));
        Assert.IsTrue(keeper.ShouldRead("orders", ["a", "b", "c"]));
    }

    [TestMethod]
    public void A_new_key_forgets_the_changes_of_the_old_one()
    {
        FakeStore store = new();
        LayoutKeeper keeper = new();
        keeper.ShouldRead("customers", ["a"]);
        keeper.Changed();
        keeper.Forget();
        Assert.IsFalse(keeper.WasRead);
        Assert.IsFalse(keeper.TrySave(store, "orders", Layout));
    }
}
