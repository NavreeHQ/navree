using System.Text.Json;

namespace Navree.Design;

/// <summary>What the person's system says that the core is told: the theme, the contrast, the text size, the language, the time zone.</summary>
/// <param name="Dark">Whether the app is drawn dark.</param>
/// <param name="MoreContrast">Whether the system asks for more contrast.</param>
/// <param name="TextPercent">The size of the person's text, in percent of the system's default.</param>
/// <param name="Locale">The language, as a tag.</param>
/// <param name="TimeZone">The time zone by its IANA name, when the system can say it.</param>
public sealed record ClientSettings(bool Dark, bool MoreContrast, int TextPercent, string Locale, string? TimeZone)
{
    /// <summary>What differs from what the core was last told; nothing when the same.</summary>
    /// <param name="told">What the core was last told.</param>
    /// <returns>The parts that changed, each null when it did not.</returns>
    public SettingsChange Since(ClientSettings told)
    {
        ArgumentNullException.ThrowIfNull(told);
        return new SettingsChange(
            Dark != told.Dark ? Dark : null,
            MoreContrast != told.MoreContrast ? (MoreContrast ? "more" : "normal") : null,
            TextPercent != told.TextPercent ? TextPercent : null,
            Locale != told.Locale ? Locale : null,
            TimeZone != told.TimeZone ? TimeZone : null);
    }
}

/// <summary>The parts of the person's settings that changed.</summary>
/// <param name="Dark">Dark or light, when it changed.</param>
/// <param name="Contrast">"more" or "normal", when it changed.</param>
/// <param name="TextPercent">The text size, when it changed.</param>
/// <param name="Locale">The language, when it changed.</param>
/// <param name="TimeZone">The time zone, when it changed.</param>
public sealed record SettingsChange(bool? Dark, string? Contrast, int? TextPercent, string? Locale, string? TimeZone)
{
    /// <summary>Whether nothing changed.</summary>
    public bool IsEmpty => Dark is null && Contrast is null && TextPercent is null && Locale is null && TimeZone is null;
}

/// <summary>The configuration the core is made from, written from what the app and the system say.</summary>
public static class ClientConfig
{
    /// <summary>Writes the configuration.</summary>
    /// <param name="origin">The app's origin.</param>
    /// <param name="also">The other origins the app treats as its own.</param>
    /// <param name="appId">The app's own id, when it has one.</param>
    /// <param name="settings">What the system says now.</param>
    /// <param name="os">The system's version.</param>
    /// <param name="app">The app's version.</param>
    /// <param name="caps">The device features this build has, by the protocol's names.</param>
    /// <param name="signInCallback">The scheme a sign-in in the browser comes back on, when the app has one.</param>
    /// <param name="loopbackHttp">Whether plain http to the machine itself is allowed.</param>
    /// <param name="undrawable">The kinds this build does not draw.</param>
    /// <returns>The configuration as UTF-8 JSON.</returns>
    public static byte[] Json(
        string origin,
        IReadOnlyList<string> also,
        string? appId,
        ClientSettings settings,
        string os,
        string app,
        IReadOnlyList<string> caps,
        string? signInCallback,
        bool loopbackHttp,
        IReadOnlyList<string> undrawable)
    {
        ArgumentNullException.ThrowIfNull(settings);
        using MemoryStream stream = new();
        using (Utf8JsonWriter w = new(stream))
        {
            w.WriteStartObject();
            w.WriteString("origin", origin);
            if (also.Count > 0)
            {
                w.WriteStartArray("also");
                foreach (string other in also)
                {
                    w.WriteStringValue(other);
                }

                w.WriteEndArray();
            }

            if (appId is not null)
            {
                w.WriteString("app_id", appId);
            }

            w.WriteStartObject("client");
            w.WriteString("platform", "windows");
            w.WriteString("os", os);
            w.WriteString("form", "desktop");
            w.WriteString("app", app);
            w.WriteString("lib", "0.1.0");
            w.WriteNumber("vocab", 0);
            w.WriteString("scheme", settings.Dark ? "dark" : "light");
            w.WriteString("contrast", settings.MoreContrast ? "more" : "normal");
            w.WriteNumber("text_percent", settings.TextPercent);
            w.WriteString("locale", settings.Locale);
            if (settings.TimeZone is string zone)
            {
                w.WriteString("tz", zone);
            }

            w.WriteStartArray("caps");
            foreach (string cap in caps)
            {
                w.WriteStringValue(cap);
            }

            w.WriteEndArray();
            w.WriteEndObject();
            if (signInCallback is not null)
            {
                w.WriteStartObject("sign_in");
                w.WriteString("callback", signInCallback);
                w.WriteEndObject();
            }

            w.WriteStartObject("cache");
            w.WriteNumber("pages", 24);
            w.WriteBoolean("persist", true);
            w.WriteEndObject();
            if (loopbackHttp)
            {
                w.WriteBoolean("loopback_http", true);
            }

            w.WriteStartArray("undrawable");
            foreach (string kind in undrawable)
            {
                w.WriteStringValue(kind);
            }

            w.WriteEndArray();
            w.WriteEndObject();
        }

        return stream.ToArray();
    }
}
