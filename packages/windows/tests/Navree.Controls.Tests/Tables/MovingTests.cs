using System.Globalization;
using Navree.Design.Tables;

namespace Navree.Controls.Tests.Tables;

[TestClass]
public sealed class AnchoringTests
{
    [TestMethod]
    public void The_view_stays_where_it_was_when_rows_join_the_start()
    {
        Geometry before = new(40, []);
        Assert.AreEqual((100, 10.0), Anchoring.AnchorAt(before, 4010, 10_000));

        Geometry after = new(40, []);
        Same.Number(5010, Anchoring.OffsetAfterChange(125, 10, after, 10_025, 800));

        Geometry withOpen = new(40, [(130, 120)]);
        Same.Number(5010, Anchoring.OffsetAfterChange(125, 10, withOpen, 10_025, 800));
        Geometry openAmongNew = new(40, [(3, 120)]);
        Same.Number(5130, Anchoring.OffsetAfterChange(125, 10, openAmongNew, 10_025, 800));
    }

    [TestMethod]
    public void The_view_stays_where_it_was_when_a_row_above_is_removed() =>
        Same.Number(3970, Anchoring.OffsetAfterChange(99, 10, new Geometry(40, []), 9_999, 800));

    [TestMethod]
    public void The_offset_never_passes_the_end() =>
        Same.Number(1200, Anchoring.OffsetAfterChange(49, 30, new Geometry(40, []), 50, 800));

    [TestMethod]
    public void The_anchor_is_found_in_an_open_row_and_at_the_top()
    {
        Geometry g = new(40, [(3, 120)]);
        Assert.AreEqual((3, 10.0), Anchoring.AnchorAt(g, 130, 100));
        Assert.AreEqual((0, 0.0), Anchoring.AnchorAt(g, 0, 100));
    }

    [TestMethod]
    [DataRow(3000.0, 800.0, 4000.0, true)]
    [DataRow(0.0, 800.0, 4000.0, false)]
    [DataRow(1600.0, 800.0, 4000.0, true)]
    [DataRow(0.0, 800.0, 500.0, true)]
    public void More_rows_are_asked_for_when_the_end_is_two_viewports_off(double offset, double viewport, double extent, bool expected) =>
        Assert.AreEqual(expected, Cursor.ShouldAskMore(offset, viewport, extent));

    [TestMethod]
    [DataRow(500.0, true)]
    [DataRow(801.0, false)]
    [DataRow(0.0, true)]
    public void Earlier_rows_are_asked_for_when_the_start_is_a_viewport_off(double offset, bool expected) =>
        Assert.AreEqual(expected, Cursor.ShouldAskBefore(offset, 800));
}

[TestClass]
public sealed class GridNavigationTests
{
    private static Cell? Go(int row, int column, NavKey key, NavMods mods = NavMods.None, bool rtl = false, bool header = true, int rows = 100, int columns = 5) =>
        GridNavigation.Move(new Cell(row, column), key, mods, rows, columns, header, 19, rtl);

    [TestMethod]
    public void The_arrows_move_one_cell()
    {
        Assert.AreEqual(new Cell(5, 3), Go(5, 2, NavKey.Right));
        Assert.AreEqual(new Cell(5, 1), Go(5, 2, NavKey.Right, rtl: true));
        Assert.AreEqual(new Cell(5, 1), Go(5, 2, NavKey.Left));
        Assert.IsNull(Go(5, 4, NavKey.Right));
        Assert.IsNull(Go(5, 0, NavKey.Left));
        Assert.AreEqual(new Cell(4, 2), Go(5, 2, NavKey.Up));
        Assert.AreEqual(new Cell(6, 2), Go(5, 2, NavKey.Down));
        Assert.IsNull(Go(99, 2, NavKey.Down));
    }

    [TestMethod]
    public void The_first_row_and_the_header_are_neighbours()
    {
        Assert.AreEqual(new Cell(-1, 2), Go(0, 2, NavKey.Up));
        Assert.IsNull(Go(-1, 2, NavKey.Up));
        Assert.AreEqual(new Cell(0, 2), Go(-1, 2, NavKey.Down));
        Assert.IsNull(Go(0, 2, NavKey.Up, header: false));
        Assert.IsNull(Go(-1, 2, NavKey.Down, rows: 0));
    }

    [TestMethod]
    public void Home_and_End_go_to_the_ends_of_the_row_or_of_the_table()
    {
        Assert.AreEqual(new Cell(5, 0), Go(5, 2, NavKey.Home));
        Assert.AreEqual(new Cell(5, 4), Go(5, 2, NavKey.End));
        Assert.AreEqual(new Cell(0, 0), Go(50, 3, NavKey.Home, NavMods.Ctrl));
        Assert.AreEqual(new Cell(99, 4), Go(50, 3, NavKey.End, NavMods.Ctrl));
        Assert.AreEqual(new Cell(-1, 0), Go(-1, 2, NavKey.Home));
        Assert.AreEqual(new Cell(-1, 4), Go(-1, 2, NavKey.End));
    }

