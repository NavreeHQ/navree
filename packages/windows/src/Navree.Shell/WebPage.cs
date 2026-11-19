using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.Web.WebView2.Core;
using Navree.Design;
using Navree.Platform;
using Navree.Runtime;
using Windows.Storage.Streams;

namespace Navree.Shell;

/// <summary>
/// A page of the app's own origins shown in the system's web view (WebView2), and held there by every means the web view
/// documents: a navigation that is not the app's own is cancelled and handed to the core as a link, a popup is never
/// opened, a document that is not the app's own is emptied by a script that runs before anything in it does (the layer
/// that sees redirects, which a callback does not), file inputs and drops are turned off with an explanation in the page,
/// a request for anything that is not the app's own is answered blocked, no host object and no message channel is offered,
/// and every permission is refused.
/// </summary>
/// <remarks>
/// What the callbacks cannot see: the web view reports the first address of a navigation, so a redirect inside a frame
/// that the web view does not report as a navigation of its own is caught by the document-start script, not by a
/// callback; and each request is paused while the app decides, which costs speed on a page with many parts.
/// </remarks>
internal sealed class WebPage : Grid, ILiveWebView
{
    // One environment for each folder the app's web views keep their data in; a page that is made while another is being started waits for the same one.
    private static readonly Dictionary<string, Task<CoreWebView2Environment>> Environments = [];

    // Every page whose web view has begun to start and is not closed, on screen or not, in the main window or in a window of its own, each with
    // the folder of the session it was made for: the end of a session reaches them all.
    private static readonly List<WebPage> Known = [];

    // Pages whose screens left while a wipe was running: they are closed when it is done.
    private static readonly List<WebPage> Deferred = [];
    private static int wiping;

    // The browser processes the web views of this app started, by the folder they serve: a folder cannot be deleted until its own have ended.
    private static readonly Dictionary<string, HashSet<int>> BrowserProcesses = [];

    private readonly ShellHost host;
    private readonly long screen;
    private readonly WebView2 view = new();
    private readonly Func<Uri, (byte[] Body, string ContentType)?>? recorded;
    private string? loaded;
    private long loadedSerial = -1;
    private WebContent? wanted;
    private Task? starting;
    private string? startedIn;
    private bool released;
    private bool closed;

    internal WebPage(ShellHost host, long screen, Func<Uri, (byte[] Body, string ContentType)?>? recorded)
    {
        this.host = host;
        this.screen = screen;
        this.recorded = recorded;

        // WebView2 offers no callback to inspect or clean a file chosen in a web page before the page reads it: file inputs, programmatic
        // file pickers, file drops and file pastes are all turned off, and the page states why.
        AllowDrop = false;
        view.AllowDrop = false;
        AddHandler(
            DragOverEvent,
            new DragEventHandler((_, args) =>
            {
                args.AcceptedOperation = Windows.ApplicationModel.DataTransfer.DataPackageOperation.None;
                args.Handled = true;
            }),
            handledEventsToo: true);
        AddHandler(
            DropEvent,
            new DragEventHandler((_, args) =>
            {
                args.AcceptedOperation = Windows.ApplicationModel.DataTransfer.DataPackageOperation.None;
                args.Handled = true;
            }),
            handledEventsToo: true);
        Children.Add(view);
    }

