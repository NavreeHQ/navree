using Navree.Controls;

namespace Navree.Shell;

/// <summary>Where a person's widths and order of a table's columns are kept: in what the app keeps between launches, under a key the table names.</summary>
/// <param name="host">The app's host.</param>
internal sealed class TableLayouts(ShellHost host) : IDataTableLayoutStore
{
    private const string Prefix = "table-layout:";

    /// <inheritdoc />
    public bool TryLoad(string key, out string json)
    {
        string? kept = host.Recall(Prefix + key);
        json = kept ?? string.Empty;
        return kept is not null;
    }

    /// <inheritdoc />
    public void Save(string key, string json) => host.Remember(Prefix + key, json);
}
