using System.Globalization;
using Navree.Design.Tables;

namespace Navree.Controls.Tests.Tables;

[TestClass]
public sealed class RowFoldingTests
{
    private static FoldColumn Col(ColumnRole role, bool numeric = false, bool secondary = false) => new(role, numeric, secondary);

    private static FoldCell Text(params string[] texts) => new(CellKind.Text, texts, 0);

    private static readonly FoldCell Menu = new(CellKind.Control, [], 1, "Actions");

    [TestMethod]
    [DataRow(640.9, 6, 500.0, 40.0, 1.0, true)]
    [DataRow(641.0, 6, 500.0, 40.0, 1.0, false)]
    [DataRow(1200.0, 2, 100.0, 0.0, 1.0, true)]
    [DataRow(1200.0, 3, 100.0, 0.0, 1.0, false)]
    [DataRow(700.0, 5, 900.0, 80.0, 2.0, true)]
    [DataRow(700.0, 5, 900.0, 80.0, 1.5, false)]
    [DataRow(700.0, 5, 500.0, 80.0, 2.25, false)]
    public void A_table_is_a_list_when_it_is_narrow_has_two_columns_or_cannot_fit_at_large_text(
        double width, int shown, double leastSum, double gutter, double scale, bool expected) =>
        Assert.AreEqual(expected, RowFolding.FoldsToList(width, shown, leastSum, gutter, scale));

    [TestMethod]
    public void A_trailing_value_moves_under_the_text_at_two_hundred_percent()
    {
        Assert.IsFalse(RowFolding.TrailingBelow(1.99));
        Assert.IsTrue(RowFolding.TrailingBelow(2.0));
    }

    [TestMethod]
    public void A_record_with_every_role_folds_into_its_parts()
    {
        FoldedRow row = RowFolding.Fold(
            [Col(ColumnRole.Title), Col(ColumnRole.Subtitle), Col(ColumnRole.Status), Col(ColumnRole.Trailing), Col(ColumnRole.Actions)],
            [Text("Gurpreet Singh"), Text("gurpreet@example.in"), new(CellKind.Badge, ["Won"], 0), Text("₹1,00,000"), Menu]);
        Assert.IsNull(row.LeadingIndex);
        Assert.AreEqual("Gurpreet Singh", row.Headline);
        Same.Items(["gurpreet@example.in"], row.Lines);
        Same.Items([2], row.StatusIndexes);
        Assert.AreEqual(3, row.TrailingIndex);
        Same.Items([4], row.MenuIndexes);
        Assert.IsFalse(row.ActionFromControl);
    }

    [TestMethod]
    public void A_numeric_column_with_no_role_stands_at_the_end()
    {
        FoldedRow row = RowFolding.Fold(
            [Col(ColumnRole.None), Col(ColumnRole.None), Col(ColumnRole.None, numeric: true)],
            [Text("Ludhiana"), Text("+91 98765 43210"), Text("12")]);
        Assert.AreEqual("Ludhiana", row.Headline);
        Same.Items(["+91 98765 43210"], row.Lines);
        Assert.AreEqual(2, row.TrailingIndex);
    }

    [TestMethod]
    public void With_no_roles_at_all_the_last_of_three_cells_stands_at_the_end()
    {
        FoldedRow row = RowFolding.Fold(
            [Col(ColumnRole.None), Col(ColumnRole.None), Col(ColumnRole.None)],
            [Text("A"), Text("B"), Text("C")]);
        Assert.AreEqual(2, row.TrailingIndex);
        Same.Items(["B"], row.Lines);
    }

    [TestMethod]
    public void A_control_that_names_the_record_gives_its_label_and_its_action()
    {
        FoldedRow row = RowFolding.Fold(
            [Col(ColumnRole.Title), Col(ColumnRole.None)],
            [new(CellKind.Control, [], 1, "INV-1042"), Text("Amritsar")]);
        Assert.AreEqual("INV-1042", row.Headline);
        Same.Items(["Amritsar"], row.Lines);
        Assert.IsTrue(row.ActionFromControl);
    }

