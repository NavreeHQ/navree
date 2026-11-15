using Navree.Platform;
using Navree.Runtime;

namespace Navree.Demo;

/// <summary>
/// Answers the core's requests for the page walk when no server is behind it: the protocol's own shell, and one screen of the walk's own that holds
/// a disclosure and a field, which no demo server draws on a screen (the boards' disclosures are details of cards, which are not drawn).
/// </summary>
internal sealed class PageFetcher(string corpus) : IFetcher
{
    private const string Screen = """
        {"navree":1,"build":"3f9a1c07b2de","vocab":0,"screen":{"title":"An order","body":{"t":"stack","gap":3,"children":[
          {"t":"text","value":"Order CS-220480 for Harpreet Kaur, Ludhiana."},
          {"t":"disclosure","id":"more","label":"More about this order","children":[{"t":"text","value":"Placed on a Tuesday, to be sent by road to Ludhiana, Punjab."}]},
          {"t":"form","id":"note","action":"/note","children":[{"t":"field","name":"note","id":"note-field","label":"A note","input":{"t":"text","value":""}}]},
          {"t":"text","value":"Rs 1,00,000 in all."}]}}}
        """;

    public Task<FetchResult> FetchAsync(FetchEffect effect, CancellationToken cancel)
    {
        string path = new Uri(effect.Url).AbsolutePath;
        string? body = path.EndsWith("/_navree/native/shell", StringComparison.Ordinal) ? File.ReadAllText(Path.Combine(corpus, "shell.json"))
            : path is "/" or "/orders" ? Screen
            : null;
        return Task.FromResult(effect.Method == "get" && body is not null
            ? new FetchResult(200, "application/vnd.navree.native+json", null, [], body, null)
            : new FetchResult(404, "text/plain", null, [], string.Empty, null));
    }
}
