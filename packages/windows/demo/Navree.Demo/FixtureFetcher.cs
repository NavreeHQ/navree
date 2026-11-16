using System.Text;
using System.Text.Json.Nodes;
using Navree.Platform;
using Navree.Runtime;

namespace Navree.Demo;

/// <summary>
/// Answers the core's requests from a folder of recorded answers, so the demo runs with no server. A page's file is
/// named by its address: <c>/orders/1</c> is <c>orders_1.json</c>, <c>/</c> is <c>index.json</c>, a query is added with
/// its <c>=</c> and <c>&amp;</c> written as <c>_</c>. An address with no file is a 404.
/// </summary>
/// <param name="folder">The folder of recorded answers.</param>
/// <param name="withSearch">Whether the orders screen is given a search of its own, which the recorded answer has none of: the title bar's search is shown by it.</param>
internal sealed class FixtureFetcher(string folder, bool withSearch = false) : IFetcher
{
    private static readonly IReadOnlyList<KeyValuePair<string, string>> NoHeaders = [];

    public Task<FetchResult> FetchAsync(FetchEffect effect, CancellationToken cancel)
    {
        string name = NameOf(effect.Url);
        string file = Path.Combine(folder, name);
        if (effect.Method != "get" || !File.Exists(file))
        {
            return Task.FromResult(new FetchResult(404, "text/plain", null, NoHeaders, string.Empty, null));
        }

        string body = File.ReadAllText(file, Encoding.UTF8);
        if (withSearch && name == "orders.json")
        {
            body = WithSearch(body);
        }

        return Task.FromResult(new FetchResult(200, "application/vnd.navree.native+json", null, NoHeaders, body, null));
    }

    /// <summary>The recorded answer with a search on its screen, the way a server that offers one says it.</summary>
    internal static string WithSearch(string body)
    {
        JsonNode? answer = JsonNode.Parse(body);
        if (answer?["screen"] is JsonObject screen)
        {
            screen["search"] = new JsonObject
            {
                ["action"] = "/orders",
                ["name"] = "q",
                ["placeholder"] = "Search orders",
                ["value"] = string.Empty,
            };
        }

        return answer?.ToJsonString() ?? body;
    }

    /// <summary>The name of the file a whole address is recorded in.</summary>
    internal static string NameOf(string url)
    {
        Uri address = new(url);
        string path = address.AbsolutePath.Trim('/');
        string stem = path.Length == 0 ? "index" : path.Replace('/', '_');
        string query = Uri.UnescapeDataString(address.Query.TrimStart('?'));
        if (query.Length > 0)
        {
            stem += "_" + query.Replace('=', '_').Replace('&', '_');
        }

        return stem + ".json";
    }

    /// <summary>The page a web view asks for, when one is recorded: <c>/</c> is <c>index.embed.html</c>.</summary>
    internal static (byte[] Body, string ContentType)? Page(string folder, Uri address)
    {
        string file = Path.Combine(folder, NameOf(address.ToString())[..^".json".Length] + ".embed.html");
        return File.Exists(file) ? (File.ReadAllBytes(file), "text/html; charset=utf-8") : null;
    }

    /// <summary>The folder of recorded answers the repository keeps for the store demo, found above the program.</summary>
    internal static string? Recorded()
    {
        DirectoryInfo? at = new(AppContext.BaseDirectory);
        while (at is not null)
        {
            string candidate = Path.Combine(at.FullName, "packages", "android", "fixtures", "store-demo");
            if (Directory.Exists(candidate))
            {
                return candidate;
            }

            at = at.Parent;
        }

        return null;
    }
}

/// <summary>A network that is not there: every request fails as one does with no connection, so the first load fails.</summary>
internal sealed class OfflineFetcher : IFetcher
{
    public Task<FetchResult> FetchAsync(FetchEffect effect, CancellationToken cancel) =>
        Task.FromResult(new FetchResult(0, null, null, [], string.Empty, "offline"));
}
