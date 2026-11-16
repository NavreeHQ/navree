using System.Security.Cryptography;
using System.Text;
using Navree.Platform;
using Navree.Runtime;

namespace Navree.Demo;

/// <summary>One part of a multipart form the walk's own server received.</summary>
/// <param name="Name">The part's field name.</param>
/// <param name="FileName">The file's name, or null for a part that is not a file.</param>
/// <param name="MediaType">The part's content type, or null.</param>
/// <param name="Length">How many bytes its content had.</param>
/// <param name="Sha256">The hash of its content, in hex.</param>
/// <param name="Content">Its content.</param>
internal sealed record ReceivedPart(string Name, string? FileName, string? MediaType, int Length, string Sha256, byte[] Content)
{
    /// <summary>The content read as text, for a part that is not a file.</summary>
    internal string Text => System.Text.Encoding.UTF8.GetString(Content);
}

/// <summary>
/// Answers the core's requests for the page walk when no server is behind it: the protocol's own shell, one screen of the walk's own that holds
/// a disclosure and a field, which no demo server draws on a screen (the boards' disclosures are details of cards, which are not drawn), and the
/// protocol's own form example (the one with a colour field and a file field) with a server for it that reads what the form sends and says what it
/// received, by hash. A form is written to the request by the client's own code, with the files read from where the client keeps them.
/// </summary>
internal sealed class PageFetcher(string corpus) : IFetcher
{
    private const string Screen = """
        {"navree":1,"build":"3f9a1c07b2de","vocab":0,"screen":{"title":"An order","body":{"t":"stack","gap":3,"children":[
          {"t":"text","value":"Order CS-220480 for Harpreet Kaur, Ludhiana."},
          {"t":"disclosure","id":"more","label":"More about this order","children":[{"t":"text","value":"Placed on a Tuesday, to be sent by road to Ludhiana, Punjab."}]},
          {"t":"form","id":"note","action":"/note","children":[{"t":"field","name":"note","id":"note-field","label":"A note","input":{"t":"text","value":""}}]},
          {"t":"text","value":"Rs 1,00,000 in all."},
          {"t":"button","label":"New customer","action":{"t":"navigate","href":"/customers/new"}},
          {"t":"button","label":"A slow order","action":{"t":"navigate","href":"/orders/slow","present":"sheet"}},
          {"t":"button","label":"An order beside","action":{"t":"navigate","href":"/orders/beside"}},
          {"t":"button","label":"A ledger","action":{"t":"navigate","href":"/ledger"}}]}}}
        """;

    // A table whose query form has no id, so it is known by its address; the first answer puts it at /ledger with an empty search, every later one
    // at /ledger/paid (a filter carried in the address) with the search the server says it holds.
    private const string Ledger = """
        {"navree":1,"build":"3f9a1c07b2de","vocab":0,"screen":{"title":"Ledger","body":{"t":"region","id":"ledger-region","src":"/ledger","children":[{"t":"table","id":"ledger","caption":"Ledger",
          "columns":[{"key":"name","label":"Name","role":"title"},{"key":"amount","label":"Amount","numeric":true,"align":"end","sort":{"href":"/ledger?sort=amount"}}],
          "rows":[{"id":"1","cells":[{"t":"text","value":"Harpreet Kaur"},{"t":"text","value":"Rs 1,00,000"}]},{"id":"2","cells":[{"t":"text","value":"Gurpreet Singh"},{"t":"text","value":"Rs 24,500"}]},{"id":"3","cells":[{"t":"text","value":"Jaspreet Kaur"},{"t":"text","value":"Rs 8,200"}]}],
          "query":{"action":"ACTION","children":[{"t":"field","name":"search","id":"ledger-search","label":"Search","input":{"t":"text","kind":"search","placeholder":"Search the ledger","value":"VALUE"},"purpose":"search","changed":{"t":"submit"}}]}}]}}}
        """;

    /// <summary>What the ledger's search holds in every answer after the first.</summary>
    internal const string LedgerSearch = "Harpreet Kaur";

    private int ledgerAsked;

