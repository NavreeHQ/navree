using System.Text;
using System.Text.Json;
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
/// that sees redirects, which a callback does not), a request for anything that is not the app's own is answered blocked,
/// no host object and no message channel is offered, and every permission is refused.
/// </summary>
/// <remarks>
/// What the callbacks cannot see: the web view reports the first address of a navigation, so a redirect inside a frame
/// that the web view does not report as a navigation of its own is caught by the document-start script, not by a
/// callback; and each request is paused while the app decides, which costs speed on a page with many parts.
/// </remarks>
internal sealed class WebPage : Grid
{
    // One environment for each folder the app's web views keep their data in; a page that is made while another is being started waits for the same one.
    private static readonly Dictionary<string, Task<CoreWebView2Environment>> Environments = [];

    // Every page whose web view has been started and not closed, on screen or not: the wipe at sign-out reaches them all, and
    // a page that was shown, left and shown again is one of them.
    private static readonly List<WebPage> Live = [];

    // Pages whose screens left while a wipe was running: they are closed when it is done.
    private static readonly List<WebPage> Deferred = [];
    private static int wiping;

    // The browser processes the web views of this app started, by id: the folder cannot be deleted until they have ended.
    private static readonly HashSet<int> BrowserProcesses = [];

    private readonly ShellHost host;
    private readonly long screen;
    private readonly WebView2 view = new();
    private readonly Func<Uri, (byte[] Body, string ContentType)?>? recorded;
    private string? loaded;
    private long loadedSerial = -1;
    private WebContent? wanted;
    private Task? starting;
    private bool released;
    private bool closed;

    internal WebPage(ShellHost host, long screen, Func<Uri, (byte[] Body, string ContentType)?>? recorded)
    {
        this.host = host;
        this.screen = screen;
        this.recorded = recorded;
        Children.Add(view);
    }

