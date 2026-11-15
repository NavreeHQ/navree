using Navree.Platform;
using Navree.Runtime;

namespace Navree.Demo;

/// <summary>What the core asked the client to send: the method, the whole address and the body as text.</summary>
/// <param name="At">When it was asked.</param>
/// <param name="Method">get, post or delete.</param>
/// <param name="Url">The whole address.</param>
/// <param name="Body">What it carried, as text; empty for none.</param>
internal sealed record SentRequest(DateTime At, string Method, string Url, string Body);

/// <summary>Passes every request on to the fetcher it wraps, and remembers each one as the core asked it.</summary>
internal sealed class RecordingFetcher(IFetcher inner) : IFetcher
{
    private static readonly List<SentRequest> Kept = [];

    /// <summary>Every request so far, oldest first.</summary>
    internal static IReadOnlyList<SentRequest> Sent
    {
        get
        {
            lock (Kept)
            {
                return [.. Kept];
            }
        }
    }

    /// <inheritdoc />
    public async Task<FetchResult> FetchAsync(FetchEffect effect, CancellationToken cancel)
    {
        lock (Kept)
        {
            Kept.Add(new SentRequest(DateTime.Now, effect.Method, effect.Url, BodyText(effect.Body)));
        }

        FetchResult answer = await inner.FetchAsync(effect, cancel);

        // The peek of any order is answered with the one that was recorded, so that pressing an order number shows a peek.
        if (answer.Status == 404 && effect.Method == "get" && RecordedPeek(effect.Url) is { } recorded)
        {
            return await inner.FetchAsync(effect with { Url = recorded }, cancel);
        }

        return answer;
    }

    private static string? RecordedPeek(string url)
    {
        Uri address = new(url);
        string[] parts = address.AbsolutePath.Trim('/').Split('/');
        bool peek = parts.Length == 3 && parts[0] == "orders" && parts[2] == "peek" && parts[1].All(char.IsAsciiDigit);
        return peek && parts[1] != "1" ? new Uri(address, "/orders/1/peek").ToString() : null;
    }

    private static string BodyText(RequestBody? body) => body switch
    {
        FormBody form => form.Text,
        JsonBody json => json.Text,
        MultipartBody multipart => string.Join("&", multipart.Parts.Select(part => part switch
        {
            TextPart text => text.Name + "=" + text.Value,
            FilePart file => file.Name + "=(file " + file.Filename + ")",
            _ => string.Empty,
        })),
        _ => string.Empty,
    };
}
