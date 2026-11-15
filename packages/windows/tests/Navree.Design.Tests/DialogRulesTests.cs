namespace Navree.Design.Tests;

/// <summary>A question a person did not answer is never told to the core as answered.</summary>
[TestClass]
public sealed class DialogRulesTests
{
    [TestMethod]
    public void A_dialog_that_could_not_be_shown_is_not_told_to_the_core_is_reported_and_is_tried_again()
    {
        DialogDecision first = DialogRules.AfterShow(couldNotBeShown: true, takenAwayByUs: false, timesFailed: 1);
        Assert.IsFalse(first.TellTheCore, "no person chose anything, and a closed question is an answer of no");
        Assert.IsTrue(first.ReportProblem);
        Assert.IsTrue(first.ShowAgain);
    }

    [TestMethod]
    public void A_dialog_that_cannot_be_shown_however_often_is_left_unshown_with_the_question_open()
    {
        DialogDecision last = DialogRules.AfterShow(true, false, DialogRules.MostTries);
        Assert.IsFalse(last.ShowAgain);
        Assert.IsFalse(last.TellTheCore);
        Assert.IsTrue(last.ReportProblem);
        Assert.IsTrue(DialogRules.AfterShow(true, false, DialogRules.MostTries - 1).ShowAgain);
    }

    [TestMethod]
    public void A_dialog_a_person_closed_is_told_and_one_the_client_took_away_is_not()
    {
        DialogDecision person = DialogRules.AfterShow(false, false, 0);
        Assert.IsTrue(person.TellTheCore);
        Assert.IsFalse(person.ReportProblem);
        Assert.IsFalse(person.ShowAgain);

        DialogDecision taken = DialogRules.AfterShow(false, true, 0);
        Assert.IsFalse(taken.TellTheCore);
        Assert.IsFalse(taken.ReportProblem);
    }

    [TestMethod]
    public void A_failure_that_came_while_the_client_was_taking_the_dialog_away_is_still_a_failure_and_still_not_an_answer()
    {
        DialogDecision decision = DialogRules.AfterShow(true, true, 1);
        Assert.IsFalse(decision.TellTheCore);
        Assert.IsTrue(decision.ReportProblem);
    }
}
