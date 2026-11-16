using System.Globalization;
using System.Text.Json;
using Navree.Design;

namespace Navree.Platform;

/// <summary>
/// The cookies of the app's origins, and of nothing else: an answer's <c>Set-Cookie</c> is kept only when the
/// address it came from is the app's own, and a request carries cookies only to an address that is the app's own.
/// A cookie is kept for the host that set it unless it names a domain that host is under; <c>Secure</c> ones are sent
/// over <c>https</c> only. Each clear ends a session: the store's <see cref="Session"/> number moves on, and the cookies of an answer to a
/// request made under an earlier number are not kept.
/// </summary>
public sealed class CookieStore
{
    private readonly Origins origins;
    private readonly object gate = new();
    private readonly List<Held> held = [];
    private long session;

    /// <summary>The number of the session the store holds cookies for; it moves on at every <see cref="Clear"/>.</summary>
    public long Session
    {
        get
        {
            lock (gate)
            {
                return session;
            }
        }
    }

    /// <summary>Makes an empty store for an app.</summary>
    /// <param name="origins">The app's origins.</param>
    public CookieStore(Origins origins)
    {
        ArgumentNullException.ThrowIfNull(origins);
        this.origins = origins;
    }

    /// <summary>How many cookies the store holds.</summary>
    public int Count
    {
        get
        {
            lock (gate)
            {
                return held.Count;
            }
        }
    }

    /// <summary>Keeps the cookies an answer set, when the address it came from is the app's own.</summary>
    /// <param name="url">The whole address the answer came from.</param>
    /// <param name="setCookie">The values of its <c>Set-Cookie</c> headers.</param>
    /// <param name="now">The time now.</param>
    public void Store(string url, IEnumerable<string> setCookie, DateTimeOffset now) => Store(url, setCookie, now, null);

    /// <summary>
    /// Keeps the cookies an answer set, when the address it came from is the app's own and the session its request was made under is
    /// still the store's: an answer that arrives after the store was cleared sets nothing.
    /// </summary>
    /// <param name="url">The whole address the answer came from.</param>
    /// <param name="setCookie">The values of its <c>Set-Cookie</c> headers.</param>
    /// <param name="now">The time now.</param>
    /// <param name="madeUnder">The <see cref="Session"/> the request was made under; null for the one now.</param>
    public void Store(string url, IEnumerable<string> setCookie, DateTimeOffset now, long? madeUnder)
    {
        ArgumentNullException.ThrowIfNull(setCookie);
        if (!origins.Ours(url) || !Uri.TryCreate(url, UriKind.Absolute, out Uri? address))
        {
            return;
        }

        lock (gate)
        {
            if (madeUnder is long made && made != session)
            {
                return;
            }

            foreach (string text in setCookie)
            {
                Held? cookie = Parse(text, address, now);
                if (cookie is null)
                {
                    continue;
                }

                held.RemoveAll(other => other.SameAs(cookie));
                if (cookie.Expires is null || cookie.Expires > now)
                {
                    held.Add(cookie);
                }
            }
        }
    }

    /// <summary>The <c>Cookie</c> header for a request to an address, or null when there is none or the address is not the app's own.</summary>
    /// <param name="url">The whole address.</param>
    /// <param name="now">The time now.</param>
    /// <returns>The header's value.</returns>
    public string? Header(string url, DateTimeOffset now)
    {
        if (!origins.Ours(url) || !Uri.TryCreate(url, UriKind.Absolute, out Uri? address))
        {
            return null;
        }

        lock (gate)
        {
            string[] pairs = [.. held
                .Where(cookie => cookie.Matches(address, now))
                .OrderByDescending(cookie => cookie.Path.Length)
                .Select(cookie => cookie.Name + "=" + cookie.Value)];
            return pairs.Length == 0 ? null : string.Join("; ", pairs);
        }
    }

    /// <summary>The value of the <c>XSRF-TOKEN</c> cookie for an address, percent-decoded: what the <c>x-xsrf-token</c> header carries.</summary>
    /// <param name="url">The whole address.</param>
    /// <param name="now">The time now.</param>
    /// <returns>The token, or null when there is none.</returns>
    public string? XsrfToken(string url, DateTimeOffset now)
    {
        if (!origins.Ours(url) || !Uri.TryCreate(url, UriKind.Absolute, out Uri? address))
        {
            return null;
        }

        lock (gate)
        {
            Held? token = held.FirstOrDefault(cookie => cookie.Name == "XSRF-TOKEN" && cookie.Matches(address, now));
            return token is null ? null : Uri.UnescapeDataString(token.Value);
        }
    }

    /// <summary>Throws away every cookie and ends the session they were for: <see cref="Session"/> moves on.</summary>
    public void Clear()
    {
        lock (gate)
        {
            held.Clear();
            session++;
        }
    }

    /// <summary>Writes every cookie the store holds as JSON, to be kept on the device.</summary>
    /// <returns>JSON text.</returns>
    public string Export() => Export(out _);