    /// <summary>
    /// Empties what the web views of the app keep: their cookies, storage, cache and history. While a web view is running its
    /// profile is cleared (the web view's own way; its folder is not deleted under it). With none running, which is the case
    /// when the person signs out from a screen that is not a web page, or in a launch that never showed one, the folder the last
    /// launch left is deleted.
    /// </summary>
    /// <param name="folder">The folder the web views keep their data in.</param>
    /// <param name="trace">Told, in words, each step of the wipe and how long it took; when the wipe is slow this says which call it was.</param>
    internal static async Task ClearAllAsync(string folder, Action<string>? trace = null)
    {
        System.Diagnostics.Stopwatch clock = System.Diagnostics.Stopwatch.StartNew();
        void Note(string what) => trace?.Invoke(clock.ElapsedMilliseconds.ToString(System.Globalization.CultureInfo.InvariantCulture) + " ms: " + what);

        // While the wipe runs no web view is closed: a clear that is on its way to the profile never answers when the web view it was asked of
        // is closed under it, which is what happens when the screens go at the same moment (the core starts the app again). The closing waits.
        wiping++;
        try
        {
            await WipeAsync(folder, Note);
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

    private static async Task WipeAsync(string folder, Action<string> note)
    {
        WebPage[] kept = [.. Live, .. Deferred];
        note(kept.Length + " web page(s) are kept for the wipe: " + string.Join(", ", kept.Select(page => "screen " + page.screen + (page.released ? " released" : string.Empty) + (page.view.CoreWebView2 is null ? " not started" : string.Empty))));

        // A web view that is still being started is waited for, for a short while: one that was closed in the middle of it never answers.
        foreach (WebPage page in kept)
        {
            if (page.starting is Task starting && !starting.IsCompleted)
            {
                Task first = await Task.WhenAny(starting, Task.Delay(TimeSpan.FromSeconds(2)));
                note("screen " + page.screen + " was still starting; " + (first == starting ? "it came up" : "it did not in 2 s"));
            }
        }

        WebPage[] started = [.. kept.Where(page => !page.closed && page.view.CoreWebView2 is not null)];

        // A page that still runs could write again into what is about to be cleared: each is stopped and emptied first, and shown again only
        // when its screen asks for it.
        foreach (WebPage page in started)
        {
            try
            {
                page.loaded = null;
                page.view.CoreWebView2!.Stop();
                page.view.CoreWebView2.Navigate("about:blank");
            }
            catch (Exception problem)
            {
                note("screen " + page.screen + ": the page could not be emptied: " + problem.GetType().Name);
            }
        }

        foreach (WebPage page in started)
        {
            try
            {
                Task clearing = page.ClearAsync();
                Task first = await Task.WhenAny(clearing, Task.Delay(TimeSpan.FromSeconds(3)));
                if (first != clearing)
                {
                    note("screen " + page.screen + ": the profile's clear did not answer in 3 s");
                    continue;
                }

                await clearing;
                note("screen " + page.screen + ": the profile was cleared");
                return;
            }
            catch (Exception problem)
            {
                note("screen " + page.screen + ": the clear failed: " + problem.GetType().Name);
            }
        }

        // The profile could not be cleared through a web view: every web view is closed, the browser's processes are waited for, and the
        // folder is deleted.
        foreach (WebPage page in kept)
        {
            page.CloseNow();
        }

        foreach (int id in BrowserProcesses.ToArray())
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

        Environments.Remove(folder);
        bool gone = await WebData.DeleteAsync(folder, TimeSpan.FromSeconds(2));
        note("the folder " + (gone ? "was deleted" : "could not be deleted"));
        if (!gone)
        {
            throw new IOException("the web view's folder could not be deleted");
        }
    }

    /// <summary>Closes the page's web view when its screen has left for good, so the page is no longer kept for the wipe and the process behind it goes.</summary>
    internal void Release()
    {
        released = true;
        Live.Remove(this);

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
        Live.Remove(this);
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
        if (loaded == web.Url && loadedSerial == web.Serial)
        {
            return;
        }

        loaded = web.Url;
        loadedSerial = web.Serial;
        wanted = web;
        try
        {
            // The web view is started and held once, whatever came while it was being started; the newest wish is the one carried out.
            starting ??= StartAsync();
            await starting;
            if (wanted != web || !host.Origins.Ours(web.Url))
            {
                return;
            }

            view.CoreWebView2!.Navigate(web.Url);
        }
        catch (Exception problem)
        {
            Children.Clear();
            Children.Add(Look.Label("The web view is not available on this device (" + problem.GetType().Name + ").", muted: true));
        }
    }

    private async Task StartAsync()
    {
        string folder = host.Options.WebFolder ?? WebData.DefaultFolder(host.Options.AppName);
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

        Hold(view.CoreWebView2!);
        BrowserProcesses.Add((int)view.CoreWebView2!.BrowserProcessId);

        // A page whose screen left while it was being started is not kept: its web view is closed.
        if (!released)
        {
            Live.Add(this);
        }
    }

    // The web view's settings and every callback that holds it to the app's origins.
    private void Hold(CoreWebView2 web)
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

        // The layer that sees what a callback does not: a document that is not the app's own is emptied before anything in it runs.
        string allowed = JsonSerializer.Serialize(host.Origins.Documents);
        _ = web.AddScriptToExecuteOnDocumentCreatedAsync(
            "(function(){var allowed=" + allowed + ";try{var o=location.origin;if(o==='null'||location.protocol==='about:'||location.protocol==='data:'){return;}" +
            "var ok=allowed.some(function(a){var star=a.indexOf('://*.');if(star<0){return a===o;}var scheme=a.slice(0,star+3);var rest=a.slice(star+4);return o.indexOf(scheme)===0&&o.slice(scheme.length).endsWith(rest)&&o.slice(scheme.length).length>rest.length;});" +
            "if(!ok){window.stop();document.documentElement.innerHTML='';}}catch(e){}})();");

        // Every request is asked of the app first: what is not the app's own is blocked, and in the demo what is recorded is answered.
        web.AddWebResourceRequestedFilter("*", CoreWebView2WebResourceContext.All);
        web.WebResourceRequested += (sender, args) =>
        {
            string uri = args.Request.Uri;
            if (WebData.IsOwnRequest(uri))
            {
                return;
            }

            // While a sign-out's steps run, nothing is asked of the network: no answer can put a cookie into what is being cleared.
            if (!host.Origins.Ours(uri) || host.Wiping)
            {
                args.Response = web.Environment.CreateWebResourceResponse(null, 403, "Blocked", string.Empty);
                return;
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

    /// <summary>Empties what the web view keeps of the app: its cookies, storage, cache and history.</summary>
    internal async Task ClearAsync()
    {
        if (view.CoreWebView2 is not null)
        {
            await view.CoreWebView2.Profile.ClearBrowsingDataAsync();
        }
    }
}
