namespace Navree.Design;

/// <summary>
/// Whether the app is in front, from what its windows say of themselves: it is in front while any window is shown (not
/// minimised, not hidden) and in the background when none is. The core is told when that changes and not before, so a second
/// window opening, or a window that says the same thing twice, tells it nothing.
/// </summary>
public sealed class ForegroundTracker
{
    private readonly HashSet<object> shown = [];
    private bool inFront;

    /// <summary>Starts with the app in front, as it is when its first window opens.</summary>
    public ForegroundTracker() => inFront = true;

    /// <summary>Whether the app is in front now.</summary>
    public bool InFront => inFront;

    /// <summary>A window says whether it is shown.</summary>
    /// <param name="window">The window, by reference.</param>
    /// <param name="visible">Whether it is shown.</param>
    /// <returns>True when the app came to the front, false when it went to the background, and nothing when it did not move.</returns>
    public bool? Moved(object window, bool visible)
    {
        ArgumentNullException.ThrowIfNull(window);
        _ = visible ? shown.Add(window) : shown.Remove(window);
        bool front = shown.Count > 0;
        if (front == inFront)
        {
            return null;
        }

        inFront = front;
        return front;
    }
}
