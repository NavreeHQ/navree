using Navree.Design.Pages;

namespace Navree.Design.Tests;

/// <summary>A sheet that holds nothing yet is waiting for its body and says so; one that holds something is not.</summary>
[TestClass]
public sealed class PaneRulesTests
{
    [TestMethod]
    public void A_sheet_with_no_parts_and_no_actions_is_waiting_for_its_body()
    {
        Assert.IsTrue(PaneRules.WaitsForItsBody(0, 0));
    }

    [TestMethod]
    [DataRow(1, 0)]
    [DataRow(0, 1)]
    [DataRow(3, 2)]
    public void A_sheet_that_holds_a_part_or_an_action_is_not_waiting(int children, int actions)
    {
        Assert.IsFalse(PaneRules.WaitsForItsBody(children, actions));
    }
}
