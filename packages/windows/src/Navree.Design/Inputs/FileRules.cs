using System.Globalization;

namespace Navree.Design.Inputs;

/// <summary>Why a file was not added to a field, in words for the person and for a screen reader.</summary>
/// <param name="Words">What is said at the field.</param>
public sealed record Refusal(string Words);

/// <summary>One thing dropped on a file field, as the system hands it over.</summary>
/// <param name="Name">Its name.</param>
/// <param name="Path">Where it is on this computer; empty for a thing that has no path here (a file on a phone seen through Explorer, a link from a browser).</param>
/// <param name="Folder">Whether it is a folder.</param>
public sealed record DroppedItem(string Name, string? Path, bool Folder);

/// <summary>What comes of a drop: the paths offered to the field, and the words said for what was not.</summary>
/// <param name="Paths">The files on this computer, in the order they were dropped.</param>
/// <param name="Words">The words for the rest, one for each, and one for all the folders.</param>
public sealed record Dropped(IReadOnlyList<string> Paths, IReadOnlyList<string> Words);

/// <summary>
/// What a file field takes: the kinds named by the web's own list (<c>image/*,.pdf</c>), the most bytes of all the files of one field together,
/// and the words said about a file. The protocol carries no largest size, so it is the client's own option.
/// </summary>
public static class FileRules
{
    /// <summary>The most bytes the files of one field may hold together unless the client's options say another: 100 MiB.</summary>
    public const long MostBytes = 100L * 1024 * 1024;

    /// <summary>What is said of a file that was chosen and has since been moved, deleted or locked.</summary>
    public const string GoneWords = "No longer available. Choose it again.";

    /// <summary>The media type a file of no known extension is sent as.</summary>
    public const string Unknown = "application/octet-stream";

    // Extension (no dot) to media type: the kinds a person is likely to attach. A kind not here is sent as application/octet-stream.
    private static readonly Dictionary<string, string> Types = new(StringComparer.Ordinal)
    {
        ["jpg"] = "image/jpeg", ["jpeg"] = "image/jpeg", ["jpe"] = "image/jpeg", ["jfif"] = "image/jpeg", ["jfi"] = "image/jpeg", ["jif"] = "image/jpeg",
        ["png"] = "image/png", ["gif"] = "image/gif",
        ["webp"] = "image/webp", ["bmp"] = "image/bmp", ["tif"] = "image/tiff", ["tiff"] = "image/tiff", ["heic"] = "image/heic",
        ["heif"] = "image/heif", ["avif"] = "image/avif", ["svg"] = "image/svg+xml", ["ico"] = "image/vnd.microsoft.icon",
        ["jxl"] = "image/jxl", ["dng"] = "image/x-adobe-dng", ["jxr"] = "image/jxr", ["wdp"] = "image/vnd.ms-photo", ["hdp"] = "image/vnd.ms-photo",
        ["psd"] = "image/vnd.adobe.photoshop", ["xcf"] = "image/x-xcf", ["ora"] = "image/openraster",
        ["mp4"] = "video/mp4", ["m4v"] = "video/mp4", ["mov"] = "video/quicktime", ["3gp"] = "video/3gpp", ["3g2"] = "video/3gpp2",
        ["webm"] = "video/webm", ["avi"] = "video/x-msvideo", ["mkv"] = "video/x-matroska", ["wmv"] = "video/x-ms-wmv",
        ["asf"] = "video/x-ms-asf", ["flv"] = "video/x-flv", ["ts"] = "video/mp2t", ["mts"] = "video/mp2t", ["m2ts"] = "video/mp2t",
        ["mpg"] = "video/mpeg", ["mpeg"] = "video/mpeg", ["ogv"] = "video/ogg",
        ["mp3"] = "audio/mpeg", ["wav"] = "audio/wav", ["m4a"] = "audio/mp4", ["ogg"] = "audio/ogg", ["flac"] = "audio/flac", ["aac"] = "audio/aac",
        ["txt"] = "text/plain", ["csv"] = "text/csv", ["html"] = "text/html", ["htm"] = "text/html", ["md"] = "text/markdown",
        ["pdf"] = "application/pdf", ["json"] = "application/json", ["xml"] = "application/xml", ["zip"] = "application/zip",
        ["doc"] = "application/msword",
        ["docx"] = "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ["xls"] = "application/vnd.ms-excel",
        ["xlsx"] = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ["ppt"] = "application/vnd.ms-powerpoint",
        ["pptx"] = "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        ["rtf"] = "application/rtf",
    };

    /// <summary>The media type a name's extension says, or <see cref="Unknown"/>.</summary>
    /// <param name="name">The file's name.</param>
    /// <returns>A media type.</returns>
    public static string MediaTypeOf(string name)
    {
        string extension = ExtensionOf(name);
        return extension.Length > 0 && Types.TryGetValue(extension, out string? found) ? found : Unknown;
    }

