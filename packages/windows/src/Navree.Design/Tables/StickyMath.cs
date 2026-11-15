namespace Navree.Design.Tables;

/// <summary>
/// What keeps the header, the frozen columns and the open details in place while the rows scroll
/// sideways. Positions are the scroll view's own: zero or more in left to right and zero or less in
/// right to left, and the content moves by minus the position in both.
/// </summary>
public static class StickyMath
{
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

    /// <summary>How far to move what is kept at the start edge: the scroll's own position.</summary>
    /// <param name="position">The scroll position.</param>
    /// <returns>The translation.</returns>
    public static double StartTranslation(double position) => position;

    /// <summary>How far to move what is kept at the end edge.</summary>
    /// <param name="position">The scroll position.</param>
    /// <param name="min">The least position.</param>
    /// <param name="max">The greatest position.</param>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>The translation.</returns>
    public static double EndTranslation(double position, double min, double max, bool rtl) => position - (rtl ? min : max);

    /// <summary>How far to move the header row, which is outside the scrolling content and must follow it.</summary>
    /// <param name="position">The scroll position.</param>
    /// <returns>The translation.</returns>
    public static double HeaderTranslation(double position) => -position;

    /// <summary>How much of the scrolling cells to cut away at each edge, in the cells' own coordinates.</summary>
    /// <param name="position">The scroll position.</param>
    /// <param name="min">The least position.</param>
    /// <param name="max">The greatest position.</param>
    /// <param name="fs">The width kept at the start.</param>
    /// <param name="fe">The width kept at the end.</param>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>The inset at the physical left and at the physical right.</returns>
    public static (double Left, double Right) ScrollClipInsets(double position, double min, double max, double fs, double fe, bool rtl)
    {
        double left = (rtl ? fe : fs) + (position - min);
        double right = (rtl ? fs : fe) + (max - position);
        return (left, right);
    }

    /// <summary>How visible the line at the start edge is: it fades in over the first 8 pixels of scroll.</summary>
    /// <param name="position">The scroll position.</param>
    /// <param name="min">The least position.</param>
    /// <param name="max">The greatest position.</param>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>An opacity from zero to one.</returns>
    public static double EdgeOpacityStart(double position, double min, double max, bool rtl) =>
        Math.Clamp((rtl ? max - position : position - min) / 8, 0, 1);

    /// <summary>How visible the line at the end edge is: it fades out over the last 8 pixels of scroll.</summary>
    /// <param name="position">The scroll position.</param>
    /// <param name="min">The least position.</param>
    /// <param name="max">The greatest position.</param>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>An opacity from zero to one.</returns>
    public static double EdgeOpacityEnd(double position, double min, double max, bool rtl) =>
        Math.Clamp((rtl ? position - min : max - position) / 8, 0, 1);

    /// <summary>The expression that moves what is kept at the start edge.</summary>
    /// <param name="rtl">Whether the flow is right to left; the expression is the same in both.</param>
    /// <returns>An expression over the names <c>scroll</c> and <c>layout</c>.</returns>
    public static string StartExpression(bool rtl) => StartPosition;

    private const string StartPosition = "scroll.Position.X";

    /// <summary>The expression that moves what is kept at the end edge.</summary>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>An expression over the names <c>scroll</c> and <c>layout</c>.</returns>
    public static string EndExpression(bool rtl) =>
        rtl ? "scroll.Position.X - scroll.MinPosition.X" : "scroll.Position.X - scroll.MaxPosition.X";

    /// <summary>The expression that moves the header row.</summary>
    /// <returns>An expression over the names <c>scroll</c> and <c>layout</c>.</returns>
    public static string HeaderExpression() => "-scroll.Position.X";

    /// <summary>The expressions of the two insets of the clip on the scrolling cells.</summary>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>The left and the right inset.</returns>
    public static (string Left, string Right) ClipExpressions(bool rtl) => rtl
        ? ("layout.Fe + scroll.Position.X - scroll.MinPosition.X", "layout.Fs + scroll.MaxPosition.X - scroll.Position.X")
        : ("layout.Fs + scroll.Position.X - scroll.MinPosition.X", "layout.Fe + scroll.MaxPosition.X - scroll.Position.X");

    /// <summary>The expressions of the opacity of the lines at the start and the end edge.</summary>
    /// <param name="rtl">Whether the flow is right to left.</param>
    /// <returns>The start and the end expression.</returns>
    public static (string Start, string End) EdgeExpressions(bool rtl) => rtl
        ? ("Clamp((scroll.MaxPosition.X - scroll.Position.X) / 8, 0, 1)", "Clamp((scroll.Position.X - scroll.MinPosition.X) / 8, 0, 1)")
        : ("Clamp((scroll.Position.X - scroll.MinPosition.X) / 8, 0, 1)", "Clamp((scroll.MaxPosition.X - scroll.Position.X) / 8, 0, 1)");
}
