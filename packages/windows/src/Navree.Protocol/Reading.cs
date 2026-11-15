using System.Text.Json;

namespace Navree.Protocol;

/// <summary>
/// The rules the generated models read by, and the same rules for the other JSON
/// the client reads (the core's view state, its updates). A field that is not
/// there, or is <c>null</c>, takes its default; a field the reader does not know
/// is passed over; a word outside a closed list is <c>Unknown</c>, or the field's
/// default where it has one; a value of the wrong shape is a <see cref="JsonException"/>.
/// </summary>
public static class Reading
{
    /// <summary>
    /// How deep a document may nest. The protocol limits a tree to 64 nodes deep, and
    /// each node nests a few levels of JSON: the library's own limit of 64 would
    /// refuse a tree the protocol allows.
    /// </summary>
    public const int MaxDepth = 2048;

    /// <summary>Parses UTF-8 JSON, allowing the depth the protocol allows.</summary>
    /// <param name="utf8">The JSON.</param>
    /// <returns>The document; its owner disposes it.</returns>
    public static JsonDocument Open(ReadOnlyMemory<byte> utf8) =>
        JsonDocument.Parse(utf8, new JsonDocumentOptions { MaxDepth = MaxDepth });

    /// <summary>Parses JSON text, allowing the depth the protocol allows.</summary>
    /// <param name="text">The JSON.</param>
    /// <returns>The document; its owner disposes it.</returns>
    public static JsonDocument Open(string text) =>
        JsonDocument.Parse(text, new JsonDocumentOptions { MaxDepth = MaxDepth });

    /// <summary>Refuses a value that is not an object.</summary>
    /// <param name="json">The value.</param>
    /// <exception cref="JsonException">It is not an object.</exception>
    public static void ExpectObject(JsonElement json)
    {
        if (json.ValueKind != JsonValueKind.Object)
        {
            throw new JsonException("an object was expected, found " + json.ValueKind);
        }
    }

    /// <summary>The text under <paramref name="name"/>, or empty when it is not text.</summary>
    /// <param name="json">An object.</param>
    /// <param name="name">The key.</param>
    /// <returns>The tag.</returns>
    public static string Tag(JsonElement json, string name) =>
        json.TryGetProperty(name, out JsonElement tag) && tag.ValueKind == JsonValueKind.String
            ? tag.GetString() ?? string.Empty
            : string.Empty;

    /// <summary>Reads text.</summary>
    /// <param name="json">The value.</param>
    /// <returns>The text.</returns>
    public static string Text(JsonElement json) =>
        json.ValueKind == JsonValueKind.String ? json.GetString() ?? string.Empty : throw Wrong(json, "text");

    /// <summary>Reads a flag.</summary>
    /// <param name="json">The value.</param>
    /// <returns>The flag.</returns>
    public static bool Flag(JsonElement json) =>
        json.ValueKind switch
        {
            JsonValueKind.True => true,
            JsonValueKind.False => false,
            _ => throw Wrong(json, "a flag"),
        };

    /// <summary>Reads a whole number that fits 32 bits.</summary>
    /// <param name="json">The value.</param>
    /// <returns>The number.</returns>
    public static int Int(JsonElement json) =>
        json.ValueKind == JsonValueKind.Number && json.TryGetInt32(out int value) ? value : throw Wrong(json, "a number");

    /// <summary>Reads a whole number.</summary>
    /// <param name="json">The value.</param>
    /// <returns>The number.</returns>
    public static long Long(JsonElement json) =>
        json.ValueKind == JsonValueKind.Number && json.TryGetInt64(out long value) ? value : throw Wrong(json, "a number");

    /// <summary>Reads a number.</summary>
    /// <param name="json">The value.</param>
    /// <returns>The number.</returns>
    public static double Double(JsonElement json) =>
        json.ValueKind == JsonValueKind.Number ? json.GetDouble() : throw Wrong(json, "a number");

    /// <summary>Keeps any value as it came.</summary>
    /// <param name="json">The value.</param>
    /// <returns>A copy that outlives the document.</returns>
    public static JsonElement Any(JsonElement json) => json.Clone();

    /// <summary>Reads a list.</summary>
    /// <typeparam name="T">What a list holds.</typeparam>
    /// <param name="json">The value.</param>
    /// <param name="read">Reads one item.</param>
    /// <returns>The items, in order.</returns>
    public static IReadOnlyList<T> ListOf<T>(JsonElement json, Func<JsonElement, T> read)
    {
        if (json.ValueKind != JsonValueKind.Array)
        {
            throw Wrong(json, "a list");
        }

        T[] items = new T[json.GetArrayLength()];
        int at = 0;
        foreach (JsonElement item in json.EnumerateArray())
        {
            items[at++] = read(item);
        }

        return items;
    }