    /// <summary>The last part of a path or a name, as a person writes it: what follows the last slash or backslash.</summary>
    /// <param name="path">A path or a name.</param>
    /// <returns>The name.</returns>
    public static string DisplayName(string path)
    {
        ArgumentNullException.ThrowIfNull(path);
        int at = path.LastIndexOfAny(['/', '\\']);
        string name = at >= 0 ? path[(at + 1)..] : path;
        return name.Trim();
    }

    /// <summary>The kinds a field's <c>accept</c> names, lower case; none when it names none (a field that takes everything).</summary>
    /// <param name="accept">The web's own list, or null.</param>
    /// <returns>The kinds.</returns>
    public static IReadOnlyList<string> Wanted(string? accept) =>
        [.. (accept ?? string.Empty).Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries).Select(kind => kind.ToLowerInvariant()).Distinct()];

    /// <summary>Whether a field with this <c>accept</c> takes a file of this name and type.</summary>
    /// <param name="accept">The web's own list, or null.</param>
    /// <param name="name">The file's name.</param>
    /// <param name="mediaType">The file's media type.</param>
    /// <returns>True when it takes it.</returns>
    public static bool Takes(string? accept, string name, string mediaType)
    {
        IReadOnlyList<string> wanted = Wanted(accept);
        if (wanted.Count == 0)
        {
            return true;
        }

        string extension = "." + ExtensionOf(name);
        string type = mediaType.ToLowerInvariant();
        foreach (string kind in wanted)
        {
            if (kind.StartsWith('.'))
            {
                if (kind == extension)
                {
                    return true;
                }
            }
            else if (kind.EndsWith("/*", StringComparison.Ordinal))
            {
                if (type != Unknown && type.StartsWith(kind[..^1], StringComparison.Ordinal))
                {
                    return true;
                }
            }
            else if (kind == type)
            {
                return true;
            }
        }

        return false;
    }

    /// <summary>
    /// The extensions the system's file picker is asked to show for a field: each kind the field names, written as <c>.ext</c>; every
    /// file when the field names none, or names one this table does not know (a wrong kind is then refused with words after it is chosen).
    /// </summary>
    /// <param name="accept">The web's own list, or null.</param>
    /// <returns>Extensions with their dot, or <c>*</c>.</returns>
    public static IReadOnlyList<string> PickerTypes(string? accept)
    {
        IReadOnlyList<string> wanted = Wanted(accept);
        SortedSet<string> shown = new(StringComparer.Ordinal);
        foreach (string kind in wanted)
        {
            if (kind.StartsWith('.'))
            {
                _ = shown.Add(kind);
                continue;
            }

            IEnumerable<string> found = kind.EndsWith("/*", StringComparison.Ordinal)
                ? Types.Where(pair => pair.Value.StartsWith(kind[..^1], StringComparison.Ordinal)).Select(pair => pair.Key)
                : Types.Where(pair => pair.Value == kind).Select(pair => pair.Key);
            int before = shown.Count;
            foreach (string extension in found)
            {
                _ = shown.Add("." + extension);
            }

            if (shown.Count == before)
            {
                return ["*"];
            }
        }

        return shown.Count == 0 ? ["*"] : [.. shown];
    }

    /// <summary>The kinds a field takes, in a few plain words: <c>pictures and .pdf</c>.</summary>
    /// <param name="accept">The web's own list, or null.</param>
    /// <returns>Words, or an empty text when the field takes everything.</returns>
    public static string KindsInWords(string? accept)
    {
        List<string> words = [];
        foreach (string kind in Wanted(accept))
        {
            words.Add(kind switch
            {
                "image/*" => "pictures",
                "video/*" => "videos",
                "audio/*" => "audio",
                "text/*" => "text files",
                _ => kind,
            });
        }

        return words.Count switch
        {
            0 => string.Empty,
            1 => words[0],
            _ => string.Join(", ", words.Take(words.Count - 1)) + " and " + words[^1],
        };
    }

    /// <summary>A size in bytes as a person reads it: <c>340 KB</c>, <c>1.2 MB</c>, <c>100 MB</c>.</summary>
    /// <param name="bytes">The size.</param>
    /// <returns>The words.</returns>
    public static string Size(long bytes)
    {
        if (bytes < 1024)
        {
            return bytes.ToString(CultureInfo.CurrentCulture) + (bytes == 1 ? " byte" : " bytes");
        }

        string[] units = ["KB", "MB", "GB", "TB"];
        double value = bytes / 1024.0;
        int unit = 0;
        while (value >= 1024 && unit < units.Length - 1)
        {
            value /= 1024;
            unit++;
        }

        // One place after the point below ten, none above: "1.2 MB", "340 KB".
        string number = value < 9.95 ? value.ToString("0.#", CultureInfo.CurrentCulture) : Math.Round(value).ToString("0", CultureInfo.CurrentCulture);
        return number + " " + units[unit];
    }

    /// <summary>
    /// Whether a file may be added: the field takes its kind, and it, with the files the field holds already, is not over the most bytes.
    /// </summary>
    /// <param name="name">The file's name.</param>
    /// <param name="mediaType">The file's media type.</param>
    /// <param name="size">The file's size in bytes.</param>
    /// <param name="heldBytes">The bytes of the files the field holds that stay when this one is added.</param>
    /// <param name="accept">The field's <c>accept</c>.</param>
    /// <param name="most">The most bytes of all the files of one field together.</param>
    /// <returns>The refusal in words, or null when the file may be added.</returns>
    public static Refusal? Refuse(string name, string mediaType, long size, long heldBytes, string? accept, long most)
    {
        if (!Takes(accept, name, mediaType))
        {
            string takes = KindsInWords(accept);
            return new Refusal(name + " is not a kind of file this field takes (it takes " + takes + "), so it was not added.");
        }

        if (size > most)
        {
            return new Refusal(name + " is larger than " + Size(most) + " and was not added.");
        }

        if (heldBytes + size > most)
        {
            return new Refusal("Adding " + name + " would take the files of this field over " + Size(most) + ", so it was not added.");
        }

        return null;
    }

    /// <summary>What is said of a folder dropped on a field.</summary>
    public const string FolderWords = "Folders cannot be added; choose the files in it.";

    /// <summary>What is said when the system's file picker could not be opened.</summary>
    public const string PickerFailedWords = "The file picker could not be opened, so nothing was added. Try again, or drop the files on the field.";

    /// <summary>What is said when what was dropped could not be read at all.</summary>
    public const string DropFailedWords = "What was dropped could not be read, so nothing was added. Save it to a folder first and choose it from there.";

    /// <summary>Whether a path is one of a file on this computer: a full path with a drive or a share, not a name alone or an address.</summary>
    /// <param name="path">The path.</param>
    /// <returns>True when it is.</returns>
    public static bool IsOnThisComputer(string? path) =>
        !string.IsNullOrWhiteSpace(path) && path.IndexOf("://", StringComparison.Ordinal) < 0 && Path.IsPathFullyQualified(path);

    /// <summary>What is said of a thing that is not a file on this computer.</summary>
    /// <param name="name">Its name, or nothing.</param>
    /// <returns>The words.</returns>
    public static string NotHere(string? name) =>
        (string.IsNullOrWhiteSpace(name) ? "This item" : name.Trim()) + " is not a file on this computer (it may be on a phone or a link from a browser), so it was not added. Save it to a folder first and choose it from there.";

    /// <summary>Sorts what was dropped on a field: files on this computer are offered; a folder, or a thing with no path here, is said in words.</summary>
    /// <param name="items">What was dropped.</param>
    /// <returns>The paths to offer and the words to say.</returns>
    public static Dropped Sort(IReadOnlyList<DroppedItem> items)
    {
        ArgumentNullException.ThrowIfNull(items);
        List<string> paths = [];
        List<string> words = [];
        bool folders = false;
        foreach (DroppedItem item in items)
        {
            if (item.Folder)
            {
                folders = true;
            }
            else if (IsOnThisComputer(item.Path))
            {
                paths.Add(item.Path!);
            }
            else
            {
                words.Add(NotHere(item.Name));
            }
        }

        if (folders)
        {
            words.Add(FolderWords);
        }

        return new Dropped(paths, words);
    }

    /// <summary>What a screen reader says of one chosen file: its name and its size.</summary>
    /// <param name="name">The file's name.</param>
    /// <param name="size">Its size.</param>
    /// <returns>The words.</returns>
    public static string Spoken(string name, long size) => name + ", " + Size(size);

    /// <summary>What a screen reader says of the field's files as a whole.</summary>
    /// <param name="count">How many files are chosen.</param>
    /// <returns>The words.</returns>
    public static string Count(int count) => count switch
    {
        0 => "no file chosen",
        1 => "1 file chosen",
        _ => count.ToString(CultureInfo.CurrentCulture) + " files chosen",
    };

    private static string ExtensionOf(string name)
    {
        string shown = DisplayName(name);
        int dot = shown.LastIndexOf('.');
        return dot > 0 && dot < shown.Length - 1 ? shown[(dot + 1)..].ToLowerInvariant() : string.Empty;
    }
}
