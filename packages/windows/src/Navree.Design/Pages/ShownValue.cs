namespace Navree.Design.Pages;

/// <summary>
/// What a field's control shows, against what the core holds for the field, for the fields that are not boxes of words: a time, a day, a
/// number, a choice. The rule is the boxes' own (<see cref="TextEcho"/>): the value in the control is the person's until the core holds a value
/// this control did not tell it; then the control shows the core's value. So what a field shows is what its form will send: a form the server
/// answers fresh, or a value the server sets, is shown, and an answer to what the person just chose never undoes a choice made since.
/// Values are compared in one form (<see cref="ShownForms"/>).
/// </summary>
public sealed class ShownValue
{
    private readonly TextEcho echo;

    /// <summary>Makes the record of one control.</summary>
    /// <param name="start">What the core held when the control was drawn, which the control was given.</param>
    public ShownValue(string start)
    {
        ArgumentNullException.ThrowIfNull(start);
        Now = start;
        echo = new TextEcho(initial: start);
    }

    /// <summary>What the control shows now.</summary>
    public string Now { get; private set; }

    /// <summary>The person chose a value: the control shows it, and the core is told.</summary>
    /// <param name="value">The value.</param>
    /// <param name="at">When.</param>
    public void Chose(string value, DateTime at)
    {
        ArgumentNullException.ThrowIfNull(value);
        Now = value;
        echo.Told(value, at);
    }

    /// <summary>The core says what the field holds.</summary>
    /// <param name="core">What the core holds.</param>
    /// <param name="at">When.</param>
    /// <returns>
    /// True when the control is to show it: the core set the value itself and the control shows another. False for the answer to what the person
    /// chose, and for a value the control shows already.
    /// </returns>
    public bool Takes(string core, DateTime at)
    {
        ArgumentNullException.ThrowIfNull(core);
        if (!echo.ShouldWrite(core, Now, at))
        {
            return false;
        }

        Now = core;
        return true;
    }
}

/// <summary>
/// The boxes ticked in a field of several choices, against what the core holds for it, by the same rule (<see cref="ShownValue"/>). What a tick
/// sends is what the boxes show: when the core's own value has been taken, the next tick is added to that value and to nothing ticked before it.
/// </summary>
public sealed class TickedValues
{
    private readonly ShownValue shown;
    private readonly List<string> ticked;

    /// <summary>Makes the record of one field.</summary>
    /// <param name="start">The values the core held when the boxes were drawn.</param>
    public TickedValues(IEnumerable<string> start)
    {
        ArgumentNullException.ThrowIfNull(start);
        ticked = [.. start.Distinct(StringComparer.Ordinal)];
        shown = new ShownValue(ShownForms.Many(ticked));
    }

    /// <summary>The values ticked now, in the order they were ticked.</summary>
    public IReadOnlyList<string> Now => ticked;

    /// <summary>Whether a value's box is ticked.</summary>
    /// <param name="value">The value.</param>
    /// <returns>Whether it is.</returns>
    public bool Has(string value) => ticked.Contains(value, StringComparer.Ordinal);

    /// <summary>The person ticked a box or took its tick away.</summary>
    /// <param name="value">The box's value.</param>
    /// <param name="on">Whether it is ticked now.</param>
    /// <param name="at">When.</param>
    /// <returns>What is sent: the values ticked now.</returns>
    public IReadOnlyList<string> Tick(string value, bool on, DateTime at)
    {
        ArgumentNullException.ThrowIfNull(value);
        _ = ticked.RemoveAll(held => string.Equals(held, value, StringComparison.Ordinal));
        if (on)
        {
            ticked.Add(value);
        }

        shown.Chose(ShownForms.Many(ticked), at);
        return [.. ticked];
    }

    /// <summary>The core says what the field holds.</summary>
    /// <param name="core">The values the core holds.</param>
    /// <param name="at">When.</param>
    /// <returns>True when the boxes are to show them: the core set them itself and the boxes show others.</returns>
    public bool Takes(IEnumerable<string> core, DateTime at)
    {
        ArgumentNullException.ThrowIfNull(core);
        List<string> values = [.. core.Distinct(StringComparer.Ordinal)];
        if (!shown.Takes(ShownForms.Many(values), at))
        {
            return false;
        }

        ticked.Clear();
        ticked.AddRange(values);
        return true;
    }
}
