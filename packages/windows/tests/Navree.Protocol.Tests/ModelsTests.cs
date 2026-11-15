using System.Text;
using System.Text.Json;

namespace Navree.Protocol.Tests;

/// <summary>Always true or false, said where the condition is not a comparison the test framework has a method for.</summary>
internal static class Expect
{
    internal static void That(bool condition, string because) => Assert.IsTrue(condition, because);
}

/// <summary>
/// The generated models read every example answer the protocol keeps and every
/// answer recorded from the store demo, and an answer from a newer server, as an
/// app older than its server must: nothing it does not know makes an answer fail.
/// </summary>
[TestClass]
public sealed class ModelsTests
{
    private const string Corpus = "crates/navree-native/corpus";

    [TestMethod]
    public void Every_example_answer_of_the_protocol_reads_and_writes_the_same_again()
    {
        string[] files = Repository.Json(Corpus);
        Expect.That(files.Length >= 15, "the protocol's examples were found");
        foreach (string file in files)
        {
            RoundTrip(file);
        }
    }

    [TestMethod]
    public void Every_answer_recorded_from_the_store_demo_reads_and_writes_the_same_again()
    {
        string[] files = Repository.Json("packages/android/fixtures/store-demo");
        Expect.That(files.Length >= 14, "the recorded answers were found");
        foreach (string file in files)
        {
            RoundTrip(file);
        }

        RoundTrip(Path.Combine(Repository.Root, "packages/android/fixtures/health/shell.json"));
    }

    [TestMethod]
    public void An_example_says_what_it_holds()
    {
        Models.Shell shell = ReadShell(Path.Combine(Repository.Root, Corpus, "shell.json"));
        Assert.AreEqual("Charkha Supply Co.", shell.Name);
        Assert.AreEqual(Models.Major, (int)shell.Navree);
        Expect.That(shell.Nav.Groups.Sum(group => group.Items.Count) >= 3, "the shell has destinations");
        Expect.That(shell.Routes.Count > 0, "the shell says how routes are presented");

        Models.Envelope list = ReadEnvelope(Path.Combine(Repository.Root, Corpus, "screen-list.json"));
        Assert.IsNotNull(list.Screen);
        Assert.AreEqual("Orders", list.Screen.Title);
        Models.Button create = (Models.Button)list.Screen.Actions[0];
        Assert.AreEqual(Models.Role.Create, create.Role);
    }

    [TestMethod]
    public void An_answer_from_a_newer_server_reads_with_its_stand_ins()
    {
        string path = Path.Combine(Repository.Root, Corpus, "from-the-future.json");
        Models.Envelope envelope = ReadEnvelope(path);
        Assert.IsNotNull(envelope.Screen);

        // Ops it does not know are kept as such; the ones it knows read.
        Assert.IsInstanceOfType<Models.OpUnknown>(envelope.Ops[0]);
        Assert.IsInstanceOfType<Models.Toast>(envelope.Ops[^1]);

        // A kind this build has never heard of keeps its tag, its id and its stand-in; written again it is what it was.
        string written = Write(w => Models.WriteEnvelope(w, envelope));
        Assert.AreEqual(2, Count(written, "\"t\":\"hologram\""));
        Assert.AreEqual(1, Count(written, "\"t\":\"teleport\""));
        Assert.AreEqual(1, Count(written, "\"id\":\"shelves-3d\""));
    }

    [TestMethod]
    public void A_word_a_closed_list_lacks_reads_as_the_fields_default()
    {
        using JsonDocument document = Reading.Open("""{"label":"Go","role":"celebrate","prominence":"loudest"}""");
        Models.Button button = Models.ReadButton(document.RootElement);
        Assert.AreEqual(Models.Role.None, button.Role);
        Assert.AreEqual(Models.Prominence.Medium, button.Prominence);
    }

    [TestMethod]
    public void A_word_a_closed_list_lacks_is_unknown_where_the_field_has_no_default()
    {
        using JsonDocument document = Reading.Open("""{"t":"haptic","kind":"thump"}""");
        Models.Call call = Models.ReadCall(document.RootElement);
        Assert.IsInstanceOfType<Models.Haptic>(call);
        Assert.AreEqual(Models.HapticKind.Unknown, ((Models.Haptic)call).Kind);
    }

    [TestMethod]
    public void A_tree_as_deep_as_the_protocol_allows_reads()
    {
        const int Depth = 64;
        StringBuilder json = new();
        for (int level = 0; level < Depth; level++)
        {
            json.Append("""{"t":"stack","children":[""");
        }

        json.Append("""{"t":"text","value":"deep"}""");
        for (int level = 0; level < Depth; level++)
        {
            json.Append("]}");
        }

        using JsonDocument document = Reading.Open(json.ToString());
        Models.Node node = Models.ReadNode(document.RootElement);
        for (int level = 0; level < Depth; level++)
        {
            node = ((Models.Stack)node).Children[0];
        }

        Assert.AreEqual("deep", ((Models.Text)node).Value);
    }

    [TestMethod]
    public void A_value_of_the_wrong_shape_is_refused_as_a_json_error()
    {
        using JsonDocument document = Reading.Open("""{"t":"text","value":7}""");
        Assert.ThrowsExactly<JsonException>(() => Models.ReadNode(document.RootElement));
    }

    private static Models.Envelope ReadEnvelope(string file)
    {
        using JsonDocument document = Reading.Open(File.ReadAllText(file));
        return Models.ReadEnvelope(document.RootElement);
    }

    private static Models.Shell ReadShell(string file)
    {
        using JsonDocument document = Reading.Open(File.ReadAllText(file));
        return Models.ReadShell(document.RootElement);
    }

    // An answer is read, written, read again and written again: the two writings are the same.
    private static void RoundTrip(string file)
    {
        string text = File.ReadAllText(file);
        bool shell = Path.GetFileName(file).Contains("shell", StringComparison.Ordinal);
        string first = shell ? WriteShell(text) : WriteEnvelope(text);
        string second = shell ? WriteShell(first) : WriteEnvelope(first);
        Assert.AreEqual(first, second, file);
    }

    private static string WriteEnvelope(string text)
    {
        using JsonDocument document = Reading.Open(text);
        Models.Envelope envelope = Models.ReadEnvelope(document.RootElement);
        return Write(w => Models.WriteEnvelope(w, envelope));
    }

    private static string WriteShell(string text)
    {
        using JsonDocument document = Reading.Open(text);
        Models.Shell shell = Models.ReadShell(document.RootElement);
        return Write(w => Models.WriteShell(w, shell));
    }

    private static string Write(Action<Utf8JsonWriter> write)
    {
        using MemoryStream stream = new();
        using (Utf8JsonWriter writer = new(stream))
        {
            write(writer);
        }

        return Encoding.UTF8.GetString(stream.ToArray());
    }

    private static int Count(string text, string part)
    {
        int count = 0;
        int at = 0;
        while ((at = text.IndexOf(part, at, StringComparison.Ordinal)) >= 0)
        {
            count++;
            at += part.Length;
        }

        return count;
    }
}
