namespace Navree.Platform;

/// <summary>One thing the wipe takes away, and how.</summary>
/// <param name="Name">What it is, for the report.</param>
/// <param name="Run">Takes it away.</param>
/// <param name="On">
/// Runs it where its state lives (the window's thread, for state only that thread may touch) and completes when it has run there; when left out
/// the step runs on the thread pool.
/// </param>
public sealed record WipeStep(string Name, Func<Task> Run, Func<Func<Task>, Task>? On = null);

/// <summary>What came of a wipe, step by step, in the order they ran.</summary>
/// <param name="Done">The steps that finished.</param>
/// <param name="Failed">The steps that failed, or ran out of time, and why.</param>
public sealed record WipeReport(IReadOnlyList<string> Done, IReadOnlyList<KeyValuePair<string, string>> Failed)
{
    /// <summary>Whether every step finished.</summary>
    public bool Complete => Failed.Count == 0;
}

/// <summary>
/// The end of a session, when the person signs out: everything of theirs goes, in an order that never leaves the
/// next person a door open (what could rebuild a session goes first), and each step has a limit, so one that hangs
/// does not keep the rest from running. A step that fails is reported, and the next still runs. A step runs where its state lives
/// (<see cref="WipeStep.On"/>), never on a thread that state is not safe on.
/// </summary>
public static class Wipe
{
    /// <summary>How long one step may take.</summary>
    public static readonly TimeSpan StepLimit = TimeSpan.FromSeconds(10);

    /// <summary>Runs the steps in order.</summary>
    /// <param name="steps">The steps, first first.</param>
    /// <param name="limit">How long one may take; <see cref="StepLimit"/> when left out.</param>
    /// <returns>What came of each.</returns>
    public static async Task<WipeReport> RunAsync(IEnumerable<WipeStep> steps, TimeSpan? limit = null)
    {
        ArgumentNullException.ThrowIfNull(steps);
        TimeSpan each = limit ?? StepLimit;
        List<string> done = [];
        List<KeyValuePair<string, string>> failed = [];
        foreach (WipeStep step in steps)
        {
            try
            {
                Task running = step.On is null ? Task.Run(step.Run) : Task.Run(() => step.On(step.Run));
                if (await Task.WhenAny(running, Task.Delay(each)).ConfigureAwait(false) == running)
                {
                    await running.ConfigureAwait(false);
                    done.Add(step.Name);
                }
                else
                {
                    failed.Add(new KeyValuePair<string, string>(step.Name, "it took longer than " + each.TotalSeconds + " seconds"));
                }
            }
            catch (Exception problem)
            {
                failed.Add(new KeyValuePair<string, string>(step.Name, problem.GetType().Name));
            }
        }

        return new WipeReport(done, failed);
    }
}