    [TestMethod]
    public void A_secondary_column_is_left_out()
    {
        FoldedRow row = RowFolding.Fold(
            [Col(ColumnRole.Title), Col(ColumnRole.None, secondary: true), Col(ColumnRole.None, numeric: true)],
            [Text("A"), Text("B"), Text("C")]);
        Assert.AreEqual(2, row.TrailingIndex);
        Assert.AreEqual(0, row.Lines.Count);
    }

    [TestMethod]
    public void A_line_that_says_what_another_says_is_said_once()
    {
        FoldedRow row = RowFolding.Fold(
            [Col(ColumnRole.Title), Col(ColumnRole.Subtitle)],
            [Text("Amritsar Depot", "Warehouse"), Text("Warehouse")]);
        Same.Items(["Warehouse"], row.Lines);
    }

    [TestMethod]
    public void A_badge_in_a_column_with_no_role_is_a_state()
    {
        FoldedRow row = RowFolding.Fold(
            [Col(ColumnRole.Title), Col(ColumnRole.None)],
            [Text("Order 7"), new(CellKind.Badge, ["Paid"], 0)]);
        Same.Items([1], row.StatusIndexes);
    }

    [TestMethod]
    public void A_leading_cell_leads_and_is_not_a_line()
    {
        FoldedRow row = RowFolding.Fold(
            [Col(ColumnRole.Leading), Col(ColumnRole.Title), Col(ColumnRole.None)],
            [new(CellKind.Other, [], 0), Text("Amandeep Singh"), Text("Jalandhar")]);
        Assert.AreEqual(0, row.LeadingIndex);
        Assert.AreEqual("Amandeep Singh", row.Headline);
        Same.Items(["Jalandhar"], row.Lines);
        Assert.IsNull(row.TrailingIndex);
    }

    [TestMethod]
    public void The_title_is_the_column_with_the_role_wherever_it_stands()
    {
        FoldedRow row = RowFolding.Fold([Col(ColumnRole.None), Col(ColumnRole.Title)], [Text("x"), Text("The name")]);
        Assert.AreEqual("The name", row.Headline);
    }

    [TestMethod]
    public void A_record_with_no_cells_folds_to_nothing() =>
        Assert.AreEqual(string.Empty, RowFolding.Fold([], []).Headline);

    [TestMethod]
    public void Every_row_of_a_list_has_the_height_of_the_tallest_of_the_first()
    {
        FoldedRow plain = new(null, "a", [], [], null, [], false);
        FoldedRow oneLine = new(null, "a", ["b"], [], null, [], false);
        FoldedRow full = new(null, "a", ["b", "c", "d"], [1], 3, [], false);
        Assert.AreEqual(1, RowFolding.ListShape([plain], false));
        Assert.AreEqual(2, RowFolding.ListShape([plain, oneLine], false));
        Assert.AreEqual(4, RowFolding.ListShape([oneLine, full], false));
        Assert.AreEqual(3, RowFolding.ListShape([new FoldedRow(null, "a", ["b", "c", "d"], [], null, [], false)], false));
        Assert.AreEqual(3, RowFolding.ListShape([new FoldedRow(null, "a", ["b"], [], 3, [], false)], true));
        Assert.AreEqual(4, RowFolding.ListShape([full], true));
        Assert.AreEqual(1, RowFolding.ListShape([], false));
    }

    [TestMethod]
    [DataRow(1, 20.0, 16.0, 1.0, 48.0)]
    [DataRow(2, 20.0, 16.0, 1.0, 64.0)]
    [DataRow(3, 20.0, 16.0, 1.0, 80.0)]
    [DataRow(4, 20.0, 16.0, 1.0, 96.0)]
    [DataRow(2, 30.0, 24.0, 1.0, 80.0)]
    [DataRow(1, 30.0, 24.0, 1.0, 56.0)]
    public void A_list_row_is_as_tall_as_its_lines_ask(int shape, double body, double caption, double scale, double expected) =>
        Same.Number(expected, RowFolding.ListRowHeight(shape, body, caption, scale));
}

