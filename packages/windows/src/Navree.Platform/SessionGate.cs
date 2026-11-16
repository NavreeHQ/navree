using Navree.Runtime;

namespace Navree.Platform;

/// <summary>
/// For whom the core asks what it asks, and whether that may still be done. What the core asks (a request, a stream, a timer, a value to keep)
/// is marked with the session it was asked in at the moment it is taken off the core, on the core's own thread, in the core's own order; it is
/// carried out later, on the window's thread. Between the two a person may have signed out: what was asked for the person who left is then not
/// started and not kept, however late the window gets round to it, and what the core asks for whoever comes next is done.
/// </summary>
/// <remarks>
/// The core lets go of a person at one of two moments, each in its own order of things: when it is told the client ended the session
/// (<see cref="CoreIsToldTheSessionEnded"/>, called on the core's thread just before it is told), or when it ends the session itself and says so
/// with <see cref="ForgetSessionEffect"/> (seen by <see cref="Asked"/>: what follows that effect in the same update is for whoever comes next).
/// The window's side moves once for each end of a session, as it begins (<see cref="SessionIsEnding"/>).
/// </remarks>
public sealed class SessionGate
{
    private readonly object gate = new();
    private long asking;
    private long live;

    /// <summary>On the core's thread, as an update is taken off the core: the session each of its effects was asked in, in their order.</summary>
    /// <param name="effects">The update's effects.</param>
    /// <returns>A mark for each effect, to give to <see cref="Live"/> when the effect is carried out.</returns>
    public long[] Asked(IReadOnlyList<Effect> effects)
    {
        ArgumentNullException.ThrowIfNull(effects);
        long[] marks = new long[effects.Count];
        lock (gate)
        {
            for (int at = 0; at < effects.Count; at++)
            {
                marks[at] = asking;
                if (effects[at] is ForgetSessionEffect)
                {
                    // The core has ended the session itself: what it asks after this is for whoever comes next.
                    asking++;
                }
            }
        }

        return marks;
    }

    /// <summary>On the core's thread, just before the core is told the client ended the session: what it asks from now on is for whoever comes next.</summary>
    public void CoreIsToldTheSessionEnded()
    {
        lock (gate)
        {
            asking++;
        }
    }

    /// <summary>On the window's thread, as the end of a session begins: from now on only what is asked for whoever comes next is done.</summary>
    public void SessionIsEnding()
    {
        lock (gate)
        {
            live++;
        }
    }

    /// <summary>
    /// Whether an effect starts something or keeps something for the person the core asked it for: a request, a stream, a timer, a call of the
    /// device, a sign-in, an address opened outside, words copied, a value kept. Such an effect is carried out only for the session that is
    /// the one now; every other effect (one that stops something, reads, reports, or forgets the session) is always carried out.
    /// </summary>
    /// <param name="effect">The effect.</param>
    /// <returns>Whether it is one of those.</returns>
    public static bool StartsOrKeeps(Effect effect) =>
        effect is FetchEffect or StreamOpenEffect or TimerEffect or DeviceEffect or SignInEffect or OpenUrlEffect or CopyEffect or CacheWriteEffect { Value: not null };

    /// <summary>Whether an effect is carried out now.</summary>
    /// <param name="effect">The effect.</param>
    /// <param name="mark">Its mark, from <see cref="Asked"/>.</param>
    /// <returns>False for what would start or keep something for a person who has signed out, or who is signing out.</returns>
    public bool MayRun(Effect effect, long mark) => Live(mark) || !StartsOrKeeps(effect);

    /// <summary>Whether what an effect asks may be done now: it was asked in the session that is the one now.</summary>
    /// <param name="mark">The effect's mark, from <see cref="Asked"/>.</param>
    /// <returns>False for what was asked for a person who has since signed out, and for what the core of the person who is leaving asks while it has not let go.</returns>
    public bool Live(long mark)
    {
        lock (gate)
        {
            return mark == live;
        }
    }
}
