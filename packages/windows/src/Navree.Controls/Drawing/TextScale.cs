namespace Navree.Controls.Drawing;

/// <summary>How large the person's text is against the default (Settings, Accessibility, Text size), for the sizes of drawings that grow with it.</summary>
internal static class TextScale
{
    /// <summary>The scale of the person's text, 1 to 2.25; 1 where it cannot be read.</summary>
    internal static double Full => Math.Clamp(ThemeState.TextScaleFactor, 1, 2.25);

    /// <summary>The scale, no more than a cap: a drawing grows with the text until it would break.</summary>
    internal static double Capped(double cap) => Math.Min(Full, cap);
}
