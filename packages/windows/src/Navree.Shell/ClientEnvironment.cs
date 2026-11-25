using System.Globalization;
using Navree.Design;
using Microsoft.UI.Xaml;
using Windows.UI.ViewManagement;

namespace Navree.Shell;

/// <summary>What the person's settings are, as the core is told them: the theme, the contrast, the text size, the language, the time zone.</summary>
internal static class ClientEnvironment
{
    /// <summary>Whether the app is drawn dark: the theme it runs in.</summary>
    internal static bool IsDark => Application.Current.RequestedTheme == ApplicationTheme.Dark;

    /// <summary>Whether the system asks for more contrast.</summary>
    internal static bool MoreContrast => new AccessibilitySettings().HighContrast;

    /// <summary>How big the person's text is, in percent of the system's default.</summary>
    internal static int TextPercent
    {
        get
        {
            try
            {
                return (int)Math.Round(new UISettings().TextScaleFactor * 100);
            }
            catch (Exception)
            {
                return 100;
            }
        }
    }

    /// <summary>The person's language.</summary>
    internal static string Locale => CultureInfo.CurrentUICulture.Name.Length == 0 ? "en-IN" : CultureInfo.CurrentUICulture.Name;

    /// <summary>The time zone by its IANA name, when the system can say it.</summary>
    internal static string? TimeZone =>
        TimeZoneInfo.Local.HasIanaId
            ? TimeZoneInfo.Local.Id
            : TimeZoneInfo.TryConvertWindowsIdToIanaId(TimeZoneInfo.Local.Id, out string? iana) ? iana : null;

    /// <summary>What the system says now.</summary>
    internal static ClientSettings Settings() => new(IsDark, MoreContrast, TextPercent, Locale, TimeZone);

    /// <summary>The package family name when the app runs from a package, null when it does not.</summary>
    internal static string? PackageFamilyName()
    {
        if (!Navree.Presence.PackageIdentity.Has())
        {
            return null;
        }

        try
        {
            return Windows.ApplicationModel.Package.Current.Id.FamilyName;
        }
        catch (InvalidOperationException)
        {
            return null;
        }
    }

    /// <summary>
    /// The configuration the core is made from. The app's id is its package family name when packaged, the configured id when not. A run that
    /// can catch the return of a sign-in in the browser (it is offered, and the app has a scheme of its own) tells the core it has a sign-in
    /// session and which addresses start one; any other run tells it neither, so no sign-in in the browser is started. The core makes its
    /// key for a person with the install's secret, and says what a person's plain key was while the mark of one is kept.
    /// </summary>
    internal static byte[] Config(ShellOptions options, string secret, bool oldKey)
    {
        IReadOnlyList<Navree.Presence.FeatureOffer> offers = PlaceCapabilities.Current();
        bool comesBack = Navree.Presence.BrowserSignIn.ComesBack(
            Navree.Presence.Capabilities.Has(offers, Navree.Presence.SystemFeature.ProtocolReturn),
            options.Place.Scheme);
        return ClientConfig.Json(
            options.Origin,
            options.Also,
            Navree.Presence.BrowserSignIn.AppId(PackageFamilyName(), options.AppId),
            Settings(),
            Environment.OSVersion.Version.ToString(),
            typeof(ClientEnvironment).Assembly.GetName().Version?.ToString(3) ?? "0.1.0",
            Navree.Presence.Capabilities.ProtocolCaps(offers, options.Place.Scheme),
            options.Place.Scheme,
            options.LoopbackHttp,
            options.Undrawable,
            secret,
            oldKey,
            signInPaths: comesBack ? options.SignInPaths : null);
    }
}
