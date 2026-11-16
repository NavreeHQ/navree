using Navree.Design.Tables;

namespace Navree.Controls.Tests.Tables;

[TestClass]
public sealed class SetCountsTests
{
    [TestMethod]
    public void The_size_of_the_set_is_the_servers_count_when_it_gave_one_and_unknown_when_rows_are_not_all_held()
    {
        Assert.AreEqual(1204, SetCounts.SizeOfSet(50, 1204, moreAfter: true, moreBefore: false), "1-50 of 1,204 is a set of 1,204");
        Assert.AreEqual(SetCounts.Unknown, SetCounts.SizeOfSet(50, null, true, false));
        Assert.AreEqual(SetCounts.Unknown, SetCounts.SizeOfSet(50, null, false, true));
        Assert.AreEqual(50, SetCounts.SizeOfSet(50, null, false, false), "all held: the count is the rows held");
        Assert.AreEqual(50, SetCounts.SizeOfSet(50, 50, false, false));
        Assert.AreEqual(SetCounts.Unknown, SetCounts.SizeOfSet(50, 10, true, false), "a count below the rows held is not believed");
        Assert.AreEqual(int.MaxValue, SetCounts.SizeOfSet(50, 5_000_000_000, true, false));
    }

    [TestMethod]
    public void The_place_of_a_row_is_known_only_when_no_rows_can_come_before_the_first_held()
    {
        Assert.AreEqual(3, SetCounts.PositionInSet(2, moreBefore: false));
        Assert.AreEqual(SetCounts.Unknown, SetCounts.PositionInSet(2, moreBefore: true));
    }
}

[TestClass]
public sealed class SelectionNoticeTests
{
    [TestMethod]
    public void A_small_change_is_told_row_by_row_and_a_big_one_or_a_whole_set_in_one_event()
    {
        Assert.AreEqual(SelectionNotice.None, SelectionNotices.For(0, wholeSet: false));
        Assert.AreEqual(SelectionNotice.PerRow, SelectionNotices.For(1, false));
        Assert.AreEqual(SelectionNotice.PerRow, SelectionNotices.For(SelectionNotices.MostPerRow, false));
        Assert.AreEqual(SelectionNotice.Invalidated, SelectionNotices.For(SelectionNotices.MostPerRow + 1, false));
        Assert.AreEqual(SelectionNotice.Invalidated, SelectionNotices.For(0, wholeSet: true), "choosing all, clearing and a reply from the host say the choice is to be read again");
    }

    [TestMethod]
    public void A_row_that_is_the_only_one_chosen_is_selected_and_one_that_joins_others_is_added()
    {
        Assert.AreEqual(RowChoiceEvent.Selected, SelectionNotices.EventFor(nowChosen: true, chosenRows: 1));
        Assert.AreEqual(RowChoiceEvent.Added, SelectionNotices.EventFor(true, 3));
        Assert.AreEqual(RowChoiceEvent.Removed, SelectionNotices.EventFor(false, 2));
        Assert.AreEqual(RowChoiceEvent.Removed, SelectionNotices.EventFor(false, 0));
    }
}

[TestClass]
public sealed class AnnouncementKindTests
{
    [TestMethod]
    public void Every_message_the_table_says_has_a_kind_and_an_activity_of_its_own()
    {
        Assert.AreEqual((AnnouncementKind.Completed, "DataTable.Sorted"), Announcements.Describe(Announcements.Sorted("Amount", true).Id));
        Assert.AreEqual((AnnouncementKind.Added, "DataTable.Added"), Announcements.Describe(Announcements.RowsAdded(3, false).Id));
        Assert.AreEqual((AnnouncementKind.Other, "DataTable.Chosen"), Announcements.Describe(Announcements.Chosen(2, null, false).Id));
        Assert.AreEqual((AnnouncementKind.Removed, "DataTable.Removed"), Announcements.Describe("DataTable_RowRemoved"));
        Assert.AreEqual((AnnouncementKind.Completed, "DataTable.Detail"), Announcements.Describe(Announcements.Detail(true).Id));
        Assert.AreEqual((AnnouncementKind.Completed, "DataTable.Detail"), Announcements.Describe(Announcements.Detail(false).Id));
        Assert.AreEqual((AnnouncementKind.Other, "DataTable.Width"), Announcements.Describe(Announcements.ColumnWidth("Amount", 120).Id));
        Assert.AreEqual((AnnouncementKind.Completed, "DataTable.Moved"), Announcements.Describe(Announcements.ColumnMoved("Amount", 1, 4).Id));
        Assert.AreEqual((AnnouncementKind.Completed, "DataTable.Columns"), Announcements.Describe(Announcements.ColumnVisibility("Amount", false).Id));
        Assert.AreEqual((AnnouncementKind.Other, "DataTable.Limit"), Announcements.Describe("DataTable_HeldLimit"));
        Assert.AreEqual((AnnouncementKind.Other, "DataTable"), Announcements.Describe("anything else"));
    }
}
