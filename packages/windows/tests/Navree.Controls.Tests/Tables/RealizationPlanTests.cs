using Navree.Design.Tables;

namespace Navree.Controls.Tests.Tables;

[TestClass]
public sealed class RealizationPlanTests
{
    private static readonly Geometry Plain = new(40, []);

    [TestMethod]
    public void The_rows_the_viewport_touches_are_made_with_a_margin()
    {
        IReadOnlyList<int> plan = RealizationPlan.Indices(Plain, 400, 200, 100, []);
        Same.Items(Enumerable.Range(9, 7), plan);
    }

    [TestMethod]
    public void A_pinned_row_far_from_the_view_is_made_too_and_one_in_view_is_made_once()
    {
        IReadOnlyList<int> plan = RealizationPlan.Indices(Plain, 400, 200, 100, [11, 80, 5000, -1, 0]);
        Same.Items([.. Enumerable.Range(9, 7), 80, 0], plan);
    }

    [TestMethod]
    public void Pins_of_three_owners_are_all_made()
    {
        IReadOnlyList<int> plan = RealizationPlan.Indices(Plain, 0, 120, 1000, [500, 900, 20]);
        Assert.IsTrue(plan.Contains(500) && plan.Contains(900) && plan.Contains(20));
        Assert.AreEqual(plan.Count, plan.Distinct().Count());
    }

    [TestMethod]
    public void A_viewport_of_no_end_makes_no_more_than_the_most_a_pass_may_and_the_pins()
    {
        IReadOnlyList<int> plan = RealizationPlan.Indices(Plain, 0, double.PositiveInfinity, 200_000, [150_000]);
        Assert.AreEqual(Geometry.MostRealized + 1, plan.Count);
        Assert.AreEqual(150_000, plan[^1]);
    }

    [TestMethod]
    public void No_rows_make_nothing_even_when_a_row_is_pinned() =>
        Assert.AreEqual(0, RealizationPlan.Indices(Plain, 0, 800, 0, [0]).Count);
}
