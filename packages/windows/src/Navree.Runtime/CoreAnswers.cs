using System.Text.Json;

namespace Navree.Runtime;

/// <summary>What the core's answers say about the core itself.</summary>
public static class CoreAnswers
{
    /// <summary>
    /// The words every call answers once the core has stopped after a failure inside it, as the core writes them (<c>STOPPED</c> in
    /// <c>crates/navree-native-core/src/ffi.rs</c>). A core that marks the answer is read by its mark; the words are for a core that does not.
    /// </summary>
    public const string StoppedWords = "the core stopped after a failure inside it and can't be used again";

    /// <summary>
    /// Whether an answer says the core has stopped for good, so the app must be started again: an error that carries <c>"stopped": true</c>, or,
    /// without the mark, the core's own words for it. An input that merely did not read, or a core that was freed, is not a stop.
    /// </summary>
    /// <param name="answer">The answer, as the core gave it.</param>
    /// <returns>Whether the core has stopped.</returns>
    public static bool IsStop(JsonElement answer)
    {
        if (answer.ValueKind != JsonValueKind.Object || !answer.TryGetProperty("error", out JsonElement error))
        {
            return false;
        }

        if (answer.TryGetProperty("stopped", out JsonElement mark))
        {
            return mark.ValueKind == JsonValueKind.True;
        }

        return error.ValueKind == JsonValueKind.String && error.GetString() == StoppedWords;
    }

    /// <summary>Whether an answer, as text, says the core has stopped for good (<see cref="IsStop(JsonElement)"/>).</summary>
    /// <param name="answer">The answer's JSON.</param>
    /// <returns>Whether the core has stopped; false for text that does not read.</returns>
    public static bool IsStop(string answer)
    {
        try
        {
            using JsonDocument document = JsonDocument.Parse(answer);
            return IsStop(document.RootElement);
        }
        catch (JsonException)
        {
            return false;
        }
    }
}
