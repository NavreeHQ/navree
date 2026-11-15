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

    /// <summary>The configuration the core is made from.</summary>
    internal static byte[] Config(ShellOptions options) =>
        ClientConfig.Json(
            options.Origin,
            options.Also,
            options.AppId,
            Settings(),
            Environment.OSVersion.Version.ToString(),
            typeof(ClientEnvironment).Assembly.GetName().Version?.ToString(3) ?? "0.1.0",
            Navree.Presence.Capabilities.ProtocolCaps(PlaceCapabilities.Current()),
            options.Place.Scheme,
            options.LoopbackHttp,
            options.Undrawable);
}
