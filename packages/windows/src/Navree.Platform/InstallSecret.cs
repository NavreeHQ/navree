using System.Security.Cryptography;
using System.Text;

namespace Navree.Platform;

/// <summary>
/// The secret of this install of the app, which the core makes its key for a person with (<c>secret</c> in the core's configuration).
/// </summary>
/// <remarks>
/// <para>
/// It is 32 bytes from the system's secure random generator, written as hex. It is made once, the first time the app starts, and kept in a
/// file of its own beside the folder of what is kept: not in that folder, which a sign-out empties. So one person has one key at every
/// start of this install, also after somebody signed out in between, and has another key in every other install. The file is protected
/// with Windows' data protection for the Windows user the app runs as (<see cref="Dpapi"/>), stands under the local application data,
/// which is not copied to other computers, and is kept out of content indexing.
/// </para>
/// <para>
/// A file that is not there, cannot be opened here, or does not hold a secret is replaced by a new secret. The keys made with the new
/// one are other keys: the person named next is then another person than the one whose mark is kept, and what was kept goes, as it does
/// for anybody else. A secret that cannot be written is used for this run alone, and the same follows at the next start.
/// </para>
/// <para>
/// It is a secret: it is given to the core and to nothing else, and is never written to a log.
/// </para>
/// </remarks>
public static class InstallSecret
{
    /// <summary>How many bytes the secret has.</summary>
    public const int Bytes = 32;

    private const string Ending = ".install";

    /// <summary>The file the secret of an install is kept in: beside the folder of what is kept, and named after it.</summary>
    /// <param name="keptFolder">The folder of what is kept (<see cref="KeptStore"/>).</param>
    /// <returns>The file's path.</returns>
    public static string FileBeside(string keptFolder)
    {
        ArgumentException.ThrowIfNullOrEmpty(keptFolder);
        return Path.TrimEndingDirectorySeparator(Path.GetFullPath(keptFolder)) + Ending;
    }

    /// <summary>Reads the secret of this install, and makes and keeps one when there is none that reads.</summary>
    /// <param name="keptFolder">The folder of what is kept: the secret's file stands beside it.</param>
    /// <param name="appName">The app's name, which must be given again to open what was kept.</param>
    /// <returns>The secret, as hex: two characters a byte.</returns>
    public static string Open(string keptFolder, string appName)
    {
        ArgumentException.ThrowIfNullOrEmpty(appName);
        string file = FileBeside(keptFolder);
        byte[] entropy = Encoding.UTF8.GetBytes("navree-install:" + appName);
        if (Read(file, entropy) is string held)
        {
            return held;
        }

        string made = Convert.ToHexString(RandomNumberGenerator.GetBytes(Bytes));
        try
        {
            if (Path.GetDirectoryName(file) is { Length: > 0 } parent)
            {
                Directory.CreateDirectory(parent);
            }

            string temporary = file + ".part";
            File.WriteAllBytes(temporary, Dpapi.Protect(Encoding.UTF8.GetBytes(made), entropy));
            File.SetAttributes(temporary, FileAttributes.NotContentIndexed);
            File.Move(temporary, file, overwrite: true);
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException or InvalidOperationException)
        {
            // It is the secret of this run alone: the next start makes another, and takes the person it then hears for another.
        }

        return made;
    }

    /// <summary>Whether a text is a secret the core takes: hex, two characters a byte, of at least <see cref="Bytes"/> bytes.</summary>
    /// <param name="text">The text.</param>
    /// <returns>Whether it is one.</returns>
    public static bool Reads(string? text) =>
        text is not null && text.Length >= 2 * Bytes && text.Length % 2 == 0 && text.All(Uri.IsHexDigit);

    private static string? Read(string file, byte[] entropy)
    {
        try
        {
            if (!File.Exists(file))
            {
                return null;
            }

            string held = Encoding.UTF8.GetString(Dpapi.Unprotect(File.ReadAllBytes(file), entropy));
            return Reads(held) ? held : null;
        }
        catch (Exception problem) when (problem is InvalidOperationException or IOException or UnauthorizedAccessException)
        {
            return null;
        }
    }
}
