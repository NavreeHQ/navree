namespace Navree.Platform;

/// <summary>What a header may hold when it leaves, and which headers of an answer are handed to the core.</summary>
public static class HeaderRules
{
    /// <summary>The longest a header's value may be.</summary>
    public const int MostValueLength = 8192;

    /// <summary>
    /// Whether HTTP can carry this header as it is: a name of the characters a name may have, and a value of visible
    /// ASCII, spaces and tabs, with no line break and no other control character.
    /// </summary>
    /// <param name="name">The header's name.</param>
    /// <param name="value">The header's value.</param>
    /// <returns>Whether it can be sent.</returns>
    public static bool CanCarry(string name, string value)
    {
        if (name.Length == 0 || !name.All(IsTokenCharacter))
        {
            return false;
        }

        return value.Length <= MostValueLength && value.All(letter => letter is '\t' or (>= ' ' and <= '~'));
    }

    /// <summary>
    /// The headers of an answer the core is given, names in lower case and values as they came: every
    /// <c>x-navree-*</c> header, and <c>cache-control</c>, by which the core decides whether a page may be kept
    /// on the device. No other header.
    /// </summary>
    /// <param name="headers">The answer's headers, each value on its own.</param>
    /// <returns>The headers to hand on.</returns>
    public static IReadOnlyList<KeyValuePair<string, string>> Passed(IEnumerable<KeyValuePair<string, IEnumerable<string>>> headers)
    {
        List<KeyValuePair<string, string>> passed = [];
        foreach (KeyValuePair<string, IEnumerable<string>> header in headers)
        {
            if (header.Key.StartsWith("x-navree-", StringComparison.OrdinalIgnoreCase)
                || header.Key.Equals("cache-control", StringComparison.OrdinalIgnoreCase))
            {
                foreach (string value in header.Value)
                {
                    passed.Add(new KeyValuePair<string, string>(header.Key.ToLowerInvariant(), value));
                }
            }
        }

        return passed;
    }

    private static bool IsTokenCharacter(char letter) =>
        char.IsAsciiLetterOrDigit(letter) || letter is '!' or '#' or '$' or '%' or '&' or '\'' or '*' or '+' or '-' or '.' or '^' or '_' or '`' or '|' or '~';
}
