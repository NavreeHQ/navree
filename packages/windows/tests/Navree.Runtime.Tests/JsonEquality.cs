using System.Text.Json;

namespace Navree.Runtime.Tests;

/// <summary>Whether two JSON values say the same: objects by their keys and not their order, everything else as written.</summary>
internal static class JsonEquality
{
    internal static bool Equal(JsonElement left, JsonElement right)
    {
        if (left.ValueKind != right.ValueKind)
        {
            return false;
        }

        switch (left.ValueKind)
        {
            case JsonValueKind.Object:
                if (left.EnumerateObject().Count() != right.EnumerateObject().Count())
                {
                    return false;
                }

                foreach (JsonProperty property in left.EnumerateObject())
                {
                    if (!right.TryGetProperty(property.Name, out JsonElement other) || !Equal(property.Value, other))
                    {
                        return false;
                    }
                }

                return true;
            case JsonValueKind.Array:
                if (left.GetArrayLength() != right.GetArrayLength())
                {
                    return false;
                }

                using (JsonElement.ArrayEnumerator mine = left.EnumerateArray())
                using (JsonElement.ArrayEnumerator theirs = right.EnumerateArray())
                {
                    while (mine.MoveNext() && theirs.MoveNext())
                    {
                        if (!Equal(mine.Current, theirs.Current))
                        {
                            return false;
                        }
                    }
                }

                return true;
            case JsonValueKind.String:
                return string.Equals(left.GetString(), right.GetString(), StringComparison.Ordinal);
            case JsonValueKind.Number:
                return left.GetDecimal() == right.GetDecimal() || left.GetDouble() == right.GetDouble();
            default:
                return true;
        }
    }
}
