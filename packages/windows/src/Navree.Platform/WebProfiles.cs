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
/// When what the web view kept for the session that ended could be neither cleared nor deleted, no web page is shown for the rest of that run
/// (<see cref="Folder"/> is null): the browser behind the old folder may still be running in this process's name. The next start names a new
/// folder and goes on. The number of the session now is the highest-numbered folder on disk, so the new folder is made on disk before the
/// session that ended counts as over.
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
    public bool Open(bool lastSessionEnded)
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
                say?.Invoke("a new folder for the web view could not be made: no web page is shown until the app is started again");
                return false;
            }

            DeleteOthers(names, lastSessionEnded ? AtStart : LeftOver);
            return true;
        }
    }

    /// <summary>
    /// The session ended, and the web view's own way of emptying its folder has been tried. The folder is never used again: a new one is made
    /// for whoever comes next, and the older ones are deleted if they can be. When what was kept was not taken away
    /// (<paramref name="taken"/> is false: neither cleared nor deleted) and its folder is still there, no web page is shown for the rest of
    /// this run.
    /// </summary>
    /// <param name="taken">Whether what the web view kept was cleared through a web view, or its folder deleted.</param>
    /// <returns>Whether nothing of the session that ended is left for a web page of this run to reach: false means the sign-out is not finished.</returns>
    public bool End(bool taken)
    {
        lock (gate)
        {
            if (refused)
            {
                return false;
            }

            long ended = current;
            if (!folders.Make(PathOf(ended + 1)))
            {
                refused = true;
                say?.Invoke("a new folder for the web view could not be made: no web page is shown until the app is started again");
                return false;
            }

            current = ended + 1;
            left.Clear();
            DeleteOthers(folders.Names(root), LeftOver);
            bool gone = !left.Contains(Name(ended), StringComparer.Ordinal);
            if (!taken && !gone)
            {
                refused = true;
                say?.Invoke("what the web view kept could be neither cleared nor deleted: no web page is shown until the app is started again");
                return false;
            }

            return true;
        }
    }

    // Deletes everything in the web folder that is not the folder of the session now; what stays is noted and said.
    private void DeleteOthers(IReadOnlyList<string> names, TimeSpan limit)
    {
        string mine = Name(current);
        foreach (string name in names)
        {
            if (string.Equals(name, mine, StringComparison.Ordinal))
            {
                continue;
            }

            if (!folders.Delete(Path.Combine(root, name), limit))
            {
                left.Add(name);
                say?.Invoke("an older folder of the web view could not be deleted yet (" + name + "): it is not used again, and is tried again later");
            }
        }
    }

    private string PathOf(long number) => Path.Combine(root, Name(number));

    private static string Name(long number) => number.ToString(CultureInfo.InvariantCulture);

    private static long? NumberOf(string name) =>
        long.TryParse(name, NumberStyles.None, CultureInfo.InvariantCulture, out long number) && number > 0 && Name(number) == name ? number : null;
}
