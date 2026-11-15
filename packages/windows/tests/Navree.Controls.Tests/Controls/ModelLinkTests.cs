using System.Runtime.CompilerServices;
using Navree.Design.Tables;

namespace Navree.Controls.Tests.Controls;

[TestClass]
public sealed class ModelLinkTests
{
    // Stands for a table: something that wants to hear of a model's changes.
    private sealed class Listener
    {
        internal int Heard { get; private set; }

        internal void On(object? sender, DataTableModelChangedEventArgs e) => Heard++;
    }

    private static ListDataTableModel<string> Model() => new([new DataTableColumn<string>("k", "K", s => s)], s => s);

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static WeakReference Listen(ModelLink link, ListDataTableModel<string> model, bool letGo)
    {
        Listener listener = new();
        link.Attach(model, listener.On);
        if (letGo)
        {
            link.Detach();
        }

        return new WeakReference(listener);
    }

    private static void Collect()
    {
        for (int i = 0; i < 3; i++)
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
        }
    }

    [TestMethod]
    public void A_model_that_outlives_its_table_does_not_hold_it_once_the_table_lets_go()
    {
        ListDataTableModel<string> model = Model();
        ModelLink link = new();
        WeakReference table = Listen(link, model, letGo: true);
        Collect();
        Assert.IsFalse(table.IsAlive, "nothing the model holds reaches the table");
        GC.KeepAlive(model);
    }

    [TestMethod]
    public void A_model_that_is_listened_to_holds_the_table_which_is_why_the_table_lets_go_when_it_leaves_the_tree()
    {
        ListDataTableModel<string> model = Model();
        ModelLink link = new();
        WeakReference table = Listen(link, model, letGo: false);
        Collect();
        Assert.IsTrue(table.IsAlive);
        GC.KeepAlive(model);
        GC.KeepAlive(link);
    }

    [TestMethod]
    public void A_table_hears_its_model_only_while_it_is_attached_and_once_for_each_change()
    {
        ListDataTableModel<string> model = Model();
        ModelLink link = new();
        Listener listener = new();
        link.Attach(model, listener.On);
        link.Attach(model, listener.On);
        model.Reset(["a"]);
        Assert.AreEqual(1, listener.Heard, "attaching twice does not listen twice");
        link.Detach();
        Assert.IsFalse(link.IsAttached);
        model.Reset(["b"]);
        Assert.AreEqual(1, listener.Heard, "out of the tree nothing reaches the table");
        link.Attach(model, listener.On);
        model.Reset(["c"]);
        Assert.AreEqual(2, listener.Heard, "in the tree again it hears again");
    }

    [TestMethod]
    public void Another_model_replaces_the_one_that_was_listened_to()
    {
        ListDataTableModel<string> first = Model();
        ListDataTableModel<string> second = Model();
        ModelLink link = new();
        Listener listener = new();
        link.Attach(first, listener.On);
        link.Attach(second, listener.On);
        first.Reset(["a"]);
        Assert.AreEqual(0, listener.Heard);
        second.Reset(["a"]);
        Assert.AreEqual(1, listener.Heard);
    }
}
