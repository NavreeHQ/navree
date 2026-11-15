using Microsoft.UI.Dispatching;
using Navree.Design;
using Windows.UI.ViewManagement;

namespace Navree.Shell;

/// <summary>
/// Watches what the person's system says that the core is told (the theme, the contrast, the text size, the language, the time
/// zone) and calls back with what changed. The system raises its events on a thread of its own, so each is handed to the
/// dispatcher and the settings are read again there; only what differs from what the core was last told is passed on. The system
/// raises no event for a language or a time zone, so those are read again whenever something else is, and when the app comes back
/// to the front.
/// </summary>
internal sealed class EnvironmentWatcher : IDisposable
{
    private readonly DispatcherQueue dispatcher;
    private readonly Action<SettingsChange> changed;
    private readonly UISettings ui = new();
    private readonly AccessibilitySettings accessibility = new();
    private readonly bool startedDark;
    private readonly bool followsSystemTheme;
    private readonly string startedLocale;
    private readonly string? startedLanguage;
    private ClientSettings told;
    private bool disposed;

    /// <param name="dispatcher">The UI thread's queue.</param>
    /// <param name="told">What the core was told when it was made.</param>
    /// <param name="changed">Called on the dispatcher with what changed.</param>
    internal EnvironmentWatcher(DispatcherQueue dispatcher, ClientSettings told, Action<SettingsChange> changed)
    {
        this.dispatcher = dispatcher;
        this.told = told;
        this.changed = changed;
        startedDark = told.Dark;
        followsSystemTheme = startedDark == SystemIsDark();
        startedLocale = told.Locale;
        startedLanguage = SystemLanguage();
        Watch(() => ui.ColorValuesChanged += OnSystemChanged);
        Watch(() => ui.TextScaleFactorChanged += OnSystemChanged);
        Watch(() => accessibility.HighContrastChanged += OnSystemChanged);
    }

    // A system setting that cannot be watched here (the runner of a server edition answers "element not found" to one of them) is one
    // that is not watched: the others are, and everything is read again when the app comes back to the front.
    private static void Watch(Action subscribe)
    {
        try
        {
            subscribe();
        }
        catch (Exception)
        {
            // Not watched.
        }
    }

    /// <summary>Reads the settings again and passes on what differs from what the core was last told.</summary>
    internal void Check()
    {
        if (disposed)
        {
            return;
        }

        TimeZoneInfo.ClearCachedData();
        string? language = SystemLanguage();
        ClientSettings now = new(
            followsSystemTheme ? SystemIsDark() : startedDark,
            ClientEnvironment.MoreContrast,
            ClientEnvironment.TextPercent,
            language is null || language == startedLanguage ? startedLocale : language,
            ClientEnvironment.TimeZone);
        SettingsChange change = now.Since(told);
        if (!change.IsEmpty)
        {
            told = now;
            changed(change);
        }
    }

    public void Dispose()
    {
        disposed = true;
        Watch(() => ui.ColorValuesChanged -= OnSystemChanged);
        Watch(() => ui.TextScaleFactorChanged -= OnSystemChanged);
        Watch(() => accessibility.HighContrastChanged -= OnSystemChanged);
    }

    private void OnSystemChanged(object? sender, object? args) => dispatcher.TryEnqueue(Check);

    // The system's own theme, from the colour it draws a window's background with.
    private bool SystemIsDark()
    {
        try
        {
            Windows.UI.Color background = ui.GetColorValue(UIColorType.Background);
            return (0.299 * background.R) + (0.587 * background.G) + (0.114 * background.B) < 128;
        }
        catch (Exception)
        {
            return startedDark;
        }
    }

    private static string? SystemLanguage()
    {
        try
        {
            return Windows.System.UserProfile.GlobalizationPreferences.Languages.FirstOrDefault();
        }
        catch (Exception)
        {
            return null;
        }
    }
}
