using Navree.Runtime;

namespace Navree.Platform;

/// <summary>A web view that is alive, as the end of a session or a cover over the app needs to know it.</summary>
public interface ILiveWebView
{
    /// <summary>The number of the screen this web view was made for.</summary>
    long Screen { get; }

    /// <summary>The folder it keeps its data in, which is the folder of the session it was started for; null when it was never started.</summary>
    string? Folder { get; }

    /// <summary>Stops what it shows and empties it, so that nothing of the page writes again into what is about to be cleared.</summary>
    void Empty();

    /// <summary>Clears everything the profile it belongs to keeps. Every web view of one folder shares one profile.</summary>
    /// <returns>When it is cleared; it may never return, and it may throw.</returns>
    Task ClearProfileAsync();

    /// <summary>Closes it for good. It is never shown a page again.</summary>
    void Close();
}

/// <summary>
/// The end of a session for the web views: every web view of the session that ends is emptied and closed, and what they kept is cleared
/// through one of them or deleted with their folder. A web view is known by the session it was made for. One of an older session that is
/// somehow still alive is closed too and never answers for the session that is ending: a profile cleared through it is its own, already
/// emptied, and says nothing of this one. A web view of whoever comes next (started after the folder moved on) is left as it is.
/// </summary>
public static class WebWipe
{
    /// <summary>How long one web view is given to clear its profile before the next is asked.</summary>
    public static readonly TimeSpan ClearLimit = TimeSpan.FromSeconds(3);

    /// <summary>Ends a session's web views.</summary>
    /// <param name="ended">The folder of the session that ends.</param>
    /// <param name="now">The folder of the session that follows, or null when no web page may be shown.</param>
    /// <param name="live">Every web view that is alive, of any session.</param>
    /// <param name="deleteFolder">Deletes the folder of the session that ends, once its web views are closed; whether it is gone.</param>
    /// <param name="clearLimit">How long one web view is given to clear its profile; <see cref="ClearLimit"/> when left out.</param>
    /// <param name="note">Told each thing that was done, in words.</param>
    /// <returns>
    /// Whether what the session kept is gone: its profile was cleared through a web view of its own, or its folder was deleted. In both cases
    /// every web view of the session, and of any older one, has been closed. False when it could be neither cleared nor deleted.
    /// </returns>
    public static async Task<bool> RunAsync(
        string ended,
        string? now,
        IReadOnlyList<ILiveWebView> live,
        Func<Task<bool>> deleteFolder,
        TimeSpan? clearLimit = null,
        Action<string>? note = null)
    {
        ArgumentException.ThrowIfNullOrEmpty(ended);
        ArgumentNullException.ThrowIfNull(live);
        ArgumentNullException.ThrowIfNull(deleteFolder);
        TimeSpan each = clearLimit ?? ClearLimit;
        List<ILiveWebView> own = [];
        int older = 0;
        foreach (ILiveWebView view in live)
        {
            string? folder = view.Folder;
            if (string.Equals(folder, ended, StringComparison.Ordinal))
            {
                own.Add(view);
            }
            else if (folder is not null && !string.Equals(folder, now, StringComparison.Ordinal))
            {
                // Left alive from a session that ended earlier: closed, and never asked to clear anything for this one.
                Quietly(view.Close);
                older++;
            }
        }

        note?.Invoke(own.Count + " web view(s) of the session that ends, " + older + " of an older session closed");

        // A page that still runs could write again into what is about to be cleared: each is stopped and emptied first.
        foreach (ILiveWebView view in own)
        {
            Quietly(view.Empty);
        }

        // One profile serves every web view of the folder: it is cleared through the first of them that answers.
        bool cleared = false;
        foreach (ILiveWebView view in own)
        {
            try
            {
                Task clearing = view.ClearProfileAsync();
                if (await Task.WhenAny(clearing, Task.Delay(each)).ConfigureAwait(true) != clearing)
                {
                    note?.Invoke("a web view's clear did not answer in " + each.TotalSeconds + " s");
                    continue;
                }

                await clearing.ConfigureAwait(true);
                cleared = true;
                note?.Invoke("the profile was cleared");
                break;
            }
            catch (Exception problem)
            {
                note?.Invoke("a web view's clear failed: " + problem.GetType().Name);
            }
        }

        // Every web view of the session that ends is closed, whether or not the profile was cleared through one of them: none is left to show
        // a page, in the main window or in a window of its own.
        foreach (ILiveWebView view in own)
        {
            Quietly(view.Close);
        }

        if (cleared)
        {
            return true;
        }

        bool gone;
        try
        {
            gone = await deleteFolder().ConfigureAwait(true);
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
        {
            gone = false;
        }

        note?.Invoke("the folder " + (gone ? "was deleted" : "could not be deleted"));
        return gone;
    }

    /// <summary>
    /// Takes down every live web view whose screen is covered by a locked cover, parked as blank, or no longer showing a web page: each is
    /// emptied and closed so nothing in the page keeps running or talking to the server under the sign-in or waiting cover, while its folder
    /// stays on disk for the same person if they sign in again.
    /// </summary>
    /// <param name="state">The state about to be shown.</param>
    /// <param name="live">Every web view that is alive.</param>
    /// <returns>How many web views were taken down.</returns>
    public static int TakeDownUnderCover(ViewState state, IReadOnlyList<ILiveWebView> live)
    {
        ArgumentNullException.ThrowIfNull(state);
        ArgumentNullException.ThrowIfNull(live);
        int closed = 0;
        foreach (ILiveWebView view in live)
        {
            if (CoverRules.AllowsWeb(state, view.Screen))
            {
                continue;
            }

            Quietly(view.Empty);
            Quietly(view.Close);
            closed++;
        }

        return closed;
    }

    private static void Quietly(Action act)
    {
        try
        {
            act();
        }
        catch (Exception)
        {
            // A web view that is already closed, or that cannot be emptied, is closed all the same in the end.
        }
    }
}