    [TestMethod]
    public void The_page_keys_move_a_page_of_rows()
    {
        Assert.AreEqual(new Cell(24, 2), Go(5, 2, NavKey.PageDown));
        Assert.AreEqual(new Cell(99, 2), Go(90, 2, NavKey.PageDown));
        Assert.AreEqual(new Cell(0, 2), Go(5, 2, NavKey.PageUp));
        Assert.IsNull(Go(0, 2, NavKey.PageUp));
        Assert.IsNull(Go(-1, 2, NavKey.PageDown));
    }

    [TestMethod]
    public void A_single_column_has_nowhere_to_go_sideways()
    {
        Assert.IsNull(Go(5, 0, NavKey.Left, columns: 1));
        Assert.IsNull(Go(5, 0, NavKey.Right, columns: 1));
    }

    [TestMethod]
    [DataRow(800.0, 40.0, 19)]
    [DataRow(100.0, 40.0, 1)]
    [DataRow(30.0, 40.0, 1)]
    [DataRow(1000.0, 56.0, 16)]
    public void A_page_is_the_whole_rows_in_view_less_one(double viewport, double row, int expected) =>
        Assert.AreEqual(expected, GridNavigation.PageRows(viewport, row));

    [TestMethod]
    [DataRow(100.0, 40.0, 0.0, 800.0, null)]
    [DataRow(780.0, 40.0, 0.0, 800.0, 20.0)]
    [DataRow(100.0, 40.0, 300.0, 800.0, 100.0)]
    [DataRow(1000.0, 40.0, 0.0, 800.0, 240.0)]
    [DataRow(100.0, 900.0, 0.0, 800.0, 100.0)]
    public void A_row_is_revealed_by_the_least_scroll(double top, double height, double viewTop, double viewHeight, double? expected)
    {
        double? got = GridNavigation.ScrollOffsetToReveal(top, height, viewTop, viewHeight);
        if (expected is null)
        {
            Assert.IsNull(got);
        }
        else
        {
            Same.Number(expected.Value, got!.Value);
        }
    }

    [TestMethod]
    [DataRow(700.0, 120.0, 0.0, 600.0, 200.0, 72.0, 292.0)]
    [DataRow(210.0, 100.0, 300.0, 600.0, 200.0, 72.0, 10.0)]
    [DataRow(300.0, 100.0, 100.0, 600.0, 200.0, 72.0, null)]
    [DataRow(250.0, 500.0, 0.0, 600.0, 200.0, 72.0, 50.0)]
    public void A_cell_is_revealed_clear_of_the_frozen_columns(
        double cellX, double cellWidth, double viewX, double viewWidth, double fs, double fe, double? expected)
    {
        double? got = GridNavigation.ScrollOffsetToRevealX(cellX, cellWidth, viewX, viewWidth, fs, fe);
        if (expected is null)
        {
            Assert.IsNull(got);
        }
        else
        {
            Same.Number(expected.Value, got!.Value);
        }
    }

    [TestMethod]
    public void Focus_after_a_removal_goes_to_the_row_that_took_the_place()
    {
        Assert.AreEqual(new Cell(5, 2), GridNavigation.FocusAfterRemoval(5, 10, 2, true));
        Assert.AreEqual(new Cell(8, 2), GridNavigation.FocusAfterRemoval(9, 9, 2, true));
        Assert.AreEqual(new Cell(-1, 2), GridNavigation.FocusAfterRemoval(0, 0, 2, true));
        Assert.IsNull(GridNavigation.FocusAfterRemoval(0, 0, 2, false));
    }

    [TestMethod]
    [DataRow(1, false, true, false, ActivationTarget.CellControl)]
    [DataRow(1, true, true, false, ActivationTarget.RowAction)]
    [DataRow(0, false, true, false, ActivationTarget.RowAction)]
    [DataRow(0, false, false, true, ActivationTarget.ToggleDetail)]
    [DataRow(0, false, false, false, ActivationTarget.None)]
    [DataRow(2, false, true, false, ActivationTarget.EnterCell)]
    [DataRow(1, true, false, false, ActivationTarget.CellControl)]
    public void Enter_goes_to_a_control_then_the_action_then_the_detail(
        int controls, bool nameCellOfActionRow, bool hasAction, bool hasDetail, ActivationTarget expected) =>
        Assert.AreEqual(expected, GridNavigation.Activation(controls, nameCellOfActionRow, hasAction, hasDetail));
}

