using System.Globalization;

namespace Navree.Design.Tables;

/// <summary>Whether a row can be chosen.</summary>
public enum Choosable
{
    /// <summary>It has a working box.</summary>
    Yes,

    /// <summary>It has a box that does not work: the server said the actions on chosen rows do not apply to it.</summary>
    No,

    /// <summary>It has no box: it has no id to send.</summary>
    None,
}

/// <summary>The state of the box in the header.</summary>
public enum HeaderBoxState
{
    /// <summary>No row that can be chosen is.</summary>
    Off,

    /// <summary>Every row that can be chosen is.</summary>
    On,

    /// <summary>Some are.</summary>
    Indeterminate,
}

/// <summary>What a click on a row means.</summary>
public enum ClickMeaning
{
    /// <summary>It runs the row's action.</summary>
    Open,

    /// <summary>It chooses or clears the row.</summary>
    Toggle,

    /// <summary>It chooses a range of rows.</summary>
    Range,

    /// <summary>It only makes the row the active one.</summary>
    Focus,
}

/// <summary>Which rows are chosen.</summary>
/// <param name="Keys">The keys of the rows chosen, when <paramref name="Across"/> is off.</param>
/// <param name="Across">Whether every record that matches the query is chosen, held or not.</param>
public readonly record struct ChoiceState(IReadOnlySet<string> Keys, bool Across);

/// <summary>Choosing rows: ranges, "all", and what a click means.</summary>
public static class Choosing
{
    /// <summary>The rows between an anchor and a target, inclusive, that can be chosen.</summary>
    /// <param name="rows">Whether each held row can be chosen.</param>
    /// <param name="anchor">The anchor's index, or minus one for none.</param>
    /// <param name="target">The target's index.</param>
    /// <returns>The indices, ascending.</returns>
    public static IReadOnlyList<int> Range(IReadOnlyList<Choosable> rows, int anchor, int target)
    {
        ArgumentNullException.ThrowIfNull(rows);
        if (target < 0 || target >= rows.Count || anchor >= rows.Count || anchor < -1)
        {
            throw new ArgumentOutOfRangeException(nameof(target), target, "The rows are not there.");
        }

        int from = anchor < 0 ? target : Math.Min(anchor, target);
        int to = anchor < 0 ? target : Math.Max(anchor, target);
        List<int> chosen = [];
        for (int i = from; i <= to; i++)
        {
            if (rows[i] == Choosable.Yes)
            {
                chosen.Add(i);
            }
        }

        return chosen;
    }

    /// <summary>The state of the box in the header.</summary>
    /// <param name="choosableHeld">How many held rows can be chosen.</param>
    /// <param name="chosenHeld">How many of them are chosen.</param>
    /// <returns>The state.</returns>
    public static HeaderBoxState HeaderBox(int choosableHeld, int chosenHeld)
    {
        if (choosableHeld <= 0 || chosenHeld <= 0)
        {
            return HeaderBoxState.Off;
        }

        return chosenHeld >= choosableHeld ? HeaderBoxState.On : HeaderBoxState.Indeterminate;
    }

    /// <summary>Whether to offer "choose all N": every record that matches, not only the rows held.</summary>
    /// <param name="acrossAvailable">Whether the table's selection allows it.</param>
    /// <param name="acrossOn">Whether it is already taken.</param>
    /// <param name="choosableHeld">How many held rows can be chosen.</param>
    /// <param name="chosenHeld">How many of them are chosen.</param>
    /// <returns>True when it should be offered.</returns>
    public static bool OfferAcross(bool acrossAvailable, bool acrossOn, int choosableHeld, int chosenHeld) =>
        acrossAvailable && !acrossOn && choosableHeld > 0 && chosenHeld >= choosableHeld;

    /// <summary>The choice after a row is cleared: a choice of every record becomes the rows held, less the one.</summary>
    /// <param name="state">The choice now.</param>
    /// <param name="cleared">The key of the row cleared.</param>
    /// <param name="choosableHeldKeys">The keys of the held rows that can be chosen.</param>
    /// <returns>The new choice.</returns>
    public static ChoiceState AfterClear(ChoiceState state, string cleared, IReadOnlyList<string> choosableHeldKeys)
    {
        ArgumentNullException.ThrowIfNull(cleared);
        ArgumentNullException.ThrowIfNull(choosableHeldKeys);
        HashSet<string> keys = state.Across ? new HashSet<string>(choosableHeldKeys) : new HashSet<string>(state.Keys);
        keys.Remove(cleared);
        return new ChoiceState(keys, false);
    }

    /// <summary>What a click on a row means.</summary>
    /// <param name="anyChosen">Whether any row is chosen.</param>
    /// <param name="ctrl">Whether Control is held.</param>
    /// <param name="shift">Whether Shift is held.</param>
    /// <param name="onBox">Whether the click is on the row's box.</param>
    /// <param name="hasAction">Whether the row has an action.</param>
    /// <returns>The meaning.</returns>
    public static ClickMeaning Click(bool anyChosen, bool ctrl, bool shift, bool onBox, bool hasAction)
    {
        if (shift)
        {
            return ClickMeaning.Range;
        }

        if (ctrl || onBox || anyChosen)
        {
            return ClickMeaning.Toggle;
        }

        return hasAction ? ClickMeaning.Open : ClickMeaning.Focus;
    }

    /// <summary>A count written as a person's culture writes it: lakhs for Indian English.</summary>
    /// <param name="n">The number.</param>
    /// <param name="culture">The culture.</param>
    /// <returns>The number with its digits grouped.</returns>
    public static string FormatCount(long n, CultureInfo culture)
    {
        ArgumentNullException.ThrowIfNull(culture);
        return n.ToString("N0", culture);
    }
}
