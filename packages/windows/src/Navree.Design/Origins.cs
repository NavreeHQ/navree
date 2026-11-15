using System.Globalization;

namespace Navree.Design;

/// <summary>
/// The app's origins: the one it was built for and the others its configuration allows. The client asks
/// <see cref="Ours"/> of every address it is about to fetch, stream, show in the web view or load a
/// picture from, whatever the core said of it: the answer is the platform's own, read from the whole
/// address as it will be sent.
/// </summary>
/// <remarks>
/// An origin of <c>also</c> whose host starts with <c>*.</c> stands for every host under that name
/// (<c>*.shop.example</c> covers <c>patiala.shop.example</c>), never for a name that merely ends in the
/// same letters. Plain <c>http</c> is an origin only for the machine itself, and only when the build says so.
/// </remarks>
/// <example>
/// <code>
/// Origins origins = new("https://shop.example");
/// bool yes = origins.Ours("https://shop.example/orders");        // true
/// bool no = origins.Ours("https://shop.example@evil.example/");  // false
/// </code>
/// </example>
public sealed class Origins
{
    private static readonly string[] Loopback = ["localhost", "127.0.0.1", "[::1]"];

    private readonly Site? home;
    private readonly List<Site> others = [];
    private readonly bool loopbackHttp;
    private readonly HashSet<string> loopbackHosts;

    /// <summary>Reads the origins of a configuration.</summary>
    /// <param name="origin">The origin the app was built for, such as <c>https://shop.example</c>.</param>
    /// <param name="also">The other origins the app treats as its own; a host may start with <c>*.</c>.</param>
    /// <param name="loopbackHttp">Whether plain <c>http</c> is allowed to the machine itself: a development build only.</param>
    /// <param name="loopbackHosts">The names of the machine itself, when they are more than <c>localhost</c>, <c>127.0.0.1</c> and <c>[::1]</c>.</param>
    public Origins(string origin, IEnumerable<string>? also = null, bool loopbackHttp = false, IEnumerable<string>? loopbackHosts = null)
    {
        ArgumentNullException.ThrowIfNull(origin);
        this.loopbackHttp = loopbackHttp;
        this.loopbackHosts = new HashSet<string>(Loopback, StringComparer.Ordinal);
        if (loopbackHosts is not null)
        {
            this.loopbackHosts.UnionWith(loopbackHosts);
        }

        Site? read = Site.Read(origin, wildcard: false);
        home = read is not null && Allowed(read) && read.Bare ? read : null;
        List<string> refused = [];
        foreach (string text in also ?? [])
        {
            Site? other = Site.Read(text, wildcard: true);
            if (other is not null && Allowed(other) && other.Bare)
            {
                others.Add(other);
            }
            else
            {
                refused.Add(text);
            }
        }

        Refused = refused;
    }

    /// <summary>The entries of <c>also</c> that do not read (a pattern with too few labels, plain <c>http</c> elsewhere than the machine itself, a path), dropped.</summary>
    public IReadOnlyList<string> Refused { get; }

    /// <summary>Whether the configured origin can be used at all.</summary>
    public bool Usable => home is not null;

    /// <summary>The configured origin, as scheme, host and port, when it can be used.</summary>
    public string? Home => home?.Text;

    /// <summary>The origins a document may have: the app's own and those it allows (a host may start with <c>*.</c>), each as scheme, host and port.</summary>
    public IReadOnlyList<string> Documents => [.. (home is null ? Array.Empty<string>() : [home.Text]), .. others.Select(other => other.Text)];

