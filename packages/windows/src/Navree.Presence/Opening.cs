namespace Navree.Presence;

/// <summary>A destination of the app, as the ways in from outside the window show it: the notification-area menu, the jump list, the taskbar count.</summary>
/// <param name="Label">What it is called.</param>
/// <param name="Href">Its address.</param>
/// <param name="Badge">A count or word beside it, in the page's words.</param>
public sealed record PlaceDestination(string Label, string Href, string? Badge);

/// <summary>What an address that came from outside the app does.</summary>
public enum OpenKind
{
    /// <summary>Nothing: it is not an address of the app's.</summary>
    Nothing,

    /// <summary>It is a destination: the destination is chosen.</summary>
    Select,

    /// <summary>It is any other address of the app: the core's own route rules decide where it leads.</summary>
    Link,
}

/// <summary>What to do with an address that came from outside.</summary>
/// <param name="Kind">What kind of thing it is.</param>
/// <param name="Index">The destination chosen, for <see cref="OpenKind.Select"/>.</param>
/// <param name="Href">The address, for <see cref="OpenKind.Link"/>.</param>
public sealed record OpenAction(OpenKind Kind, int Index, string Href);

/// <summary>What the app does with an address that was handed to it: from a jump list, a notification, a second start.</summary>
public static class OpeningRules
{
    /// <summary>The longest address taken from outside.</summary>
    public const int Longest = 2048;

    /// <summary>
    /// Whether text is a path on the app's own origin and nothing else: one slash first (not two, which names another host),
    /// no backslash, no control character, not past the longest. Anything from outside the app is held to this before it is
    /// handed to the core, which asks its own question after.
    /// </summary>
    /// <param name="text">The text.</param>
    /// <returns>Whether it is such a path.</returns>
    public static bool IsAppPath(string? text)
    {
        if (string.IsNullOrEmpty(text) || text.Length > Longest || text[0] != '/')
        {
            return false;
        }

        if (text.Length > 1 && (text[1] == '/' || text[1] == '\\'))
        {
            return false;
        }

        foreach (char letter in text)
        {
            if (letter == '\\' || letter < ' ' || letter == '\u007F')
            {
                return false;
            }
        }

        return true;
    }

    /// <summary>What an address does: a destination is chosen; another path of the app is a link; anything else is nothing.</summary>
    /// <param name="href">The address.</param>
    /// <param name="destinations">The app's destinations.</param>
    /// <returns>What to do.</returns>
    public static OpenAction Decide(string? href, IReadOnlyList<PlaceDestination> destinations)
    {
        if (!IsAppPath(href))
        {
            return new OpenAction(OpenKind.Nothing, -1, string.Empty);
        }

        string wanted = Trimmed(href!);
        for (int at = 0; at < destinations.Count; at++)
        {
            if (string.Equals(Trimmed(destinations[at].Href), wanted, StringComparison.Ordinal))
            {
                return new OpenAction(OpenKind.Select, at, href!);
            }
        }

        return new OpenAction(OpenKind.Link, -1, href!);
    }

    private static string Trimmed(string path)
    {
        string bare = path.Split('?', '#')[0];
        return bare.Length > 1 ? bare.TrimEnd('/') : bare;
    }
}

/// <summary>What the app was started with, read from its command line.</summary>
/// <param name="OpenHref">A path of the app to open, from a jump list or a second start.</param>
/// <param name="Return">An address that came on the app's own scheme, which a sign-in in the browser ends in.</param>
/// <param name="Background">The app was started when the person signed in to Windows: its window waits for the person.</param>
/// <param name="NewWindow">A new window was asked for.</param>
public sealed record LaunchRequest(string? OpenHref, string? Return, bool Background, bool NewWindow)
{
    /// <summary>Nothing was asked.</summary>
    public static LaunchRequest Plain { get; } = new(null, null, false, false);
}

/// <summary>The words of the app's command line.</summary>
public static class LaunchArguments
{
    /// <summary>The flag that opens a path of the app.</summary>
    public const string Open = "--open";

    /// <summary>The flag a start at sign-in carries.</summary>
    public const string Background = "--background";

    /// <summary>The flag that asks for a new window.</summary>
    public const string NewWindow = "--new-window";

    /// <summary>The arguments a jump list entry starts the app with: the address, percent-encoded so no space or quote can break it.</summary>
    /// <param name="href">The address.</param>
    /// <returns>The arguments, or null when the address is not a path of the app.</returns>
    public static string? ForJumpList(string href) =>
        OpeningRules.IsAppPath(href) ? Open + "=" + Uri.EscapeDataString(href) : null;

    /// <summary>Reads the command line: the flags this build knows, and an address on the app's scheme.</summary>
    /// <param name="args">The arguments, without the program's own name.</param>
    /// <param name="scheme">The app's own scheme, or null when it has none.</param>
    /// <returns>What was asked. Anything not understood is left out, never an error.</returns>
    public static LaunchRequest Read(IReadOnlyList<string> args, string? scheme)
    {
        string? open = null;
        string? back = null;
        bool background = false;
        bool window = false;
        for (int at = 0; at < args.Count; at++)
        {
            string argument = args[at];
            if (string.Equals(argument, Background, StringComparison.OrdinalIgnoreCase))
            {
                background = true;
            }
            else if (string.Equals(argument, NewWindow, StringComparison.OrdinalIgnoreCase))
            {
                window = true;
            }
            else if (argument.StartsWith(Open + "=", StringComparison.OrdinalIgnoreCase))
            {
                open = Decoded(argument[(Open.Length + 1)..]);
            }
            else if (string.Equals(argument, Open, StringComparison.OrdinalIgnoreCase) && at + 1 < args.Count)
            {
                open = Decoded(args[++at]);
            }
            else if (scheme is not null && SignInRules.IsOnScheme(argument, scheme))
            {
                back = argument;
            }
        }

        return new LaunchRequest(open, back, background, window);
    }

    /// <summary>Splits one line of arguments the way the system does for a program's command line: spaces separate, double quotes group, a backslash before a quote is that quote.</summary>
    /// <param name="line">The line.</param>
    /// <returns>The arguments.</returns>
    public static IReadOnlyList<string> Split(string? line)
    {
        List<string> parts = [];
        if (string.IsNullOrWhiteSpace(line))
        {
            return parts;
        }

        System.Text.StringBuilder current = new();
        bool quoted = false;
        bool any = false;
        for (int at = 0; at < line.Length; at++)
        {
            char letter = line[at];
            if (letter == '\\' && at + 1 < line.Length && line[at + 1] == '"')
            {
                _ = current.Append('"');
                any = true;
                at++;
            }
            else if (letter == '"')
            {
                quoted = !quoted;
                any = true;
            }
            else if (char.IsWhiteSpace(letter) && !quoted)
            {
                if (any)
                {
                    parts.Add(current.ToString());
                    _ = current.Clear();
                    any = false;
                }
            }
            else
            {
                _ = current.Append(letter);
                any = true;
            }
        }

        if (any)
        {
            parts.Add(current.ToString());
        }

        return parts;
    }

    // The path a jump list carries, or nothing when what came is not a path of the app.
    private static string? Decoded(string text)
    {
        string path;
        try
        {
            path = Uri.UnescapeDataString(text);
        }
        catch (UriFormatException)
        {
            return null;
        }

        return OpeningRules.IsAppPath(path) ? path : null;
    }
}