    private static readonly Navree.Design.Origins Origin = new("https://charkha.example", [], false);

    private const string Slow = """
        {"navree":1,"build":"3f9a1c07b2de","vocab":0,"screen":{"title":"Order","body":{"t":"stack","gap":3,"children":[
          {"t":"text","value":"Order CS-220481 for Gurpreet Singh, Amritsar."},
          {"t":"text","value":"Rs 24,500 in all, to be sent by road."}]}}}
        """;

    // A page that opens a sheet with nothing in it, as a server does that fills the sheet with the answer that follows; the second time it is asked for the sheet holds the order.
    private const string BesideEmpty = """
        {"navree":1,"build":"3f9a1c07b2de","vocab":0,"screen":{"title":"Orders","body":{"t":"stack","gap":3,"children":[
          {"t":"text","value":"Orders of Charkha Supply Co."},
          {"t":"sheet","id":"beside","title":"Order","open":true,"size":"large"}]}}}
        """;

    private const string BesideFilled = """
        {"navree":1,"build":"3f9a1c07b2de","vocab":0,"screen":{"title":"Orders","body":{"t":"stack","gap":3,"children":[
          {"t":"text","value":"Orders of Charkha Supply Co."},
          {"t":"sheet","id":"beside","title":"Order","open":true,"size":"large","children":[{"t":"text","value":"Order CS-220482 for Jaspreet Kaur, Patiala."}]}]}}}
        """;

    private int besideAsked;

    private HttpFetcher? writer;

    /// <summary>How long the walk's slow order takes to come, so a pane that is waiting for it can be looked at.</summary>
    internal TimeSpan SlowOrder { get; set; } = TimeSpan.FromSeconds(3);

    /// <summary>How long the server takes to answer a form, so a walk can read what the page shows while it is on its way.</summary>
    internal TimeSpan Delay { get; set; }

    /// <summary>Where the bytes of a file the person chose are read from, by the token the core was given: the host's registry, once the host exists.</summary>
    internal Func<string, Stream?> Files { get; set; } = static _ => null;

    /// <summary>What the form posts the server received, in the order they came.</summary>
    internal List<IReadOnlyList<ReceivedPart>> Received { get; } = [];

    /// <summary>Every request that came, as its method and address, in order.</summary>
    internal List<string> Requests { get; } = [];

    public async Task<FetchResult> FetchAsync(FetchEffect effect, CancellationToken cancel)
    {
        string path = new Uri(effect.Url).AbsolutePath;
        Requests.Add(effect.Method + " " + effect.Url + (effect.Body is null ? string.Empty : " with a " + effect.Body.GetType().Name));
        if (effect.Method.Equals("post", StringComparison.OrdinalIgnoreCase) && path == "/customers")
        {
            return await ReceiveAsync(effect);
        }

        if (path == "/orders/beside")
        {
            besideAsked++;
            return new FetchResult(200, "application/vnd.navree.native+json", null, [], besideAsked == 1 ? BesideEmpty : BesideFilled, null);
        }

        if (path is "/ledger" or "/ledger/paid")
        {
            ledgerAsked++;
            string page = ledgerAsked == 1
                ? Ledger.Replace("ACTION", "/ledger", StringComparison.Ordinal).Replace("VALUE", string.Empty, StringComparison.Ordinal)
                : Ledger.Replace("ACTION", "/ledger/paid", StringComparison.Ordinal).Replace("VALUE", LedgerSearch, StringComparison.Ordinal);
            return new FetchResult(200, "application/vnd.navree.native+json", null, [], page, null);
        }

        if (path == "/orders/slow")
        {
            await Task.Delay(SlowOrder, cancel);
            return new FetchResult(200, "application/vnd.navree.native+json", null, [], Slow, null);
        }

        string? body = path.EndsWith("/_navree/native/shell", StringComparison.Ordinal) ? File.ReadAllText(Path.Combine(corpus, "shell.json"))
            : path is "/" or "/orders" ? Screen
            : path == "/customers/new" ? File.ReadAllText(Path.Combine(corpus, "screen-form.json"))
            : null;
        return effect.Method == "get" && body is not null
            ? new FetchResult(200, "application/vnd.navree.native+json", null, [], body, null)
            : new FetchResult(404, "text/plain", null, [], string.Empty, null);
    }

