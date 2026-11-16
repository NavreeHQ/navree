using System.Text.Json;
using System.Text.RegularExpressions;

namespace Navree.Runtime.Tests;

/// <summary>The client knows when the core has stopped for good, from what the core really answers.</summary>
[TestClass]
public sealed class CoreAnswerTests
{
    private static readonly string Ffi = File.ReadAllText(Path.Combine(Repository.Root, "crates", "navree-native-core", "src", "ffi.rs"));

    // A constant of the core's C interface, as the core's source writes it.
    private static string CoreWords(string name)
    {
        string words = Regex.Match(Ffi, "const " + name + ": &str = \"([^\"]+)\";").Groups[1].Value;
        Assert.IsTrue(words.Length > 0, "the core's " + name + " was not found in ffi.rs");
        return words;
    }

    private static string Answer(string error, bool? stopped = null) =>
        "{\"error\":" + JsonSerializer.Serialize(error) + (stopped is bool mark ? ",\"stopped\":" + (mark ? "true" : "false") : string.Empty) + "}";

    [TestMethod]
    public void The_words_a_stopped_core_really_answers_are_a_stop()
    {
        string stopped = CoreWords("STOPPED");
        Expect.That(stopped == CoreAnswers.StoppedWords, "the client's copy of the core's words is not the core's: " + stopped);
        Expect.That(CoreAnswers.IsStop(Answer(stopped)), "the answer of a core that stopped, without a mark, is not taken as a stop");
    }

    [TestMethod]
    public void The_mark_of_a_stopped_core_is_a_stop_whatever_its_words()
    {
        Expect.That(CoreAnswers.IsStop(Answer(CoreWords("STOPPED"), stopped: true)), "the core's marked answer is not taken as a stop");
        Expect.That(CoreAnswers.IsStop(Answer("other words in a later core", stopped: true)), "the mark is not read before the words");
        Expect.That(!CoreAnswers.IsStop(Answer(CoreWords("STOPPED"), stopped: false)), "a mark that says no is read as yes");
    }

    [TestMethod]
    public void A_refused_input_a_freed_core_and_text_that_does_not_read_are_not_a_stop()
    {
        Expect.That(!CoreAnswers.IsStop(Answer(CoreWords("FREED"))), "a freed core is taken as one that stopped after a failure");
        Expect.That(!CoreAnswers.IsStop(Answer("the event does not read: unknown variant `teleport`")), "a refused input is taken as a stop");
        Expect.That(!CoreAnswers.IsStop("{\"changed\":[],\"effects\":[]}"), "an update is taken as a stop");
        Expect.That(!CoreAnswers.IsStop("not json"), "text that does not read is taken as a stop");
    }

    [TestMethod]
    public void The_real_core_refusing_an_event_is_not_said_to_have_stopped()
    {
        List<string> problems = [];
        int stops = 0;
        using CoreClient client = new(
            System.Text.Encoding.UTF8.GetBytes("""{"origin":"https://shop.example","client":{"platform":"android","form":"phone","vocab":0,"text_percent":100}}"""),
            _ => { },
            problems.Add,
            () => stops++);
        Update update = client.SendAsync("""{"t":"teleport"}""").GetAwaiter().GetResult();

        Expect.That(problems.Count == 1, "the refusal is reported");
        Expect.That(!CoreAnswers.IsStop(update.Answer.Span.ToArray() is byte[] bytes ? System.Text.Encoding.UTF8.GetString(bytes) : string.Empty), "the core's real refusal reads as a stop");
        Expect.That(stops == 0 && !client.HasStopped, "a refusal was told as a stop");
    }
}
