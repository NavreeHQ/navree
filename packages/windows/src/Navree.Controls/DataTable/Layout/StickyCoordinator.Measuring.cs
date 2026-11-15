#if NAVREE_MEASURING
namespace Navree.Controls;

// What the gallery's measuring build adds: the notes the coordinator makes. A build without the symbol has none of it.
internal sealed partial class StickyCoordinator
{
    /// <summary>What happened that a measuring run may print: when the animations could first be made.</summary>
    internal List<string> Notes { get; } = [];

    partial void Note(string line) => Notes.Add(line);
}
#endif
