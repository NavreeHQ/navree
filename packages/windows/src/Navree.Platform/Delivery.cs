using Navree.Runtime;

namespace Navree.Platform;

/// <summary>
/// How what the core says is handed on and what it asks is carried out, so that one part that fails never costs the rest:
/// a window that cannot show a state does not lose the effects of the same update, an effect that fails does not stop the
/// ones after it, and a request whose fetcher threw is answered to the core as one that failed.
/// </summary>
public static class Delivery
{
    /// <summary>Shows the state, then runs each effect in order; whatever fails is reported by its kind alone and the rest go on.</summary>
    /// <param name="update">What the core said.</param>
    /// <param name="show">Shows the state.</param>
    /// <param name="run">Carries out an effect.</param>
    /// <param name="problem">Told what failed.</param>
    public static void Deliver(Update update, Action<ViewState> show, Action<Effect> run, Action<string> problem)
    {
        ArgumentNullException.ThrowIfNull(update);
        ArgumentNullException.ThrowIfNull(show);
        ArgumentNullException.ThrowIfNull(run);
        ArgumentNullException.ThrowIfNull(problem);
        try
        {
            show(update.State);
        }
        catch (Exception failure)
        {
            problem("a window could not show the state: " + failure.GetType().Name);
        }

        foreach (Effect effect in update.Effects)
        {
            try
            {
                run(effect);
            }
            catch (Exception failure)
            {
                problem("an effect failed: " + effect.GetType().Name + ": " + failure.GetType().Name);
            }
        }
    }

    /// <summary>Asks the fetcher; a fetcher that throws is a request that failed, which the core is told. A request that was cancelled throws as such.</summary>
    /// <param name="fetcher">What answers requests.</param>
    /// <param name="effect">The request.</param>
    /// <param name="cancel">Cancelled when the answer is no longer wanted.</param>
    /// <returns>What came of it.</returns>
    public static async Task<FetchResult> FetchAsync(IFetcher fetcher, FetchEffect effect, CancellationToken cancel)
    {
        ArgumentNullException.ThrowIfNull(fetcher);
        try
        {
            return await fetcher.FetchAsync(effect, cancel).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (cancel.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception)
        {
            return new FetchResult(0, null, null, [], string.Empty, "other");
        }
    }
}
