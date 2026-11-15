using System.Net;
using System.Net.Http.Headers;
using System.Text;
using Navree.Design;
using Navree.Runtime;

namespace Navree.Platform;

/// <summary>What came of a request: an answer of any status, or the reason none came.</summary>
/// <param name="Status">The answer's status, or 0 when none came.</param>
/// <param name="ContentType">Its content type.</param>
/// <param name="Location">Where a redirect leads, as it came.</param>
/// <param name="Headers">The headers handed to the core.</param>
/// <param name="Body">The body as text.</param>
/// <param name="Error">When no answer came: <c>offline</c>, <c>timeout</c> or <c>other</c>.</param>
public sealed record FetchResult(
    int Status,
    string? ContentType,
    string? Location,
    IReadOnlyList<KeyValuePair<string, string>> Headers,
    string Body,
    string? Error);

/// <summary>Carries out the core's <c>fetch</c> effects.</summary>
public interface IFetcher
{
    /// <summary>Sends the request and answers what came of it.</summary>
    /// <param name="effect">The effect.</param>
    /// <param name="cancel">Cancelled when the answer is no longer wanted.</param>
    /// <returns>What came of it. It never throws for a request that failed.</returns>
    Task<FetchResult> FetchAsync(FetchEffect effect, CancellationToken cancel);
}

/// <summary>
/// Carries out <c>fetch</c> effects over HTTP: exactly the headers the core gives (those HTTP can carry), the
/// cookie store's cookies, no redirect followed (a 3xx goes back to the core as it came, after its cookies are
/// kept), and the answer handed back whatever its status, with the <c>x-navree-*</c> headers and <c>cache-control</c>.
/// An address that is not the app's own is never asked.
/// </summary>
public sealed class HttpFetcher : IFetcher, IDisposable
{
    /// <summary>The most bytes the body of one answer may have: 8 MiB. A longer body is answered as a failed request.</summary>
    public const long MostBodyBytes = 8L * 1024 * 1024;

    private readonly Origins origins;
    private readonly CookieStore cookies;
    private readonly HttpClient client;
    private readonly Func<string, Stream?> files;
    private readonly Func<DateTimeOffset> clock;

    /// <summary>Makes a fetcher for an app.</summary>
    /// <param name="origins">The app's origins.</param>
    /// <param name="cookies">The cookies of the app's origins.</param>
    /// <param name="handler">The handler that makes the connections. The default follows no redirect, keeps no cookies of its own, and speaks TLS 1.2 or 1.3.</param>
    /// <param name="files">The bytes of a file the person picked, by the token the client keeps it under.</param>
    /// <param name="clock">The time, as the cookie store asks for it.</param>
    public HttpFetcher(Origins origins, CookieStore cookies, HttpMessageHandler? handler = null, Func<string, Stream?>? files = null, Func<DateTimeOffset>? clock = null)
    {
        ArgumentNullException.ThrowIfNull(origins);
        ArgumentNullException.ThrowIfNull(cookies);
        this.origins = origins;
        this.cookies = cookies;
        this.files = files ?? (static _ => null);
        this.clock = clock ?? (static () => DateTimeOffset.UtcNow);
        client = new HttpClient(handler ?? Connections(), disposeHandler: true) { Timeout = Timeout.InfiniteTimeSpan };
    }

    /// <summary>The connections the client makes: no redirect followed, no cookie kept by the handler, TLS 1.2 or 1.3 only.</summary>
    /// <returns>A handler.</returns>
    public static SocketsHttpHandler Connections() => new()
    {
        AllowAutoRedirect = false,
        UseCookies = false,
        UseProxy = true,
        AutomaticDecompression = DecompressionMethods.All,
        ConnectTimeout = TimeSpan.FromSeconds(15),
        PooledConnectionLifetime = TimeSpan.FromMinutes(5),
        SslOptions = new System.Net.Security.SslClientAuthenticationOptions
        {
            EnabledSslProtocols = System.Security.Authentication.SslProtocols.Tls12 | System.Security.Authentication.SslProtocols.Tls13,
        },
    };