    /// <summary>Reads an object whose keys are names and whose values are all of one kind.</summary>
    /// <typeparam name="T">What the object holds.</typeparam>
    /// <param name="json">The value.</param>
    /// <param name="read">Reads one value.</param>
    /// <returns>The entries, in the order they came.</returns>
    public static IReadOnlyDictionary<string, T> MapOf<T>(JsonElement json, Func<JsonElement, T> read)
    {
        ExpectObject(json);
        Dictionary<string, T> entries = new(StringComparer.Ordinal);
        foreach (JsonProperty entry in json.EnumerateObject())
        {
            entries[entry.Name] = read(entry.Value);
        }

        return entries;
    }

    /// <summary>Finds a field that is there and not <c>null</c>.</summary>
    /// <param name="json">An object.</param>
    /// <param name="name">The key.</param>
    /// <param name="value">The field's value.</param>
    /// <returns>Whether there is one.</returns>
    public static bool TryField(JsonElement json, string name, out JsonElement value)
    {
        if (json.TryGetProperty(name, out value) && value.ValueKind != JsonValueKind.Null && value.ValueKind != JsonValueKind.Undefined)
        {
            return true;
        }

        value = default;
        return false;
    }

    /// <summary>Reads a field every value holds.</summary>
    /// <typeparam name="T">The field's type.</typeparam>
    /// <param name="json">An object.</param>
    /// <param name="name">The key.</param>
    /// <param name="read">Reads the value.</param>
    /// <returns>The field.</returns>
    /// <exception cref="JsonException">The field is not there.</exception>
    public static T Required<T>(JsonElement json, string name, Func<JsonElement, T> read) =>
        TryField(json, name, out JsonElement value) ? read(value) : throw new JsonException("`" + name + "` is missing");

    /// <summary>Reads a field that may be left out, and then holds <paramref name="fallback"/>.</summary>
    /// <typeparam name="T">The field's type.</typeparam>
    /// <param name="json">An object.</param>
    /// <param name="name">The key.</param>
    /// <param name="read">Reads the value.</param>
    /// <param name="fallback">What it holds when it is left out.</param>
    /// <returns>The field.</returns>
    public static T Or<T>(JsonElement json, string name, Func<JsonElement, T> read, T fallback) =>
        TryField(json, name, out JsonElement value) ? read(value) : fallback;

    /// <summary>Reads a field of a closed list: a word this build does not know is <paramref name="fallback"/>.</summary>
    /// <typeparam name="T">The enumeration.</typeparam>
    /// <param name="json">An object.</param>
    /// <param name="name">The key.</param>
    /// <param name="read">Reads the word.</param>
    /// <param name="fallback">What it holds when it is left out or not known.</param>
    /// <returns>The field.</returns>
    public static T Word<T>(JsonElement json, string name, Func<JsonElement, T> read, T fallback)
        where T : struct, Enum
    {
        if (!TryField(json, name, out JsonElement value))
        {
            return fallback;
        }

        T word = read(value);
        return EqualityComparer<T>.Default.Equals(word, default) ? fallback : word;
    }

    /// <summary>Reads a field that may be left out, and then is <c>null</c>.</summary>
    /// <typeparam name="T">The field's type, a class.</typeparam>
    /// <param name="json">An object.</param>
    /// <param name="name">The key.</param>
    /// <param name="read">Reads the value.</param>
    /// <returns>The field, or null.</returns>
    public static T? Opt<T>(JsonElement json, string name, Func<JsonElement, T> read)
        where T : class =>
        TryField(json, name, out JsonElement value) ? read(value) : null;

    /// <summary>Reads a field that may be left out, and then is <c>null</c>.</summary>
    /// <typeparam name="T">The field's type, a struct.</typeparam>
    /// <param name="json">An object.</param>
    /// <param name="name">The key.</param>
    /// <param name="read">Reads the value.</param>
    /// <returns>The field, or null.</returns>
    public static T? OptV<T>(JsonElement json, string name, Func<JsonElement, T> read)
        where T : struct =>
        TryField(json, name, out JsonElement value) ? read(value) : null;

    /// <summary>Reads a field of a closed list that may be left out: a word this build does not know is <c>null</c>.</summary>
    /// <typeparam name="T">The enumeration.</typeparam>
    /// <param name="json">An object.</param>
    /// <param name="name">The key.</param>
    /// <param name="read">Reads the word.</param>
    /// <returns>The field, or null.</returns>
    public static T? OptWord<T>(JsonElement json, string name, Func<JsonElement, T> read)
        where T : struct, Enum
    {
        if (!TryField(json, name, out JsonElement value))
        {
            return null;
        }

        T word = read(value);
        return EqualityComparer<T>.Default.Equals(word, default) ? null : word;
    }

    private static JsonException Wrong(JsonElement json, string expected) =>
        new("expected " + expected + ", found " + json.ValueKind);
}