    /// <summary>
    /// Ends a session for the web views (<see cref="WebWipe"/>): every web view that was made for the session is emptied and closed, in the
    /// main window or in a window of its own, and what they kept (cookies, storage, cache, history) is cleared through one of them or, when
    /// none can clear it, deleted with the session's folder once the browser behind it has ended. A web view left alive from an older session
    /// is closed too and never answers for this one; a web view of whoever comes next is left as it is.
    /// </summary>
    /// <param name="ended">The folder the web views of the session that ends keep their data in.</param>
    /// <param name="now">The folder of the session that follows, or null when no web page may be shown.</param>
    /// <param name="trace">Told, in words, each step of the wipe and how long it took; when the wipe is slow this says which call it was.</param>
    /// <returns>
    /// Whether what was kept is gone: the profile was cleared through a web view of that session, or the folder was deleted. Every web view
    /// of the session is closed either way.
    /// </returns>
    internal static async Task<bool> ClearAllAsync(string ended, string? now, Action<string>? trace = null)
    {
        System.Diagnostics.Stopwatch clock = System.Diagnostics.Stopwatch.StartNew();
        void Note(string what) => trace?.Invoke(clock.ElapsedMilliseconds.ToString(System.Globalization.CultureInfo.InvariantCulture) + " ms: " + what);

        // While the wipe runs no web view is closed from outside it: a clear that is on its way to the profile never answers when the web view it
        // was asked of is closed under it, which is what happens when the screens go at the same moment (the core starts the app again). A web
        // view released meanwhile waits, and is closed when the wipe is done.
        wiping++;
        try
        {
            WebPage[] known = [.. Known];
            Note(known.Length + " web view(s) are alive: " + string.Join(", ", known.Select(page => "screen " + page.screen + (page.released ? " released" : string.Empty) + (page.view.CoreWebView2 is null ? " not started" : string.Empty))));
            return await WebWipe.RunAsync(ended, now, known, () => DeleteAsync(ended, Note), note: Note);
        }
        finally
        {
            wiping--;
            if (wiping == 0)
            {
                foreach (WebPage page in Deferred.ToArray())
                {
                    page.CloseNow();
                }

                Deferred.Clear();
            }
        }
    }

    // The folder of a session that ended is deleted once the browser behind it has ended: its processes are waited for, each for a short while.
    private static async Task<bool> DeleteAsync(string folder, Action<string> note)
    {
        if (BrowserProcesses.Remove(folder, out HashSet<int>? browsers))
        {
            foreach (int id in browsers)
            {
                try
                {
                    using System.Diagnostics.Process browser = System.Diagnostics.Process.GetProcessById(id);
                    using CancellationTokenSource limit = new(TimeSpan.FromSeconds(3));
                    await browser.WaitForExitAsync(limit.Token);
                    note("browser process " + id + " ended");
                }
                catch (OperationCanceledException)
                {
                    note("browser process " + id + " was still there after 3 s");
                }
                catch (Exception)
                {
                    // Not there.
                }
            }
        }

        Environments.Remove(folder);

        // Off the window's thread: a profile is thousands of files.
        return await Task.Run(() => WebData.DeleteAsync(folder, TimeSpan.FromSeconds(2)));
    }

    /// <inheritdoc />
    long ILiveWebView.Screen => screen;

    /// <inheritdoc />
    string? ILiveWebView.Folder => startedIn;

    /// <inheritdoc />
    void ILiveWebView.Empty()
    {
        if (!closed && view.CoreWebView2 is { } web)
        {
            web.Stop();
            web.Navigate("about:blank");
        }
    }

    /// <inheritdoc />
    async Task ILiveWebView.ClearProfileAsync()
    {
        // A web view that is still being started is waited for, for a short while: one that was closed in the middle of it never answers.
        if (starting is { IsCompleted: false } coming)
        {
            _ = await Task.WhenAny(coming, Task.Delay(TimeSpan.FromSeconds(2)));
        }

        if (closed || view.CoreWebView2 is not { } web)
        {
            throw new InvalidOperationException("the web view was not started");
        }

        await web.Profile.ClearBrowsingDataAsync();
    }

    /// <inheritdoc />
    void ILiveWebView.Close() => CloseNow();

    /// <summary>Whether the page has been closed or let go of.</summary>
    internal bool IsClosed => closed || released;

    /// <summary>
    /// Empties and closes every live web view whose screen is covered by a locked cover, parked as blank, or no longer showing a web page,
    /// leaving the session's folder on disk for the same person if they return.
    /// </summary>
    /// <param name="state">The state about to be shown.</param>
    internal static void TakeDownUnder(ViewState state) => _ = WebWipe.TakeDownUnderCover(state, [.. Known]);

