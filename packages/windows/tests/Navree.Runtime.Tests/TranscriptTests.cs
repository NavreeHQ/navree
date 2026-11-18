using System.Text.Json;
using System.Text.Json.Nodes;
using Navree.Protocol;

namespace Navree.Runtime.Tests;

/// <summary>
/// Every conversation the core keeps (<c>crates/navree-native-core/transcripts</c>) replayed through the
/// real core by the runtime: what the core answers at each step is what the transcript says,
/// and what the client holds after the step is what a whole read of the core's view state gives.
/// </summary>
[TestClass]
public sealed class TranscriptTests
{
    private const string Transcripts = "crates/navree-native-core/transcripts";

    [TestMethod]
    public async Task Every_conversation_of_the_core_is_replayed_and_the_client_holds_what_a_whole_read_gives()
    {
        string[] files = Repository.Json(Transcripts);
        Expect.That(files.Length >= 60, "the core's conversations were found");

        int steps = 0;
        foreach (string file in files)
        {
            steps += await Replay(file);
        }

        Expect.That(steps > 300, "the conversations have steps");
    }

    [TestMethod]
    public async Task The_tree_the_protocol_allows_deepest_is_read_on_the_cores_own_thread()
    {
        int steps = await Replay(Path.Combine(Repository.Root, Transcripts, "deep-tree.json"));
        Expect.That(steps > 0, "the conversation has steps");
    }

    [TestMethod]
    public void The_core_runs_on_a_thread_that_is_not_the_callers()
    {
        List<int> threads = [];
        using CoreClient client = new(
            Config(null),
            arrived => threads.Add(Environment.CurrentManagedThreadId));
        client.SendAsync(Events.Start).GetAwaiter().GetResult();
        Expect.That(threads.Count == 1, "the update was handed on once");
        Expect.That(threads[0] != Environment.CurrentManagedThreadId, "on a thread of its own");
    }

    [TestMethod]
    public void An_event_the_core_refuses_is_a_problem_and_changes_nothing()
    {
        List<string> problems = [];
        using CoreClient client = new(Config(null), _ => { }, problems.Add);
        Update update = client.SendAsync("""{"t":"teleport"}""").GetAwaiter().GetResult();
        Expect.That(problems.Count == 1, "the refusal is reported");
        Expect.That(ReferenceEquals(ViewState.Empty, update.State), "and the state is what it was");
    }

    // One conversation: returns the number of steps.
    private static async Task<int> Replay(string file)
    {
        using JsonDocument document = Reading.Open(await File.ReadAllTextAsync(file));
        JsonElement transcript = document.RootElement;
        string name = Path.GetFileName(file);
        List<string> problems = [];
        List<Effect> effects = [];
        CoreClient? client = null;
        try
        {
            transcript.TryGetProperty("config", out JsonElement over);
            byte[] config = Config(over.ValueKind == JsonValueKind.Object ? over : null);
            client = new CoreClient(config, arrived => effects.AddRange(arrived.Effects), problems.Add);
            int number = 0;
            long given = 0;
            foreach (JsonElement step in transcript.GetProperty("steps").EnumerateArray())
            {
                number++;
                string place = name + " step " + number;
                if (step.TryGetProperty("restart", out JsonElement restart) && restart.ValueKind == JsonValueKind.True)
                {
                    client.Dispose();
                    client = new CoreClient(config, arrived => effects.AddRange(arrived.Effects), problems.Add);
                    given = 0;
                }

                string eventJson = EventOf(step, file);
                Update update = await client.SendAsync(eventJson);

                // The core's answer is the transcript's.
                if (step.TryGetProperty("expect", out JsonElement expect))
                {
                    using JsonDocument answered = Reading.Open(update.Answer);
                    Assert.IsTrue(JsonEquality.Equal(expect, answered.RootElement), place + ": the core's answer differs from the transcript's");
                }

                // What the client holds is what a whole read says.
                ViewState held = update.State;
                ViewState whole = await client.ReadAllAsync();
                Assert.AreEqual(ViewWriting.ToJson(whole), ViewWriting.ToJson(held), place + ": the state held differs from a whole read");

                // The client counts each time the core says the app's part changed, which is how a part the core gave anew is told
                // from the same part read again. A whole read is no such time: the count stays.
                using (JsonDocument said = Reading.Open(update.Answer))
                {
                    if (said.RootElement.TryGetProperty("changed", out JsonElement changed)
                        && changed.ValueKind == JsonValueKind.Array
                        && changed.EnumerateArray().Any(change => change.ValueKind == JsonValueKind.Object && change.TryGetProperty("t", out JsonElement tag) && tag.ValueKind == JsonValueKind.String && tag.GetString() == "app"))
                    {
                        given++;
                    }
                }

                Assert.AreEqual(given, held.AppGiven, place + ": the count of the times the core gave the app's part is not what its answers say");
                Assert.AreEqual(given, whole.AppGiven, place + ": reading every part again was counted as the core giving the app's part anew");
            }

            Expect.That(problems.Count == 0, name + ": " + string.Join(" | ", problems));
            Expect.That(!effects.Any(effect => effect is UnknownEffect), name + ": an effect the client does not know");
            return number;
        }
        finally
        {
            client?.Dispose();
        }
    }

    // The step's event, with its body put in: a response's body, or a cache event's value, is a file of the fixtures.
    private static string EventOf(JsonElement step, string file)
    {
        JsonNode send = JsonNode.Parse(step.GetProperty("send").GetRawText())!;
        if (step.TryGetProperty("body", out JsonElement body) && body.ValueKind == JsonValueKind.String)
        {
            string text = File.ReadAllText(Path.Combine(Path.GetDirectoryName(file)!, "fixtures", body.GetString()!));
            JsonObject sent = send.AsObject();
            sent[sent["t"]!.GetValue<string>() == "cache" ? "value" : "body"] = text;
        }

        return send.ToJsonString();
    }

    private static byte[] Config(JsonElement? over)
    {
        JsonObject config = JsonNode.Parse("""{"origin":"https://shop.example","client":{"platform":"android","form":"phone","vocab":0,"text_percent":100}}""")!.AsObject();
        if (over is JsonElement extra)
        {
            foreach (JsonProperty property in extra.EnumerateObject())
            {
                config[property.Name] = JsonNode.Parse(property.Value.GetRawText());
            }
        }

        return System.Text.Encoding.UTF8.GetBytes(config.ToJsonString());
    }
}
