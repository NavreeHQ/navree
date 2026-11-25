namespace Navree.Presence;

/// <summary>What is done with an address that arrived on the app's scheme.</summary>
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
/// Signing in through the system's browser, decided in one place. The browser's cookies are not the app's, so the only sign-in it can give is
/// the hand-over (a one-time code bound to a <c>state</c> and an S256 challenge, exchanged by the app). The server gives it to the Windows apps
/// it lists (<c>auth.apps.windows</c>): an app names itself by its package family name when it runs from a package and by its configured id when
/// not, and its code comes back on a link of the app's own scheme. So it is offered. A run takes part only when it can catch that return, which
/// takes a scheme of the app's own (<see cref="ComesBack"/>): only then is the core told the build has a sign-in session, is the scheme
/// registered, and does a link to a sign-in address open the browser. Every address that arrives on the scheme is handed to the core as it came,
/// and the core decides: it takes a return only while the sign-in it opened is open, on the way back that sign-in named, with its
/// <c>state</c>. Turned off (<see cref="Offered"/> false), no sign-in in the browser is started, the scheme is neither registered nor offered,
/// and an address that arrives on it is dropped, with or without a sign-in open.
/// </summary>
/// <remarks>
/// What it stands on, each of which has to stay true while it is on: the server lists Windows apps for the hand-over (the app's id is its package
/// family name when packaged, its configured id when not) with their callback on the app's scheme, and names that callback to a Windows client
/// in the shell; and a transcript made by the core (<c>hand-over-windows.json</c>) shows a Windows sign-in starting with its <c>state</c> and
/// challenge and refusing a return with another <c>state</c>, with none, or with no sign-in open. The server gives a Windows app's sign-in to a
/// link of its own scheme and to nothing else, so a return on a port of this machine (<see cref="LoopbackReturn"/>) or on a link of the site
/// is not taken, and no port is listened on (<see cref="WhyNotReturn"/>).
/// </remarks>
public static class BrowserSignIn
{
    /// <summary>Whether this client offers signing in through the system's browser: the one place it is decided.</summary>
    public static readonly bool Offered = true;

    /// <summary>Why a sign-in in the browser the core asks for is not started, or null when it may be.</summary>
    /// <param name="offered">Whether signing in through the browser is offered (<see cref="Offered"/>).</param>
    /// <returns>The words, or null.</returns>
    public static string? WhyNotStart(bool offered) =>
        offered ? null : "signing in through the browser is turned off in this build";

    /// <summary>
    /// Whether a run can catch the return of a sign-in in the browser: it is offered, and the app has a scheme of its own (the scheme alone,
    /// <c>charkha</c>, not an address on it). Without one nothing could come back, so the core is not told the build has a sign-in session and
    /// no link starts one.
    /// </summary>
    /// <param name="offered">Whether signing in through the browser is offered (<see cref="Offered"/>).</param>
    /// <param name="scheme">The app's own scheme, or null when it has none.</param>
    /// <returns>Whether it can.</returns>
    public static bool ComesBack(bool offered, string? scheme) =>
        offered
        && scheme is not null
        && SignInReturn.Parse(scheme) is { Kind: ReturnKind.Scheme } back
        && string.Equals(back.Scheme, scheme, StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// Why a sign-in that would come back this way is not started, or null when it may be. Only a return on the app's own scheme is taken: it is
    /// the one way back the server gives a Windows app.
    /// </summary>
    /// <param name="comesBack">Where the sign-in is told to come back.</param>
    /// <returns>The words, or null.</returns>
    public static string? WhyNotReturn(SignInReturn comesBack)
    {
        ArgumentNullException.ThrowIfNull(comesBack);
        return comesBack.Kind switch
        {
            ReturnKind.Scheme => null,
            ReturnKind.Loopback => "a return on a port of this machine is not one the server gives a Windows app, so none is listened on",
            _ => comesBack.WhyNot() ?? "the return is not on the app's own scheme",
        };
    }

    /// <summary>What is done with an address that arrived on the app's scheme.</summary>
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
