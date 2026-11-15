using System.Text;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.Web.WebView2.Core;
using Navree.Design;
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
    private static CoreWebView2Environment? environment;
    private static readonly List<WebPage> Live = [];

    private readonly ShellHost host;
    private readonly long screen;
    private readonly WebView2 view = new();
    private readonly Func<Uri, (byte[] Body, string ContentType)?>? recorded;
    private string? loaded;
    private long loadedSerial = -1;

    internal WebPage(ShellHost host, long screen, Func<Uri, (byte[] Body, string ContentType)?>? recorded)
    {
        this.host = host;
        this.screen = screen;
        this.recorded = recorded;
        Children.Add(view);
        Live.Add(this);
        Unloaded += (_, _) => Live.Remove(this);
    }

    /// <summary>Empties what every web view of the app keeps: its cookies, storage, cache and history.</summary>
    internal static async Task ClearAllAsync()
    {
        foreach (WebPage page in Live.ToArray())
        {
            await page.ClearAsync();
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
        try
        {
            if (view.CoreWebView2 is null)
            {
                environment ??= await CoreWebView2Environment.CreateWithOptionsAsync(
                    null,
                    Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Navree", host.Options.AppName, "web"),
                    new CoreWebView2EnvironmentOptions());
                await view.EnsureCoreWebView2Async(environment);
                Hold(view.CoreWebView2!);
            }

            if (!host.Origins.Ours(web.Url))
            {
                return;
            }

            view.CoreWebView2.Navigate(web.Url);
        }
        catch (Exception problem)
        {
            Children.Clear();
            Children.Add(Look.Label("The web view is not available on this device (" + problem.GetType().Name + ").", muted: true));
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
            if (!host.Origins.Ours(args.Uri) && !IsInternal(args.Uri))
            {
                args.Cancel = true;
                host.Send(Events.WebLink(screen, args.Uri));
            }
        };
        web.FrameNavigationStarting += (_, args) =>
        {
            if (!host.Origins.Ours(args.Uri) && !IsInternal(args.Uri))
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
            if (IsInternal(uri))
            {
                return;
            }

            if (!host.Origins.Ours(uri))
            {
                args.Response = environment!.CreateWebResourceResponse(null, 403, "Blocked", string.Empty);
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
                args.Response = environment!.CreateWebResourceResponse(stream, 200, "OK", "Content-Type: " + type);
            }
        };
    }

    private static bool IsInternal(string uri) =>
        uri.StartsWith("about:", StringComparison.OrdinalIgnoreCase)
        || uri.StartsWith("data:", StringComparison.OrdinalIgnoreCase)
        || uri.StartsWith("blob:", StringComparison.OrdinalIgnoreCase);

    /// <summary>Empties what the web view keeps of the app: its cookies, storage, cache and history.</summary>
    internal async Task ClearAsync()
    {
        if (view.CoreWebView2 is not null)
        {
            await view.CoreWebView2.Profile.ClearBrowsingDataAsync();
        }
    }
}
