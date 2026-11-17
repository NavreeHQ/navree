using System.Globalization;

namespace Navree.Platform;

/// <summary>The folders under the web view's own folder, as the rule of <see cref="WebProfiles"/> needs them: named, made and deleted.</summary>
public interface IWebFolders
{
    /// <summary>The names of everything directly in a folder.</summary>
    /// <param name="root">The folder.</param>
    /// <returns>The names, or none when the folder is not there or cannot be read.</returns>
    IReadOnlyList<string> Names(string root);

    /// <summary>Makes a folder.</summary>
    /// <param name="path">Where.</param>
    /// <returns>Whether it is there now.</returns>
    bool Make(string path);

    /// <summary>Deletes a folder (or a file) and everything in it.</summary>
    /// <param name="path">What.</param>
    /// <param name="limit">How long to keep trying while something still holds a file in it.</param>
    /// <returns>Whether it is gone.</returns>
    bool Delete(string path, TimeSpan limit);
}

/// <summary>The folders on the disk.</summary>
public sealed class DiskWebFolders : IWebFolders
{
    /// <inheritdoc />
    public IReadOnlyList<string> Names(string root)
    {
        try
        {
            return Directory.Exists(root) ? [.. Directory.GetFileSystemEntries(root).Select(entry => Path.GetFileName(entry))] : [];
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
        {
            return [];
        }
    }

    /// <inheritdoc />
    public bool Make(string path)
    {
        try
        {
            _ = Directory.CreateDirectory(path);
            return Directory.Exists(path);
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException or NotSupportedException or ArgumentException)
        {
            return false;
        }
    }

    /// <inheritdoc />
    public bool Delete(string path, TimeSpan limit)
    {
        try
        {
            if (File.Exists(path))
            {
                File.Delete(path);
                return !File.Exists(path);
            }
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
        {
            return false;
        }

        return WebData.DeleteAsync(path, limit).GetAwaiter().GetResult();
    }
}

/// <summary>
/// Where the web view keeps what web pages leave (cookies, storage, cache, history): a folder of its own for each session, named by a number
/// that only goes up, under the app's web folder. A session that ends never has its folder used again: whoever comes next gets a new folder
/// that the profile of the person who left was never in, whether or not the old folder could be emptied or deleted. Old folders are deleted
/// when they can be, at the end of a session and at every start, until they are gone.
/// </summary>
/// <remarks>
/// <para>
/// The folder moves on first. The end of a session begins with <see cref="MoveOn"/>, before anything is emptied and before any wait: from that
/// line on <see cref="Folder"/> is the new folder or none, and never again the folder it was. Emptying and deleting the old folder is what
/// remains (<see cref="Finish"/>); it may take as long as it takes, be given up, or fail, and no web page of whoever comes next is opened in
/// the old folder meanwhile.
/// </para>
/// <para>
/// When what the web view kept for the session that ended could be neither cleared nor deleted, no web page is shown for the rest of that run
/// (<see cref="Folder"/> is null): the browser behind the old folder may still be running in this process's name. The next start names a new
/// folder and goes on. The number of the session now is the highest-numbered folder on disk, so the new folder is made on disk before the
/// session that ended counts as over.
/// </para>
/// </remarks>
public sealed class WebProfiles
{
    /// <summary>How long a start keeps trying to delete the folder of a session whose sign-out it is finishing.</summary>
    public static readonly TimeSpan AtStart = TimeSpan.FromSeconds(3);

    /// <summary>How long a start, or the end of a session, tries to delete an older folder that is only left over.</summary>
    public static readonly TimeSpan LeftOver = TimeSpan.FromMilliseconds(300);

    private readonly string root;
    private readonly IWebFolders folders;
    private readonly Action<string>? say;
    private readonly object gate = new();
    private readonly List<string> left = [];
    private long current = 1;
    private bool refused;

    /// <summary>Makes the rule for one app's web folder. Nothing is read until <see cref="Open"/>.</summary>
    /// <param name="root">The app's web folder, which holds the sessions' folders.</param>
    /// <param name="folders">The folders; the disk when left out.</param>
    /// <param name="say">Told, in words, what could not be deleted or made.</param>
    public WebProfiles(string root, IWebFolders? folders = null, Action<string>? say = null)
    {
        ArgumentException.ThrowIfNullOrEmpty(root);
        this.root = root;
        this.folders = folders ?? new DiskWebFolders();
        this.say = say;
    }

    /// <summary>The folder the web views of the session now keep their data in; null when no web page may be shown in this run.</summary>
    public string? Folder
    {
        get
        {
            lock (gate)
            {
                return refused ? null : PathOf(current);
            }
        }
    }

    /// <summary>
    /// Whether the folder of the session now holds anything: a web view has kept something there, in this run or in an earlier one. A start
    /// asks this when it finds no mark of whom what is kept is kept for.
    /// </summary>
    public bool HoldsData
    {
        get
        {
            lock (gate)
            {
                return folders.Names(PathOf(current)).Count > 0;
            }
        }
    }

    /// <summary>The older folders that are still on disk: never used again, and tried again at the end of a session and at the next start.</summary>
    public IReadOnlyList<string> Left
    {
        get
        {
            lock (gate)
            {
                return [.. left];
            }
        }
    }

    /// <summary>
    /// At the start, before any web view: names the folder of the session now and deletes every other thing in the web folder (the folders of
    /// sessions that ended, and what an earlier build kept directly in it). When the last run left a sign-out unfinished, the folder it used is
    /// not used either: a new one is made first. A folder that cannot be deleted is left, said, and never used.
    /// </summary>
    /// <param name="lastSessionEnded">Whether the last run's session ended (its sign-out is being finished by this start).</param>
    /// <returns>
    /// Whether nothing the web view kept for a session that ended can be reached by this run: false only when a new folder was needed and could
    /// not be made, and then no web page is shown in this run.
    /// </returns>
    /// <remarks>
    /// It is <see cref="Choose"/> and then <see cref="Tidy"/>. Only the first has to be done before a web view starts, and it deletes nothing;
    /// the second may take as long as deleting a profile takes, so a caller on a window's thread does the two apart and the second elsewhere.
    /// </remarks>
    public bool Open(bool lastSessionEnded)
    {
        if (!Choose(lastSessionEnded))
        {
            return false;
        }

        Tidy(lastSessionEnded ? AtStart : LeftOver);
        return true;
    }

    /// <summary>
    /// At the start, before any web view: names the folder of the session now, and makes a new one first when the last run left a sign-out
    /// unfinished. Nothing is deleted and nothing waits.
    /// </summary>
    /// <param name="lastSessionEnded">Whether the last run's session ended (its sign-out is being finished by this start).</param>
    /// <returns>False only when a new folder was needed and could not be made; no web page is shown in this run then.</returns>
    public bool Choose(bool lastSessionEnded)
    {
        lock (gate)
        {
            IReadOnlyList<string> names = folders.Names(root);
            long highest = 0;
            foreach (string name in names)
            {
                if (NumberOf(name) is long number && number > highest)
                {
                    highest = number;
                }
            }

            current = lastSessionEnded ? highest + 1 : Math.Max(highest, 1);
            refused = false;
            left.Clear();
            if (lastSessionEnded && !folders.Make(PathOf(current)))
            {
                // Without the new folder on disk a later start would take the old one for the session's: nothing is shown instead.
                refused = true;
                Say("a new folder for the web view could not be made: no web page is shown until the app is started again");
                return false;
            }

            return true;
        }
    }

    /// <summary>
    /// Deletes every thing in the web folder that is not the folder of the session now: the folders of sessions that ended, and what an earlier
    /// build kept directly in it. A folder that cannot be deleted is left, said, never used, and tried again at the end of a session and at
    /// the next start. It may run on any thread while web views run: nothing here keeps a web page waiting for its folder.
    /// </summary>
    /// <param name="limit">How long to keep trying to delete one folder that something still holds.</param>
    public void Tidy(TimeSpan limit)
    {
        IReadOnlyList<string> names;
        string mine;
        lock (gate)
        {
            names = folders.Names(root);
            mine = Name(current);
        }

        List<string> still = DeleteOthers(names, mine, limit);
        lock (gate)
        {
            left.Clear();
            left.AddRange(still);
        }
    }

    /// <summary>
    /// The end of a session begins: its folder is never used again from this line on. The session's number goes up and the folder of whoever
    /// comes next is made and in force at once, before anything of the old one is emptied; when it cannot be made, no web page is shown for
    /// the rest of this run. Nothing here waits, nothing here throws, and nothing here can leave the old folder in use.
    /// </summary>
    /// <returns>The folder of the session that ended, to be emptied and deleted (<see cref="Finish"/>).</returns>
    public string MoveOn()
    {
        lock (gate)
        {
            long ended = current;
            current = ended + 1;
            bool made = false;
            try
            {
                made = folders.Make(PathOf(current));
            }
            catch (Exception)
            {
                // Whatever kept it from being made, it was not made. The end of the session goes on.
            }

            if (!made)
            {
                // Without the new folder on disk a later start would take the old one for the session's: nothing is shown instead.
                refused = true;
                Say("a new folder for the web view could not be made: no web page is shown until the app is started again");
            }

            return PathOf(ended);
        }
    }

    /// <summary>
    /// The emptying of the folder a session left (<see cref="MoveOn"/>) has been tried, or given up. The older folders are deleted if they
    /// can be. When what was kept was not taken away (<paramref name="taken"/> is false: neither cleared nor deleted) and its folder is still
    /// there, no web page is shown for the rest of this run.
    /// </summary>
    /// <param name="ended">The folder of the session that ended, as <see cref="MoveOn"/> gave it.</param>
    /// <param name="taken">Whether what the web view kept there was cleared through a web view of that session, or the folder deleted.</param>
    /// <returns>
    /// Whether the session's end is finished as far as the web view goes: what it kept is gone or emptied, the folder of whoever comes next is
    /// on disk, and web pages may still be shown in this run. False means the sign-out is not finished.
    /// </returns>
    public bool Finish(string ended, bool taken)
    {
        ArgumentException.ThrowIfNullOrEmpty(ended);
        IReadOnlyList<string> names;
        string mine;
        lock (gate)
        {
            names = folders.Names(root);
            mine = Name(current);
        }

        // Not while the folder's name is held: deleting a profile takes as long as it takes, and a web page of whoever comes next that asks
        // for its folder meanwhile is not kept waiting.
        List<string> still = DeleteOthers(names, mine, LeftOver);
        lock (gate)
        {
            left.Clear();
            left.AddRange(still);
            bool gone = !still.Contains(Path.GetFileName(ended), StringComparer.Ordinal);
            if (!taken && !gone)
            {
                refused = true;
                Say("what the web view kept could be neither cleared nor deleted: no web page is shown until the app is started again");
            }

            // While no web page may be shown in this run (this session's data is still there, an earlier one's is, or no new folder could be
            // made), no end of a session is finished: the next start finishes it, in a new folder.
            return !refused;
        }
    }

    // Deletes everything named that is not the folder of the session; what stays is said and given back. The names and the session's folder
    // were read together, so a folder made after that (for whoever comes next, by a session that ended meanwhile) is not among them.
    private List<string> DeleteOthers(IReadOnlyList<string> names, string mine, TimeSpan limit)
    {
        List<string> still = [];
        foreach (string name in names)
        {
            if (string.Equals(name, mine, StringComparison.Ordinal))
            {
                continue;
            }

            if (!folders.Delete(Path.Combine(root, name), limit))
            {
                still.Add(name);
                Say("an older folder of the web view could not be deleted yet (" + name + "): it is not used again, and is tried again later");
            }
        }

        return still;
    }

    // Says it to whoever listens. What a listener throws changes nothing here: the folders are settled before anybody is told.
    private void Say(string what)
    {
        try
        {
            say?.Invoke(what);
        }
        catch (Exception)
        {
            // Told or not, what was decided stands.
        }
    }

    private string PathOf(long number) => Path.Combine(root, Name(number));

    private static string Name(long number) => number.ToString(CultureInfo.InvariantCulture);

    private static long? NumberOf(string name) =>
        long.TryParse(name, NumberStyles.None, CultureInfo.InvariantCulture, out long number) && number > 0 && Name(number) == name ? number : null;
}
