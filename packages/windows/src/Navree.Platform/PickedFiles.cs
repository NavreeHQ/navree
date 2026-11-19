using System.Collections.Concurrent;
using Navree.Design.Inputs;
using Navree.Runtime;

namespace Navree.Platform;

/// <summary>What a file field says about the files it may take.</summary>
/// <param name="Accept">The kinds it takes, by the web's own list, or null for any.</param>
/// <param name="Multiple">Whether it takes more than one.</param>
/// <param name="KeepLocation">Whether a photo keeps the place it was taken.</param>
/// <param name="MostBytes">The most bytes of all the files of the field together.</param>
public sealed record FieldRule(string? Accept, bool Multiple, bool KeepLocation, long MostBytes);

/// <summary>What came of one file offered to a field: the file now held, or why it was not added.</summary>
/// <param name="File">The file, under the token the core knows it by; null when it was refused.</param>
/// <param name="Refusal">The words said at the field when it was refused.</param>
public sealed record Picked(FileRef? File, string? Refusal);

/// <summary>
/// The files a person chose for forms, held under the tokens the core gives to a request. The core is told a token, a name, a type and a size and never a
/// path; the request's body is written from <see cref="Open"/>, which opens the file at that moment and streams it, so a file is never held in memory
/// to be sent and one that can no longer be read is left out (never sent as an empty file) and reported through <see cref="WentMissing"/>.
/// A photograph whose field does not keep the place it was taken is read once, has the place taken out, and is kept as a copy in the client's own
/// folder: what is sent is exactly what was checked, and it is the copy that is read at send time.
/// The files are held for one session: <see cref="Session"/> moves on at every clear, and a file chosen in a session that has ended by the time it
/// would be held (a photograph still being read when the person signed out) is not held, and its copy is not left on disk.
/// </summary>
public sealed class PickedFiles
{
    private sealed record Entry(string Path, bool Copy);

    private readonly ConcurrentDictionary<string, Entry> held = new(StringComparer.Ordinal);
    private readonly string folder;
    private readonly object gate = new();
    private long session;
    private int adding;
    private TaskCompletionSource? noneAdding;

    /// <summary>Makes the registry; copies a previous run left in its folder are deleted.</summary>
    /// <param name="copiesFolder">Where the copies of photographs are kept.</param>
    public PickedFiles(string copiesFolder)
    {
        ArgumentException.ThrowIfNullOrEmpty(copiesFolder);
        folder = copiesFolder;
        _ = Purge();
    }

    /// <summary>Raised, on whatever thread was reading, with the tokens of files that could not be opened when a request was written.</summary>
    public event Action<IReadOnlyList<string>>? WentMissing;

    /// <summary>The session the files are held for. It moves on at every <see cref="Clear"/> and <see cref="ClearAsync"/>.</summary>
    public long Session
    {
        get
        {
            lock (gate)
            {
                return session;
            }
        }
    }

    /// <summary>Offers one file to a field, chosen in the session of now: checks its kind and size, takes the place out of a photograph, and holds it.</summary>
    /// <param name="path">Where the file is.</param>
    /// <param name="rule">What the field takes.</param>
    /// <param name="heldBytes">The bytes of the files the field keeps when this one is added.</param>
    /// <param name="cancel">Cancelled when the answer is not wanted.</param>
    /// <returns>The file held, or the words it was refused with.</returns>
    public Task<Picked> AddAsync(string path, FieldRule rule, long heldBytes, CancellationToken cancel) => AddAsync(path, rule, heldBytes, Session, cancel);

