using Navree.Protocol;

namespace Navree.Design.Tests;

/// <summary>The words of a hint and the step line that holds each value across its own place.</summary>
[TestClass]
public sealed class KindsHintTests
{
    [TestMethod]
    public void A_hints_button_is_named_for_what_it_is_about()
    {
        Models.Hint hint = new() { About = "the health score", Text = "How it is made." };
        Assert.AreEqual("About the health score", HintRules.ActionName(hint));
        Assert.AreEqual("About", HintRules.ActionName(hint with { About = "  " }));
    }

    [TestMethod]
    public void A_chart_does_not_say_its_title_again_when_the_screen_has_just_said_it()
    {
        Assert.IsTrue(HintRules.ShowsTitle("Sleep this week", ["Today", "Overview"]));
        Assert.IsFalse(HintRules.ShowsTitle("Sleep this week", ["sleep THIS week"]));
        Assert.IsFalse(HintRules.ShowsTitle(null, []));
        Assert.IsFalse(HintRules.ShowsTitle(string.Empty, []));
    }

    [TestMethod]
    public void A_step_line_holds_each_value_across_its_place_and_changes_halfway_between()
    {
        IReadOnlyList<Pt> corners = Curves.StepsCentred([new Pt(0, 5), new Pt(2, 1), new Pt(4, 3)]);
        CollectionAssert.AreEqual(
            new[] { new Pt(0, 5), new Pt(1, 5), new Pt(1, 1), new Pt(3, 1), new Pt(3, 3), new Pt(4, 3) },
            corners.ToArray());
        Assert.AreEqual(1, Curves.StepsCentred([new Pt(0, 0)]).Count);
        Assert.AreEqual(0, Curves.StepsCentred([]).Count);
    }
}
