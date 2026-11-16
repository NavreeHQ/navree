namespace Navree.Design;

/// <summary>Where a card stands after one step: possible or not, and the card it then stands before (none: at the end of its lane).</summary>
/// <param name="Possible">Whether there is a place to go.</param>
/// <param name="Before">The card it stands before, or none for the end.</param>
public readonly record struct CardStep(bool Possible, string? Before)
{
    /// <summary>No place to go.</summary>
    public static CardStep Nowhere { get; } = new(false, null);
}

/// <summary>
/// The steps a card is moved by with the keys and by its handle's menu: one place up, one place down, and into the next lane across. Each
/// says the card it would stand before, which is what the core is told; a move by dragging says the same thing by where the card was let go.
/// </summary>
public static class CardMoves
{
    /// <summary>One place up: before the card above. There is none from the top.</summary>
    /// <param name="ids">The cards of the lane, top first.</param>
    /// <param name="index">The place of the card.</param>
    /// <returns>The step.</returns>
    public static CardStep Up(IReadOnlyList<string> ids, int index) =>
        index > 0 && index < ids.Count ? new CardStep(true, ids[index - 1]) : CardStep.Nowhere;

    /// <summary>One place down: before the card two below, or at the end when the next is the last. There is none from the bottom.</summary>
    /// <param name="ids">The cards of the lane, top first.</param>
    /// <param name="index">The place of the card.</param>
    /// <returns>The step.</returns>
    public static CardStep Down(IReadOnlyList<string> ids, int index) =>
        index >= 0 && index < ids.Count - 1 ? new CardStep(true, index + 2 < ids.Count ? ids[index + 2] : null) : CardStep.Nowhere;

    /// <summary>Into the next lane across: at the same height where that lane has a card there, else at its end.</summary>
    /// <param name="there">The cards of the lane it goes to, top first.</param>
    /// <param name="index">The place of the card in the lane it leaves.</param>
    /// <returns>The step.</returns>
    public static CardStep Across(IReadOnlyList<string> there, int index) =>
        new(true, index >= 0 && index < there.Count ? there[index] : null);

    /// <summary>The lane across from this one, by its place among the lanes; none past either end.</summary>
    /// <param name="lane">The place of this lane.</param>
    /// <param name="lanes">How many lanes there are.</param>
    /// <param name="step">-1 for the one before, 1 for the one after.</param>
    /// <returns>The place of the other lane, or -1.</returns>
    public static int Neighbour(int lane, int lanes, int step)
    {
        int at = lane + step;
        return lane >= 0 && at >= 0 && at < lanes ? at : -1;
    }

    /// <summary>Where a card let go at a height stands in a lane: before the first card whose middle is below it, none for the end.</summary>
    /// <param name="ids">The cards of the lane, top first.</param>
    /// <param name="middles">The height of the middle of each card, in the same order.</param>
    /// <param name="y">The height it was let go at.</param>
    /// <returns>The card it stands before, or none.</returns>
    public static string? DropBefore(IReadOnlyList<string> ids, IReadOnlyList<double> middles, double y)
    {
        for (int at = 0; at < ids.Count && at < middles.Count; at++)
        {
            if (middles[at] > y)
            {
                return ids[at];
            }
        }

        return null;
    }
}
