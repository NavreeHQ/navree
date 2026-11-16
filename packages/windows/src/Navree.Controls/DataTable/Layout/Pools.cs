namespace Navree.Controls;

/// <summary>
/// Elements given back by the repeater and kept for the next row, up to a number: past it an element is let go, and the owner is
/// told so it can let go of what it holds for the element.
/// </summary>
/// <typeparam name="T">The type of an element.</typeparam>
internal sealed class ElementPool<T>(int capacity, Action<T> letGo)
    where T : class
{
    private readonly Stack<T> kept = [];

    /// <summary>How many elements are kept.</summary>
    internal int Count => kept.Count;

    /// <summary>Takes an element given back.</summary>
    /// <param name="element">The element.</param>
    internal void Give(T element)
    {
        if (kept.Count >= capacity)
        {
            letGo(element);
            return;
        }

        kept.Push(element);
    }

    /// <summary>Takes an element to use, or null when none is kept.</summary>
    /// <returns>The element.</returns>
    internal T? Take() => kept.Count > 0 ? kept.Pop() : null;
}

/// <summary>The parts kept in place against the scroll, each with what it is.</summary>
/// <typeparam name="T">The type of a part.</typeparam>
internal sealed class StickyParts<T>
    where T : notnull
{
    private readonly Dictionary<T, StickyRole> roles = [];

    /// <summary>How many parts are kept.</summary>
    internal int Count => roles.Count;

    /// <summary>The parts and what each is.</summary>
    internal IEnumerable<KeyValuePair<T, StickyRole>> All => roles;

    /// <summary>Starts keeping a part; a part kept already takes the new role.</summary>
    /// <param name="part">The part.</param>
    /// <param name="role">What it is.</param>
    internal void Add(T part, StickyRole role) => roles[part] = role;

    /// <summary>Stops keeping a part.</summary>
    /// <param name="part">The part.</param>
    /// <param name="role">What it was.</param>
    /// <returns>True when it was kept.</returns>
    internal bool Remove(T part, out StickyRole role) => roles.Remove(part, out role);
}
