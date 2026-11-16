namespace Navree.Controls;

/// <summary>The characters of the symbol font that the table draws with.</summary>
internal static class Glyphs
{
    /// <summary>The three dots of a menu.</summary>
    internal static readonly string More = ((char)0xE712).ToString();

    /// <summary>A chevron pointing down: the detail is shut.</summary>
    internal static readonly string ChevronDown = ((char)0xE70D).ToString();

    /// <summary>A chevron pointing up: the detail is open.</summary>
    internal static readonly string ChevronUp = ((char)0xE70E).ToString();

    /// <summary>The small chevron that marks an ascending sort.</summary>
    internal static readonly string SortAscending = ((char)0xE96D).ToString();

    /// <summary>The small chevron that marks a descending sort.</summary>
    internal static readonly string SortDescending = ((char)0xE96E).ToString();
}
