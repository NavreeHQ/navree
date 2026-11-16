using Navree.Design.Tables;
using Navree.Protocol;

namespace Navree.Design.Tests;

/// <summary>A table's bar of query fields stands across an answer only while it sends to the same form.</summary>
[TestClass]
public sealed class QueryBarRulesTests
{
    private static Models.Form Query(string action, string? id = null, string typed = "", string status = "open") => new()
    {
        Id = id,
        Action = action,
        Children =
        [
            new Models.Field { Name = "status", Label = "Status", Input = new Models.TextInput { Value = status } },
            new Models.Field { Name = "search", Label = "Search", Input = new Models.TextInput { Kind = Models.TextKind.Search, Value = typed } },
        ],
    };

    [TestMethod]
    public void The_bar_stands_when_only_what_its_fields_hold_changed()
    {
        Assert.IsTrue(QueryBarRules.Stands(Query("/ledger"), Query("/ledger", typed: "kaur", status: "paid")));
        Assert.IsTrue(QueryBarRules.Stands(Query("/ledger?sort=amount", "ledger-f"), Query("/ledger?sort=date", "ledger-f")), "a form named by its id is the same form at any address");
    }

    [TestMethod]
    public void The_bar_is_drawn_again_when_the_form_it_sends_to_is_another()
    {
        Assert.IsFalse(QueryBarRules.Stands(Query("/ledger"), Query("/ledger?sort=amount")), "a form with no id is known by its address");
        Assert.IsFalse(QueryBarRules.Stands(Query("/ledger", "ledger-f"), Query("/ledger", "ledger-g")));
    }

    [TestMethod]
    public void The_search_field_is_drawn_first()
    {
        Assert.AreEqual("search", ((Models.Field)QueryBarRules.Fields(Query("/ledger"))[0]).Name);
    }
}