[TestClass]
public sealed class ColumnSetTests
{
    private static readonly ColInfo[] Columns =
    [
        new("a", ColumnKind.Words, ColumnRole.None, false),
        new("b", ColumnKind.Words, ColumnRole.None, false),
        new("c", ColumnKind.Words, ColumnRole.None, true),
        new("d", ColumnKind.Number, ColumnRole.None, false),
    ];

    private static IReadOnlySet<string> Set(params string[] keys) => new HashSet<string>(keys);

    [TestMethod]
    public void Hidden_and_secondary_columns_are_left_out_unless_the_width_or_the_person_brings_them_back()
    {
        Same.Items([0, 2, 3], ColumnSet.Shown(Columns, Set("b"), Set(), 1200));
        Same.Items([0, 3], ColumnSet.Shown(Columns, Set("b"), Set(), 900));
        Same.Items([0, 2, 3], ColumnSet.Shown(Columns, Set("b"), Set("c"), 900));
    }

    [TestMethod]
    public void A_subtitle_column_is_not_a_table_column() =>
        Same.Items([0], ColumnSet.Shown([new("a", ColumnKind.Title, ColumnRole.Title, false), new("s", ColumnKind.Words, ColumnRole.Subtitle, false)], Set(), Set(), 1200));

    [TestMethod]
    public void No_more_than_forty_eight_columns_are_shown()
    {
        ColInfo[] sixty = Enumerable.Range(0, 60).Select(i => new ColInfo("c" + i, ColumnKind.Words, ColumnRole.None, false)).ToArray();
        Assert.AreEqual(48, ColumnSet.Shown(sixty, Set(), Set(), 1200).Count);
    }

    [TestMethod]
    public void The_name_the_actions_and_the_last_words_are_not_hidden()
    {
        ColInfo title = new("t", ColumnKind.Title, ColumnRole.Title, false);
        ColInfo words = new("w", ColumnKind.Words, ColumnRole.None, false);
        ColInfo number = new("n", ColumnKind.Number, ColumnRole.None, false);
        ColInfo actions = new("x", ColumnKind.Controls, ColumnRole.Actions, false);
        Assert.IsFalse(ColumnSet.CanHide([title, words], 0));
        Assert.IsTrue(ColumnSet.CanHide([title, words], 1));
        Assert.IsTrue(ColumnSet.CanHide([words, number], 0));
        Assert.IsFalse(ColumnSet.CanHide([words], 0));
        Assert.IsFalse(ColumnSet.CanHide([title, actions], 1));
    }

    [TestMethod]
    public void The_first_column_is_frozen_and_a_picture_before_the_name_is_frozen_with_it()
    {
        ColInfo leading = new("l", ColumnKind.Leading, ColumnRole.Leading, false);
        ColInfo title = new("t", ColumnKind.Title, ColumnRole.Title, false);
        ColInfo words = new("w", ColumnKind.Words, ColumnRole.None, false);
        Assert.AreEqual(2, ColumnSet.DefaultFrozen([leading, title, words]));
        Assert.AreEqual(1, ColumnSet.DefaultFrozen([title, words]));
        Assert.AreEqual(1, ColumnSet.DefaultFrozen([words, title]));
        Assert.AreEqual(0, ColumnSet.DefaultFrozen([]));
    }
}

[TestClass]
public sealed class ColumnOrderTests
{
    [TestMethod]
    public void A_stored_order_is_made_to_fit_the_servers_columns()
    {
        Same.Items(["c", "d", "a", "b"], ColumnOrder.Reconcile(["a", "b", "c", "d"], ["c", "a", "b"]));
        Same.Items(["b", "a", "x"], ColumnOrder.Reconcile(["a", "x", "b"], ["b", "a"]));
        Same.Items(["n", "b", "a"], ColumnOrder.Reconcile(["n", "a", "b"], ["b", "a"]));
        Same.Items(["c", "a"], ColumnOrder.Reconcile(["a", "c"], ["c", "b", "a"]));
        Same.Items(["a", "b"], ColumnOrder.Reconcile(["a", "b"], ["x", "y"]));
        Same.Items(["a", "b"], ColumnOrder.Reconcile(["a", "b"], []));
        Same.Items(["b", "a"], ColumnOrder.Reconcile(["a", "b"], ["b", "b", "a"]));
    }

