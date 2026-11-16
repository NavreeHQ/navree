using Navree.Protocol;

namespace Navree.Design.Tests;

/// <summary>The words the drawn kinds say themselves: all of them come from one place that a person's language can replace.</summary>
[TestClass]
public sealed class KindStringsTests
{
    private static readonly string[] Folders =
    [
        "packages/windows/src/Navree.Controls/Charts",
        "packages/windows/src/Navree.Controls/Health",
        "packages/windows/src/Navree.Controls/Composition",
        "packages/windows/src/Navree.Controls/Drawing",
        "packages/windows/src/Navree.Controls/Flags",
        "packages/windows/src/Navree.Design/Kinds",
    ];

    private static IEnumerable<(string File, string Text)> Sources() =>
        Folders
            .SelectMany(folder => Directory.GetFiles(Path.Combine(Repository.Root, folder), "*.cs", SearchOption.AllDirectories))
            .Where(file => Path.GetFileName(file) != "KindStrings.cs")
            .Select(file => (file, string.Join('\n', File.ReadAllLines(file).Where(line => !line.TrimStart().StartsWith("//", StringComparison.Ordinal)))));

    [TestMethod]
    public void Every_word_has_english_words_and_the_identity_of_one_that_is_not_a_word_shows_plainly()
    {
        Assert.IsTrue(KindStrings.Ids.Count >= 30);
        foreach (string id in KindStrings.Ids)
        {
            Assert.IsFalse(string.IsNullOrWhiteSpace(KindStrings.English(id)), id);
            if (!KindStrings.English(id)!.Contains('{', StringComparison.Ordinal))
            {
                Assert.AreEqual(KindStrings.English(id), KindStrings.Get(id), id);
            }
        }

        Assert.AreEqual("Kind_NotAWord", KindStrings.Get("Kind_NotAWord"));
        Assert.IsNull(KindStrings.English("Kind_NotAWord"));
    }

    [TestMethod]
    public void A_host_with_the_persons_language_replaces_every_word_a_kind_says()
    {
        Func<string, string?>? before = KindStrings.Provider;
        try
        {
            KindStrings.Provider = id => id switch
            {
                "Kind_NoReading" => "कोई रीडिंग नहीं",
                "Kind_PageOf" => "पृष्ठ {0} / {1}",
                "Kind_PreviousDay" => "पिछला दिन",
                "Kind_TrendUp" => "ऊपर",
                "Kind_AboutThing" => "{0} के बारे में",
                "Kind_NotSent" => "भेजा नहीं गया",
                "Kind_TypeRing" => "रिंग",
                _ => null,
            };
            Assert.AreEqual("कोई रीडिंग नहीं", KindStrings.NoReading);
            Assert.AreEqual("पृष्ठ 2 / 5", KindStrings.PageOf(2, 5));
            Assert.AreEqual("पिछला दिन", KindStrings.PreviousDay);
            Assert.AreEqual("ऊपर", TrendRules.Word(Models.Direction.Up));
            Assert.AreEqual("नींद के बारे में", HintRules.ActionName(new Models.Hint { About = "नींद", Text = "t" }));
            Assert.AreEqual("भेजा नहीं गया", MessageRules.Delivery(Models.Delivery.Failed)?.Text);
            Assert.AreEqual("रिंग", KindStrings.TypeRing);

            // A word the host does not have stays English.
            Assert.AreEqual("Not tested", KindStrings.NotTested);
            Assert.AreEqual("Down", TrendRules.Word(Models.Direction.Down));
        }
        finally
        {
            KindStrings.Provider = before;
        }
    }

    [TestMethod]
    public void Words_with_numbers_and_names_put_them_in()
    {
        Assert.AreEqual("Page 3 of 7", KindStrings.PageOf(3, 7));
        Assert.AreEqual("Sleep and Recovery", KindStrings.Pair("Sleep", "Recovery"));
        Assert.AreEqual("About the health score", KindStrings.AboutThing("the health score"));
        Assert.AreEqual("About", HintRules.ActionName(new Models.Hint { About = "  ", Text = "t" }));
    }

    [TestMethod]
    public void No_drawn_kind_keeps_one_of_its_words_in_its_own_code()
    {
        List<string> found = [];
        (string File, string Text)[] sources = [.. Sources()];
        foreach (string id in KindStrings.Ids)
        {
            // The words, cut where a number or a name is put in: each piece that says something is looked for as a literal.
            string[] pieces = System.Text.RegularExpressions.Regex.Split(KindStrings.English(id)!, @"\{\d\}");
            for (int at = 0; at < pieces.Length; at++)
            {
                string piece = pieces[at];
                if (piece.Trim().Length < (pieces.Length == 1 ? 1 : 3))
                {
                    continue;
                }

                string[] literals = pieces.Length == 1 || at == pieces.Length - 1 ? ["\"" + piece + "\""] : ["\"" + piece + "\"", "\"" + piece];
                foreach ((string file, string text) in sources)
                {
                    foreach (string literal in literals)
                    {
                        // A name that is only a single letter is the page's protocol word elsewhere (such as the key "x"): looked for only in full words.
                        if (piece.Trim().Length >= 2 && text.Contains(literal, StringComparison.Ordinal))
                        {
                            found.Add(Path.GetFileName(file) + ": " + literal);
                        }
                    }
                }
            }
        }

        Assert.AreEqual(0, found.Count, string.Join("; ", found.Distinct()));
    }

    [TestMethod]
    public void Every_word_is_used_by_a_kind_so_none_is_left_in_the_strings_that_nothing_says()
    {
        string all = string.Join('\n', Sources().Select(source => source.Text));
        List<string> unused = [];
        foreach (System.Reflection.PropertyInfo property in typeof(KindStrings).GetProperties(System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Static).Where(property => property.PropertyType == typeof(string)))
        {
            if (!all.Contains("KindStrings." + property.Name, StringComparison.Ordinal))
            {
                unused.Add(property.Name);
            }
        }

        foreach (System.Reflection.MethodInfo method in typeof(KindStrings).GetMethods(System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Static).Where(method => method.ReturnType == typeof(string) && method.Name is not ("Get" or "English") && !method.IsSpecialName))
        {
            if (!all.Contains("KindStrings." + method.Name + "(", StringComparison.Ordinal))
            {
                unused.Add(method.Name);
            }
        }

        Assert.AreEqual(0, unused.Count, string.Join(", ", unused));
    }
}