[TestClass]
public sealed class ChoosingTests
{
    private static readonly Choosable[] Rows =
    [
        Choosable.Yes, Choosable.Yes, Choosable.No, Choosable.Yes, Choosable.None, Choosable.Yes,
    ];

    [TestMethod]
    public void A_range_holds_the_rows_that_can_be_chosen()
    {
        Same.Items([0, 1, 3, 5], Choosing.Range(Rows, 0, 5));
        Same.Items([0, 1, 3, 5], Choosing.Range(Rows, 5, 0));
        Same.Items([3], Choosing.Range(Rows, 3, 3));
        Same.Items(Array.Empty<int>(), Choosing.Range(Rows, 2, 2));
        Same.Items([3], Choosing.Range(Rows, -1, 3));
        Same.Items([1, 3], Choosing.Range(Rows, 1, 4));
    }

    [TestMethod]
    public void A_range_out_of_the_rows_is_refused() =>
        Assert.ThrowsExactly<ArgumentOutOfRangeException>(() => Choosing.Range(Rows, 0, 6));

    [TestMethod]
    [DataRow(0, 0, HeaderBoxState.Off)]
    [DataRow(5, 0, HeaderBoxState.Off)]
    [DataRow(5, 5, HeaderBoxState.On)]
    [DataRow(5, 2, HeaderBoxState.Indeterminate)]
    public void The_header_box_shows_all_none_or_some(int choosable, int chosen, HeaderBoxState expected) =>
        Assert.AreEqual(expected, Choosing.HeaderBox(choosable, chosen));

    [TestMethod]
    [DataRow(true, false, 6, 6, true)]
    [DataRow(true, false, 6, 5, false)]
    [DataRow(true, true, 6, 6, false)]
    [DataRow(false, false, 6, 6, false)]
    [DataRow(true, false, 0, 0, false)]
    public void Choosing_every_record_is_offered_once_every_held_row_is_chosen(bool available, bool on, int choosable, int chosen, bool expected) =>
        Assert.AreEqual(expected, Choosing.OfferAcross(available, on, choosable, chosen));

    [TestMethod]
    public void Clearing_a_row_ends_a_choice_of_every_record()
    {
        ChoiceState across = new(new HashSet<string>(), true);
        ChoiceState after = Choosing.AfterClear(across, "b", ["a", "b", "c"]);
        Assert.IsFalse(after.Across);
        Same.Items(new[] { "a", "c" }, after.Keys.OrderBy(k => k, StringComparer.Ordinal));

        ChoiceState some = new(new HashSet<string> { "a", "b", "c" }, false);
        Same.Items(new[] { "a", "c" }, Choosing.AfterClear(some, "b", []).Keys.OrderBy(k => k, StringComparer.Ordinal));
        Same.Items(new[] { "a", "b", "c" }, Choosing.AfterClear(some, "z", []).Keys.OrderBy(k => k, StringComparer.Ordinal));
    }

    [TestMethod]
    [DataRow(false, false, false, true, false, ClickMeaning.Toggle)]
    [DataRow(false, false, false, false, true, ClickMeaning.Open)]
    [DataRow(false, false, false, false, false, ClickMeaning.Focus)]
    [DataRow(true, false, false, false, true, ClickMeaning.Toggle)]
    [DataRow(false, true, false, false, true, ClickMeaning.Toggle)]
    [DataRow(false, true, true, false, false, ClickMeaning.Range)]
    [DataRow(true, false, true, true, false, ClickMeaning.Range)]
    public void A_click_opens_a_row_until_something_is_chosen(
        bool anyChosen, bool ctrl, bool shift, bool onBox, bool hasAction, ClickMeaning expected) =>
        Assert.AreEqual(expected, Choosing.Click(anyChosen, ctrl, shift, onBox, hasAction));

    [TestMethod]
    public void Counts_are_written_by_the_persons_culture()
    {
        Assert.AreEqual("100,000", Choosing.FormatCount(100_000, CultureInfo.GetCultureInfo("en-US")));
        Assert.AreEqual("0", Choosing.FormatCount(0, CultureInfo.GetCultureInfo("en-US")));

        // Indian English groups in lakhs when the system's culture data has it, which the first run confirms.
        CultureInfo india = CultureInfo.GetCultureInfo("en-IN");
        Assert.AreEqual("1,00,000", Choosing.FormatCount(100_000, india));
        Assert.AreEqual("12,34,567", Choosing.FormatCount(1_234_567, india));
        Assert.AreEqual("1,204", Choosing.FormatCount(1204, india));
    }
}
