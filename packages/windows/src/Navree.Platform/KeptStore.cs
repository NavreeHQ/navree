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

    /// <summary>Throws away everything kept.</summary>
    public void Clear()
    {
        lock (gate)
        {
            if (Directory.Exists(folder))
            {
                foreach (string file in Directory.GetFiles(folder))
                {
                    File.Delete(file);
                }
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