    /// <summary>
    /// Whether this whole address is the app's own: an <c>http</c> or <c>https</c> address with no white space,
    /// control character or backslash anywhere, a host of ASCII letters, digits, <c>-</c> and <c>.</c> (or a
    /// bracketed IPv6 literal), no user name, a path that does not begin with <c>//</c>, which the platform's
    /// own parser reads the same way, and the scheme, host and port of the app's origin or of one it allows.
    /// </summary>
    /// <param name="address">The whole address.</param>
    /// <returns>Whether it is ours.</returns>
    public bool Ours(string address)
    {
        if (home is null)
        {
            return false;
        }

        Site? site = Site.Read(address, wildcard: false);
        if (site is null || (site.Scheme == "http" && !loopbackHosts.Contains(site.Host)))
        {
            return false;
        }

        // The platform's own parser is what connects, so it must read the address as the strict reading does.
        if (!Uri.TryCreate(address, UriKind.Absolute, out Uri? parsed)
            || parsed.Scheme != site.Scheme
            || parsed.UserInfo.Length != 0
            || !string.Equals(parsed.Host, site.Host, StringComparison.OrdinalIgnoreCase)
            || parsed.Port != site.Port)
        {
            return false;
        }

        return site.SameOrigin(home) || others.Any(other => other.Covers(site));
    }

    /// <summary>
    /// Whether an address the core hands out may be given to the system to open: <c>http</c>, <c>https</c>,
    /// <c>mailto</c>, <c>tel</c>, <c>sms</c> or <c>geo</c>, written cleanly, and a web address whose parts read.
    /// </summary>
    /// <param name="address">The address.</param>
    /// <returns>Whether it may be opened outside.</returns>
    public static bool HandedToSystem(string address)
    {
        if (address.Length == 0 || !Site.Clean(address))
        {
            return false;
        }

        string? scheme = Site.SchemeOf(address);
        if (scheme is not ("http" or "https" or "mailto" or "tel" or "sms" or "geo"))
        {
            return false;
        }

        return scheme is not ("http" or "https") || Site.Read(address, wildcard: false) is not null;
    }

    /// <summary>
    /// Whether a link the app was opened with can be the return of the sign-in session that is open: for a
    /// callback that is a whole web address, the same scheme, host, port and path (any query); for one that is
    /// a scheme, or an address of the app's own scheme, that scheme; with no callback, an address of the app's origins.
    /// </summary>
    /// <param name="link">The link.</param>
    /// <param name="callback">The callback the sign-in named.</param>
    /// <returns>Whether the link is the sign-in's return.</returns>
    public bool SignInReturn(string link, string? callback)
    {
        if (link.Length == 0 || !Site.Clean(link))
        {
            return false;
        }

        if (string.IsNullOrEmpty(callback))
        {
            return Ours(link);
        }

        if (!callback.Contains("://", StringComparison.Ordinal))
        {
            return Site.SchemeOf(link) == (Site.SchemeOf(callback) ?? callback.ToLowerInvariant());
        }

        Site? wanted = Site.Read(callback, wildcard: false);
        Site? got = Site.Read(link, wildcard: false);
        return wanted is not null && got is not null && got.SameOrigin(wanted) && Site.PathOf(link) == Site.PathOf(callback);
    }

    private bool Allowed(Site site) =>
        site.Scheme == "https" || (site.Scheme == "http" && loopbackHttp && loopbackHosts.Contains(site.Host));

    /// <summary>The scheme, host and port of a whole web address, read strictly: what does not read is refused, never repaired.</summary>
    private sealed record Site(string Scheme, string Host, int Port, bool Bare)
    {
        internal string Text => Scheme + "://" + Host + (Port == (Scheme == "http" ? 80 : 443) ? string.Empty : ":" + Port.ToString(CultureInfo.InvariantCulture));

        internal bool SameOrigin(Site other) => Scheme == other.Scheme && Host == other.Host && Port == other.Port;

        internal bool Covers(Site other) => Scheme == other.Scheme && Port == other.Port && HostCovers(Host, other.Host);

