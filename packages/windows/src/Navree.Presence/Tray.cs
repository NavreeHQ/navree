namespace Navree.Presence;

/// <summary>What an entry of the notification-area icon's menu does.</summary>
public enum TrayCommand
{
    /// <summary>Opens the app's own window.</summary>
    OpenWindow,

    /// <summary>Opens the app's window at a destination.</summary>
    Destination,

    /// <summary>Opens the app's settings.</summary>
    Settings,

    /// <summary>Ends the app.</summary>
    Quit,

    /// <summary>A line between groups.</summary>
    Separator,
}

/// <summary>One entry of the icon's menu.</summary>
/// <param name="Command">What it does.</param>
/// <param name="Label">What it says.</param>
/// <param name="Index">The destination, for <see cref="TrayCommand.Destination"/>.</param>
public sealed record TrayEntry(TrayCommand Command, string Label, int Index);

/// <summary>A rectangle in screen pixels, the way the system gives one.</summary>
/// <param name="Left">Its left edge.</param>
/// <param name="Top">Its top edge.</param>
/// <param name="Right">Its right edge.</param>
/// <param name="Bottom">Its bottom edge.</param>
public readonly record struct Box(int Left, int Top, int Right, int Bottom)
{
    /// <summary>Its width.</summary>
    public int Width => Right - Left;

    /// <summary>Its height.</summary>
    public int Height => Bottom - Top;

    /// <summary>The middle across.</summary>
    public int CentreX => Left + Width / 2;

    /// <summary>The middle down.</summary>
    public int CentreY => Top + Height / 2;
}

/// <summary>The notification-area icon and the small window by it, as plain rules.</summary>
public static class TrayRules
{
    /// <summary>The room kept between the small window and the icon, and between it and the edge of the screen.</summary>
    public const int Gap = 8;

    /// <summary>The most characters the system's tooltip holds, less the one it ends with.</summary>
    public const int TooltipLongest = 127;

    /// <summary>The menu the icon's right click opens: the window, the destinations, settings, quit.</summary>
    /// <param name="destinations">The app's destinations.</param>
    /// <param name="app">The app's name.</param>
    /// <returns>The entries, in order.</returns>
    public static IReadOnlyList<TrayEntry> Menu(IReadOnlyList<PlaceDestination> destinations, string app)
    {
        List<TrayEntry> entries = [new(TrayCommand.OpenWindow, "Open " + app, -1)];
        if (destinations.Count > 0)
        {
            entries.Add(new(TrayCommand.Separator, string.Empty, -1));
            for (int at = 0; at < destinations.Count; at++)
            {
                PlaceDestination destination = destinations[at];
                string badge = int.TryParse(destination.Badge, out int count) && count > 0 ? "  (" + count + ")" : string.Empty;
                entries.Add(new(TrayCommand.Destination, destination.Label + badge, at));
            }
        }

        entries.Add(new(TrayCommand.Separator, string.Empty, -1));
        entries.Add(new(TrayCommand.Settings, "Settings", -1));
        entries.Add(new(TrayCommand.Quit, "Quit " + app, -1));
        return entries;
    }

    /// <summary>What the icon's tooltip says: the app's name, and the count when there is one.</summary>
    /// <param name="app">The app's name.</param>
    /// <param name="count">The count.</param>
    /// <returns>The words, no longer than the tooltip holds.</returns>
    public static string Tooltip(string app, long? count)
    {
        string text = BadgeRules.Description(app, count);
        return text.Length <= TooltipLongest ? text : text[..TooltipLongest];
    }

    /// <summary>
    /// Where the small window goes: centred on the icon across, kept inside the work area, above the icon when the icon is in the lower half
    /// of the screen (a taskbar at the bottom) and below it otherwise.
    /// </summary>
    /// <param name="icon">The icon's rectangle.</param>
    /// <param name="width">The window's width.</param>
    /// <param name="height">The window's height.</param>
    /// <param name="work">The part of the screen the taskbar leaves.</param>
    /// <returns>The window's left and top.</returns>
    public static (int Left, int Top) Place(Box icon, int width, int height, Box work)
    {
        int left = icon.CentreX - width / 2;
        int top = icon.CentreY > work.CentreY ? icon.Top - height - Gap : icon.Bottom + Gap;
        left = Math.Clamp(left, work.Left + Gap, Math.Max(work.Left + Gap, work.Right - width - Gap));
        top = Math.Clamp(top, work.Top + Gap, Math.Max(work.Top + Gap, work.Bottom - height - Gap));
        return (left, top);
    }

    /// <summary>Whether closing the last window leaves the app running in the notification area: the icon is there and the person wants it.</summary>
    /// <param name="iconShown">Whether the icon is in the notification area.</param>
    /// <param name="keepRunning">Whether the person chose to keep the app running.</param>
    /// <returns>Whether closing the window only hides it.</returns>
    public static bool ClosingHides(bool iconShown, bool keepRunning) => iconShown && keepRunning;
}
