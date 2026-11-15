using System.Globalization;
using System.Text;
using System.Text.Json;
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

    /// <summary>The configuration the core is made from.</summary>
    internal static byte[] Config(ShellOptions options)
    {
        using MemoryStream stream = new();
        using (Utf8JsonWriter w = new(stream))
        {
            w.WriteStartObject();
            w.WriteString("origin", options.Origin);
            if (options.Also.Count > 0)
            {
                w.WriteStartArray("also");
                foreach (string other in options.Also)
                {
                    w.WriteStringValue(other);
                }

                w.WriteEndArray();
            }

            if (options.AppId is not null)
            {
                w.WriteString("app_id", options.AppId);
            }

            w.WriteStartObject("client");
            w.WriteString("platform", "windows");
            w.WriteString("os", Environment.OSVersion.Version.ToString());
            w.WriteString("form", "desktop");
            w.WriteString("app", typeof(ClientEnvironment).Assembly.GetName().Version?.ToString(3) ?? "0.1.0");
            w.WriteString("lib", "0.1.0");
            w.WriteNumber("vocab", 0);
            w.WriteString("scheme", IsDark ? "dark" : "light");
            w.WriteString("contrast", MoreContrast ? "more" : "normal");
            w.WriteNumber("text_percent", TextPercent);
            w.WriteString("locale", Locale);
            if (TimeZone is string zone)
            {
                w.WriteString("tz", zone);
            }

            w.WriteStartArray("caps");
            w.WriteStringValue("clipboard");
            w.WriteStringValue("open_url");
            w.WriteEndArray();
            w.WriteEndObject();
            w.WriteStartObject("cache");
            w.WriteNumber("pages", 24);
            w.WriteBoolean("persist", true);
            w.WriteEndObject();
            if (options.LoopbackHttp)
            {
                w.WriteBoolean("loopback_http", true);
            }

            if (options.Undrawable.Count > 0)
            {
                w.WriteStartArray("undrawable");
                foreach (string kind in options.Undrawable)
                {
                    w.WriteStringValue(kind);
                }

                w.WriteEndArray();
            }

            w.WriteEndObject();
        }

        return stream.ToArray();
    }
}
