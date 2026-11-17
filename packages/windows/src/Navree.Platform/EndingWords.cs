using Navree.Runtime;

namespace Navree.Platform;

/// <summary>
/// The core's two words at the end of a session it ends itself, as the window carries them out: that a session is ending
/// (<see cref="SessionEndingEffect"/>), and that it is forgotten (<see cref="ForgetSessionEffect"/>). The core says the second once for
/// each time it said the first, whatever ended its wait, and says it alone when it had nothing to wait for. This says, for each word, whether
/// a sign-out begins at it. It is used from one thread, the window's.
/// </summary>
/// <remarks>
/// A sign-out begins at the first word, when the session it speaks of is still the one now, and then none begins at the second: that one is
/// told the core has let go. When the session has ended on the window's side already (the person signed out in the client, and that
/// sign-out began before the word reached the window and before the core was told), nothing begins at the first word either; it is only
/// counted (<see cref="SessionGate.SessionIsEnding"/>), so that the second word, which the core says when it is told of the client's
/// sign-out, finds both sides of the gate level and begins nothing. A second word with no first before it begins a sign-out itself.
/// </remarks>
public sealed class EndingWords
{
    private readonly SessionGate gate;
    private TaskCompletionSource? letsGo;

    /// <summary>Makes the pair for one gate.</summary>
    /// <param name="gate">The gate the host marks what the core asks with.</param>
    public EndingWords(SessionGate gate)
    {
        ArgumentNullException.ThrowIfNull(gate);
        this.gate = gate;
    }

    /// <summary>Whether the core has said a session is ending and not yet that it is forgotten.</summary>
    public bool Waiting => letsGo is not null;

    /// <summary>The core said a session is ending.</summary>
    /// <param name="mark">The word's mark, from <see cref="SessionGate.Asked(IReadOnlyList{Effect})"/>.</param>
    /// <returns>
    /// When a sign-out is to begin now: what completes when the core lets go, for that sign-out to wait on. Null when none begins: the
    /// session the word speaks of has ended on the window's side already and the word was only counted, or a word before it is still open.
    /// </returns>
    public Task? Ending(long mark)
    {
        if (letsGo is not null)
        {
            return null;
        }

        letsGo = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        if (gate.Live(mark))
        {
            return letsGo.Task;
        }

        gate.SessionIsEnding();
        return null;
    }

    /// <summary>The core said the session is forgotten.</summary>
    /// <returns>
    /// Whether a sign-out is to begin here: true when the core did not say before that the session was ending. False when it did: the
    /// sign-out that began at that word, if one did, is told the core has let go, and none begins.
    /// </returns>
    public bool Forgotten()
    {
        if (letsGo is not { } begun)
        {
            return true;
        }

        letsGo = null;
        _ = begun.TrySetResult();
        return false;
    }
}
