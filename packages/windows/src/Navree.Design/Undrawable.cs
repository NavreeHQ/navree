using Navree.Protocol;

namespace Navree.Design;

/// <summary>
/// The kinds this build of the client has no drawing for, by the names the core knows them by. The core shows a page that
/// holds one in the web view instead, so a part is never replaced by a note. The list is one place: the renderer's own table
/// of kinds is read against it by a test, and a kind that gains a drawing is taken off it.
/// </summary>
public static class Undrawable
{
    /// <summary>Each model that has no drawing, and the name the core knows its kind by.</summary>
    public static IReadOnlyDictionary<Type, string> Named { get; } = new Dictionary<Type, string>
    {
        [typeof(Models.WebRegion)] = "web",
        [typeof(Models.RangeInput)] = "input:range",
    };

    /// <summary>The kinds, as the core is told them in the configuration.</summary>
    public static IReadOnlyList<string> Kinds { get; } = [.. Named.Values];
}