    /// <summary>
    /// Offers one file to a field: checks its kind and size, takes the place out of a photograph, and holds it, when the session it was chosen in
    /// is still the one now. A file whose session ended meanwhile is not held, and a copy made for it is deleted before this returns.
    /// </summary>
    /// <param name="path">Where the file is.</param>
    /// <param name="rule">What the field takes.</param>
    /// <param name="heldBytes">The bytes of the files the field keeps when this one is added.</param>
    /// <param name="chosenIn">What <see cref="Session"/> said when the person chose the file.</param>
    /// <param name="cancel">Cancelled when the answer is not wanted.</param>
    /// <returns>The file held, or the words it was refused with.</returns>
    public async Task<Picked> AddAsync(string path, FieldRule rule, long heldBytes, long chosenIn, CancellationToken cancel)
    {
        ArgumentNullException.ThrowIfNull(rule);
        lock (gate)
        {
            adding++;
        }

        try
        {
            return await AddOneAsync(path, rule, heldBytes, chosenIn, cancel).ConfigureAwait(false);
        }
        finally
        {
            lock (gate)
            {
                if (--adding == 0)
                {
                    noneAdding?.TrySetResult();
                    noneAdding = null;
                }
            }
        }
    }

    private async Task<Picked> AddOneAsync(string path, FieldRule rule, long heldBytes, long chosenIn, CancellationToken cancel)
    {
        if (!FileRules.IsOnThisComputer(path))
        {
            // A thing with no path here (dropped from a phone, a link from a browser) is refused with words, never an error.
            return new Picked(null, FileRules.NotHere(path is null ? null : FileRules.DisplayName(path)));
        }

        string name = FileRules.DisplayName(path);
        string? written = null;
        bool holds = false;
        try
        {
            FileInfo info = new(path);
            if (!info.Exists)
            {
                return new Picked(null, name + " could not be read, so it was not added.");
            }

            // What the file is comes from its first bytes, whatever its name says: a photo or video with no extension is still known by its bytes.
            byte[] head = await HeadAsync(path, cancel).ConfigureAwait(false);
            string type = FileRules.MediaTypeOf(name);
            if (type == FileRules.Unknown
                && (PlaceKinds.MediaTypeOf(PlaceKinds.KindOf(head)) ?? PlaceKinds.VideoMediaTypeOf(PlaceKinds.VideoOf(head), head)) is { } read)
            {
                type = read;
            }

            if (FileRules.Refuse(name, type, info.Length, heldBytes, rule.Accept, rule.MostBytes) is { } refusal)
            {
                return new Picked(null, refusal.Words);
            }

            string token = Guid.NewGuid().ToString("N");
            string at = path;
            bool copy = false;
            long size = info.Length;
            if (!rule.KeepLocation && PlaceRules.MustLook(head, name, type))
            {
                byte[] bytes = await File.ReadAllBytesAsync(path, cancel).ConfigureAwait(false);
                PlaceResult result = PlaceRules.TakeOut(bytes, name);
                if (result.Outcome == PlaceOutcome.Refused || result.Bytes is not { } clean)
                {
                    return new Picked(null, name + ": " + result.Why);
                }

                // The session may have ended while the picture was read: nothing of it is written then.
                if (Session != chosenIn)
                {
                    return new Picked(null, SessionEnded(name));
                }

                _ = Directory.CreateDirectory(folder);
                at = Path.Combine(folder, token);
                written = at;
                await File.WriteAllBytesAsync(at, clean, cancel).ConfigureAwait(false);
                copy = true;
                size = clean.Length;
            }

            // Held only for the session it was chosen in: the clear that ends a session moves the number under this same lock.
            lock (gate)
            {
                if (session == chosenIn)
                {
                    held[token] = new Entry(at, copy);
                    holds = true;
                }
            }

            return holds ? new Picked(new FileRef(token, name, type, size), null) : new Picked(null, SessionEnded(name));
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException or NotSupportedException or ArgumentException)
        {
            return new Picked(null, name + " could not be read, so it was not added.");
        }
        finally
        {
            // A copy that is not held is not left: one whose session ended while it was made, or whose writing was cut short.
            if (!holds && written is not null)
            {
                DeleteQuietly(written);
            }
        }
    }

    private static string SessionEnded(string name) => name + " was not added, because the session it was chosen in has ended.";