    /// <summary>
    /// Whether the page's web view was started for a session that has ended: it keeps its data in a folder that is not the one of the session
    /// now, and is never shown a page again.
    /// </summary>
    internal bool OfAnEndedSession => startedIn is not null && !string.Equals(startedIn, host.WebFolder, StringComparison.Ordinal);

    /// <summary>Whether the page was last asked to show this very content.</summary>
    /// <param name="web">The content.</param>
    /// <returns>Whether it is what the page holds.</returns>
    internal bool Shows(WebContent web) => loaded == web.Url && loadedSerial == web.Serial;

    /// <summary>Closes the page's web view when its screen has left for good, so the page is no longer kept for the wipe and the process behind it goes.</summary>
    internal void Release()
    {
        released = true;

        // A wipe that is running may be asking this web view to clear the profile: it is closed when the wipe is done.
        if (wiping > 0)
        {
            Deferred.Add(this);
            return;
        }

        CloseNow();
    }

    private void CloseNow()
    {
        closed = true;
        Known.Remove(this);
        try
        {
            view.Close();
        }
        catch (Exception)
        {
            // Already closed.
        }
    }

    /// <summary>Shows the page; loads it again when the serial has moved on.</summary>
    internal async Task ShowAsync(WebContent web)
    {
        if (closed || released || !CoverRules.AllowsWeb(host.State, screen))
        {
            CloseNow();
            return;
        }

        if (loaded == web.Url && loadedSerial == web.Serial)
        {
            return;
        }

        loaded = web.Url;
        loadedSerial = web.Serial;
        wanted = web;

        // A page asked for by a screen of a person who is signing out is theirs, and the web view's folder is already that of whoever
        // comes next: the page is not loaded and no web view is started for it. The screen goes when the core lets go of the person.
        if (host.ShowsSomeoneLeaving)
        {
            host.ReportProblem("a web page was not loaded: the screen that asked for it is of a person who is signing out");
            return;
        }

        try
        {
            // The web view is started and held once, whatever came while it was being started; the newest wish is the one carried out.
            starting ??= StartAsync();
            await starting;

            // A page asked for while a sign-out is still taking away what the web view keeps loads when that is done, not into it (its
            // requests would be refused meanwhile). The wait is bounded by the wipe's own limit for its steps.
            for (int waited = 0; host.Wiping && waited < 400; waited++)
            {
                await Task.Delay(50);
            }

            if (closed || released || !CoverRules.AllowsWeb(host.State, screen))
            {
                CloseNow();
                return;
            }

            if (wanted != web || !host.Origins.Ours(web.Url))
            {
                return;
            }

            // A web view whose session ended while it waited keeps the profile of the person who left: it is never sent anywhere again.
            // It is said, so that a page that stays empty has its reason among the app's own words.
            if (OfAnEndedSession)
            {
                host.ReportProblem("a web page was not loaded: its web view was started for a session that has ended");
                return;
            }

            view.CoreWebView2!.Navigate(web.Url);
        }
        catch (WebViewTooOldException)
        {
            // A web view that cannot be held to the app's own addresses shows nothing: it is closed, and the person is told why.
            CloseNow();
            Children.Clear();
            Children.Add(Look.Label("This page cannot be shown: the web view on this device is too old to be kept to the app's own addresses. Updating the Microsoft Edge WebView2 Runtime lets it be shown.", muted: true));
            host.ReportProblem("a web page was not shown: the web view runtime has no request filter for workers");
        }
        catch (WebViewNotEmptiedException)
        {
            // What the web view kept for the person before (one who signed out, or one whose place another person took with no sign-out
            // between) is still on the device: no web view is started in this run, so no page can be shown with it, and the person is
            // told the way on.
            CloseNow();
            Children.Clear();
            Children.Add(Look.Label("This page cannot be shown until the app is started again: what its web pages kept for the person who was signed in before could not be removed from this device. Close the app and start it again.", muted: true));
            host.ReportProblem("a web page was not shown: what the web view kept for the person before could not be taken away");
        }
        catch (Exception problem)
        {
            Children.Clear();
            Children.Add(Look.Label("The web view is not available on this device (" + problem.GetType().Name + ").", muted: true));
            host.ReportProblem("a web page was not shown: the web view could not be started (" + problem.GetType().Name + ")");
        }
    }

