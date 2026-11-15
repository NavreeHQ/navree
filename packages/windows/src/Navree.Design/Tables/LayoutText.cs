using System.Text.Json;

namespace Navree.Design.Tables;

/// <summary>What is kept of one column.</summary>
/// <param name="Kind">The kind of the column when it was kept: a width is used only if the kind has not changed.</param>
/// <param name="Width">The width the person gave it, or null.</param>
/// <param name="Hidden">True when the person hid it, false when the person showed a secondary column, null for neither.</param>
/// <param name="Seen">The save counter when the column was last in the table.</param>
public sealed record StoredColumn(string? Kind, double? Width, bool? Hidden, int Seen);

/// <summary>What is kept of a table's layout for a person.</summary>
/// <param name="Version">The version of the text: one.</param>
/// <param name="Density">The density's name, or an empty text for none.</param>
/// <param name="Frozen">How many columns the person froze at the start.</param>
/// <param name="Order">The keys of the columns in the person's order.</param>
/// <param name="Columns">What is kept of each column, by key.</param>
public sealed record StoredLayout(
    int Version,
    string Density,
    int Frozen,
    IReadOnlyList<string> Order,
    IReadOnlyDictionary<string, StoredColumn> Columns);

/// <summary>The text a person's table layout is kept as.</summary>
public static class LayoutText
{
    /// <summary>The version of the text this code writes and reads.</summary>
    public const int CurrentVersion = 1;

    /// <summary>The most columns kept in one text.</summary>
    public const int MostEntries = 64;

    /// <summary>Reads a kept layout.</summary>
    /// <param name="json">The text.</param>
    /// <returns>The layout, or null when the text is not one this code reads.</returns>
    public static StoredLayout? Parse(string json)
    {
        if (string.IsNullOrWhiteSpace(json))
        {
            return null;
        }

        try
        {
            using JsonDocument document = JsonDocument.Parse(json);
            JsonElement root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object
                || !root.TryGetProperty("v", out JsonElement version)
                || version.ValueKind != JsonValueKind.Number
                || !version.TryGetInt32(out int v)
                || v != CurrentVersion)
            {
                return null;
            }

            string density = root.TryGetProperty("density", out JsonElement d) && d.ValueKind == JsonValueKind.String ? d.GetString() ?? string.Empty : string.Empty;
            int frozen = root.TryGetProperty("frozen", out JsonElement f) && f.ValueKind == JsonValueKind.Number && f.TryGetInt32(out int fz) ? Math.Max(0, fz) : 0;

            List<string> order = [];
            if (root.TryGetProperty("order", out JsonElement o) && o.ValueKind == JsonValueKind.Array)
            {
                foreach (JsonElement key in o.EnumerateArray())
                {
                    string? text = key.ValueKind == JsonValueKind.String ? key.GetString() : null;
                    if (!string.IsNullOrEmpty(text) && !order.Contains(text))
                    {
                        order.Add(text);
                    }
                }
            }

            Dictionary<string, StoredColumn> columns = [];
            if (root.TryGetProperty("cols", out JsonElement cols) && cols.ValueKind == JsonValueKind.Object)
            {
                foreach (JsonProperty column in cols.EnumerateObject())
                {
                    columns[column.Name] = ReadColumn(column.Value);
                }
            }

            return new StoredLayout(CurrentVersion, density, frozen, order, columns);
        }
        catch (JsonException)
        {
            return null;
        }
    }

    /// <summary>Writes a layout, keeping at most <paramref name="maxEntries"/> columns.</summary>
    /// <param name="layout">The layout.</param>
    /// <param name="maxEntries">The most columns to keep; those in the order are kept before those that are not, the most recently seen first.</param>
    /// <returns>The text.</returns>
    public static string Serialize(StoredLayout layout, int maxEntries = MostEntries)
    {
        ArgumentNullException.ThrowIfNull(layout);
        HashSet<string> inOrder = [.. layout.Order];
        List<KeyValuePair<string, StoredColumn>> keep = layout.Columns
            .OrderByDescending(entry => inOrder.Contains(entry.Key))
            .ThenByDescending(entry => entry.Value.Seen)
            .ThenBy(entry => entry.Key, StringComparer.Ordinal)
            .Take(Math.Max(0, maxEntries))
            .ToList();

        using MemoryStream stream = new();
        using (Utf8JsonWriter writer = new(stream))
        {
            writer.WriteStartObject();
            writer.WriteNumber("v", CurrentVersion);
            writer.WriteString("density", layout.Density);
            writer.WriteNumber("frozen", layout.Frozen);
            writer.WriteStartArray("order");
            foreach (string key in layout.Order)
            {
                writer.WriteStringValue(key);
            }

            writer.WriteEndArray();
            writer.WriteStartObject("cols");
            foreach (KeyValuePair<string, StoredColumn> entry in keep.OrderBy(e => e.Key, StringComparer.Ordinal))
            {
                writer.WriteStartObject(entry.Key);
                if (entry.Value.Kind is not null)
                {
                    writer.WriteString("k", entry.Value.Kind);
                }

                if (entry.Value.Width is { } width)
                {
                    writer.WriteNumber("w", width);
                }

                if (entry.Value.Hidden is { } hidden)
                {
                    writer.WriteBoolean("h", hidden);
                }

                writer.WriteNumber("s", entry.Value.Seen);
                writer.WriteEndObject();
            }

            writer.WriteEndObject();
            writer.WriteEndObject();
        }

        return System.Text.Encoding.UTF8.GetString(stream.ToArray());
    }

    private static StoredColumn ReadColumn(JsonElement element)
    {
        if (element.ValueKind != JsonValueKind.Object)
        {
            return new StoredColumn(null, null, null, 0);
        }

        string? kind = element.TryGetProperty("k", out JsonElement k) && k.ValueKind == JsonValueKind.String ? k.GetString() : null;
        double? width = element.TryGetProperty("w", out JsonElement w) && w.ValueKind == JsonValueKind.Number && w.TryGetDouble(out double wd) && wd > 0 && double.IsFinite(wd) ? wd : null;
        bool? hidden = element.TryGetProperty("h", out JsonElement h) && h.ValueKind is JsonValueKind.True or JsonValueKind.False ? h.GetBoolean() : null;
        int seen = element.TryGetProperty("s", out JsonElement s) && s.ValueKind == JsonValueKind.Number && s.TryGetInt32(out int sn) ? sn : 0;
        return new StoredColumn(kind, width, hidden, seen);
    }
}
