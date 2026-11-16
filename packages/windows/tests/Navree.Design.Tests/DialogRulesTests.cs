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
    public void A_question_whose_window_was_closed_is_closed_with_nothing_chosen_and_is_not_tried_again()
    {
        // The person closed the window of one screen while its question was up, and showing it ended in an error.
        DialogDecision closed = DialogRules.AfterShow(couldNotBeShown: true, takenAwayByUs: false, timesFailed: 1, windowClosed: true);
        Assert.IsTrue(closed.TellTheCore, "the question stays open with no window to answer it in");
        Assert.IsFalse(closed.ShowAgain, "a dialog is tried again on a window that is gone");
        Assert.IsTrue(closed.ReportProblem);

        // The same failure on a window that is still there is no answer, and is tried again.
        DialogDecision open = DialogRules.AfterShow(couldNotBeShown: true, takenAwayByUs: false, timesFailed: 1, windowClosed: false);
        Assert.IsFalse(open.TellTheCore);
        Assert.IsTrue(open.ShowAgain);

        // A dialog the person answered before the window closed is told as it was answered.
        DialogDecision answered = DialogRules.AfterShow(couldNotBeShown: false, takenAwayByUs: false, timesFailed: 0, windowClosed: true);
        Assert.IsTrue(answered.TellTheCore);
        Assert.IsFalse(answered.ReportProblem);
    }

    [TestMethod]
    public void A_failure_that_came_while_the_client_was_taking_the_dialog_away_is_still_a_failure_and_still_not_an_answer()
    {
        DialogDecision decision = DialogRules.AfterShow(true, true, 1);
        Assert.IsFalse(decision.TellTheCore);
        Assert.IsTrue(decision.ReportProblem);
    }
}
