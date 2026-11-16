#if NAVREE_MEASURING
namespace Navree.Controls;

// What the gallery's measuring build adds: a place the table writes its lines to. A build without the symbol has none of it.
public sealed partial class DataTable
{
    private static Action<string>? measuringSink;

    /// <summary>Tells the table where to write what it did while a pointer widened or moved a column.</summary>
    /// <param name="sink">Where it is written.</param>
    public static void Watch(Action<string> sink) => measuringSink = sink;

    partial void Note(string line) => measuringSink?.Invoke(line);
}
#endif
