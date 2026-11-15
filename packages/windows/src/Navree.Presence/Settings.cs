using System.Text.Json;

namespace Navree.Presence;

/// <summary>What the person chose for the app's place in Windows. Nothing here is on until the person turns it on.</summary>
/// <param name="NotificationAreaIcon">Whether the icon is in the notification area.</param>
/// <param name="KeepRunning">Whether closing the window leaves the app running in the notification area.</param>
/// <param name="TaskbarCount">Whether the taskbar button shows the count.</param>
public sealed record Choices(bool NotificationAreaIcon, bool KeepRunning, bool TaskbarCount)
{
    /// <summary>What a person who has chosen nothing has: no icon, the count shown when the app is asked to show one.</summary>
    public static Choices Initial { get; } = new(false, true, true);

    /// <summary>Writes the choices.</summary>
    /// <returns>JSON text.</returns>
    public string Write()
    {
        using MemoryStream stream = new();
        using (Utf8JsonWriter w = new(stream))
        {
            w.WriteStartObject();
            w.WriteBoolean("tray", NotificationAreaIcon);
            w.WriteBoolean("keep_running", KeepRunning);
            w.WriteBoolean("taskbar_count", TaskbarCount);
            w.WriteEndObject();
        }

        return System.Text.Encoding.UTF8.GetString(stream.ToArray());
    }

    /// <summary>Reads what <see cref="Write"/> wrote. What does not read is what a person who chose nothing has.</summary>
    /// <param name="text">The text.</param>
    /// <returns>The choices.</returns>
    public static Choices Read(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return Initial;
        }

        try
        {
            using JsonDocument document = JsonDocument.Parse(text);
            JsonElement root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object)
            {
                return Initial;
            }

            return new Choices(
                Flag(root, "tray", Initial.NotificationAreaIcon),
                Flag(root, "keep_running", Initial.KeepRunning),
                Flag(root, "taskbar_count", Initial.TaskbarCount));
        }
        catch (JsonException)
        {
            return Initial;
        }
    }

    private static bool Flag(JsonElement root, string name, bool fallback) =>
        root.TryGetProperty(name, out JsonElement value) && (value.ValueKind == JsonValueKind.True || value.ValueKind == JsonValueKind.False)
            ? value.ValueKind == JsonValueKind.True
            : fallback;
}

/// <summary>Where the choices are kept: a plain file in the app's own folder under the person's local data, which nothing secret is in.</summary>
public sealed class ChoicesFile(string folder)
{
    /// <summary>The file's name.</summary>
    public const string Name = "place.json";

    /// <summary>Reads the choices.</summary>
    /// <returns>The choices, or the initial ones when there is no file or it does not read.</returns>
    public Choices Read()
    {
        try
        {
            string file = Path.Combine(folder, Name);
            return File.Exists(file) ? Choices.Read(File.ReadAllText(file)) : Choices.Initial;
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
        {
            return Choices.Initial;
        }
    }

    /// <summary>Keeps the choices.</summary>
    /// <param name="choices">The choices.</param>
    /// <returns>Whether they were kept.</returns>
    public bool Write(Choices choices)
    {
        try
        {
            _ = Directory.CreateDirectory(folder);
            File.WriteAllText(Path.Combine(folder, Name), choices.Write());
            return true;
        }
        catch (Exception problem) when (problem is IOException or UnauthorizedAccessException)
        {
            return false;
        }
    }
}

/// <summary>What a card of the settings page is.</summary>
public enum CardKind
{
    /// <summary>A switch.</summary>
    Switch,

    /// <summary>A button that does something.</summary>
    Button,

    /// <summary>A line of information.</summary>
    Info,
}

/// <summary>One card of the settings page: a row with a title, a line under it, and a control at its end.</summary>
/// <param name="Id">What the app knows it by.</param>
/// <param name="Group">The heading it stands under.</param>
/// <param name="Title">Its title.</param>
/// <param name="Description">The line under the title.</param>
/// <param name="Kind">What is at its end.</param>
/// <param name="On">For a switch, whether it is on.</param>
/// <param name="Enabled">Whether the control can be used.</param>
/// <param name="Button">For a button, what it says.</param>
public sealed record SettingsCard(string Id, string Group, string Title, string Description, CardKind Kind, bool On, bool Enabled, string? Button);

/// <summary>The ids of the cards.</summary>
public static class CardIds
{
    /// <summary>The notification-area icon.</summary>
    public const string Tray = "tray";

    /// <summary>Keeping the app running when its window is closed.</summary>
    public const string KeepRunning = "keep-running";

    /// <summary>Opening at sign-in.</summary>
    public const string Startup = "startup";

    /// <summary>The count on the taskbar.</summary>
    public const string TaskbarCount = "taskbar-count";

    /// <summary>The notification settings of Windows.</summary>
    public const string NotificationSettings = "notification-settings";

    /// <summary>The startup settings of Windows.</summary>
    public const string StartupSettings = "startup-settings";
}

/// <summary>The settings page as plain rules: which cards it holds, and what each says, from what the build can do and what the person chose.</summary>
public static class SettingsRules
{
    /// <summary>The group of what the app does in Windows.</summary>
    public const string General = "General";

