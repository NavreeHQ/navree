using System.Text.Json;

namespace Navree.Runtime.Tests;

/// <summary>What the core asks is read from its JSON, and what this build does not know is read as such and never fails.</summary>
[TestClass]
public sealed class EffectReadingTests
{
    private static Effect Read(string json)
    {
        using JsonDocument document = JsonDocument.Parse(json);
        return EffectReading.Read(document.RootElement);
    }

    [TestMethod]
    public void The_word_that_the_session_is_ending_is_read_and_is_not_the_word_that_it_is_to_be_forgotten()
    {
        Assert.AreEqual(new SessionEndingEffect(), Read("{\"t\":\"session_ending\"}"));
        Assert.AreEqual(new ForgetSessionEffect(), Read("{\"t\":\"forget_session\"}"));
        Assert.AreNotEqual<Effect>(new ForgetSessionEffect(), Read("{\"t\":\"session_ending\"}"));

        // A later core may say more with it: what this build does not read is left alone.
        Assert.AreEqual(new SessionEndingEffect(), Read("{\"t\":\"session_ending\",\"waits_for\":7,\"why\":{\"removal\":true}}"));
    }

    [TestMethod]
    [DataRow("{\"t\":\"session_paused\"}", "session_paused")]
    [DataRow("{\"t\":\"session_paused\",\"until\":\"2026-11-17T09:00:00+05:30\",\"more\":{\"a\":[1,2,{\"b\":null}]}}", "session_paused")]
    [DataRow("{\"t\":\"Session_Ending\"}", "Session_Ending")]
    [DataRow("{\"t\":\"\"}", "")]
    [DataRow("{\"t\":7}", "")]
    [DataRow("{\"t\":null}", "")]
    [DataRow("{}", "")]
    [DataRow("[]", "")]
    [DataRow("\"session_ending\"", "")]
    [DataRow("null", "")]
    public void An_effect_this_build_does_not_know_is_read_as_one_it_does_not_know_and_reading_it_never_fails(string json, string tag)
    {
        Assert.AreEqual(new UnknownEffect(tag), Read(json));
    }

    [TestMethod]
    public void An_effect_this_build_does_not_know_does_not_cost_the_effects_beside_it()
    {
        // One update's effects, as the core writes them: a request, something from a later core, and the word that the session is ending.
        using JsonDocument update = JsonDocument.Parse(
            "[{\"t\":\"fetch\",\"request\":7,\"method\":\"delete\",\"url\":\"https://shop.example/_navree/native/devices/this\",\"headers\":[],\"timeout_ms\":5000},"
            + "{\"t\":\"session_paused\",\"until\":7},"
            + "{\"t\":\"session_ending\"}]");
        Effect[] read = [.. update.RootElement.EnumerateArray().Select(EffectReading.Read)];
        Assert.AreEqual(3, read.Length);
        Expect.That(read[0] is FetchEffect { Request: 7, Method: "delete", TimeoutMs: 5000 }, "the request beside it was not read: " + read[0]);
        Assert.AreEqual(new UnknownEffect("session_paused"), read[1]);
        Assert.AreEqual(new SessionEndingEffect(), read[2]);
    }
}
