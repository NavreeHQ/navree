using System.Collections.Specialized;
using Navree.Controls.Tests.Tables;
using Navree.Design.Tables;

namespace Navree.Controls.Tests.Controls;

[TestClass]
public sealed class RowsSourceTests
{
    private static RowHandle[] Handles(params string[] keys) => keys.Select(k => new RowHandle(k)).ToArray();

    private static RowsSource Source(out List<NotifyCollectionChangedEventArgs> told)
    {
        RowsSource source = new();
        List<NotifyCollectionChangedEventArgs> log = [];
        source.CollectionChanged += (_, e) => log.Add(e);
        told = log;
        source.Reset(Handles("a", "b", "c"));
        log.Clear();
        return source;
    }

    [TestMethod]
    public void A_reset_holds_the_rows_and_tells_the_repeater_once()
    {
        RowsSource source = new();
        List<NotifyCollectionChangedAction> told = [];
        source.CollectionChanged += (_, e) => told.Add(e.Action);
        source.Reset(Handles("a", "b", "c"));
        Assert.AreEqual(3, source.Count);
        Assert.AreEqual(1, source.PlaceOf("b"));
        Same.Items([NotifyCollectionChangedAction.Reset], told);
    }

    [TestMethod]
    public void Rows_added_at_the_end_keep_the_places_of_the_rows_before_them()
    {
        RowsSource source = Source(out List<NotifyCollectionChangedEventArgs> told);
        source.Append(Handles("d", "e"));
        Assert.AreEqual(5, source.Count);
        Assert.AreEqual(3, source.PlaceOf("d"));
        Assert.AreEqual(1, source.PlaceOf("b"));
        Assert.AreEqual(NotifyCollectionChangedAction.Add, told.Single().Action);
        Assert.AreEqual(3, told.Single().NewStartingIndex);
    }

    [TestMethod]
    public void Rows_added_at_the_start_move_the_places_of_every_row()
    {
        RowsSource source = Source(out _);
        source.Prepend(Handles("x", "y"));
        Assert.AreEqual(0, source.PlaceOf("x"));
        Assert.AreEqual(2, source.PlaceOf("a"));
        Assert.AreEqual(4, source.PlaceOf("c"));
        Assert.AreEqual("a", source.KeyFromIndex(2));
        Assert.AreEqual(4, source.IndexFromKey("c"));
    }

    [TestMethod]
    public void Rows_taken_out_leave_no_place_and_move_the_rows_after_them()
    {
        RowsSource source = Source(out List<NotifyCollectionChangedEventArgs> told);
        source.RemoveRange(0, 2);
        Assert.AreEqual(-1, source.PlaceOf("a"));
        Assert.AreEqual(0, source.PlaceOf("c"));
        Assert.AreEqual(NotifyCollectionChangedAction.Remove, told.Single().Action);
        Assert.AreEqual(2, told.Single().OldItems!.Count);
    }

    [TestMethod]
    public void A_row_given_another_key_has_the_new_key_and_not_the_old()
    {
        RowsSource source = Source(out List<NotifyCollectionChangedEventArgs> told);
        source.Replace(1, new RowHandle("z"));
        Assert.AreEqual(-1, source.PlaceOf("b"));
        Assert.AreEqual(1, source.PlaceOf("z"));
        Assert.AreEqual(NotifyCollectionChangedAction.Replace, told.Single().Action);
    }

    [TestMethod]
    public void A_row_drawn_again_keeps_its_key_and_its_place()
    {
        RowsSource source = Source(out List<NotifyCollectionChangedEventArgs> told);
        source.Redraw(2);
        Assert.AreEqual(2, source.PlaceOf("c"));
        Assert.AreEqual(NotifyCollectionChangedAction.Replace, told.Single().Action);
    }

    [TestMethod]
    public void The_source_cannot_be_changed_through_the_list_interface()
    {
        RowsSource source = Source(out _);
        Assert.ThrowsExactly<NotSupportedException>(() => source.Add(new RowHandle("q")));
        Assert.ThrowsExactly<NotSupportedException>(() => source.Clear());
        Assert.ThrowsExactly<NotSupportedException>(() => source[0] = new RowHandle("q"));
        Assert.IsTrue(source.Contains(source[0]));
        Assert.AreEqual(2, source.IndexOf(source[2]));
    }
}

[TestClass]
public sealed class ListDataTableModelTests
{
    private sealed record Item(string Id, string Name);

    private static ListDataTableModel<Item> Model() =>
        new([new DataTableColumn<Item>("name", "Name", i => i.Name, ColumnKind.Title, Sortable: true)], i => i.Id);

    [TestMethod]
    public void The_model_tells_the_table_what_changed_and_where()
    {
        ListDataTableModel<Item> model = Model();
        List<(DataTableChangeKind Kind, int Index, int Count)> told = [];
        model.Changed += (_, e) => told.Add((e.Kind, e.Index, e.Count));
        model.Reset([new("1", "Amrit"), new("2", "Gurpreet")]);
        model.Append([new("3", "Harjot")]);
        model.Prepend([new("0", "Aman"), new("00", "Arjun")]);
        model.Remove("2");
        model.Replace(new Item("3", "Harjot Singh"));
        Same.Items(
            [
                (DataTableChangeKind.Reset, 0, 0),
                (DataTableChangeKind.Insert, 2, 1),
                (DataTableChangeKind.Insert, 0, 2),
                (DataTableChangeKind.Remove, 3, 1),
                (DataTableChangeKind.Replace, 3, 1),
            ],
            told);
        Assert.AreEqual(4, model.RowCount);
        Assert.AreEqual(3, model.IndexOfKey("3"));
        Assert.AreEqual(-1, model.IndexOfKey("2"));
        Assert.AreEqual("Harjot Singh", model.ItemAt(3).Name);
    }

    [TestMethod]
    public void A_record_says_what_the_model_knows_of_the_item_including_a_detail_open_on_arrival()
    {
        ListDataTableModel<Item> model = Model();
        model.HasDetailOf = i => i.Id == "1";
        model.DetailOpenOf = i => i.Id == "1";
        model.HasActionOf = _ => true;
        model.Reset([new("1", "Amrit"), new("2", "Gurpreet")]);
        DataTableRecord first = model.GetRow(0);
        Assert.AreEqual("1", first.Key);
        Assert.IsTrue(first.HasDetail && first.DetailOpen && first.HasAction);
        Assert.IsFalse(model.GetRow(1).DetailOpen);
        Assert.AreEqual(Choosable.Yes, first.Choosable);
    }

    [TestMethod]
    public void A_sort_marks_one_column_and_the_total_is_the_servers()
    {
        ListDataTableModel<Item> model = Model();
        model.SetSort("name", DataTableSortDirection.Descending);
        Assert.AreEqual(DataTableSortDirection.Descending, model.Columns[0].Sort!.Value.Direction);
        Assert.IsNull(model.TotalCount);
        model.TotalCount = 1204;
        Assert.AreEqual(1204, model.TotalCount);
    }

    [TestMethod]
    public void The_text_presenter_gives_a_cells_words_and_the_rows_name()
    {
        ListDataTableModel<Item> model = Model();
        model.Reset([new("1", "Amrit")]);
        TextTablePresenter<Item> presenter = new(model);
        DataTableRecord record = model.GetRow(0);
        Assert.AreEqual("Amrit", presenter.CellText(record, 0));
        Assert.AreEqual("Amrit", presenter.RowName(record));
        Assert.IsNull(presenter.SecondLine(record));
        Assert.AreEqual(0, presenter.ControlCount(record, 0));
    }
}
