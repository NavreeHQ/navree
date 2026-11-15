namespace Navree.Design;

/// <summary>When a drawing is made again.</summary>
public static class PlotRules
{
    /// <summary>A change of size smaller than this is not one a person can see.</summary>
    public const double Tolerance = 0.5;

    /// <summary>
    /// Whether a drawing painted for one size is painted again for another: when it was asked for, when nothing is painted yet, or when its
    /// width or its height changed by more than half a pixel (a chart whose pane grows taller keeps its width and needs its labels and its
    /// plot area made again).
    /// </summary>
    /// <param name="paintedWidth">The width it was painted for, or none (-1).</param>
    /// <param name="paintedHeight">The height it was painted for, or none (-1).</param>
    /// <param name="width">The width it has now.</param>
    /// <param name="height">The height it has now.</param>
    /// <param name="painted">Whether there is something painted.</param>
    /// <param name="forced">Whether a new painting was asked for (the theme changed).</param>
    /// <returns>Whether to paint.</returns>
    public static bool Repaints(double paintedWidth, double paintedHeight, double width, double height, bool painted, bool forced) =>
        forced
        || !painted
        || !(Math.Abs(width - paintedWidth) < Tolerance)
        || !(Math.Abs(height - paintedHeight) < Tolerance);
}
