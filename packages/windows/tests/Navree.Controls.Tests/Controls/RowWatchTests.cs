using Navree.Controls.Tests.Tables;
using Navree.Design.Tables;

namespace Navree.Controls.Tests.Controls;

[TestClass]
public sealed class CursorWatchTests
{
    [TestMethod]
    public void More_rows_are_asked_for_once_for_each_count_of_rows_held()
    {
        CursorWatch watch = new();
        Assert.IsTrue(watch.ShouldAskAfter(50, available: true, idle: true, near: true));
        Assert.IsFalse(watch.ShouldAskAfter(50, true, true, true), "the host was asked at this count");
        Assert.IsTrue(watch.ShouldAskAfter(75, true, true, true), "more rows arrived and the end is near again");
        Assert.IsFalse(watch.ShouldAskAfter(100, available: false, idle: true, near: true));
        Assert.IsFalse(watch.ShouldAskAfter(100, true, idle: false, near: true));
        Assert.IsFalse(watch.ShouldAskAfter(100, true, true, near: false));
    }

    [TestMethod]
    public void A_refresh_with_the_same_count_of_rows_asks_again()
    {
        CursorWatch watch = new();
        Assert.IsTrue(watch.ShouldAskAfter(50, true, true, true));
        watch.Forget();
        Assert.IsTrue(watch.ShouldAskAfter(50, true, true, true), "the rows were replaced as a whole: the end is a new end");
        Assert.IsTrue(watch.ShouldAskBefore(50, true, true, true));
        watch.Asked();
        Assert.IsTrue(watch.ShouldAskBefore(50, true, true, true), "a cursor changed");
    }

    [TestMethod]
    public void The_held_limit_is_said_once_until_the_rows_are_replaced()
    {
        CursorWatch watch = new();
        Assert.IsTrue(watch.LimitReached());
        Assert.IsFalse(watch.LimitReached());
        watch.Asked();
        Assert.IsFalse(watch.LimitReached(), "a cursor changing does not make the limit new");
        watch.Forget();
        Assert.IsTrue(watch.LimitReached(), "a second limit in the same model, after a new query, is said");
    }
}

[TestClass]
public sealed class DetailSeedsTests
{
    [TestMethod]
    public void A_row_that_arrives_open_is_opened_once_and_a_person_shutting_it_is_not_undone_by_a_redraw()
    {
        DetailSeeds seeds = new();
        Assert.IsTrue(seeds.Arrive("c1", hasDetail: true, detailOpen: true));
        Assert.IsFalse(seeds.Arrive("c1", true, true), "the same row drawn again says nothing new");
        Assert.IsFalse(seeds.Arrive("c2", true, detailOpen: false));
        Assert.IsFalse(seeds.Arrive("c3", hasDetail: false, detailOpen: true), "a row with no detail has nothing to open");
        Assert.IsFalse(seeds.Arrive(null, true, true));
        seeds.Forget();
        Assert.IsTrue(seeds.Arrive("c1", true, true), "another model starts again");
    }
}

[TestClass]
public sealed class RowShapeWatchTests
{
    [TestMethod]
    public void A_later_page_with_a_second_line_or_a_detail_is_noticed_once_for_each()
    {
        RowShapeWatch watch = new();
        Assert.AreEqual((false, false), watch.Observe(hasSecondLine: false, hasDetail: false));
        Assert.AreEqual((true, false), watch.Observe(true, false));
        Assert.AreEqual((false, false), watch.Observe(true, false), "the rows are two lines tall already");
        Assert.AreEqual((false, true), watch.Observe(true, true));
        Assert.IsTrue(watch.SawSecondLine);
        Assert.IsTrue(watch.SawDetail);
        watch.Forget();
        Assert.AreEqual((true, true), watch.Observe(true, true), "a new query starts again");
    }
}

[TestClass]
public sealed class StoredLayoutApplierTests
{
    private static readonly DataTableColumn[] Columns =
    [
        new("name", "Name", ColumnKind.Title, ColumnRole.Title, DataTableAlign.Start, null, false),
        new("city", "City", ColumnKind.Words, ColumnRole.None, DataTableAlign.Start, null, false),
        new("plan", "Plan", ColumnKind.Words, ColumnRole.None, DataTableAlign.Start, null, true),
    ];

    [TestMethod]
    public void A_kept_layout_gives_widths_hidden_and_shown_columns_and_says_the_density_and_the_order()
    {
        StoredLayout stored = new(
            LayoutText.CurrentVersion,
            "comfortable",
            2,
            ["city", "name", "gone"],
            new Dictionary<string, StoredColumn>
            {
                ["city"] = new("words", 200, null, 3),
                ["plan"] = new("words", null, false, 5),
                ["name"] = new("title", 999, true, 4),
            });
        Dictionary<string, double> widths = [];
        HashSet<string> person = [];
        HashSet<string> hidden = [];
        HashSet<string> forced = [];
        AppliedLayout? applied = StoredLayoutApplier.Apply(stored, Columns, widths, person, hidden, forced);
        Assert.IsNotNull(applied);
        Same.Items(["city", "plan", "name"], applied.Order);
        Assert.AreEqual(2, applied.Frozen);
        Assert.AreEqual(Density.Comfortable, applied.Density);
        Assert.AreEqual(5, applied.Seen);
        Same.Number(200, widths["city"]);
        Assert.IsFalse(widths.ContainsKey("plan"));
        Same.Number(800, widths["name"]);
        Same.Items(["city", "name"], person.Order());
        Same.Items(["name"], hidden);
        Same.Items(["plan"], forced);
    }

    [TestMethod]
    public void A_width_kept_for_another_kind_of_column_is_not_used_and_a_text_naming_no_column_is_not_applied()
    {
        StoredLayout kindChanged = new(LayoutText.CurrentVersion, "standard", 1, ["city"], new Dictionary<string, StoredColumn> { ["city"] = new("number", 200, null, 1) });
        Dictionary<string, double> widths = [];
        HashSet<string> person = [];
        Assert.IsNotNull(StoredLayoutApplier.Apply(kindChanged, Columns, widths, person, [], []));
        Assert.AreEqual(0, widths.Count);

        StoredLayout other = new(LayoutText.CurrentVersion, "compact", 1, ["x", "y"], new Dictionary<string, StoredColumn> { ["x"] = new("words", 100, null, 1) });
        Assert.IsNull(StoredLayoutApplier.Apply(other, Columns, widths, person, [], []));
    }

    [TestMethod]
    public void A_density_the_text_does_not_name_is_not_said()
    {
        StoredLayout stored = new(LayoutText.CurrentVersion, string.Empty, 1, ["city"], new Dictionary<string, StoredColumn> { ["city"] = new("words", 100, null, 1) });
        Assert.IsNull(StoredLayoutApplier.Apply(stored, Columns, [], [], [], [])!.Density);
    }
}
