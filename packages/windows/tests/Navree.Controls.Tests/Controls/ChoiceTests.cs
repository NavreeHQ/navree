using Navree.Controls.Tests.Tables;
using Navree.Design.Tables;

namespace Navree.Controls.Tests.Controls;

[TestClass]
public sealed class ChoiceTallyTests
{
    [TestMethod]
    public void The_rows_are_counted_once_and_each_change_adjusts_the_counts()
    {
        ChoiceTally tally = new();
        int recounts = 0;
        (int, int) Recount()
        {
            recounts++;
            return (100_000, 3);
        }

        Assert.AreEqual((100_000, 3), tally.Get(Recount));
        tally.Changed(1);
        tally.Changed(1);
        tally.Changed(-1);
        Assert.AreEqual((100_000, 4), tally.Get(Recount));
        Assert.AreEqual(1, recounts, "a key held down does not count a hundred thousand rows each time");
        tally.NoneChosen();
        Assert.AreEqual((100_000, 0), tally.Get(Recount));
        tally.Invalidate();
        tally.Get(Recount);
        Assert.AreEqual(2, recounts, "the rows changed: they are counted again");
    }

    [TestMethod]
    public void A_count_never_goes_below_none_or_above_the_rows_held_and_a_change_before_the_first_count_is_not_lost()
    {
        ChoiceTally tally = new();
        tally.Changed(5);
        Assert.AreEqual((10, 2), tally.Get(() => (10, 2)), "the first count reads the rows as they are");
        tally.Changed(50);
        Assert.AreEqual((10, 10), tally.Get(() => (0, 0)));
        tally.Changed(-50);
        Assert.AreEqual((10, 0), tally.Get(() => (0, 0)));
    }
}

[TestClass]
public sealed class RowChoosablesTests
{
    [TestMethod]
    public void A_range_reads_only_the_rows_in_it()
    {
        int reads = 0;
        RowChoosables rows = new(100_000, i =>
        {
            reads++;
            return i % 10 == 0 ? Choosable.No : Choosable.Yes;
        });
        IReadOnlyList<int> range = Choosing.Range(rows, 50_000, 50_004);
        Same.Items([50_001, 50_002, 50_003, 50_004], range);
        Assert.AreEqual(5, reads, "five rows read, not a hundred thousand");
        Assert.AreEqual(100_000, rows.Count);
    }
}

[TestClass]
public sealed class KeylessKeysTests
{
    [TestMethod]
    public void Rows_with_no_key_never_get_a_key_another_row_has_or_had()
    {
        KeylessKeys keys = new();
        string[] made = [.. Enumerable.Range(0, 1000).Select(_ => keys.Next())];
        Assert.AreEqual(1000, made.Distinct().Count());
        Assert.IsTrue(made.All(KeylessKeys.IsKeyless));
        Assert.IsFalse(KeylessKeys.IsKeyless("c00001"));
        Assert.IsFalse(KeylessKeys.IsKeyless(string.Empty));
    }
}

[TestClass]
public sealed class MetricsWatchTests
{
    [TestMethod]
    public void A_change_of_the_text_size_or_of_the_scale_is_noticed_once()
    {
        MetricsWatch watch = new();
        Assert.IsTrue(watch.Changed(1.0, 1.0), "the first reading is a change");
        Assert.IsFalse(watch.Changed(1.0, 1.0));
        Assert.IsTrue(watch.Changed(1.5, 1.0));
        Assert.IsFalse(watch.Changed(1.5, 1.0));
        Assert.IsTrue(watch.Changed(1.5, 2.25));
        Assert.IsFalse(watch.Changed(1.5, 2.25));
    }
}

[TestClass]
public sealed class PruneTests
{
    [TestMethod]
    public void Keys_of_rows_that_are_gone_are_taken_out_and_said()
    {
        HashSet<string> keys = ["a", "b", "c"];
        IReadOnlyList<string> gone = Choosing.Prune(keys, k => k != "b");
        Same.Items(["b"], gone);
        Same.Items(["a", "c"], keys.Order());
        Assert.AreEqual(0, Choosing.Prune(keys, _ => true).Count);
    }
}