    // Says over the page that a download it started was not saved.
    private void ShowRefusedDownload() => ShowWebNotice("Download not saved", "The app does not save files from its web pages.");

    // Says over the page why a file picker cannot open inside the web view.
    private void ShowRefusedFilePicker() => ShowWebNotice("File upload unavailable here", WebData.FilePickerOffReason);

    private void ShowWebNotice(string title, string message)
    {
        if (Children.OfType<InfoBar>().FirstOrDefault() is InfoBar shown)
        {
            shown.Title = title;
            shown.Message = message;
            shown.IsOpen = true;
            return;
        }

        InfoBar bar = new()
        {
            IsOpen = true,
            IsClosable = true,
            Severity = InfoBarSeverity.Informational,
            Title = title,
            Message = message,
            VerticalAlignment = VerticalAlignment.Top,
        };
        Children.Add(bar);
    }

    // The runtime has no way to ask the app about the requests of a page's workers, so the page is not shown.
    private sealed class WebViewTooOldException(Exception inner) : Exception("the web view runtime has no request filter for workers", inner);

    // What the web view kept for the person before could be neither cleared nor deleted, so no web view is started in this run.
    private sealed class WebViewNotEmptiedException() : Exception("what the web view kept for the person before could not be taken away");

    private async Task StartAsync()
    {
        // The folder of the session now, which no earlier session's profile was ever in; none when a sign-out or a change of person of this
        // run could not take away what the web view kept.
        string folder = host.WebFolder ?? throw new WebViewNotEmptiedException();
        startedIn = folder;
        if (!closed)
        {
            Known.Add(this);
        }
        if (!Environments.TryGetValue(folder, out Task<CoreWebView2Environment>? made))
        {
            made = CoreWebView2Environment.CreateWithOptionsAsync(null, folder, new CoreWebView2EnvironmentOptions()).AsTask();
            Environments[folder] = made;
        }

        try
        {
            await view.EnsureCoreWebView2Async(await made);
        }
        catch (Exception)
        {
            Environments.Remove(folder);
            throw;
        }

        await HoldAsync(view.CoreWebView2!);
        if (!BrowserProcesses.TryGetValue(folder, out HashSet<int>? browsers))
        {
            BrowserProcesses[folder] = browsers = [];
        }

        _ = browsers.Add((int)view.CoreWebView2!.BrowserProcessId);

        // A web view whose session ended or whose screen was covered while it was being started is closed at once: it is never shown a page,
        // and nothing of whoever comes next is ever in it.
        if (closed || released || OfAnEndedSession || !CoverRules.AllowsWeb(host.State, screen))
        {
            CloseNow();
        }
    }

