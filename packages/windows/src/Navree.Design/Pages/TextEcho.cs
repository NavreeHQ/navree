namespace Navree.Design.Pages;

/// <summary>
/// The words a box has told the core, so that what the core says back can be told from what the core itself did. The text in a box is the person's:
/// the core's value is written into it only when the core set it, never when it is only an answer to what was typed (which arrives a moment
/// later, when the person has typed on).
/// </summary>
public sealed class TextEcho
{
    private readonly List<(string Text, DateTime At)> told = [];
    private readonly TimeSpan lifetime;

    /// <summary>Makes the record of one box.</summary>
    /// <param name="lifetime">How long a told text is waited for before it is given up (an answer that never came); ten seconds when none is said.</param>
    public TextEcho(TimeSpan? lifetime = null) => this.lifetime = lifetime ?? TimeSpan.FromSeconds(10);

    /// <summary>The box told the core this.</summary>
    /// <param name="text">What the box held.</param>
    /// <param name="at">When.</param>
    public void Told(string text, DateTime at)
    {
        told.Add((text, at));
    }

    /// <summary>The core says what the field holds: whether that is to be written into the box.</summary>
    /// <param name="core">What the core holds.</param>
    /// <param name="box">What the box holds now.</param>
    /// <param name="at">When.</param>
    /// <returns>
    /// True when the core set the value itself (it is not any text the box told it, still waited for) and the box does not hold it already;
    /// false for an answer to what was typed, which says nothing the box does not know.
    /// </returns>
    public bool ShouldWrite(string core, string box, DateTime at)
    {
        _ = told.RemoveAll(entry => at - entry.At > lifetime);
        int echo = told.FindIndex(entry => entry.Text == core);
        if (echo >= 0)
        {
            // What was typed comes back, the texts before it with it; what was typed after it is still on its way.
            told.RemoveRange(0, echo + 1);
            return false;
        }

        // Not what the box said: the core's own change, which wins from here, and nothing the box said before is waited for any more.
        told.Clear();
        return core != box;
    }
}
