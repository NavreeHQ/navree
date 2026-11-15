using Navree.Controls.Tests.Tables;

namespace Navree.Controls.Tests.Controls;

[TestClass]
public sealed class ElementPoolTests
{
    [TestMethod]
    public void Elements_given_back_are_kept_up_to_the_most_and_the_rest_are_let_go_and_told_of()
    {
        List<string> letGo = [];
        ElementPool<string> pool = new(2, letGo.Add);
        pool.Give("a");
        pool.Give("b");
        pool.Give("c");
        Assert.AreEqual(2, pool.Count);
        Same.Items(["c"], letGo);
        Assert.AreEqual("b", pool.Take(), "the one given last is used first");
        Assert.AreEqual("a", pool.Take());
        Assert.IsNull(pool.Take());
    }
}

[TestClass]
public sealed class StickyPartsTests
{
    [TestMethod]
    public void A_part_kept_is_counted_once_takes_a_new_role_and_is_forgotten_when_it_is_let_go()
    {
        StickyParts<string> parts = new();
        parts.Add("start", StickyRole.Start);
        parts.Add("end", StickyRole.End);
        parts.Add("start", StickyRole.Header);
        Assert.AreEqual(2, parts.Count);
        Assert.AreEqual(StickyRole.Header, parts.All.First(p => p.Key == "start").Value);
        Assert.IsTrue(parts.Remove("start", out StickyRole role));
        Assert.AreEqual(StickyRole.Header, role);
        Assert.IsFalse(parts.Remove("start", out _), "letting go twice finds nothing");
        Assert.AreEqual(1, parts.Count);
    }
}