    [TestMethod]
    public void A_column_moves()
    {
        Same.Items(["a", "c", "d", "b"], ColumnOrder.Move(["a", "b", "c", "d"], 1, 3));
        Same.Items(["d", "a", "b", "c"], ColumnOrder.Move(["a", "b", "c", "d"], 3, 0));
        Same.Items(["a", "b", "c", "d"], ColumnOrder.Move(["a", "b", "c", "d"], 2, 2));
        Assert.ThrowsExactly<ArgumentOutOfRangeException>(() => ColumnOrder.Move(["a", "b"], 0, 2));
    }

    [TestMethod]
    [DataRow(20.0, 0, 3, 0)]
    [DataRow(100.0, 0, 3, 1)]
    [DataRow(200.0, 0, 3, 1)]
    [DataRow(300.0, 0, 3, 2)]
    [DataRow(400.0, 0, 3, 3)]
    [DataRow(20.0, 1, 2, 1)]
    public void The_drop_place_is_the_number_of_other_columns_before_the_pointer(double pointer, int start, int end, int expected) =>
        Assert.AreEqual(expected, ColumnOrder.DropIndex(1, pointer, [50, 150, 250, 350], start, end));

    [TestMethod]
    [DataRow(2, 1, 1, 1, 5, true)]
    [DataRow(3, 1, 1, 1, 5, false)]
    [DataRow(1, -1, 1, 1, 5, false)]
    [DataRow(0, 1, 1, 1, 5, false)]
    [DataRow(0, 1, 2, 1, 5, true)]
    public void A_column_moves_only_inside_its_own_region(int index, int delta, int frozenStart, int frozenEnd, int count, bool expected) =>
        Assert.AreEqual(expected, ColumnOrder.CanMove(index, delta, frozenStart, frozenEnd, count));
}

[TestClass]
public sealed class LayoutTextTests
{
    private static StoredLayout Two() => new(
        1,
        "standard",
        1,
        ["name", "amount"],
        new Dictionary<string, StoredColumn>
        {
            ["name"] = new("title", 260, null, 41),
            ["amount"] = new("number", 132, true, 41),
        });

    [TestMethod]
    public void A_layout_is_read_back_as_it_was_written()
    {
        StoredLayout back = LayoutText.Parse(LayoutText.Serialize(Two()))!;
        Assert.AreEqual("standard", back.Density);
        Assert.AreEqual(1, back.Frozen);
        Same.Items(["name", "amount"], back.Order);
        Assert.AreEqual(2, back.Columns.Count);
        Assert.AreEqual(new StoredColumn("title", 260, null, 41), back.Columns["name"]);
        Assert.AreEqual(new StoredColumn("number", 132, true, 41), back.Columns["amount"]);
    }

    [TestMethod]
    public void Text_that_is_not_a_layout_is_not_read()
    {
        Assert.IsNull(LayoutText.Parse("{"));
        Assert.IsNull(LayoutText.Parse(string.Empty));
        Assert.IsNull(LayoutText.Parse("""{"v":2,"order":[]}"""));
        Assert.IsNull(LayoutText.Parse("[1]"));
    }

    [TestMethod]
    public void A_bad_width_and_a_repeated_key_are_dropped_on_reading()
    {
        StoredLayout layout = LayoutText.Parse("""{"v":1,"order":["a","a","b"],"cols":{"a":{"w":-4,"s":2},"b":{"w":90,"s":3}}}""")!;
        Same.Items(["a", "b"], layout.Order);
        Assert.IsNull(layout.Columns["a"].Width);
        Assert.AreEqual(90.0, layout.Columns["b"].Width);
    }