    // The first bytes of a file, as many as say what kind of file it is; reading them also shows that the file can be read at all.
    private static async Task<byte[]> HeadAsync(string path, CancellationToken cancel)
    {
        await using FileStream stream = new(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete, 1, FileOptions.Asynchronous);
        byte[] head = new byte[PlaceKinds.Head];
        int read = 0;
        while (read < head.Length)
        {
            int got = await stream.ReadAsync(head.AsMemory(read), cancel).ConfigureAwait(false);
            if (got == 0)
            {
                break;
            }

            read += got;
        }

        return head[..read];
    }

    /// <summary>Opens the file held under a token, to be read from its start; null when it is not held or can no longer be read.</summary>
    /// <param name="token">The token the core was given.</param>
    /// <returns>A stream the caller disposes, or null.</returns>
    public Stream? Open(string token)
    {
        if (!held.TryGetValue(token, out Entry? entry))
        {
            return null;
        }

        try
        {
            return new FileStream(entry.Path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete, 64 * 1024, FileOptions.Asynchronous | FileOptions.SequentialScan);
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException or NotSupportedException)
        {
            WentMissing?.Invoke([token]);
            return null;
        }
    }

    /// <summary>Whether the file held under a token is still there to be read.</summary>
    /// <param name="token">The token.</param>
    /// <returns>True when it is.</returns>
    public bool Available(string token) => held.TryGetValue(token, out Entry? entry) && File.Exists(entry.Path);

    /// <summary>Lets go of a file the field no longer holds; a copy of a photograph is deleted.</summary>
    /// <param name="token">The token.</param>
    public void Release(string token)
    {
        if (held.TryRemove(token, out Entry? entry) && entry.Copy)
        {
            DeleteQuietly(entry.Path);
        }
    }

    /// <summary>
    /// Lets go of every file and deletes every copy: the end of a session. A file still being added is not held when it is done, and deletes
    /// its own copy then; <see cref="ClearAsync"/> also waits for that.
    /// </summary>
    public void Clear() => _ = End(out _);

    /// <summary>
    /// Lets go of every file and deletes every copy, and completes when no file is still being added: a photograph that was being read when
    /// the session ended has by then been refused and its copy deleted, so nothing of the session is on disk. When a copy is still there
    /// after that (another program holds it), this fails: the sign-out it is a step of is then not finished, and the copy goes at the next
    /// start, when the registry is made.
    /// </summary>
    /// <returns>When it is so.</returns>
    /// <exception cref="IOException">A copy of the session that ended could not be deleted.</exception>
    public async Task ClearAsync()
    {
        await End(out IReadOnlyList<string> left).ConfigureAwait(false);

        // Only the copies that were there when the session ended: what whoever comes next has chosen since is theirs.
        int still = 0;
        foreach (string file in left)
        {
            DeleteQuietly(file);
            if (File.Exists(file))
            {
                still++;
            }
        }

        if (still > 0)
        {
            throw new IOException(still + " copies of chosen files could not be deleted");
        }
    }

    private Task End(out IReadOnlyList<string> left)
    {
        Task none;
        lock (gate)
        {
            session++;
            held.Clear();
            none = adding == 0
                ? Task.CompletedTask
                : (noneAdding ??= new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously)).Task;
        }

        left = Purge();
        return none;
    }

    // Deletes every copy in the folder; the ones that are still there afterwards.
    private List<string> Purge()
    {
        List<string> left = [];
        try
        {
            if (Directory.Exists(folder))
            {
                foreach (string file in Directory.GetFiles(folder))
                {
                    DeleteQuietly(file);
                    if (File.Exists(file))
                    {
                        left.Add(file);
                    }
                }
            }
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
        {
            // A folder that cannot be listed holds nothing this client can delete; the next purge tries again.
        }

        return left;
    }

    private static void DeleteQuietly(string path)
    {
        try
        {
            File.Delete(path);
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
        {
            // A copy that is still being read is deleted by the next purge.
        }
    }
}
