using System.Net;

namespace Navree.Presence;

/// <summary>How a sign-in in the browser comes back to the app.</summary>
public enum ReturnKind
{
    /// <summary>On the app's own scheme, by protocol activation.</summary>
    Scheme,

    /// <summary>On a link of the site: only an installed package that the site names can catch it, which no build here is.</summary>
    Https,

    /// <summary>On a port of the machine itself. It is read so that it can be refused by name: the app listens on no port, which every account on the machine could reach.</summary>
    Loopback,
}

/// <summary>Where a sign-in session is told to come back, read from the core's words.</summary>
/// <param name="Kind">How it comes back.</param>
/// <param name="Scheme">The scheme, for <see cref="ReturnKind.Scheme"/>.</param>
/// <param name="Host">The host, for the other two.</param>
/// <param name="Port">The port of a loopback return, 0 for any.</param>
/// <param name="Path">The path of a link or a loopback return.</param>
public sealed record SignInReturn(ReturnKind Kind, string Scheme, string? Host, int Port, string? Path)
{
    /// <summary>Reads what the core names: a scheme (<c>charkha</c>, <c>charkha:/signed-in</c>, <c>charkha://signed-in</c>), a link of the site, or a loopback address.</summary>
    /// <param name="text">What the core said.</param>
    /// <returns>The return, or null when it is none this build knows.</returns>
    public static SignInReturn? Parse(string? text)
    {
        if (string.IsNullOrWhiteSpace(text) || text.Length > 2048 || text.Any(char.IsControl) || text.Contains(' '))
        {
            return null;
        }

        int colon = text.IndexOf(':');
        string scheme = colon >= 0 ? text[..colon] : text;
        if (!ValidScheme(scheme))
        {
            return null;
        }

        if (scheme.Equals("https", StringComparison.OrdinalIgnoreCase))
        {
            return Uri.TryCreate(text, UriKind.Absolute, out Uri? link) && link.Host.Length > 0 && link.UserInfo.Length == 0
                ? new SignInReturn(ReturnKind.Https, "https", link.Host.ToLowerInvariant(), 0, link.AbsolutePath)
                : null;
        }

        if (scheme.Equals("http", StringComparison.OrdinalIgnoreCase))
        {
            if (Uri.TryCreate(text, UriKind.Absolute, out Uri? local) && IsLoopbackHost(local.Host) && local.UserInfo.Length == 0)
            {
                return new SignInReturn(ReturnKind.Loopback, "http", local.Host, local.IsDefaultPort ? 0 : local.Port, local.AbsolutePath);
            }

            return null;
        }

        return new SignInReturn(ReturnKind.Scheme, scheme.ToLowerInvariant(), null, 0, null);
    }

    /// <summary>Whether this build can catch the return, and if not, why.</summary>
    /// <returns>Null when it can; the reason when it cannot.</returns>
    public string? WhyNot() =>
        Kind == ReturnKind.Https
            ? "a return on a link of the site needs the app to be installed from a package that the site names, which this build is not"
            : null;

    /// <summary>Whether an address is this return: the same scheme, or the same machine, port and path.</summary>
    /// <param name="address">The whole address something came back on.</param>
    /// <returns>Whether it is.</returns>
    public bool Matches(string? address)
    {
        if (string.IsNullOrWhiteSpace(address) || address.Length > 8192 || !Uri.TryCreate(address, UriKind.Absolute, out Uri? back) || back.UserInfo.Length > 0)
        {
            return false;
        }

        return Kind switch
        {
            ReturnKind.Scheme => string.Equals(back.Scheme, Scheme, StringComparison.OrdinalIgnoreCase),
            ReturnKind.Loopback => back.Scheme == "http"
                && IsLoopbackHost(back.Host)
                && (Port == 0 || back.Port == Port)
                && (Path is null or "/" or "" || string.Equals(back.AbsolutePath.TrimEnd('/'), Path.TrimEnd('/'), StringComparison.Ordinal)),
            _ => false,
        };
    }

    private static bool ValidScheme(string scheme) =>
        scheme.Length is > 0 and <= 64
        && char.IsAsciiLetter(scheme[0])
        && scheme.All(letter => char.IsAsciiLetterOrDigit(letter) || letter is '+' or '-' or '.');

