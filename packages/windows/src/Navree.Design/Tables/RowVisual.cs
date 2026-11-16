namespace Navree.Design.Tables;

/// <summary>Which visual state a row of a table is in.</summary>
public static class RowVisual
{
    /// <summary>The name of the state a row is drawn in.</summary>
    /// <param name="chosen">Whether the row is chosen.</param>
    /// <param name="pressed">Whether the pointer is pressed on it.</param>
    /// <param name="over">Whether the pointer is over it.</param>
    /// <param name="highContrast">Whether the system's contrast colours are on.</param>
    /// <returns>
    /// In the contrast colours a row that is chosen, pressed or pointed at is the highlight with the highlight's text, the pair the
    /// system gives for selected and interacted-with items; otherwise the state of the light and dark themes.
    /// </returns>
    public static string State(bool chosen, bool pressed, bool over, bool highContrast)
    {
        if (highContrast && (chosen || pressed || over))
        {
            return chosen ? "HighlightedChosen" : "Highlighted";
        }

        return (chosen, pressed, over) switch
        {
            (true, true, _) => "ChosenPressed",
            (true, false, true) => "ChosenPointerOver",
            (true, false, false) => "Chosen",
            (false, true, _) => "Pressed",
            (false, false, true) => "PointerOver",
            _ => "Normal",
        };
    }

    /// <summary>Whether the words of the row take the highlight's text colour: the row stands on the highlight.</summary>
    /// <param name="state">The state's name, from <see cref="State"/>.</param>
    /// <returns>True for the highlighted states.</returns>
    public static bool TextOnHighlight(string state) => state is "Highlighted" or "HighlightedChosen";
}
