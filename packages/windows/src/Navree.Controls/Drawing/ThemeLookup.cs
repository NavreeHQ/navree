using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;

namespace Navree.Controls.Drawing;

/// <summary>
/// Finds a theme resource for the theme an element stands in. The application's resources answer for the application's theme; an element
/// that has its own theme (a light card in a dark window) needs the answer of its own, which is in the same dictionaries under the key of
/// its theme (<c>Light</c> or <c>Dark</c>, where <c>Dark</c> may be kept under <c>Default</c>).
/// </summary>
internal static class ThemeLookup
{
    private const int Depth = 8;
    private static readonly Dictionary<(string Theme, string Key), (int Version, Brush? Brush)> Cache = [];

    /// <summary>The brush of a resource for a theme, or none where the resources have no such brush.</summary>
    /// <param name="key">The resource's key, such as <c>TextFillColorPrimaryBrush</c>.</param>
    /// <param name="theme">The theme: <c>Light</c> or <c>Dark</c>.</param>
    /// <returns>The brush; a plain brush is a copy that no one else holds.</returns>
    internal static Brush? Brush(string key, string theme)
    {
        int version = ThemeState.Version;
        if (Cache.TryGetValue((theme, key), out (int Version, Brush? Brush) known) && known.Version == version)
        {
            return known.Brush;
        }

        Brush? found = Find(Application.Current.Resources, key, theme, 0) as Brush;

        // A colour that is nothing at all is a resource that was not worked out where it was found: the application's own answer is taken instead.
        if (found is null or SolidColorBrush { Color.A: 0 })
        {
            found = Application.Current.Resources.TryGetValue(key, out object? plain) ? plain as Brush : null;
        }

        // A plain brush is copied: the resource's own brush is shared with every control that uses it, and is theirs to change.
        Brush? made = found is SolidColorBrush solid ? new SolidColorBrush(solid.Color) { Opacity = solid.Opacity } : found;
        Cache[(theme, key)] = (version, made);
        return made;
    }

    private static object? Find(ResourceDictionary dictionary, string key, string theme, int depth)
    {
        if (depth > Depth)
        {
            return null;
        }

        // The dark theme is kept under Default where no Dark dictionary is given; the light theme is never taken from it.
        foreach (string name in theme == "Dark" ? new[] { "Dark", "Default" } : new[] { theme })
        {
            if (dictionary.ThemeDictionaries.TryGetValue(name, out object? themed)
                && themed is ResourceDictionary inside
                && inside.TryGetValue(key, out object? value))
            {
                return value;
            }
        }

        // A dictionary's own entries win over what it merges, and what is merged later wins over what is merged earlier.
        for (int at = dictionary.MergedDictionaries.Count - 1; at >= 0; at--)
        {
            if (Find(dictionary.MergedDictionaries[at], key, theme, depth + 1) is object inner)
            {
                return inner;
            }
        }

        return null;
    }
}
