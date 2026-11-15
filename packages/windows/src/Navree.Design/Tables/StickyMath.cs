namespace Navree.Design.Tables;

/// <summary>
/// What keeps the header, the frozen columns and the open details in place while the rows scroll
/// sideways. The scroll view's positions are zero or more in left to right and zero or less in
/// right to left. A translation and a clip on an element are counted from the inline start in
/// both flows (a translation is toward the end, a left inset cuts from the start), so every number
/// here is first made into how far the rows have scrolled from the start, which is the position in
/// left to right and minus the position in right to left.
/// </summary>
public static class StickyMath
{
    private const string Reach = "(scroll.MaxPosition.X - scroll.MinPosition.X)";

    /// <summary>The range the scroll position takes for a row and a viewport.</summary>
    /// <param name="rowWidth">The width of a row.</param>
    /// <param name="viewport">The width of the viewport.</param>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>The least and the greatest position.</returns>
    public static (double Min, double Max) PositionRange(double rowWidth, double viewport, bool rtl)
    {
        double reach = Math.Max(0, rowWidth - viewport);
        return rtl ? (-reach, 0) : (0, reach);
    }

    /// <summary>How far the rows have scrolled from the start.</summary>
    /// <param name="position">The scroll position.</param>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>Zero or more.</returns>
    public static double Scrolled(double position, bool rtl) => rtl ? -position : position;

    /// <summary>How far to move what is kept at the start edge, toward the end: as far as the rows have scrolled.</summary>
    /// <param name="position">The scroll position.</param>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>The translation.</returns>
    public static double StartTranslation(double position, bool rtl) => Scrolled(position, rtl);

    /// <summary>How far to move what is kept at the end edge, toward the end.</summary>
    /// <param name="position">The scroll position.</param>
    /// <param name="min">The least position.</param>
    /// <param name="max">The greatest position.</param>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>The translation.</returns>
    public static double EndTranslation(double position, double min, double max, bool rtl) => Scrolled(position, rtl) - (max - min);

    /// <summary>How far to move the header row, which is outside the scrolling content and must follow it.</summary>
    /// <param name="position">The scroll position.</param>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>The translation.</returns>
    public static double HeaderTranslation(double position, bool rtl) => -Scrolled(position, rtl);

    /// <summary>How much of the scrolling cells to cut away at each edge, in the cells' own coordinates, from the start and from the end.</summary>
    /// <param name="position">The scroll position.</param>
    /// <param name="min">The least position.</param>
    /// <param name="max">The greatest position.</param>
    /// <param name="fs">The width kept at the start.</param>
    /// <param name="fe">The width kept at the end.</param>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>The inset at the start (the clip's left inset) and at the end (its right inset).</returns>
    public static (double Left, double Right) ScrollClipInsets(double position, double min, double max, double fs, double fe, bool rtl)
    {
        double scrolled = Scrolled(position, rtl);
        return (fs + scrolled, fe + (max - min) - scrolled);
    }

    /// <summary>How visible the line at the start edge is: it fades in over the first 8 pixels of scroll.</summary>
    /// <param name="position">The scroll position.</param>
    /// <param name="min">The least position.</param>
    /// <param name="max">The greatest position.</param>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>An opacity from zero to one.</returns>
    public static double EdgeOpacityStart(double position, double min, double max, bool rtl) =>
        Math.Clamp(Scrolled(position, rtl) / 8, 0, 1);

    /// <summary>How visible the line at the end edge is: it fades out over the last 8 pixels of scroll.</summary>
    /// <param name="position">The scroll position.</param>
    /// <param name="min">The least position.</param>
    /// <param name="max">The greatest position.</param>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>An opacity from zero to one.</returns>
    public static double EdgeOpacityEnd(double position, double min, double max, bool rtl) =>
        Math.Clamp(((max - min) - Scrolled(position, rtl)) / 8, 0, 1);

    /// <summary>The expression that moves what is kept at the start edge.</summary>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>An expression over the names <c>scroll</c> and <c>layout</c>.</returns>
    public static string StartExpression(bool rtl) => ScrolledExpression(rtl);

    /// <summary>The expression that moves what is kept at the end edge.</summary>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>An expression over the names <c>scroll</c> and <c>layout</c>.</returns>
    public static string EndExpression(bool rtl) => ScrolledExpression(rtl) + " - " + Reach;

    /// <summary>The expression that moves the header row.</summary>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>An expression over the names <c>scroll</c> and <c>layout</c>.</returns>
    public static string HeaderExpression(bool rtl) => "-" + ScrolledExpression(rtl);

    /// <summary>The expressions of the two insets of the clip on the scrolling cells.</summary>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>The left (start) and the right (end) inset.</returns>
    public static (string Left, string Right) ClipExpressions(bool rtl) =>
        ("layout.Fs + " + ScrolledExpression(rtl), "layout.Fe + " + Reach + " - " + ScrolledExpression(rtl));

    /// <summary>The expressions of the opacity of the lines at the start and the end edge.</summary>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>The start and the end expression.</returns>
    public static (string Start, string End) EdgeExpressions(bool rtl) =>
        ("Clamp(" + ScrolledExpression(rtl) + " / 8, 0, 1)", "Clamp((" + Reach + " - " + ScrolledExpression(rtl) + ") / 8, 0, 1)");

    private static string ScrolledExpression(bool rtl) => rtl ? "(-scroll.Position.X)" : "scroll.Position.X";
}
