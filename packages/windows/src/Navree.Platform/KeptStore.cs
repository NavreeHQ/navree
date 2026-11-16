using System.Security.Cryptography;
using System.Text;

namespace Navree.Platform;

/// <summary>
/// What the core asks the client to keep between launches (the shell, the pages it may keep, the snapshot), under
/// the keys it names. Each value is protected for this person on this device (<see cref="Dpapi"/>), written in the
/// app's own folder under the person's local application data (not the roaming one, which is copied to other
/// devices), and kept out of content indexing. A file that cannot be opened here is not an error: it is nothing.
/// </summary>
public sealed class KeptStore
{
    private const string WipeMarkName = "signing-out.mark";
    private readonly string folder;
    private readonly byte[] entropy;
    private readonly object gate = new();

    /// <summary>Opens the store in a folder.</summary>
    /// <param name="folder">The app's own folder for what it keeps.</param>
    /// <param name="appName">The app's name, which must be given again to open what was kept.</param>
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
            if (!File.Exists(file))
            {
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
        }
    }

    /// <summary>Throws away everything kept. The mark of a sign-out under way stays (<see cref="MarkWipe"/>).</summary>
    public void Clear()
    {
        lock (gate)
        {
            if (Directory.Exists(folder))
            {
                foreach (string file in Directory.GetFiles(folder))
                {
                    if (!string.Equals(Path.GetFileName(file), WipeMarkName, StringComparison.OrdinalIgnoreCase))
                    {
                        File.Delete(file);
                    }
                }
            }
        }
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
                return Directory.Exists(folder) ? Directory.GetFiles(folder, "*.kept").Length : 0;
            }
        }
    }

    // A key is any text, so the file is named by what the key hashes to.
    private string PathOf(string key) =>
        Path.Combine(folder, Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(key))) + ".kept");
}