    /// <summary>Writes every cookie the store holds as JSON, with the session they are for, read together.</summary>
    /// <param name="forSession">The <see cref="Session"/> the cookies are for.</param>
    /// <returns>JSON text.</returns>
    public string Export(out long forSession)
    {
        lock (gate)
        {
            forSession = session;
            return JsonSerializer.Serialize(
                held.Select(cookie => new string[]
                {
                    cookie.Name,
                    cookie.Value,
                    cookie.Host,
                    cookie.HostOnly ? "1" : "0",
                    cookie.Path,
                    cookie.Secure ? "1" : "0",
                    cookie.Expires?.ToUnixTimeSeconds().ToString(CultureInfo.InvariantCulture) ?? string.Empty,
                }).ToArray());
        }
    }

    /// <summary>Reads what <see cref="Export()"/> wrote.</summary>
    /// <param name="json">The JSON text.</param>
    public void Import(string json)
    {
        string[][]? rows;
        try
        {
            rows = JsonSerializer.Deserialize<string[][]>(json);
        }
        catch (JsonException)
        {
            return;
        }

        lock (gate)
        {
            held.Clear();
            foreach (string[] row in rows ?? [])
            {
                if (row.Length < 7)
                {
                    continue;
                }

                DateTimeOffset? expires = long.TryParse(row[6], NumberStyles.None, CultureInfo.InvariantCulture, out long seconds)
                    ? DateTimeOffset.FromUnixTimeSeconds(seconds)
                    : null;
                held.Add(new Held(row[0], row[1], row[2], row[3] == "1", row[4], row[5] == "1", expires));
            }
        }
    }

    private static Held? Parse(string text, Uri from, DateTimeOffset now)
    {
        string[] parts = text.Split(';');
        int equals = parts[0].IndexOf('=', StringComparison.Ordinal);
        if (equals <= 0)
        {
            return null;
        }

        string name = parts[0][..equals].Trim();
        string value = parts[0][(equals + 1)..].Trim();
        if (name.Length == 0 || name.Any(letter => char.IsControl(letter) || letter is ' ' or ';' or ',' or '='))
        {
            return null;
        }

        string host = from.Host.ToLowerInvariant();
        bool hostOnly = true;
        string path = DefaultPath(from);
        bool secure = false;
        DateTimeOffset? expires = null;
        bool maxAge = false;
        foreach (string attribute in parts.Skip(1))
        {
            string[] pair = attribute.Split('=', 2);
            string key = pair[0].Trim().ToLowerInvariant();
            string setting = pair.Length > 1 ? pair[1].Trim() : string.Empty;
            switch (key)
            {
                case "domain":
                    string domain = setting.TrimStart('.').ToLowerInvariant();
                    // A cookie may name only a domain the host that sets it is under.
                    if (domain.Length == 0 || !(host == domain || host.EndsWith("." + domain, StringComparison.Ordinal)))
                    {
                        return null;
                    }

                    host = domain;
                    hostOnly = false;
                    break;
                case "path":
                    if (setting.StartsWith('/'))
                    {
                        path = setting;
                    }

                    break;
                case "secure":
                    secure = true;
                    break;
                case "max-age":
                    if (long.TryParse(setting, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out long seconds))
                    {
                        expires = seconds <= 0 ? DateTimeOffset.MinValue : now.AddSeconds(seconds);
                        maxAge = true;
                    }

                    break;
                case "expires":
                    if (!maxAge && DateTimeOffset.TryParse(setting, CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal, out DateTimeOffset date))
                    {
                        expires = date;
                    }

                    break;
                default:
                    break;
            }
        }

        return new Held(name, value, host, hostOnly, path, secure, expires);
    }

    private static string DefaultPath(Uri from)
    {
        string path = from.AbsolutePath;
        int last = path.LastIndexOf('/');
        return last <= 0 ? "/" : path[..last];
    }

    private sealed record Held(string Name, string Value, string Host, bool HostOnly, string Path, bool Secure, DateTimeOffset? Expires)
    {
        internal bool SameAs(Held other) => Name == other.Name && Host == other.Host && Path == other.Path;

        internal bool Matches(Uri address, DateTimeOffset now)
        {
            if (Expires is DateTimeOffset end && end <= now)
            {
                return false;
            }

            if (Secure && address.Scheme != Uri.UriSchemeHttps)
            {
                return false;
            }

            string host = address.Host.ToLowerInvariant();
            bool hostMatches = HostOnly ? host == Host : host == Host || host.EndsWith("." + Host, StringComparison.Ordinal);
            if (!hostMatches)
            {
                return false;
            }

            string requested = address.AbsolutePath;
            return requested == Path
                || (requested.StartsWith(Path, StringComparison.Ordinal) && (Path.EndsWith('/') || requested[Path.Length] == '/'));
        }
    }
}
