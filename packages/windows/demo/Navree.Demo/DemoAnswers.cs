using System.Text;
using Navree.Platform;
using Navree.Runtime;

namespace Navree.Demo;

/// <summary>
/// What the demo answers by itself: the demo servers have no sign-in, so the answer a server that signs a person out gives
/// (the one the core's own recorded conversation of signing out holds) is given to a request to sign out; and a page of its
/// own for the web view that keeps something in the web view's storage and says what it finds there.
/// </summary>
internal static class DemoAnswers
{
    /// <summary>The reply of a server that signs the person out: the session ends and a message says so.</summary>
    internal const string SignedOut = """{"navree":1,"build":"152bd84abf4d","vocab":0,"ops":[{"op":"signed_out"},{"op":"toast","text":"You are signed out"}]}""";

    /// <summary>The answer to a request to sign out, and none to any other request.</summary>
    /// <param name="effect">The request.</param>
    /// <returns>The reply, or null.</returns>
    internal static FetchResult? SignOut(FetchEffect effect) =>
        effect.Method == "post" && new Uri(effect.Url).AbsolutePath.EndsWith("/sign-out", StringComparison.Ordinal)
            ? new FetchResult(200, "application/vnd.navree.native+json", null, [], SignedOut, null)
            : null;
}

/// <summary>
/// A web page that writes a cookie and a stored value the first time it is loaded and, each time it is loaded, tells the app
/// what the web view held when it started: so what signing out leaves in the web view is read from the web view itself.
/// </summary>
internal static class WipeProbe
{
    private const string Name = "navree_probe";

    private static readonly byte[] Pixel = Convert.FromBase64String("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7");
    private static readonly List<(string Cookie, string Stored)> Told = [];

    private static readonly string Page =
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Probe</title></head><body>Probe"
        + "<script>"
        + "var had={c:document.cookie,l:localStorage.getItem('" + Name + "')||''};"
        + "if(!/" + Name + "=1/.test(document.cookie)){document.cookie='" + Name + "=1; max-age=3600; path=/';}"
        + "localStorage.setItem('" + Name + "','1');"
        + "new Image().src='/probe-report?c='+encodeURIComponent(had.c)+'&l='+encodeURIComponent(had.l)+'&t='+Date.now();"
        + "</script></body></html>";

    /// <summary>How many times a loaded page has said what it held.</summary>
    internal static int Reports
    {
        get
        {
            lock (Told)
            {
                return Told.Count;
            }
        }
    }

    /// <summary>What the page said the nth time (from 0) it was loaded: what its cookies and its stored value held when it began.</summary>
    /// <param name="at">The place in the order.</param>
    /// <returns>The cookies and the stored value as the page found them.</returns>
    internal static (string Cookie, string Stored) Said(int at)
    {
        lock (Told)
        {
            return Told[at];
        }
    }

    /// <summary>What a request of the web view is answered with: the probe's page for the app's front page, its pixel for its report.</summary>
    /// <param name="address">The address asked for.</param>
    /// <returns>The body and its type, or none for any other address.</returns>
    internal static (byte[] Body, string ContentType)? Handle(Uri address)
    {
        if (address.AbsolutePath == "/probe-report")
        {
            System.Collections.Specialized.NameValueCollection query = System.Web.HttpUtility.ParseQueryString(address.Query);
            lock (Told)
            {
                Told.Add((query["c"] ?? string.Empty, query["l"] ?? string.Empty));
            }

            return (Pixel, "image/gif");
        }

        return address.AbsolutePath == "/" ? (Encoding.UTF8.GetBytes(Page), "text/html; charset=utf-8") : null;
    }
}
