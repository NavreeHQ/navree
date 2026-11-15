namespace Navree.Platform;

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

    /// <summary>Whether a request is one the web view answers without the network: a document of its own, or inline data of a page.</summary>
    /// <param name="address">The address.</param>
    /// <returns>True for a blank page, a blob and inline data.</returns>
    public static bool IsOwnRequest(string address) =>
        IsOwnDocument(address) || address.StartsWith("data:", StringComparison.OrdinalIgnoreCase);
}