    [TestMethod]
    public void No_more_than_sixty_four_columns_are_kept_and_the_ones_in_use_stay()
    {
        Dictionary<string, StoredColumn> many = [];
        for (int i = 0; i < 70; i++)
        {
            many["k" + i.ToString(CultureInfo.InvariantCulture)] = new StoredColumn(null, 100, null, i);
        }

        StoredLayout layout = new(1, string.Empty, 0, ["k0"], many);
        StoredLayout back = LayoutText.Parse(LayoutText.Serialize(layout))!;
        Assert.AreEqual(64, back.Columns.Count);
        Assert.IsTrue(back.Columns.ContainsKey("k0"));
        Assert.IsTrue(back.Columns.ContainsKey("k69"));
        Assert.IsFalse(back.Columns.ContainsKey("k1"));
    }
}

[TestClass]
public sealed class SmallRulesTests
{
    private static readonly string[] Cities = ["Amritsar", "Bathinda", "Barnala", "Chandigarh"];
    private static readonly CultureInfo English = CultureInfo.GetCultureInfo("en-US");

    [TestMethod]
    public void Typing_finds_the_next_name_that_starts_so()
    {
        Assert.AreEqual(1, TypeAhead.Next(Cities, 0, "b", English));
        Assert.AreEqual(1, TypeAhead.Next(Cities, 0, "ba", English));
        Assert.AreEqual(2, TypeAhead.Next(Cities, 0, "bar", English));
        Assert.AreEqual(1, TypeAhead.Next(Cities, 2, "b", English));
        Assert.AreEqual(-1, TypeAhead.Next(Cities, 0, "z", English));
        Assert.AreEqual(-1, TypeAhead.Next(Cities, 0, string.Empty, English));
        Assert.AreEqual(2, TypeAhead.Next(["ਅੰਮ੍ਰਿਤਸਰ", "ਬਠਿੰਡਾ", "ਬਰਨਾਲਾ"], 1, "ਬ", CultureInfo.GetCultureInfo("pa-IN")));
    }

    [TestMethod]
    [DataRow(24.0, 600.0, -600.0)]
    [DataRow(0.0, 600.0, -1200.0)]
    [DataRow(-20.0, 600.0, -1200.0)]
    [DataRow(576.0, 600.0, 600.0)]
    [DataRow(300.0, 600.0, 0.0)]
    [DataRow(24.0, 90.0, 0.0)]
    public void Dragging_near_an_edge_scrolls_that_way(double pointer, double viewport, double expected) =>
        Same.Number(expected, AutoScroll.Velocity(pointer, viewport));

    [TestMethod]
    public void Copied_rows_are_lines_of_tab_separated_cells()
    {
        Assert.AreEqual(
            "Name\tCity\r\nGurpreet Singh\tLudhiana",
            ClipboardText.ForRows(["Name", "City"], [["Gurpreet Singh", "Ludhiana"]]));
        Assert.AreEqual("a b c", ClipboardText.ForCell("a\tb\nc"));
    }

    [TestMethod]
    public void Messages_are_resource_ids_and_values()
    {
        Announcement sorted = Announcements.Sorted("Amount", true);
        Assert.AreEqual("DataTable_Sorted_Ascending", sorted.Id);
        Same.Items<object>(["Amount"], sorted.Arguments);
        Assert.AreEqual("DataTable_Sorted_Descending", Announcements.Sorted("Amount", false).Id);
        Assert.AreEqual("DataTable_Chosen_None", Announcements.Chosen(0, null, false).Id);
        Assert.AreEqual("DataTable_Chosen_Count", Announcements.Chosen(3, null, false).Id);
        Same.Items<object>([3L], Announcements.Chosen(3, null, false).Arguments);
        Assert.AreEqual("DataTable_Chosen_All", Announcements.Chosen(0, 1204, true).Id);
        Assert.AreEqual("DataTable_Chosen_AllUnknown", Announcements.Chosen(0, null, true).Id);
        Assert.AreEqual("DataTable_RowsAdded_Earlier", Announcements.RowsAdded(25, true).Id);
        Assert.AreEqual("DataTable_RowsAdded_Later", Announcements.RowsAdded(25, false).Id);
        Assert.AreEqual("DataTable_ColumnShown", Announcements.ColumnVisibility("Amount", true).Id);
        Assert.AreEqual("DataTable_Details_Hidden", Announcements.Detail(false).Id);
    }
}