        internal static Site? Read(string text, bool wildcard)
        {
            if (text.Length == 0 || !Clean(text))
            {
                return null;
            }

            int split = text.IndexOf("://", StringComparison.Ordinal);
            if (split <= 0)
            {
                return null;
            }

            string scheme = text[..split].ToLowerInvariant();
            if (scheme is not ("http" or "https"))
            {
                return null;
            }

            string rest = text[(split + 3)..];
            int end = rest.IndexOfAny(['/', '?', '#']);
            if (end < 0)
            {
                end = rest.Length;
            }

            string authority = rest[..end];
            string tail = rest[end..];
            if (authority.Length == 0 || authority.Contains('@', StringComparison.Ordinal) || tail.StartsWith("//", StringComparison.Ordinal))
            {
                return null;
            }

            string host;
            string? portText;
            if (authority.StartsWith('['))
            {
                int close = authority.IndexOf(']', StringComparison.Ordinal);
                if (close < 0)
                {
                    return null;
                }

                host = authority[..(close + 1)];
                string inner = host[1..^1];
                if (inner.Length == 0 || !inner.All(letter => IsHex(letter) || letter is ':' or '.'))
                {
                    return null;
                }

                string after = authority[(close + 1)..];
                if (after.Length == 0)
                {
                    portText = null;
                }
                else if (after[0] == ':')
                {
                    portText = after[1..];
                }
                else
                {
                    return null;
                }
            }
            else
            {
                int colon = authority.IndexOf(':', StringComparison.Ordinal);
                host = colon < 0 ? authority : authority[..colon];
                portText = colon < 0 ? null : authority[(colon + 1)..];
                if (!HostReads(host.ToLowerInvariant(), wildcard))
                {
                    return null;
                }
            }

            int port;
            if (portText is null)
            {
                port = scheme == "http" ? 80 : 443;
            }
            else
            {
                if (portText.Length is 0 or > 5 || !portText.All(char.IsAsciiDigit)
                    || !int.TryParse(portText, NumberStyles.None, CultureInfo.InvariantCulture, out port)
                    || port is < 1 or > 65535)
                {
                    return null;
                }
            }

            return new Site(scheme, host.ToLowerInvariant(), port, tail.Length == 0 || tail == "/");
        }

        /// <summary>No white space, control or format character and no backslash anywhere.</summary>
        internal static bool Clean(string text) =>
            text.All(letter => !char.IsWhiteSpace(letter)
                && !char.IsControl(letter)
                && letter != '\\'
                && char.GetUnicodeCategory(letter) != UnicodeCategory.Format);

        /// <summary>The scheme an address starts with, in lower case, when it has one.</summary>
        internal static string? SchemeOf(string text)
        {
            int colon = text.IndexOf(':', StringComparison.Ordinal);
            if (colon <= 0)
            {
                return null;
            }

            string scheme = text[..colon];
            bool reads = char.IsAsciiLetter(scheme[0])
                && scheme.Skip(1).All(letter => char.IsAsciiLetterOrDigit(letter) || letter is '+' or '.' or '-');
            return reads ? scheme.ToLowerInvariant() : null;
        }

        /// <summary>The path of a whole address, without its query or fragment; <c>/</c> when it has none.</summary>
        internal static string PathOf(string address)
        {
            string rest = address[(address.IndexOf("://", StringComparison.Ordinal) + 3)..];
            int start = rest.IndexOfAny(['/', '?', '#']);
            if (start < 0 || rest[start] != '/')
            {
                return "/";
            }

            string path = rest[start..];
            int cut = path.IndexOfAny(['?', '#']);
            return cut < 0 ? path : path[..cut];
        }

        /// <summary>A host name of ASCII letters, digits and <c>-</c>, in labels that are not empty; <c>*.</c> in front only where a pattern is allowed.</summary>
        private static bool HostReads(string host, bool wildcard)
        {
            bool pattern = wildcard && host.StartsWith("*.", StringComparison.Ordinal);
            string name = pattern ? host[2..] : host;
            if (name.Length == 0)
            {
                return false;
            }

            string[] labels = name.Split('.');
            // A pattern covers hosts under a name of at least two labels: *.cdn.example, never *.com.
            if (pattern && labels.Length < 2)
            {
                return false;
            }

            return labels.All(label => label.Length > 0 && label.All(letter => letter is (>= 'a' and <= 'z') or (>= '0' and <= '9') or '-'));
        }

        /// <summary>Whether a host, or a <c>*.</c> pattern, covers another host: under the name, never a name that merely ends in its letters.</summary>
        private static bool HostCovers(string pattern, string host)
        {
            if (!pattern.StartsWith("*.", StringComparison.Ordinal))
            {
                return pattern == host;
            }

            string under = pattern[1..];
            return host.EndsWith(under, StringComparison.Ordinal) && host.Length > under.Length;
        }

        private static bool IsHex(char letter) => char.IsAsciiHexDigit(letter);
    }
}
