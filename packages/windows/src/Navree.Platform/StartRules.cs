namespace Navree.Platform;

/// <summary>
/// What a start gives the core to begin from. The core is given the snapshot the last run kept, so the person finds the app as they left it;
/// but a start begins clean when the snapshot may be what ended the run before it, since giving it back would end every start the same way.
/// That is so after a run whose core said it had stopped, and after a start that never got to the end of putting a snapshot back (the app
/// ended in the middle of it): a note is written before the snapshot is given to the core and taken away when the core has answered and the
/// window has carried the answer out, so a start that finds the note knows the one before it did not get that far. The snapshot is then kept
/// aside for that one start, and dropped at the start after it.
/// </summary>
public static class StartRules
{
    /// <summary>The key the snapshot is kept under.</summary>
    public const string SnapshotKey = "snapshot";

    /// <summary>The key that says the core of the last run stopped.</summary>
    public const string StoppedKey = "core-stopped";

    /// <summary>The key that says a start is putting the snapshot back, from before it begins until it is over.</summary>
    public const string RestoringKey = "restoring";

    /// <summary>The key a snapshot is kept aside under, for the one start that begins clean because of it.</summary>
    public const string AsideKey = "snapshot-aside";

    /// <summary>
    /// The snapshot the core is given at this start, or null to begin clean. When a snapshot is given, the note that it is being put back is
    /// written first; the caller takes it away with <see cref="Restored"/> once the restore is over.
    /// </summary>
    /// <param name="kept">What the last run kept.</param>
    /// <returns>The snapshot, or null: none was kept, the core of the last run stopped, or the last start ended while it put a snapshot back.</returns>
    public static string? SnapshotToRestore(KeptStore kept)
    {
        ArgumentNullException.ThrowIfNull(kept);
        try
        {
            if (kept.Read(StoppedKey) is not null || kept.Read(RestoringKey) is not null)
            {
                // Clean. The snapshot is kept aside this once, and is no longer where a start would take it from.
                kept.Write(AsideKey, kept.Read(SnapshotKey));
                kept.Write(SnapshotKey, null);
                kept.Write(StoppedKey, null);
                kept.Write(RestoringKey, null);
                return null;
            }

            // A start that follows no such run: what an earlier start kept aside is dropped.
            kept.Write(AsideKey, null);
            if (kept.Read(SnapshotKey) is not string snapshot)
            {
                return null;
            }

            // The note is on disk before the core is given the snapshot: if this start ends in the middle, the next one finds it.
            kept.Write(RestoringKey, "1");
            return snapshot;
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
        {
            // What is kept could not be changed: the start is clean, which is always safe.
            return null;
        }
    }

    /// <summary>The snapshot has been put back, or was not given to the core after all: the note that it is being put back is taken away.</summary>
    /// <param name="kept">What is kept.</param>
    public static void Restored(KeptStore kept)
    {
        ArgumentNullException.ThrowIfNull(kept);
        try
        {
            kept.Write(RestoringKey, null);
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
        {
            // The note stays: the next start begins clean, which is always safe.
        }
    }
}
