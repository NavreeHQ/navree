using System.Security.Cryptography;
using System.Text;

namespace Navree.Platform;

/// <summary>
/// What the core asks the client to keep between launches (the shell, the pages it may keep, the snapshot), under
/// the keys it names. Each value is protected with Windows' data protection for the Windows user the app runs as
/// (<see cref="Dpapi"/>): other people who sign in to the computer cannot read it, and any program that runs as the same
/// Windows user can. It is written in the app's own folder under the local application data (not the roaming one, which
/// is copied to other computers), and kept out of content indexing. A file that cannot be opened here is not an error: it is nothing.
/// A file of a session that ended which the system would not let go (another program holds it) is set aside: it is named in a list beside
/// it, never read again, and deleted as soon as it can be.
/// </summary>
public sealed class KeptStore
{
    private const string WipeMarkName = "signing-out.mark";
    private const string AsideName = "set-aside.list";
    private readonly string folder;
    private readonly byte[] entropy;
    private readonly object gate = new();
    private HashSet<string>? aside;

    /// <summary>Opens the store in a folder.</summary>
    /// <param name="folder">The app's own folder for what it keeps.</param>
    /// <param name="appName">The app's name, which must be given again to open what was kept (so one app's store is not read as another's by mistake).</param>
    public KeptStore(string folder, string appName)
    {
        ArgumentException.ThrowIfNullOrEmpty(folder);
        ArgumentException.ThrowIfNullOrEmpty(appName);
        this.folder = folder;
        entropy = Encoding.UTF8.GetBytes("navree:" + appName);
    }

    /// <summary>The folder the app's own data goes in: under the local application data, never the roaming one.</summary>
    /// <param name="appName">The app's name.</param>
    /// <returns>The folder.</returns>
    public static string DefaultFolder(string appName) =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Navree", appName, "kept");

    /// <summary>Reads what is kept under a key.</summary>
    /// <param name="key">The key.</param>
    /// <returns>The value, or null when nothing is kept or it cannot be opened here.</returns>
    public string? Read(string key)
    {
        lock (gate)
        {
            string file = PathOf(key);
            if (!File.Exists(file) || Aside().Contains(Path.GetFileName(file)))
            {
                // Nothing kept, or a file of a session that ended, which is never read again.
                return null;
            }

            try
            {
                return Encoding.UTF8.GetString(Dpapi.Unprotect(File.ReadAllBytes(file), entropy));
            }
            catch (Exception problem) when (problem is InvalidOperationException or IOException or UnauthorizedAccessException)
            {
                return null;
            }
        }
    }

    /// <summary>Keeps a value under a key, or takes the key away when there is no value.</summary>
    /// <param name="key">The key.</param>
    /// <param name="value">The value.</param>
    public void Write(string key, string? value)
    {
        lock (gate)
        {
            string file = PathOf(key);
            if (value is null)
            {
                if (File.Exists(file))
                {
                    File.Delete(file);
                }

                return;
            }

            Directory.CreateDirectory(folder);
            string temporary = file + ".part";
            File.WriteAllBytes(temporary, Dpapi.Protect(Encoding.UTF8.GetBytes(value), entropy));
            File.SetAttributes(temporary, FileAttributes.NotContentIndexed);
            File.Move(temporary, file, overwrite: true);

            // What the file held before is gone: it is this value's file now.
            if (Aside().Remove(Path.GetFileName(file)))
            {
                SaveAside();
            }
        }
    }

    /// <summary>
    /// Throws away everything kept. The mark of a sign-out under way stays (<see cref="MarkWipe"/>). Every file that can be deleted is
    /// deleted, whatever happens to another; when one could not be, this throws after the rest have gone.
    /// </summary>
    /// <exception cref="IOException">A file is still there: another program holds it.</exception>
    public void Clear()
    {
        lock (gate)
        {
            List<string> left = DeleteEverything();
            HashSet<string> known = Aside();
            known.IntersectWith(left);
            try
            {
                SaveAside();
            }
            catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
            {
                // The list is written again by whatever finishes the clear.
            }

            if (left.Count > 0)
            {
                throw new IOException(left.Count + " kept file(s) could not be deleted");
            }
        }
    }

    /// <summary>
    /// Throws away everything kept, and sets aside what the system will not let go: a file another program holds is named in a list written
    /// beside it, is never read again (<see cref="Read"/> answers nothing for it), and is deleted when it can be (<see cref="DeleteSetAside"/>,
    /// and every later clear). The mark of a sign-out under way stays.
    /// </summary>
    /// <returns>How many files were set aside.</returns>
    /// <exception cref="IOException">The list could not be written: nothing is set aside, and the caller keeps the mark.</exception>
    public int ClearOrSetAside()
    {
        lock (gate)
        {
            List<string> left = DeleteEverything();
            aside = new HashSet<string>(left, StringComparer.OrdinalIgnoreCase);
            SaveAside();
            return left.Count;
        }
    }

