using System.Runtime.InteropServices;

// Windows' own libraries are looked for in the system folder and nowhere else.
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.System32)]

namespace Navree.Presence;

/// <summary>What the app can do in its place in Windows.</summary>
public enum SystemFeature
{
    /// <summary>An icon by the clock with a small window by it.</summary>
    NotificationAreaIcon,

    /// <summary>Opening when the person signs in to Windows.</summary>
    StartAtSignIn,

    /// <summary>A count on the app's button in the taskbar.</summary>
    TaskbarCount,

    /// <summary>Notifications the app shows itself.</summary>
    Notifications,

    /// <summary>A menu of quick ways in on the taskbar button.</summary>
    JumpList,

    /// <summary>The system's share sheet.</summary>
    Share,

    /// <summary>More than one window of the app.</summary>
    SeveralWindows,

    /// <summary>Coming back from a sign-in in the browser on the app's own scheme.</summary>
    ProtocolReturn,

    /// <summary>A second start hands what it was asked to the app already running.</summary>
    SingleInstance,

    /// <summary>A window that takes the whole screen.</summary>
    FullScreen,

    /// <summary>Windows Hello and passkeys.</summary>
    Passkeys,
}

/// <summary>Whether one feature is offered by this build, how, and if not, why.</summary>
/// <param name="Feature">The feature.</param>
/// <param name="Offered">Whether this run can do it.</param>
/// <param name="Means">What it is done with, in words.</param>
/// <param name="Reason">Why it is not offered, when it is not.</param>
/// <param name="NeedsIdentity">Whether the way it is done needs package identity.</param>
public sealed record FeatureOffer(SystemFeature Feature, bool Offered, string Means, string? Reason, bool NeedsIdentity);

/// <summary>
/// What a build can do where it stands in Windows, told from the two facts that decide it: whether the app has package
/// identity (it was installed from a package) and whether it runs as an administrator (Windows shows no app
/// notification to an app that does). A call that needs identity is never made without it.
/// </summary>
public static class Capabilities
{
    /// <summary>What this run offers, one entry for each feature, in a fixed order.</summary>
    /// <param name="hasIdentity">Whether the app has package identity.</param>
    /// <param name="elevated">Whether the app runs with an administrator's full rights.</param>
    /// <returns>The offers.</returns>
    public static IReadOnlyList<FeatureOffer> For(bool hasIdentity, bool elevated) =>
    [
        new(SystemFeature.NotificationAreaIcon, true, "an icon of the notification area", null, false),
        new(
            SystemFeature.StartAtSignIn,
            true,
            hasIdentity ? "the package's startup task" : "this person's list of programs to run at sign-in",
            null,
            false),
        new(
            SystemFeature.TaskbarCount,
            true,
            hasIdentity ? "the taskbar badge" : "an overlay icon on the taskbar button",
            null,
            false),
        elevated
            ? new(SystemFeature.Notifications, false, "app notifications", "Windows shows no notification to an app that runs as an administrator", false)
            : new(SystemFeature.Notifications, true, "app notifications", null, false),
        hasIdentity
            ? new(SystemFeature.JumpList, true, "the taskbar button's jump list", null, true)
            : new(SystemFeature.JumpList, false, "the taskbar button's jump list", "the jump list needs the app to be installed from a package", true),
        new(SystemFeature.Share, true, "the share sheet", null, false),
        new(SystemFeature.SeveralWindows, true, "windows of the one app", null, false),
        BrowserSignIn.Offered
            ? new(
                SystemFeature.ProtocolReturn,
                true,
                hasIdentity ? "the package's own scheme" : "a scheme registered for this person at run time",
                null,
                false)
            : new(SystemFeature.ProtocolReturn, false, "the app's own scheme", "signing in through the browser is turned off in this build, so nothing is taken on the app's scheme", false),
        new(SystemFeature.SingleInstance, true, "the app's own instance key", null, false),
        new(SystemFeature.FullScreen, true, "the full-screen window", null, false),
        new(SystemFeature.Passkeys, false, "Windows Hello and passkeys", "written behind one interface and not proven on a device", false),
    ];

    /// <summary>Whether a feature is offered in a list of offers.</summary>
    /// <param name="offers">The offers.</param>
    /// <param name="feature">The feature.</param>
    /// <returns>Whether it is offered.</returns>
    public static bool Has(IEnumerable<FeatureOffer> offers, SystemFeature feature) =>
        offers.Any(offer => offer.Feature == feature && offer.Offered);

    /// <summary>
    /// The device features this build tells the core it has, by the protocol's names. A control that calls a feature the
    /// build does not list is drawn as the call's stand-in says, so only what is carried out is listed. Listing
    /// <c>local_notification</c> commits the build to carrying out <c>notify</c>, <c>cancel_notification</c> and <c>set_badge</c>. Listing
    /// <c>auth_session</c> commits it to carrying out a sign-in in the browser: opening the address the core gives and handing back what
    /// arrives on the app's scheme. It is listed only for a run that can catch that return (<see cref="BrowserSignIn.ComesBack"/>).
    /// </summary>
    /// <param name="offers">What this run offers.</param>
    /// <param name="scheme">The app's own scheme, or null when it has none.</param>
    /// <returns>The names.</returns>
    public static IReadOnlyList<string> ProtocolCaps(IReadOnlyList<FeatureOffer> offers, string? scheme = null)
    {
        List<string> caps = ["clipboard", "open_url", "open_settings"];
        if (Has(offers, SystemFeature.Share))
        {
            caps.Add("share");
        }

        if (Has(offers, SystemFeature.Notifications))
        {
            caps.Add("local_notification");
        }

        if (BrowserSignIn.ComesBack(Has(offers, SystemFeature.ProtocolReturn), scheme))
        {
            caps.Add("auth_session");
        }

        return caps;
    }
}

/// <summary>Package identity, and whether the process has the administrator's full rights, asked of the system.</summary>
public static partial class PackageIdentity
{
    private const int InsufficientBuffer = 122;

    /// <summary>Whether the app was started from an installed package: only then may a call that needs identity be made.</summary>
    /// <returns>Whether it has package identity.</returns>
    public static bool Has()
    {
        try
        {
            uint length = 0;
            int answer = GetCurrentPackageFullName(ref length, 0);
            return Decides(answer);
        }
        catch (EntryPointNotFoundException)
        {
            return false;
        }
    }

    /// <summary>
    /// What the system's answer to "what is this process's package" says: asked with no room for the name, a process that
    /// has a package is told the room is too small (122), and one that has none is told there is no package (15700).
    /// </summary>
    /// <param name="answer">The error code the system answered.</param>
    /// <returns>Whether there is a package.</returns>
    public static bool Decides(int answer) => answer == InsufficientBuffer;

    /// <summary>Whether the process runs with an administrator's full rights.</summary>
    /// <returns>Whether it does.</returns>
    public static bool Elevated() => Environment.IsPrivilegedProcess;

    [LibraryImport("kernel32.dll")]
    private static partial int GetCurrentPackageFullName(ref uint packageFullNameLength, nint packageFullName);
}
