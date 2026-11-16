namespace Navree.Presence;

/// <summary>What is done with an address that arrived on the app's scheme (or on a loopback port).</summary>
public enum Arrival
{
    /// <summary>Nothing: it is not used, not opened as a page, and the core is not told of it.</summary>
    Dropped,

    /// <summary>The core is told the sign-in it opened came back with it; the core takes it only with that sign-in's state.</summary>
    SignInDone,

    /// <summary>The core is given it as a link from outside; the core never opens a sign-in's return as a page.</summary>
    OpenLink,
}

/// <summary>
/// Signing in through the system's browser, decided in one place. It is not offered on Windows yet: the browser's cookies are not the
/// app's, so the only sign-in it can give is the hand-over (a one-time code bound to a <c>state</c> and an S256 challenge, exchanged by the
/// app), and the server has no hand-over for Windows apps yet. While it is not offered: no sign-in in the browser is started, the app's
/// scheme is neither registered nor offered, and an address that arrives on it is dropped, with or without a sign-in open. Once it is
/// offered, every address that arrives is handed to the core as it came, and the core decides.
/// </summary>
/// <remarks>
/// What must be true before <see cref="Offered"/> is turned on: the server lists Windows apps for the hand-over (the app's id is its package
/// family name when packaged, its configured id when not) with their callback on the app's scheme, and names that callback to a Windows
/// client in the shell; and a transcript made by the core shows a Windows sign-in starting with its <c>state</c> and challenge and refusing
/// a return with another <c>state</c>, with none, or with no sign-in open.
/// </remarks>
public static class BrowserSignIn
{
    /// <summary>Whether this client offers signing in through the system's browser: the one place it is decided.</summary>
    public static readonly bool Offered;

    /// <summary>Why a sign-in in the browser the core asks for is not started, or null when it may be.</summary>
    /// <param name="offered">Whether signing in through the browser is offered (<see cref="Offered"/>).</param>
    /// <returns>The words, or null.</returns>
    public static string? WhyNotStart(bool offered) =>
        offered ? null : "signing in through the browser is not offered on Windows";

    /// <summary>What is done with an address that arrived on the app's scheme or on a loopback port.</summary>
    /// <param name="offered">Whether signing in through the browser is offered (<see cref="Offered"/>).</param>
    /// <param name="signInOpen">Whether a sign-in in the browser that this app started is open.</param>
    /// <param name="address">The whole address, as it came.</param>
    /// <returns>What is done.</returns>
    public static Arrival Route(bool offered, bool signInOpen, string address)
    {
        if (!offered || string.IsNullOrWhiteSpace(address))
        {
            return Arrival.Dropped;
        }

        return signInOpen ? Arrival.SignInDone : Arrival.OpenLink;
    }

    /// <summary>The app's id the hand-over names it by: the package family name when the app runs from a package, the configured id when not.</summary>
    /// <param name="familyName">The package family name, or null without package identity.</param>
    /// <param name="configured">The id the app was configured with.</param>
    /// <returns>The id, or null when there is none.</returns>
    public static string? AppId(string? familyName, string? configured) =>
        string.IsNullOrEmpty(familyName) ? configured : familyName;
}
