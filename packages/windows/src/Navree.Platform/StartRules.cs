namespace Navree.Platform;

/// <summary>
/// What a start gives the core to begin from. The core is given the snapshot the last run kept, so the person finds the app as they left it;
/// but a start that follows a run whose core stopped begins clean, because the snapshot may be what brought the stop, and giving it back would
/// stop the core again at every start. That snapshot is kept aside for that one start, and dropped at the start after it.
/// </summary>
public static class StartRules
{
    /// <summary>The key the snapshot is kept under.</summary>
    public const string SnapshotKey = "snapshot";

    /// <summary>The key that says the core of the last run stopped.</summary>
    public const string StoppedKey = "core-stopped";

    /// <summary>The key a snapshot is kept aside under, for the one start that follows a stop.</summary>
    public const string AsideKey = "snapshot-aside";

    /// <summary>The snapshot the core is given at this start, or null to begin clean.</summary>
    /// <param name="kept">What the last run kept.</param>
    /// <returns>The snapshot, or null: none was kept, or the core of the last run stopped.</returns>
    public static string? SnapshotToRestore(KeptStore kept)
    {
        ArgumentNullException.ThrowIfNull(kept);
        try
        {
            if (kept.Read(StoppedKey) is not null)
            {
                // The start after a stop: clean. The snapshot is kept aside this once, and no longer where a start would take it from.
                kept.Write(AsideKey, kept.Read(SnapshotKey));
                kept.Write(SnapshotKey, null);
                kept.Write(StoppedKey, null);
                return null;
            }

            // A start that follows no stop: what an earlier start kept aside is dropped.
            kept.Write(AsideKey, null);
            return kept.Read(SnapshotKey);
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
        {
            // What is kept could not be changed: the start is clean, which is always safe.
            return null;
        }
    }
}