    /// <inheritdoc />
    public async Task<FetchResult> FetchAsync(FetchEffect effect, CancellationToken cancel)
    {
        ArgumentNullException.ThrowIfNull(effect);
        using CancellationTokenSource timeout = CancellationTokenSource.CreateLinkedTokenSource(cancel);
        timeout.CancelAfter(TimeSpan.FromMilliseconds(Math.Clamp(effect.TimeoutMs, 1000, 120_000)));
        try
        {
            using HttpRequestMessage? request = Build(effect);
            if (request is null)
            {
                return Failed("other");
            }

            using HttpResponseMessage response = await client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, timeout.Token).ConfigureAwait(false);
            if (response.Headers.TryGetValues("Set-Cookie", out IEnumerable<string>? setCookie))
            {
                cookies.Store(effect.Url, setCookie, clock());
            }

            byte[]? body = await ReadCappedAsync(response.Content, timeout.Token).ConfigureAwait(false);
            if (body is null)
            {
                return Failed("other");
            }

            int status = (int)response.StatusCode;
            bool redirect = status is >= 300 and < 400;
            IEnumerable<KeyValuePair<string, IEnumerable<string>>> all = response.Headers.Concat(response.Content.Headers);
            return new FetchResult(
                status,
                response.Content.Headers.ContentType?.ToString(),
                redirect ? response.Headers.Location?.OriginalString : null,
                HeaderRules.Passed(all),
                Encoding.UTF8.GetString(body),
                null);
        }
        catch (OperationCanceledException) when (cancel.IsCancellationRequested)
        {
            return Failed("other");
        }
        catch (OperationCanceledException)
        {
            return Failed("timeout");
        }
        catch (HttpRequestException)
        {
            return Failed("offline");
        }
        catch (IOException)
        {
            return Failed("offline");
        }
        catch (InvalidOperationException)
        {
            return Failed("other");
        }
    }

    /// <inheritdoc />
    public void Dispose() => client.Dispose();

    /// <summary>
    /// The request a <c>fetch</c> effect asks for, with the store's cookies as they are now, or null when it cannot
    /// be sent: an address that is not one of the app's origins, a header HTTP cannot carry, a body of a kind that
    /// the method cannot have.
    /// </summary>
    /// <param name="effect">The effect.</param>
    /// <returns>The request, or null.</returns>
    public HttpRequestMessage? Build(FetchEffect effect)
    {
        ArgumentNullException.ThrowIfNull(effect);
        if (!origins.Ours(effect.Url))
        {
            return null;
        }

        HttpMethod? method = effect.Method.ToUpperInvariant() switch
        {
            "GET" => HttpMethod.Get,
            "POST" => HttpMethod.Post,
            "DELETE" => HttpMethod.Delete,
            _ => null,
        };
        if (method is null)
        {
            return null;
        }

        HttpRequestMessage request = new(method, effect.Url);
        string? type = null;
        foreach (KeyValuePair<string, string> header in effect.Headers)
        {
            if (!HeaderRules.CanCarry(header.Key, header.Value))
            {
                request.Dispose();
                return null;
            }

            if (header.Key.Equals("content-type", StringComparison.OrdinalIgnoreCase))
            {
                type = header.Value;
            }
            else if (!header.Key.Equals("cookie", StringComparison.OrdinalIgnoreCase)
                && !header.Key.Equals("host", StringComparison.OrdinalIgnoreCase)
                && !header.Key.Equals("content-length", StringComparison.OrdinalIgnoreCase))
            {
                request.Headers.TryAddWithoutValidation(header.Key, header.Value);
            }
        }

        DateTimeOffset now = clock();
        if (cookies.Header(effect.Url, now) is string cookie)
        {
            request.Headers.TryAddWithoutValidation("Cookie", cookie);
        }

        if (effect.Xsrf && cookies.XsrfToken(effect.Url, now) is string token && HeaderRules.CanCarry("x-xsrf-token", token))
        {
            request.Headers.TryAddWithoutValidation("x-xsrf-token", token);
        }

        switch (effect.Body)
        {
            case FormBody form:
                request.Content = Text(form.Text, type);
                break;
            case JsonBody json:
                request.Content = Text(json.Text, type);
                break;
            case MultipartBody multipart:
                MultipartFormDataContent content = new();
                foreach (BodyPart part in multipart.Parts)
                {
                    switch (part)
                    {
                        case TextPart text:
                            content.Add(new StringContent(text.Value, Encoding.UTF8), text.Name);
                            break;
                        case FilePart file when files(file.Token) is Stream stream:
                            StreamContent bytes = new(stream);
                            bytes.Headers.ContentType = MediaTypeHeaderValue.TryParse(file.MediaType, out MediaTypeHeaderValue? media) ? media : null;
                            content.Add(bytes, file.Name, file.Filename);
                            break;
                        default:
                            break;
                    }
                }

                request.Content = content;
                break;
            case null when method != HttpMethod.Get:
                request.Content = Text(string.Empty, type);
                break;
            default:
                break;
        }

        return request;
    }

    private static ByteArrayContent Text(string text, string? type)
    {
        ByteArrayContent content = new(Encoding.UTF8.GetBytes(text));
        if (type is not null && MediaTypeHeaderValue.TryParse(type, out MediaTypeHeaderValue? media))
        {
            content.Headers.ContentType = media;
        }

        return content;
    }

    // Reads only as far as one byte past the limit, so a body that never ends costs no more.
    private static async Task<byte[]?> ReadCappedAsync(HttpContent content, CancellationToken cancel)
    {
        await using Stream stream = await content.ReadAsStreamAsync(cancel).ConfigureAwait(false);
        using MemoryStream bytes = new();
        byte[] buffer = new byte[16 * 1024];
        while (true)
        {
            int read = await stream.ReadAsync(buffer, cancel).ConfigureAwait(false);
            if (read == 0)
            {
                return bytes.ToArray();
            }

            if (bytes.Length + read > MostBodyBytes)
            {
                return null;
            }

            bytes.Write(buffer, 0, read);
        }
    }

    private static FetchResult Failed(string error) =>
        new(0, null, null, [], string.Empty, error);
}
