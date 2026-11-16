namespace Navree.Design;

/// <summary>How words are set on a line: what the system's own text does not decide.</summary>
public static class TextRules
{
    /// <summary>The longest last word, in characters, that is kept with the word before it.</summary>
    public const int ShortWord = 3;

    /// <summary>
    /// The words as they are said and copied: the server's own. What is drawn may be shaped (a space that does not break, a mark that fixes the
    /// order of an amount in a right-to-left page); what a screen reader, voice control, a search or the clipboard is given is not.
    /// </summary>
    /// <param name="drawn">The words as drawn.</param>
    /// <returns>The words with the shaping taken out: a no-break space is a space, and the direction mark is gone.</returns>
    public static string Said(string drawn) => drawn.Replace('\u00A0', ' ').Replace("\u200E", string.Empty, StringComparison.Ordinal);

    /// <summary>
    /// Keeps a short last word with the one before it, so a wrapped line never ends with a word alone ("... the point it reorders" and then "at"):
    /// the space before it is one that does not break. Windows' text has no option for this (the text block breaks lines by the Unicode line
    /// breaking rules, greedily, and knows no widows), so the words say it. Text of fewer than three words, text that already has more than one
    /// line, and a last word that is not short are left as they are.
    /// </summary>
    /// <param name="text">The words.</param>
    /// <returns>The words, with the last space not breaking when the last word is short.</returns>
    public static string KeepEnd(string text)
    {
        if (string.IsNullOrEmpty(text) || text.Contains('\n', StringComparison.Ordinal) || text.Contains('\r', StringComparison.Ordinal))
        {
            return text;
        }

        int last = text.LastIndexOf(' ');
        if (last <= 0 || last == text.Length - 1 || text.Length - last - 1 > ShortWord)
        {
            return text;
        }

        // At least two spaces: the first word and the one before the short last word are not the same word.
        int before = text.LastIndexOf(' ', last - 1);
        if (before < 0 || text[last - 1] == ' ')
        {
            return text;
        }

        return string.Concat(text.AsSpan(0, last), "\u00A0", text.AsSpan(last + 1));
    }
}
