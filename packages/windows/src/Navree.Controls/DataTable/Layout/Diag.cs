namespace Navree.Controls;

/// <summary>
/// Lets a host that watches the table (the gallery's runs) be told of an exception that happens in
/// a place the framework calls into, where it would otherwise surface far from its cause.
/// </summary>
internal static class Diag
{
    /// <summary>Where such exceptions are written; null in a product, where they go on as they are.</summary>
    internal static Action<string>? Sink { get; set; }

    /// <summary>Runs an action, and when a sink is set tells it of an exception and lets the run go on.</summary>
    /// <param name="where">Where it happened.</param>
    /// <param name="action">What to run.</param>
    internal static void Guard(string where, Action action)
    {
        try
        {
            action();
        }
        catch (Exception problem) when (Sink is not null)
        {
            Sink(where + ": " + problem);
        }
    }
}
