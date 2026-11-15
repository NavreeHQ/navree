using Microsoft.UI.Xaml;
using Windows.UI.ViewManagement;

namespace Navree.Controls.Drawing;

/// <summary>
/// What the system says of itself that a drawing follows: whether a contrast theme is on, the colours of one, the accent the person chose
/// and the size of their text. The settings are read through one object of each kind for the whole process, and a change in any of them is
/// announced to whoever is listening, so a drawing is made again when the person changes one.
/// </summary>
internal static class ThemeState
{
    private static readonly object Gate = new();
    private static AccessibilitySettings? access;
    private static UISettings? settings;
    private static bool started;
    private static int version;

    /// <summary>Raised, on a thread of the system's own, when the contrast theme, the accent or the text size changed.</summary>
    internal static event Action? Changed;

    /// <summary>Counts the changes seen, so what was worked out for an earlier one is known to be stale.</summary>
    internal static int Version => Volatile.Read(ref version);

    /// <summary>Whether a contrast theme is on.</summary>
    internal static bool HighContrast
    {
        get
        {
            Start();
            try
            {
                return access?.HighContrast ?? false;
            }
            catch (Exception)
            {
                return false;
            }
        }
    }

    /// <summary>The accent colour the person chose.</summary>
    internal static Windows.UI.Color Accent => Colour(UIColorType.Accent);

    /// <summary>The scale of the person's text against the default, 1 where it cannot be read.</summary>
    internal static double TextScaleFactor
    {
        get
        {
            Start();
            try
            {
                return settings?.TextScaleFactor ?? 1;
            }
            catch (Exception)
            {
                return 1;
            }
        }
    }

    /// <summary>Whether the person turned animations off.</summary>
    internal static bool AnimationsOff
    {
        get
        {
            Start();
            try
            {
                return settings is not null && !settings.AnimationsEnabled;
            }
            catch (Exception)
            {
                return false;
            }
        }
    }

    /// <summary>One of the system's colours: in a contrast theme these are the person's chosen colours for text, ground, highlight and the rest.</summary>
    internal static Windows.UI.Color System(UIElementType element)
    {
        Start();
        try
        {
            return settings?.UIElementColor(element) ?? Fallback(element);
        }
        catch (Exception)
        {
            return Fallback(element);
        }
    }

    private static Windows.UI.Color Colour(UIColorType kind)
    {
        Start();
        try
        {
            return settings?.GetColorValue(kind) ?? Microsoft.UI.Colors.Gray;
        }
        catch (Exception)
        {
            return Microsoft.UI.Colors.Gray;
        }
    }

    // Where the system cannot be asked: white on black, the pair every contrast theme can be read in.
    private static Windows.UI.Color Fallback(UIElementType element) =>
        element is UIElementType.Window or UIElementType.ButtonFace or UIElementType.HighlightText
            ? Microsoft.UI.Colors.Black
            : Microsoft.UI.Colors.White;

    private static void Start()
    {
        if (started)
        {
            return;
        }

        lock (Gate)
        {
            if (started)
            {
                return;
            }

            started = true;
            try
            {
                access = new AccessibilitySettings();
                access.HighContrastChanged += (_, _) => Raise();
            }
            catch (Exception)
            {
                access = null;
            }

            try
            {
                settings = new UISettings();
                settings.ColorValuesChanged += (_, _) => Raise();
                settings.TextScaleFactorChanged += (_, _) => Raise();
            }
            catch (Exception)
            {
                settings = null;
            }
        }
    }

    private static void Raise()
    {
        Interlocked.Increment(ref version);
        Changed?.Invoke();
    }

    /// <summary>Starts listening, so the first change is not missed.</summary>
    internal static void Watch() => Start();

    /// <summary>Runs an action on an element's own thread, whichever thread it is called from.</summary>
    internal static void OnThreadOf(FrameworkElement element, Action action)
    {
        Microsoft.UI.Dispatching.DispatcherQueue? queue = element.DispatcherQueue;
        if (queue is null)
        {
            return;
        }

        if (queue.HasThreadAccess)
        {
            action();
        }
        else
        {
            queue.TryEnqueue(() => action());
        }
    }
}
