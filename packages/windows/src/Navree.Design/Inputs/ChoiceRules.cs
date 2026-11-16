using Navree.Protocol;

namespace Navree.Design.Inputs;

/// <summary>What a line of the list a searched choice offers is.</summary>
public enum SuggestionKind
{
    /// <summary>An option the person may choose.</summary>
    Option,

    /// <summary>An option that is shown and cannot be chosen.</summary>
    Disabled,

    /// <summary>Not an option: the line that asks for more of the list.</summary>
    More,

    /// <summary>Not an option: the line that makes a new one from what was typed.</summary>
    Create,
}

/// <summary>
/// One line of the list a searched choice offers: an option with the value the server gave it, the line that asks for more, or the line that makes
/// a new one. It is known by which line it is, never by its words: two options with the same words are two lines with two values.
/// </summary>
public sealed class Suggestion
{
    /// <summary>Makes a line.</summary>
    /// <param name="kind">What it is.</param>
    /// <param name="label">The words shown for it.</param>
    /// <param name="value">The value sent when it is chosen; none for a line that is not an option.</param>
    public Suggestion(SuggestionKind kind, string label, string? value)
    {
        Kind = kind;
        Label = label;
        Value = value;
    }

    /// <summary>What it is.</summary>
    public SuggestionKind Kind { get; }

    /// <summary>The words shown for it.</summary>
    public string Label { get; }

    /// <summary>The value sent when it is chosen.</summary>
    public string? Value { get; }

    /// <summary>The words the list shows for the line.</summary>
    /// <returns>Its label.</returns>
    public override string ToString() => Label;
}

/// <summary>What choosing a line of a searched choice's list does.</summary>
public enum ChoiceAct
{
    /// <summary>Nothing: it was not a line of the list that is offered now, or it cannot be chosen.</summary>
    Nothing,

    /// <summary>The option's value is sent.</summary>
    Send,

    /// <summary>More of the list is asked for.</summary>
    More,

    /// <summary>A new one is made.</summary>
    Create,
}

/// <summary>A choice is found by what it is, never by what it says: the rules of the lists a choice field offers.</summary>
public static class ChoiceRules
{
    /// <summary>The words of the line that asks for more of a list.</summary>
    public const string MoreWords = "Show more";

    /// <summary>The list a searched choice offers: the options in their order, each with its own value, then the line for more, then the line that makes a new one.</summary>
    /// <param name="options">The options found.</param>
    /// <param name="more">Whether the list goes on.</param>
    /// <param name="createWords">The words of the way to make a new one, or null when the field has none.</param>
    /// <param name="typed">What the person typed.</param>
    /// <returns>The lines, first first.</returns>
    public static IReadOnlyList<Suggestion> Offer(IReadOnlyList<Models.ChoiceOption> options, bool more, string? createWords, string? typed)
    {
        ArgumentNullException.ThrowIfNull(options);
        List<Suggestion> lines = [];
        foreach (Models.ChoiceOption option in options)
        {
            lines.Add(new Suggestion(option.Disabled ? SuggestionKind.Disabled : SuggestionKind.Option, option.Label, option.Value));
        }

        if (more)
        {
            lines.Add(new Suggestion(SuggestionKind.More, MoreWords, null));
        }

        if (createWords is not null && !string.IsNullOrEmpty(typed))
        {
            lines.Add(new Suggestion(SuggestionKind.Create, createWords + ": " + typed, null));
        }

        return lines;
    }

    /// <summary>
    /// What choosing does. The line is the one that was offered, the very same one: a thing that only reads the same (its words, another list's
    /// line) is not it, and nothing is done.
    /// </summary>
    /// <param name="offered">The list that is offered now.</param>
    /// <param name="chosen">What the control says was chosen.</param>
    /// <returns>What is done, and the line it is done for.</returns>
    public static (ChoiceAct Act, Suggestion? Line) Chosen(IReadOnlyList<Suggestion> offered, object? chosen)
    {
        ArgumentNullException.ThrowIfNull(offered);
        foreach (Suggestion line in offered)
        {
            if (ReferenceEquals(line, chosen))
            {
                return line.Kind switch
                {
                    SuggestionKind.Option when line.Value is not null => (ChoiceAct.Send, line),
                    SuggestionKind.More => (ChoiceAct.More, line),
                    SuggestionKind.Create => (ChoiceAct.Create, line),
                    _ => (ChoiceAct.Nothing, line),
                };
            }
        }

        return (ChoiceAct.Nothing, null);
    }

    /// <summary>
    /// Whether two lists of options are one list: the same words, the same values and the same states in the same order. Two lists that read the
    /// same and send other values are two lists, and a control that shows the first must be filled again for the second.
    /// </summary>
    /// <param name="shown">The options a control was filled with.</param>
    /// <param name="now">The options the field has now.</param>
    /// <returns>Whether they are the same.</returns>
    public static bool SameOptions(IReadOnlyList<Models.ChoiceOption> shown, IReadOnlyList<Models.ChoiceOption> now)
    {
        ArgumentNullException.ThrowIfNull(shown);
        ArgumentNullException.ThrowIfNull(now);
        if (shown.Count != now.Count)
        {
            return false;
        }

        for (int at = 0; at < shown.Count; at++)
        {
            if (shown[at].Label != now[at].Label || shown[at].Value != now[at].Value || shown[at].Disabled != now[at].Disabled)
            {
                return false;
            }
        }

        return true;
    }

    /// <summary>The place of a value in a list of options: the first option that holds it.</summary>
    /// <param name="options">The options.</param>
    /// <param name="value">The value.</param>
    /// <returns>Its place, or -1 when no option holds it.</returns>
    public static int PlaceOf(IReadOnlyList<Models.ChoiceOption> options, string? value)
    {
        ArgumentNullException.ThrowIfNull(options);
        for (int at = 0; at < options.Count; at++)
        {
            if (options[at].Value == value)
            {
                return at;
            }
        }

        return -1;
    }
}
