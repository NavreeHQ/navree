namespace Navree.Platform;

/// <summary>What the client answers a request of the web view.</summary>
public enum WebAnswer
{
    /// <summary>It goes as it is: the web view makes it itself (a blank document, a blob, inline data).</summary>
    AsItIs,

    /// <summary>It goes to the app's own origin (or is answered from what the app recorded).</summary>
    Ours,

    /// <summary>It is answered 403 and never reaches the network.</summary>
    Refused,
}

/// <summary>
/// The folder the web view keeps what a web page leaves (cookies, storage, cache, history) and how it is taken away when no web
/// view is running to do it: the web view cannot be asked to clear what it keeps when it is not there, and a folder it has left
/// can be deleted once nothing holds its files.
/// </summary>
public static class WebData
{
    /// <summary>The folder the app's web views keep their data in, unless the app says another.</summary>
    /// <param name="appName">The app's name.</param>
    /// <returns>The folder's path.</returns>
    public static string DefaultFolder(string appName) =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Navree", appName, "web");

    /// <summary>Deletes the folder and everything in it, trying again while a file is still held by a process that is leaving.</summary>
    /// <param name="folder">The folder.</param>
    /// <param name="limit">How long to keep trying.</param>
    /// <param name="pause">How long between tries; a tenth of a second when left out.</param>
    /// <returns>Whether the folder is gone.</returns>
    public static async Task<bool> DeleteAsync(string folder, TimeSpan limit, TimeSpan? pause = null)
    {
        ArgumentException.ThrowIfNullOrEmpty(folder);
        TimeSpan between = pause ?? TimeSpan.FromMilliseconds(100);
        DateTime until = DateTime.UtcNow + limit;
        while (true)
        {
            try
            {
                if (!Directory.Exists(folder))
                {
                    return true;
                }

                Directory.Delete(folder, recursive: true);
                return !Directory.Exists(folder);
            }
            catch (Exception failure) when (failure is IOException or UnauthorizedAccessException)
            {
                if (DateTime.UtcNow >= until)
                {
                    return false;
                }
            }

            await Task.Delay(between).ConfigureAwait(false);
        }
    }

    /// <summary>Whether a web address is one the web view makes itself and not a page: a blank document or one made from the page's own data.</summary>
    /// <param name="address">The address.</param>
    /// <returns>True for a blank page and a blob.</returns>
    public static bool IsOwnDocument(string address) =>
        address.StartsWith("about:", StringComparison.OrdinalIgnoreCase)
        || address.StartsWith("blob:", StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// What the client answers a request the web view asks it about, whichever part of a page made it (the page, a frame, a worker of any
    /// kind): what the web view makes itself goes as it is; an address that is not the app's own is refused; and while a sign-out's steps run,
    /// every request that would reach the network is refused, so no answer can put a cookie into what is being cleared.
    /// </summary>
    /// <param name="address">The whole address asked for.</param>
    /// <param name="ours">Whether an address is one of the app's own.</param>
    /// <param name="wiping">Whether a sign-out's steps are running.</param>
    /// <returns>What to do with it.</returns>
    public static WebAnswer Answer(string address, Func<string, bool> ours, bool wiping)
    {
        ArgumentNullException.ThrowIfNull(address);
        ArgumentNullException.ThrowIfNull(ours);
        if (IsOwnRequest(address))
        {
            return WebAnswer.AsItIs;
        }

        return !wiping && ours(address) ? WebAnswer.Ours : WebAnswer.Refused;
    }

    /// <summary>
    /// Whether an address a web page was sent to is a file of this computer: what the web view makes of a file dropped on a page that does not
    /// take drops. Such an address holds a path of the person's own (their Windows user name, a folder's name), so it is cancelled where it
    /// arrives and told to nobody: not to the core, and not to a log.
    /// </summary>
    /// <param name="address">The address.</param>
    /// <returns>True for an address on the <c>file</c> scheme, and for a path written without one.</returns>
    public static bool IsLocalFile(string? address)
    {
        if (string.IsNullOrWhiteSpace(address))
        {
            return false;
        }

        string text = address.TrimStart();
        if (text.StartsWith("file:", StringComparison.OrdinalIgnoreCase) || text.StartsWith(@"\\", StringComparison.Ordinal))
        {
            return true;
        }

        // A drive's path: a letter, a colon, a slash.
        return text.Length >= 3 && char.IsAsciiLetter(text[0]) && text[1] == ':' && text[2] is '\\' or '/';
    }

    /// <summary>Whether a request is one the web view answers without the network: a document of its own, or inline data of a page.</summary>
    /// <param name="address">The address.</param>
    /// <returns>True for a blank page, a blob and inline data.</returns>
    public static bool IsOwnRequest(string address) =>
        IsOwnDocument(address) || address.StartsWith("data:", StringComparison.OrdinalIgnoreCase);
}