    /// <summary>The group of notifications.</summary>
    public const string Notifications = "Notifications";

    /// <summary>The cards of the page, in order; a feature the build does not offer has no card of its own, and says so where one would be told.</summary>
    /// <param name="offers">What this run offers.</param>
    /// <param name="choices">What the person chose.</param>
    /// <param name="startup">Whether the app opens at sign-in.</param>
    /// <returns>The cards.</returns>
    public static IReadOnlyList<SettingsCard> Cards(IReadOnlyList<FeatureOffer> offers, Choices choices, StartupState startup)
    {
        List<SettingsCard> cards = [];
        if (Capabilities.Has(offers, SystemFeature.NotificationAreaIcon))
        {
            cards.Add(new(
                CardIds.Tray,
                General,
                "Show in the notification area",
                "An icon by the clock that opens a small window with the app's places. Windows keeps its own list of which icons show.",
                CardKind.Switch,
                choices.NotificationAreaIcon,
                true,
                null));
            cards.Add(new(
                CardIds.KeepRunning,
                General,
                "Keep running when the window is closed",
                choices.NotificationAreaIcon
                    ? "Closing the window hides it; the icon in the notification area opens it again, and Quit ends the app."
                    : "Needs the icon in the notification area.",
                CardKind.Switch,
                choices.KeepRunning && choices.NotificationAreaIcon,
                choices.NotificationAreaIcon,
                null));
        }

        if (Capabilities.Has(offers, SystemFeature.StartAtSignIn))
        {
            bool mayChange = StartupRules.AppMayChange(startup);
            cards.Add(new(
                CardIds.Startup,
                General,
                "Open when I sign in to Windows",
                startup switch
                {
                    StartupState.DisabledByUser => "Turned off in Windows' own settings; only you can turn it on again there.",
                    StartupState.ByPolicy => "Your organisation decides this.",
                    _ => "The app opens with its window hidden when the notification-area icon is on.",
                },
                CardKind.Switch,
                startup is StartupState.On or StartupState.ByPolicy,
                mayChange,
                null));
            if (startup == StartupState.DisabledByUser)
            {
                cards.Add(new(CardIds.StartupSettings, General, "Startup apps in Windows", "Where the choice was made.", CardKind.Button, false, true, "Open"));
            }
        }

        if (Capabilities.Has(offers, SystemFeature.TaskbarCount))
        {
            cards.Add(new(
                CardIds.TaskbarCount,
                General,
                "Show a count on the taskbar button",
                "How many things are waiting for you, as the app says.",
                CardKind.Switch,
                choices.TaskbarCount,
                true,
                null));
        }

        if (Capabilities.Has(offers, SystemFeature.Notifications))
        {
            cards.Add(new(
                CardIds.NotificationSettings,
                Notifications,
                "Notifications in Windows",
                "Choose whether this app's notifications show, make sound or stay in the notification centre.",
                CardKind.Button,
                false,
                true,
                "Open"));
        }
        else
        {
            FeatureOffer? offer = offers.FirstOrDefault(item => item.Feature == SystemFeature.Notifications);
            cards.Add(new(
                CardIds.NotificationSettings,
                Notifications,
                "Notifications are not available",
                offer?.Reason ?? "This build cannot show notifications.",
                CardKind.Info,
                false,
                false,
                null));
        }

        return cards;
    }

    /// <summary>The lines of the About section: what the build is and what it can do here.</summary>
    /// <param name="app">The app's name.</param>
    /// <param name="version">Its version.</param>
    /// <param name="hasIdentity">Whether it has package identity.</param>
    /// <param name="offers">What this run offers.</param>
    /// <returns>Pairs of a title and a line.</returns>
    public static IReadOnlyList<(string Title, string Line)> About(string app, string version, bool hasIdentity, IReadOnlyList<FeatureOffer> offers)
    {
        List<(string, string)> lines =
        [
            (app, "Version " + version),
            ("Installed from a package", hasIdentity ? "Yes" : "No: it runs from a folder, so the features that need a package are off"),
        ];
        foreach (FeatureOffer offer in offers)
        {
            lines.Add((Words(offer.Feature), offer.Offered ? "On offer, with " + offer.Means : "Not on offer: " + offer.Reason));
        }

        return lines;
    }

    /// <summary>What a feature is called on the page.</summary>
    /// <param name="feature">The feature.</param>
    /// <returns>The words.</returns>
    public static string Words(SystemFeature feature) =>
        feature switch
        {
            SystemFeature.NotificationAreaIcon => "Notification-area icon",
            SystemFeature.StartAtSignIn => "Open at sign-in",
            SystemFeature.TaskbarCount => "Count on the taskbar",
            SystemFeature.Notifications => "Notifications",
            SystemFeature.JumpList => "Jump list",
            SystemFeature.Share => "Share",
            SystemFeature.SeveralWindows => "Several windows",
            SystemFeature.ProtocolReturn => "Return from signing in in the browser",
            SystemFeature.SingleInstance => "One running app",
            SystemFeature.FullScreen => "Full screen",
            SystemFeature.Passkeys => "Windows Hello and passkeys",
            _ => feature.ToString(),
        };
}