    private static bool IsLoopbackHost(string host)
    {
        if (host.Equals("localhost", StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        return IPAddress.TryParse(host.Trim('[', ']'), out IPAddress? address) && IPAddress.IsLoopback(address);
    }
}

/// <summary>What the app does about the system's browser sign-in, as plain rules.</summary>
public static class SignInRules
{
    /// <summary>Whether an address is on a scheme: it starts with the scheme and a colon.</summary>
    /// <param name="address">The address.</param>
    /// <param name="scheme">The scheme.</param>
    /// <returns>Whether it is.</returns>
    public static bool IsOnScheme(string address, string scheme) =>
        address.Length > scheme.Length
        && address.StartsWith(scheme, StringComparison.OrdinalIgnoreCase)
        && address[scheme.Length] == ':';

    /// <summary>Whether an address is one the system may be asked to open to start a sign-in: a whole <c>http</c> or <c>https</c> address with a host.</summary>
    /// <param name="address">The address the core gave.</param>
    /// <returns>Whether it is.</returns>
    public static bool MayStart(string? address) =>
        Uri.TryCreate(address, UriKind.Absolute, out Uri? url)
        && (url.Scheme == Uri.UriSchemeHttps || url.Scheme == Uri.UriSchemeHttp)
        && url.Host.Length > 0
        && url.UserInfo.Length == 0
        && address!.Length <= 8192;

    /// <summary>The name a scheme is registered under: the scheme itself, in lower case.</summary>
    /// <param name="scheme">The scheme.</param>
    /// <returns>The name.</returns>
    public static string RegisteredName(string scheme) => scheme.ToLowerInvariant();

    /// <summary>The value of a parameter in an address's query, read as the core reads one: the first of that name, decoded; nothing after a <c>#</c> is read.</summary>
    /// <param name="address">The whole address.</param>
    /// <param name="name">The parameter's name.</param>
    /// <returns>Its value, or null when the query has none of that name.</returns>
    public static string? QueryValue(string? address, string name)
    {
        ArgumentNullException.ThrowIfNull(name);
        if (address is null)
        {
            return null;
        }

        int mark = address.IndexOf('?', StringComparison.Ordinal);
        if (mark < 0)
        {
            return null;
        }

        string query = address[(mark + 1)..];
        int hash = query.IndexOf('#', StringComparison.Ordinal);
        if (hash >= 0)
        {
            query = query[..hash];
        }

        foreach (string pair in query.Split('&'))
        {
            int equals = pair.IndexOf('=', StringComparison.Ordinal);
            string key = equals < 0 ? pair : pair[..equals];
            if (Decoded(key) == name)
            {
                return equals < 0 ? string.Empty : Decoded(pair[(equals + 1)..]);
            }
        }

        return null;
    }

    /// <summary>Whether two texts are the same, in time that does not say where they differ.</summary>
    /// <param name="one">One text.</param>
    /// <param name="other">The other.</param>
    /// <returns>Whether they are the same.</returns>
    public static bool Same(string? one, string? other) =>
        one is not null
        && other is not null
        && System.Security.Cryptography.CryptographicOperations.FixedTimeEquals(System.Text.Encoding.UTF8.GetBytes(one), System.Text.Encoding.UTF8.GetBytes(other));

    private static string Decoded(string text)
    {
        try
        {
            return Uri.UnescapeDataString(text.Replace('+', ' '));
        }
        catch (UriFormatException)
        {
            return text;
        }
    }
}

/// <summary>What became of an address offered to a sign-in.</summary>
public enum SignInOffer
{
    /// <summary>No sign-in is open: the address is not used for one.</summary>
    NoneOpen,

    /// <summary>A sign-in is open and this is not its return.</summary>
    NotTheReturn,

    /// <summary>It is the return: the sign-in is ended with it.</summary>
    Taken,
}

/// <summary>
/// The one sign-in that may be open: a new one takes the place of an older one, whose end is then no longer waited for, and one
/// that is not answered ends by itself after a time. It holds no clock of its own. A sign-in is over once its own return has arrived (on
/// the way back it named and, when it was started with a <c>state</c>, carrying that state): from then on it is not open, an address that
/// arrives is no longer its return, and its time running out says nothing.
/// </summary>
public sealed class SignInTracker
{
    private SignInReturn? expected;
    private string? state;
    private DateTimeOffset expires;
    private int serial;

    /// <summary>Whether a sign-in is open.</summary>
    public bool IsOpen => expected is not null;

    /// <summary>The number of the sign-in that is open, or of the last one.</summary>
    public int Serial => serial;

    /// <summary>Opens a sign-in; one already open is dropped.</summary>
    /// <param name="comesBack">Where it comes back.</param>
    /// <param name="now">The time.</param>
    /// <param name="limit">How long it may stay open.</param>
    /// <param name="startedWith">The <c>state</c> the sign-in's address carried when the browser was sent to it, or null when it carried none.</param>
    /// <returns>Its number.</returns>
    public int Begin(SignInReturn comesBack, DateTimeOffset now, TimeSpan limit, string? startedWith = null)
    {
        ArgumentNullException.ThrowIfNull(comesBack);
        expected = comesBack;
        state = string.IsNullOrEmpty(startedWith) ? null : startedWith;
        expires = now + limit;
        return ++serial;
    }

    /// <summary>Offers an address that came to the app; when it is the open sign-in's return, the sign-in ends.</summary>
    /// <param name="address">The whole address.</param>
    /// <param name="ticket">The number of the sign-in that took it, when one did.</param>
    /// <returns>What became of it.</returns>
    public SignInOffer Offer(string? address, out int ticket)
    {
        ticket = 0;
        if (expected is null)
        {
            return SignInOffer.NoneOpen;
        }

        // Its own return comes back the way the sign-in named and, when the sign-in was started with a state, carries that state: an address
        // that only comes back the same way is not it, and leaves the sign-in open.
        if (!expected.Matches(address) || (state is not null && !SignInRules.Same(state, SignInRules.QueryValue(address, "state"))))
        {
            return SignInOffer.NotTheReturn;
        }

        ticket = serial;
        expected = null;
        state = null;
        return SignInOffer.Taken;
    }

    /// <summary>Ends the sign-in that is open, because the person did, or the time is up.</summary>
    /// <param name="now">The time.</param>
    /// <param name="force">End it whatever the time.</param>
    /// <returns>The number of the one that ended, or null when none did.</returns>
    public int? End(DateTimeOffset now, bool force = false)
    {
        if (expected is null || (!force && now < expires))
        {
            return null;
        }

        expected = null;
        state = null;
        return serial;
    }
}