    // The web view's settings and every callback that holds it to the app's origins.
    private async Task HoldAsync(CoreWebView2 web)
    {
        web.Settings.AreHostObjectsAllowed = false;
        web.Settings.IsWebMessageEnabled = false;
        web.Settings.AreDevToolsEnabled = false;
        web.Settings.IsStatusBarEnabled = false;
        web.Settings.IsGeneralAutofillEnabled = false;
        web.Settings.IsPasswordAutosaveEnabled = false;
        web.Settings.AreBrowserAcceleratorKeysEnabled = false;

        // A page of the app's own, in this window; any other address goes to the core, which decides, and the web view stays.
        web.NavigationStarting += (_, args) =>
        {
            // A file of this computer (what a dropped file would be opened as) is not opened, and its path is told to nobody.
            if (WebData.IsLocalFile(args.Uri))
            {
                args.Cancel = true;
                host.ReportProblem("a web page was not sent to a file of this computer");
                return;
            }

            if (!host.Origins.Ours(args.Uri) && !WebData.IsOwnDocument(args.Uri))
            {
                args.Cancel = true;
                host.Send(Events.WebLink(screen, args.Uri));
            }
        };
        web.FrameNavigationStarting += (_, args) =>
        {
            if (!host.Origins.Ours(args.Uri) && !WebData.IsOwnDocument(args.Uri))
            {
                args.Cancel = true;
            }
        };
        web.NewWindowRequested += (_, args) =>
        {
            args.Handled = true;
            if (host.Origins.Ours(args.Uri) || Origins.HandedToSystem(args.Uri))
            {
                host.Send(Events.WebLink(screen, args.Uri));
            }
        };
        web.PermissionRequested += (_, args) => args.State = CoreWebView2PermissionState.Deny;

        // A download a page starts is refused, and the person is told: never saved to the Downloads folder by itself, which is what the web view
        // does with one nobody handles.
        web.DownloadStarting += (_, args) =>
        {
            args.Cancel = true;
            ShowRefusedDownload();
            host.ReportProblem("a download a web page started was refused");
        };

        // The layer that sees what a callback does not: a document that is not the app's own is emptied before anything in it runs, and
        // file inputs, programmatic file pickers, file drops and file pastes are turned off with an explanation in the page. The browser
        // process's own file-chooser dialog interception is turned on beside it as a backstop so no OS file dialog ever opens.
        _ = await web.AddScriptToExecuteOnDocumentCreatedAsync(WebData.DocumentGuardScript(host.Origins.Documents));
        try
        {
            _ = await web.CallDevToolsProtocolMethodAsync("Page.setInterceptFileChooserDialog", "{\"enabled\":true}");
            web.GetDevToolsProtocolEventReceiver("Page.fileChooserOpened").DevToolsProtocolEventReceived += (_, _) =>
            {
                ShowRefusedFilePicker();
                host.ReportProblem("a file picker a web page asked for was refused");
            };
        }
        catch (Exception problem) when (problem is not OutOfMemoryException)
        {
            // Runtimes that refuse the DevTools command remain guarded by the document script and drop handlers.
        }

        // Every request is asked of the app first, whichever part of the page made it: the page, its frames, and its workers of every kind (a
        // service worker or a shared worker is raised only through this overload, with all its source kinds). A runtime without the overload shows
        // no page (WebViewTooOld). What is not the app's own is refused, nothing goes out while a sign-out runs, and in the demo what is recorded is
        // answered. A WebSocket is not a request the web view asks about: the server's connect-src is what holds it.
        try
        {
            web.AddWebResourceRequestedFilter("*", CoreWebView2WebResourceContext.All, CoreWebView2WebResourceRequestSourceKinds.All);
        }
        catch (Exception problem) when (problem is not OutOfMemoryException)
        {
            throw new WebViewTooOldException(problem);
        }

        web.WebResourceRequested += (sender, args) =>
        {
            string uri = args.Request.Uri;
            bool covered = closed || released || !CoverRules.AllowsWeb(host.State, screen);
            switch (WebData.Answer(uri, host.Origins.Ours, host.Wiping, covered))
            {
                case WebAnswer.AsItIs:
                    return;
                case WebAnswer.Refused:
                    args.Response = web.Environment.CreateWebResourceResponse(null, 403, "Blocked", string.Empty);
                    return;
                default:
                    break;
            }

            if (recorded is not null && Uri.TryCreate(uri, UriKind.Absolute, out Uri? address) && recorded(address) is (byte[] body, string type))
            {
                InMemoryRandomAccessStream stream = new();
                using (DataWriter writer = new(stream))
                {
                    writer.WriteBytes(body);
                    _ = writer.StoreAsync().AsTask().GetAwaiter().GetResult();
                    writer.DetachStream();
                }

                stream.Seek(0);
                args.Response = web.Environment.CreateWebResourceResponse(stream, 200, "OK", "Content-Type: " + type);
            }
        };
    }

}
