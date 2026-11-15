using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using Navree.Design;
using Navree.Protocol;
using Windows.UI.ViewManagement;

namespace Navree.Controls.Drawing;

/// <summary>
/// The colours a drawn kind uses, for the theme it stands in. Words, lines and surfaces are the system's own brushes for the running
/// theme; the marks take the hue the page names (the system has names for the accent and for status only). In a contrast theme every
/// mark is drawn in the system's colours and never in ours.
/// </summary>
internal sealed class Palette
{
    private readonly bool contrast;

    /// <summary>Reads the theme an element stands in.</summary>
    internal Palette(FrameworkElement owner)
    {
        // An element not yet in the tree has no theme of its own: it takes the app's.
        Dark = owner.ActualTheme switch
        {
            ElementTheme.Dark => true,
            ElementTheme.Light => false,
            _ => Application.Current.RequestedTheme == ApplicationTheme.Dark,
        };
        contrast = IsContrast();
    }

    /// <summary>Whether the ground is dark.</summary>
    internal bool Dark { get; }

    /// <summary>Whether a contrast theme is on.</summary>
    internal bool Contrast => contrast;

    /// <summary>Words.</summary>
    internal Brush Primary => Resource("TextFillColorPrimaryBrush");

    /// <summary>Words said in passing.</summary>
    internal Brush Secondary => Resource("TextFillColorSecondaryBrush");

    /// <summary>A hairline between things.</summary>
    internal Brush Divider => Resource("DividerStrokeColorDefaultBrush");

    /// <summary>A faint wash of the text colour, to lay over what is being read out.</summary>
    internal Brush Wash =>
        Application.Current.Resources.TryGetValue("TextFillColorPrimaryBrush", out object? value) && value is SolidColorBrush found
            ? new SolidColorBrush(found.Color) { Opacity = 0.06 }
            : new SolidColorBrush(Microsoft.UI.Colors.Gray) { Opacity = 0.06 };

    /// <summary>The surface of a card.</summary>
    internal Brush Card => Resource("CardBackgroundFillColorDefaultBrush");

    /// <summary>The edge of a card.</summary>
    internal Brush CardEdge => Resource("CardStrokeColorDefaultBrush");

    /// <summary>The surface of a small floating note.</summary>
    internal Brush Flyout => Resource("AcrylicInAppFillColorDefaultBrush");

    /// <summary>The edge of a small floating note.</summary>
    internal Brush FlyoutEdge => Resource("SurfaceStrokeColorFlyoutBrush");

    /// <summary>The system's brush of that name, or a plain grey where it has none.</summary>
    internal static Brush Resource(string key) =>
        Application.Current.Resources.TryGetValue(key, out object? value) && value is Brush found
            ? found
            : new SolidColorBrush(Microsoft.UI.Colors.Gray);

    /// <summary>The colour a hue is drawn in.</summary>
    internal Windows.UI.Color Of(Models.Hue hue)
    {
        if (contrast)
        {
            return Ink.Colour(Rgb.Of(ContrastMark(hue)));
        }

        Swatch swatch = HueRules.Of(hue);
        if (swatch.FromTheme && TryTheme(hue, out Windows.UI.Color themed))
        {
            return themed;
        }

        return Ink.Colour(swatch.For(Dark));
    }

    /// <summary>The colour of a part's own colour, or of a fallback hue when the page names none.</summary>
    internal Windows.UI.Color Of(Models.Colour? colour, Models.Hue fallback)
    {
        if (colour is not null && !contrast && HueRules.Fallback(colour, Dark) is Rgb future)
        {
            return Ink.Colour(future);
        }

        return Of(colour?.Hue ?? fallback);
    }

    /// <summary>The colour a blend ends in: the page's <c>to</c> hue, or the colour itself.</summary>
    internal Windows.UI.Color End(Models.Colour? colour, Models.Hue fallback) =>
        colour?.To is Models.Hue to ? Of(to) : Of(colour, fallback);

    /// <summary>A plain brush of a colour of a part.</summary>
    internal SolidColorBrush Fill(Models.Colour? colour, Models.Hue fallback, double opacity = 1) => Mark(Of(colour, fallback), opacity);

    /// <summary>A plain brush of a hue.</summary>
    internal SolidColorBrush Fill(Models.Hue hue, double opacity = 1) => Mark(Of(hue), opacity);

    /// <summary>A brush of a colour at an opacity.</summary>
    internal static SolidColorBrush Mark(Windows.UI.Color colour, double opacity = 1) => new(colour) { Opacity = opacity };

    /// <summary>The brush for words in a part's colour: as dark as it needs to be to read on a light ground.</summary>
    internal Brush Words(Models.Colour? colour, Models.Hue fallback)
    {
        if (contrast)
        {
            return Mark(Of(colour, fallback));
        }

        Windows.UI.Color mark = Of(colour, fallback);
        Rgb rgb = new(mark.R / 255.0, mark.G / 255.0, mark.B / 255.0);
        return new SolidColorBrush(Ink.Colour(ContrastRules.ForWords(rgb, Dark)));
    }

    /// <summary>The brush for words in the colour of a tone.</summary>
    internal Brush Words(Models.Tone? tone) => Words(null, HueRules.OfTone(tone));

    /// <summary>The hue of the series at a place.</summary>
    internal static Models.Hue SeriesHue(int index) => HueRules.SeriesHue(index);

    /// <summary>The colour of a track a mark runs along: the mark's own hue, faint.</summary>
    internal SolidColorBrush Track(Windows.UI.Color colour) => Mark(colour, Dark ? 0.26 : 0.18);

    // The accent the person chose, and the secondary text colour, are the system's; asked of the theme's resources.
    private bool TryTheme(Models.Hue hue, out Windows.UI.Color colour)
    {
        string key = hue == Models.Hue.Accent ? "AccentFillColorDefaultBrush" : "TextFillColorSecondaryBrush";
        if (Application.Current.Resources.TryGetValue(key, out object? value) && value is SolidColorBrush brush)
        {
            colour = brush.Color;
            return true;
        }

        colour = default;
        return false;
    }

    // In a contrast theme the marks are the system's: the text colour, the highlight, the link colour and the disabled text colour.
    private string ContrastMark(Models.Hue hue)
    {
        string key = hue switch
        {
            Models.Hue.Neutral or Models.Hue.Slate => "TextFillColorSecondaryBrush",
            Models.Hue.Accent or Models.Hue.Blue or Models.Hue.Info => "AccentFillColorDefaultBrush",
            Models.Hue.Green or Models.Hue.Success or Models.Hue.Violet or Models.Hue.Cyan => "AccentTextFillColorPrimaryBrush",
            _ => "TextFillColorPrimaryBrush",
        };
        if (Application.Current.Resources.TryGetValue(key, out object? value) && value is SolidColorBrush brush)
        {
            return "#" + brush.Color.R.ToString("X2", System.Globalization.CultureInfo.InvariantCulture)
                + brush.Color.G.ToString("X2", System.Globalization.CultureInfo.InvariantCulture)
                + brush.Color.B.ToString("X2", System.Globalization.CultureInfo.InvariantCulture);
        }

        return Dark ? "#FFFFFF" : "#000000";
    }

    private static bool IsContrast()
    {
        try
        {
            return new AccessibilitySettings().HighContrast;
        }
        catch (Exception)
        {
            return false;
        }
    }
}