    // Writes the request as the client would send it, reads it back as a server does, and answers with the words a server gives when it refuses a form.
    private async Task<FetchResult> ReceiveAsync(FetchEffect effect)
    {
        writer ??= new HttpFetcher(Origin, new CookieStore(Origin), files: token => Files(token));
        using HttpRequestMessage? request = writer.Build(effect);
        if (request?.Content is not { } content)
        {
            return new FetchResult(0, null, null, [], string.Empty, "other");
        }

        byte[] bytes = await content.ReadAsByteArrayAsync();
        string boundary = content.Headers.ContentType?.Parameters.FirstOrDefault(parameter => parameter.Name == "boundary")?.Value?.Trim('"') ?? string.Empty;
        List<ReceivedPart> parts = Parts(bytes, boundary);
        Received.Add(parts);
        if (Delay > TimeSpan.Zero)
        {
            await Task.Delay(Delay);
        }

        int files = parts.Count(part => part.FileName is not null);
        string said = files == 0 ? "No paper came with it." : files + (files == 1 ? " paper came" : " papers came") + " with it: " + string.Join(", ", parts.Where(part => part.FileName is not null).Select(part => part.FileName + " " + part.Length + " bytes")) + ".";
        string answer = "{\"navree\":1,\"build\":\"3f9a1c07b2de\",\"vocab\":0,\"ops\":[{\"op\":\"errors\",\"form\":\"customer\",\"fields\":{\"papers\":[" + System.Text.Json.JsonSerializer.Serialize(said) + "]}}]}";
        return new FetchResult(422, "application/vnd.navree.native+json", null, [], answer, null);
    }

    // The parts of a multipart body, read as a server reads them: split at the boundary, headers then content, the content kept as bytes.
    internal static List<ReceivedPart> Parts(byte[] body, string boundary)
    {
        byte[] delimiter = Encoding.ASCII.GetBytes("--" + boundary);
        byte[] blank = "\r\n\r\n"u8.ToArray();
        List<ReceivedPart> parts = [];
        int at = body.AsSpan().IndexOf(delimiter);
        while (at >= 0)
        {
            int start = at + delimiter.Length;
            if (start + 2 > body.Length || (body[start] == '-' && body[start + 1] == '-'))
            {
                break;
            }

            start += 2;
            int next = body.AsSpan(start).IndexOf(delimiter);
            if (next < 0)
            {
                break;
            }

            int end = start + next - 2;
            int split = body.AsSpan(start, end - start).IndexOf(blank);
            string headers = Encoding.UTF8.GetString(body, start, split);
            byte[] content = body[(start + split + blank.Length)..end];
            string? name = Quoted(headers, "name");
            string? file = Quoted(headers, "filename");
            string? type = headers.Split("\r\n").FirstOrDefault(line => line.StartsWith("Content-Type:", StringComparison.OrdinalIgnoreCase))?["Content-Type:".Length..].Trim();
            parts.Add(new ReceivedPart(name ?? string.Empty, file, type, content.Length, Convert.ToHexString(SHA256.HashData(content)).ToLowerInvariant(), content));
            at = start + next;
        }

        return parts;
    }

    // The value of a parameter of the part's headers (name, filename), quoted or not.
    private static string? Quoted(string headers, string key)
    {
        foreach (string marker in new[] { " " + key + "=", ";" + key + "=" })
        {
            int at = headers.IndexOf(marker, StringComparison.Ordinal);
            if (at < 0)
            {
                continue;
            }

            int from = at + marker.Length;
            if (from < headers.Length && headers[from] == '"')
            {
                int to = headers.IndexOf('"', from + 1);
                return to < 0 ? null : headers[(from + 1)..to];
            }

            int stop = headers.IndexOfAny([';', '\r', '\n'], from);
            return stop < 0 ? headers[from..] : headers[from..stop];
        }

        return null;
    }
}
