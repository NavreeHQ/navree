namespace Navree.Design;

/// <summary>What is done when a dialog the core asked for has stopped being shown.</summary>
/// <param name="TellTheCore">Whether the person's choice is told to the core. A dialog that could not be shown has no choice to tell.</param>
/// <param name="ReportProblem">Whether the failure is reported as a problem of the client's own.</param>
/// <param name="ShowAgain">Whether the dialog is tried again, the question staying open meanwhile.</param>
public sealed record DialogDecision(bool TellTheCore, bool ReportProblem, bool ShowAgain);

/// <summary>
/// A question is answered by a person. A dialog that could not be shown (another is open on the window, the window is not laid out yet) has had no person
/// to answer it, so it must never be reported as closed with nothing chosen: that is an answer of no. It is reported as a problem and left open, and tried
/// again a few times, since what stopped it is usually over in a moment.
/// </summary>
public static class DialogRules
{
    /// <summary>The most times a dialog that could not be shown is tried again before it is left unshown (and the question open).</summary>
    public const int MostTries = 5;

    /// <summary>Decides what to do now that showing a dialog has ended.</summary>
    /// <param name="couldNotBeShown">Whether showing it failed (it threw), as against being closed by a person or taken away by the client.</param>
    /// <param name="takenAwayByUs">Whether the client closed it itself (what is wanted over the window changed).</param>
    /// <param name="timesFailed">How many times in a row showing this dialog has failed, counting this one.</param>
    /// <returns>What to do.</returns>
    public static DialogDecision AfterShow(bool couldNotBeShown, bool takenAwayByUs, int timesFailed)
    {
        if (couldNotBeShown)
        {
            return new DialogDecision(TellTheCore: false, ReportProblem: true, ShowAgain: timesFailed < MostTries);
        }

        return new DialogDecision(TellTheCore: !takenAwayByUs, ReportProblem: false, ShowAgain: false);
    }
}
