using System.Text.Json;
using System.Text.RegularExpressions;
using Navree.Design;

namespace Navree.Design.Tests;

/// <summary>
/// Every icon name a page can send has a symbol of its own on Windows. The names come from the protocol's examples, the recorded answers of the
/// demos, the pages of the demo's kinds, the table of the symbols the toolkit names, and the Android client's own table: a name that is in one of
/// those and not in the map fails here, so the next missing name is caught by this and not by a person's eye.
/// </summary>
[TestClass]
public sealed class IconNamesTests
{
    private static readonly string[] Folders =
    [
        "crates/navree-native/corpus",
        "packages/android/fixtures",
        "packages/windows/demo/Navree.Demo/kinds",
    ];

    [TestMethod]
    public void Every_icon_name_the_corpus_the_demos_and_the_kinds_send_has_a_symbol()
    {
        SortedDictionary<string, string> sent = [];
        foreach (string folder in Folders)
        {
            string root = Path.Combine(Repository.Root, folder);
            if (!Directory.Exists(root))
            {
                continue;
            }

            foreach (string file in Directory.GetFiles(root, "*.json", SearchOption.AllDirectories))
            {
                try
                {
                    using JsonDocument document = JsonDocument.Parse(File.ReadAllText(file));
                    Collect(document.RootElement, Path.GetFileName(file), sent);
                }
                catch (JsonException)
                {
                    // A file that is not one document (a stream of lines) is read line by line.
                    foreach (string line in File.ReadLines(file).Where(line => line.TrimStart().StartsWith('{')))
                    {
                        using JsonDocument one = JsonDocument.Parse(line);
                        Collect(one.RootElement, Path.GetFileName(file), sent);
                    }
                }
            }
        }

        Assert.IsTrue(sent.Count > 20, "the icons of the corpus and the demos were found: " + sent.Count);
        string[] missing = [.. sent.Where(pair => !SystemGlyphs.Has(pair.Key)).Select(pair => pair.Key + " (in " + pair.Value + ")")];
        Assert.AreEqual(0, missing.Length, "icon names with no symbol on Windows: " + string.Join(", ", missing));
    }

    [TestMethod]
    public void Every_name_in_the_toolkits_table_of_symbols_and_in_the_Android_clients_table_has_a_symbol()
    {
        List<string> names = [];
        string toolkit = Path.Combine(Repository.Root, "crates/navree-native/src/symbols.rs");
        foreach (Match entry in Regex.Matches(File.ReadAllText(toolkit), @"entry\(\s*&\[(?<names>[^\]]*)\]", RegexOptions.Singleline))
        {
            names.AddRange(Regex.Matches(entry.Groups["names"].Value, "\"([a-z0-9-]+)\"").Select(found => found.Groups[1].Value));
        }

        string android = Path.Combine(Repository.Root, "packages/android/navree-native/src/main/kotlin/com/navree/client/design/Symbols.kt");
        if (File.Exists(android))
        {
            // The names the Android client maps ("name" to "symbol"), and the words of its label table are not icon names.
            string text = File.ReadAllText(android);
            int from = text.IndexOf("private val names", StringComparison.Ordinal);
            names.AddRange(Regex.Matches(text[from..], "\"([a-z0-9-]+)\" to \"").Select(found => found.Groups[1].Value));
        }

        Assert.IsTrue(names.Count > 90, "the names of the tables were found: " + names.Count);
        string[] missing = [.. names.Distinct(StringComparer.OrdinalIgnoreCase).Where(name => !SystemGlyphs.Has(name)).Order(StringComparer.Ordinal)];
        Assert.AreEqual(0, missing.Length, "names of the tables with no symbol on Windows: " + string.Join(", ", missing));
    }

    [TestMethod]
    public void A_name_that_is_not_known_is_a_neutral_dot_and_not_a_document()
    {
        Assert.AreEqual(SystemGlyphs.NeutralDot, SystemGlyphs.For("a-name-nobody-has-sent"));
        Assert.AreEqual(SystemGlyphs.NeutralDot, SystemGlyphs.For(null));
        Assert.AreEqual(SystemGlyphs.NeutralDot, SystemGlyphs.For(null, "Nothing in these words"));
        Assert.AreNotEqual(SystemGlyphs.For("file-text"), SystemGlyphs.For("a-name-nobody-has-sent"));
        Assert.AreNotEqual(SystemGlyphs.NeutralDot, SystemGlyphs.For("flame"));
        Assert.AreNotEqual(SystemGlyphs.For("footprints"), SystemGlyphs.For("flame"));
        Assert.AreNotEqual(SystemGlyphs.For("heart-pulse"), SystemGlyphs.For("moon"));
    }

    // An icon is an object with a name, either a node that says it is an icon or the value of an "icon" field.
    private static void Collect(JsonElement element, string file, SortedDictionary<string, string> sent)
    {
        switch (element.ValueKind)
        {
            case JsonValueKind.Object:
                foreach (JsonProperty property in element.EnumerateObject())
                {
                    if (property.Name == "icon" && property.Value.ValueKind == JsonValueKind.Object
                        && property.Value.TryGetProperty("name", out JsonElement named) && named.ValueKind == JsonValueKind.String)
                    {
                        sent.TryAdd(named.GetString()!, file);
                    }

                    Collect(property.Value, file, sent);
                }

                if (element.TryGetProperty("t", out JsonElement kind) && kind.ValueKind == JsonValueKind.String && kind.GetString() == "icon"
                    && element.TryGetProperty("name", out JsonElement own) && own.ValueKind == JsonValueKind.String)
                {
                    sent.TryAdd(own.GetString()!, file);
                }

                break;
            case JsonValueKind.Array:
                foreach (JsonElement item in element.EnumerateArray())
                {
                    Collect(item, file, sent);
                }

                break;
            default:
                break;
        }
    }
}
