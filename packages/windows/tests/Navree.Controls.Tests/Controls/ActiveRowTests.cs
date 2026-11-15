using Navree.Controls.Tests.Tables;
using Navree.Design.Tables;

namespace Navree.Controls.Tests.Controls;

[TestClass]
public sealed class RowPinsTests
{
    [TestMethod]
    public void Each_owner_keeps_its_own_row_and_letting_go_leaves_the_others()
    {
        RowPins pins = new();
        pins.Set(PinOwner.Active, 900);
        pins.Set(PinOwner.Anchor, 20);
        pins.Set(PinOwner.Assistive, 500);
        Same.Items([20, 500, 900], pins.Indices);
        pins.Release(PinOwner.Anchor);
        Same.Items([500, 900], pins.Indices);
        pins.Set(PinOwner.Assistive, 7);
        Assert.AreEqual(900, pins.Of(PinOwner.Active), "asking for another row does not take the focused row's pin");
        Same.Items([7, 900], pins.Indices);
    }

    [TestMethod]
    public void Two_owners_on_one_row_pin_it_once_and_the_row_stays_until_both_let_go()
    {
        RowPins pins = new();
        pins.Set(PinOwner.Active, 4);
        pins.Set(PinOwner.Anchor, 4);
        Same.Items([4], pins.Indices);
        pins.Release(PinOwner.Anchor);
        Same.Items([4], pins.Indices);
        pins.Release(PinOwner.Active);
        Assert.AreEqual(0, pins.Indices.Count);
    }

    [TestMethod]
    public void Rows_that_join_or_leave_before_a_pin_move_it_and_rows_that_leave_with_it_let_it_go()
    {
        RowPins pins = new();
        pins.Set(PinOwner.Active, 100);
        pins.Set(PinOwner.Assistive, 10);
        pins.RowsJoined(50, 25);
        Same.Items([10, 125], pins.Indices);
        pins.RowsLeft(60, 40);
        Same.Items([10, 85], pins.Indices);
        pins.RowsLeft(5, 10);
        Same.Items([75], pins.Indices);
        Assert.AreEqual(-1, pins.Of(PinOwner.Assistive), "the row that left is not pinned any more");
    }
}

[TestClass]
public sealed class ActiveRowRulesTests
{
    [TestMethod]
    public void After_a_reset_the_active_row_is_the_same_row_by_key_else_the_first_row()
    {
        Assert.AreEqual(42, ActiveRowRules.AfterReset(onHeader: false, indexOfKey: 42, rowCount: 100));
        Assert.AreEqual(0, ActiveRowRules.AfterReset(false, -1, 100));
        Assert.AreEqual(-1, ActiveRowRules.AfterReset(false, -1, 0));
        Assert.AreEqual(-1, ActiveRowRules.AfterReset(true, 3, 100), "a heading stays a heading");
    }

    [TestMethod]
    public void After_rows_join_the_active_row_is_the_row_it_was()
    {
        Assert.AreEqual(60, ActiveRowRules.AfterInsert(10, 0, 50));
        Assert.AreEqual(10, ActiveRowRules.AfterInsert(10, 11, 50));
        Assert.AreEqual(60, ActiveRowRules.AfterInsert(10, 10, 50));
        Assert.AreEqual(-1, ActiveRowRules.AfterInsert(-1, 0, 50));
    }

    [TestMethod]
    public void After_rows_leave_the_active_row_is_the_row_it_was_or_the_row_at_its_place()
    {
        Assert.AreEqual(new Cell(5, 2), ActiveRowRules.AfterRemove(10, 0, 5, 95, 2, hasHeader: true).Active);
        Assert.IsFalse(ActiveRowRules.AfterRemove(10, 0, 5, 95, 2, true).Removed);
        Assert.AreEqual(new Cell(10, 2), ActiveRowRules.AfterRemove(10, 20, 5, 95, 2, true).Active);

        (Cell? active, bool removed) = ActiveRowRules.AfterRemove(10, 10, 1, 94, 2, true);
        Assert.IsTrue(removed);
        Assert.AreEqual(new Cell(10, 2), active, "the row now at the same place");

        (active, removed) = ActiveRowRules.AfterRemove(93, 93, 1, 93, 2, true);
        Assert.IsTrue(removed);
        Assert.AreEqual(new Cell(92, 2), active, "the one before when it was the last");

        (active, removed) = ActiveRowRules.AfterRemove(0, 0, 1, 0, 3, true);
        Assert.IsTrue(removed);
        Assert.AreEqual(new Cell(-1, 3), active, "the heading of the same column when no row is left");
        Assert.IsNull(ActiveRowRules.AfterRemove(0, 0, 1, 0, 3, hasHeader: false).Active);
    }

    [TestMethod]
    public void Rows_joining_the_start_move_the_view_only_when_it_is_at_the_very_top()
    {
        Assert.IsTrue(ActiveRowRules.KeepsFirstRowInView(0, 300, 0));
        Assert.IsFalse(ActiveRowRules.KeepsFirstRowInView(0, 300, 3000), "the scroll view's anchoring and the pinned row hold a view that is down");
        Assert.IsFalse(ActiveRowRules.KeepsFirstRowInView(300, 300, 0), "rows at the end are not above the view");
        Assert.IsFalse(ActiveRowRules.KeepsFirstRowInView(0, 0, 0), "a table with no rows has no first row to keep");
    }
}

[TestClass]
public sealed class RowNameCacheTests
{
    [TestMethod]
    public void The_names_of_a_new_model_are_read_again_after_the_cache_is_cleared()
    {
        RowNameCache cache = new();
        string[] old = ["Amrit", "Gurpreet", "Harjot"];
        string[] fresh = ["Navdeep", "Simran"];
        Same.Items(old, cache.Get(3, i => old[i]));
        Same.Items(old, cache.Get(2, i => fresh[i]));
        cache.Clear();
        Assert.IsFalse(cache.HasNames);
        Same.Items(fresh, cache.Get(2, i => fresh[i]));
    }
}
