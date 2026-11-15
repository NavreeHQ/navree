using System.Collections.Specialized;
using Navree.Controls.Tests.Tables;

namespace Navree.Controls.Tests.Controls;

[TestClass]
public sealed class RowsSourceTests
{
    private static RowHandle[] Handles(params string[] keys) => keys.Select(k => new RowHandle(k)).ToArray();

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
}