    /// <summary>Deletes the files that were set aside, as far as they can be now.</summary>
    /// <returns>How many are still there.</returns>
    public int DeleteSetAside()
    {
        lock (gate)
        {
            HashSet<string> known = Aside();
            if (known.Count == 0)
            {
                return 0;
            }

            foreach (string name in known.ToArray())
            {
                string file = Path.Combine(folder, name);
                try
                {
                    if (File.Exists(file))
                    {
                        File.Delete(file);
                    }

                    _ = known.Remove(name);
                }
                catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
                {
                    // Still held: it stays set aside.
                }
            }

            try
            {
                SaveAside();
            }
            catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
            {
                // The list names files that are gone: naming them does no harm, and the next start writes it again.
            }

            return known.Count;
        }
    }

    // Deletes every file of the folder but the mark and the list of what is set aside; the names of those still there afterwards.
    private List<string> DeleteEverything()
    {
        List<string> left = [];
        if (!Directory.Exists(folder))
        {
            return left;
        }

        foreach (string file in Directory.GetFiles(folder))
        {
            string name = Path.GetFileName(file);
            if (string.Equals(name, WipeMarkName, StringComparison.OrdinalIgnoreCase) || string.Equals(name, AsideName, StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }

            try
            {
                File.Delete(file);
            }
            catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
            {
                left.Add(name);
            }
        }

        return left;
    }

    // The names of the files set aside, read once from the list beside them. A list that cannot be read names nothing.
    private HashSet<string> Aside()
    {
        if (aside is not null)
        {
            return aside;
        }

        aside = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        try
        {
            string list = Path.Combine(folder, AsideName);
            if (File.Exists(list))
            {
                foreach (string line in File.ReadAllLines(list))
                {
                    if (line.Length > 0 && line == Path.GetFileName(line))
                    {
                        _ = aside.Add(line);
                    }
                }
            }
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
        {
            // Nothing named.
        }

        return aside;
    }

    // Writes the list of what is set aside, through to the disk; no list when nothing is.
    private void SaveAside()
    {
        string list = Path.Combine(folder, AsideName);
        if (aside is null || aside.Count == 0)
        {
            if (File.Exists(list))
            {
                File.Delete(list);
            }

            return;
        }

        Directory.CreateDirectory(folder);
        using FileStream file = new(list, FileMode.Create, FileAccess.Write, FileShare.None, 4096, FileOptions.WriteThrough);
        file.Write(Encoding.UTF8.GetBytes(string.Join("\n", aside.Order(StringComparer.Ordinal)) + "\n"));
        file.Flush(flushToDisk: true);
    }

    /// <summary>Whether a sign-out was started and not finished: the mark <see cref="MarkWipe"/> leaves is there.</summary>
    public bool WipeMarked
    {
        get
        {
            lock (gate)
            {
                return File.Exists(Path.Combine(folder, WipeMarkName));
            }
        }
    }

    /// <summary>
    /// Leaves the mark that a sign-out is under way, and asks the system to write it through to the disk before this returns
    /// (<see cref="FileStream.Flush(bool)"/> with true). It holds nothing of the person.
    /// </summary>
    public void MarkWipe()
    {
        lock (gate)
        {
            Directory.CreateDirectory(folder);
            using FileStream mark = new(Path.Combine(folder, WipeMarkName), FileMode.Create, FileAccess.Write, FileShare.None, 1, FileOptions.WriteThrough);
            mark.WriteByte((byte)'1');
            mark.Flush(flushToDisk: true);
        }
    }

    /// <summary>Takes the mark of a sign-out under way away: the sign-out has finished.</summary>
    public void UnmarkWipe()
    {
        lock (gate)
        {
            string mark = Path.Combine(folder, WipeMarkName);
            if (File.Exists(mark))
            {
                File.Delete(mark);
            }
        }
    }

    /// <summary>How many values are kept.</summary>
    public int Count
    {
        get
        {
            lock (gate)
            {
                return Directory.Exists(folder) ? Directory.GetFiles(folder, "*.kept").Count(file => !Aside().Contains(Path.GetFileName(file))) : 0;
            }
        }
    }

    // A key is any text, so the file is named by what the key hashes to.
    private string PathOf(string key) =>
        Path.Combine(folder, Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(key))) + ".kept");
}
