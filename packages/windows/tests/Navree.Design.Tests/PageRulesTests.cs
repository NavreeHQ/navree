using System.Reflection;
using System.Text.Json;
using Navree.Design.Pages;
using Navree.Protocol;

namespace Navree.Design.Tests;

/// <summary>What stands when a page is brought up to date, and how the children of a container are matched with the ones it had.</summary>
[TestClass]
public sealed class PageRulesTests
{
    private const string Corpus = "crates/navree-native/corpus";

    // ---- sameness ----------------------------------------------------------------------------------------

    [TestMethod]
    public void A_tree_equal_by_value_but_made_of_other_objects_stands_in_every_example_of_the_protocol()
    {
        int nodes = 0;
        foreach (string file in Repository.Json(Corpus).Concat(Repository.Json("packages/android/fixtures/store-demo")))
        {
            foreach ((Models.Screen screen, Models.Screen again) in ScreensOf(file).Zip(ScreensOf(file)))
            {
                Assert.IsFalse(ReferenceEquals(screen.Body, again.Body), "two readings make two trees");
                List<Models.Node> left = [.. Tree.Nodes(screen)];
                List<Models.Node> right = [.. Tree.Nodes(again)];
                Assert.AreEqual(left.Count, right.Count);
                for (int at = 0; at < left.Count; at++)
                {
                    Assert.IsFalse(ReferenceEquals(left[at], right[at]));
                    Assert.IsTrue(PageRules.Same(left[at], right[at]), file + ": " + left[at].GetType().Name + " " + at + " reads the same twice");
                    nodes++;
                }
            }
        }

        Expect.That(nodes > 500, "a good many nodes were compared: " + nodes);
    }

    [TestMethod]
    public void One_changed_word_deep_in_a_card_makes_that_text_and_its_ancestors_not_stand_and_every_sibling_stand()
    {
        const string Page = """
            {"t":"stack","id":"page","children":[
              {"t":"text","id":"intro","value":"Welcome"},
              {"t":"section","id":"card","children":[
                {"t":"text","id":"first","value":"WORDS"},
                {"t":"stack","id":"inner","children":[{"t":"text","id":"deep","value":"Open"},{"t":"badge","id":"mark","text":"New"}]},
                {"t":"button","id":"go","label":"Go"}]},
              {"t":"text","id":"outro","value":"Thank you"}]}
            """;
        Models.Node before = Read(Page.Replace("WORDS", "Sunil", StringComparison.Ordinal));
        Models.Node after = Read(Page.Replace("WORDS", "Sunita", StringComparison.Ordinal));
        Dictionary<string, Models.Node> was = ById(before);
        Dictionary<string, Models.Node> now = ById(after);
        HashSet<string> changed = ["first", "card", "page"];
        foreach (string id in was.Keys)
        {
            Assert.AreEqual(!changed.Contains(id), PageRules.Same(was[id], now[id]), id + (changed.Contains(id) ? " holds the changed word" : " does not"));
        }
    }

    [TestMethod]
    public void The_written_form_sees_every_plain_property_of_every_kind_the_examples_hold()
    {
        Dictionary<Type, Models.Node> kinds = [];
        foreach (string file in Repository.Json(Corpus).Concat(Repository.Json("packages/android/fixtures/store-demo")).Concat(Repository.Json("packages/android/fixtures/health")))
        {
            foreach (Models.Screen screen in ScreensOf(file))
            {
                foreach (Models.Node node in Tree.Nodes(screen))
                {
                    _ = kinds.TryAdd(node.GetType(), node);
                }
            }
        }

        Expect.That(kinds.Count >= 20, "many kinds were found: " + kinds.Count);
        MethodInfo clone = typeof(object).GetMethod("MemberwiseClone", BindingFlags.Instance | BindingFlags.NonPublic)!;
        List<string> unseen = [];
        foreach ((Type kind, Models.Node node) in kinds)
        {
            foreach (PropertyInfo property in kind.GetProperties(BindingFlags.Public | BindingFlags.Instance))
            {
                if (property.SetMethod is null || ChangedValue(property.PropertyType, property.GetValue(node)) is not { } other)
                {
                    continue;
                }

                Models.Node changedOne = (Models.Node)clone.Invoke(node, null)!;
                property.SetValue(changedOne, other.Value);
                if (PageRules.Same(node, changedOne))
                {
                    unseen.Add(kind.Name + "." + property.Name);
                }
            }
        }

        Assert.AreEqual(0, unseen.Count, "a property that no change of it shows: " + string.Join(", ", unseen));
    }

    [TestMethod]
    public void Nodes_of_two_kinds_never_stand_for_each_other()
    {
        Assert.IsFalse(PageRules.Same(Read("""{"t":"text","id":"a","value":"x"}"""), Read("""{"t":"badge","id":"a","text":"x"}""")));
    }

    // ---- what the screen holds around a part --------------------------------------------------------------

    [TestMethod]
    public void A_state_added_for_one_id_touches_only_the_nodes_that_hold_that_id()
    {
        const string Page = """
            {"t":"stack","id":"page","children":[
              {"t":"text","id":"intro","value":"Welcome"},
              {"t":"section","id":"card","children":[{"t":"button","id":"save","label":"Save"},{"t":"text","id":"note","value":"n"}]},
              {"t":"text","id":"outro","value":"Thank you"}]}
            """;
        Models.Node page = Read(Page);
        Dictionary<string, Models.Node> nodes = ById(page);
        Facts before = new();
        Facts after = new() { States = { ["save"] = "disabled" } };
        foreach ((string id, Models.Node node) in nodes)
        {
            bool holdsIt = id is "save" or "card" or "page";
            Assert.AreEqual(!holdsIt, PageRules.SameAround(node, before, after), id);
        }
    }

    [TestMethod]
    public void A_list_a_region_an_overlay_and_a_field_each_have_their_state_looked_at_by_the_id_the_node_holds()
    {
        const string Page = """
            {"t":"stack","id":"page","children":[
              {"t":"list","id":"rows","children":[]},
              {"t":"region","id":"live","src":"/live","children":[]},
              {"t":"sheet","id":"help","title":"Help","children":[]},
              {"t":"form","id":"signup","action":"/s","children":[{"t":"field","name":"email","id":"email","label":"Email","input":{"t":"text","value":""}}]},
              {"t":"form","id":"other","action":"/o","children":[{"t":"field","name":"email","id":"email","label":"Email","input":{"t":"text","value":""}}]}]}
            """;
        Dictionary<string, Models.Node> nodes = ById(Read(Page));
        Facts quiet = new();
        Assert.IsFalse(PageRules.SameAround(nodes["rows"], quiet, new Facts { Lists = { ["rows"] = "loading" } }));
        Assert.IsTrue(PageRules.SameAround(nodes["live"], quiet, new Facts { Lists = { ["rows"] = "loading" } }));
        Assert.IsFalse(PageRules.SameAround(nodes["live"], quiet, new Facts { Regions = { ["live"] = "failed" } }));
        Assert.IsFalse(PageRules.SameAround(nodes["help"], quiet, new Facts { Open = { "help" } }));
        Assert.IsTrue(PageRules.SameAround(nodes["rows"], quiet, new Facts { Open = { "help" } }));

        // The same field id in two forms: what one form holds is not what the other holds.
        Facts typed = new() { Fields = { [("signup", "email")] = "a@b" } };
        Assert.IsFalse(PageRules.SameAround(nodes["signup"], quiet, typed));
        Assert.IsTrue(PageRules.SameAround(nodes["other"], quiet, typed));
        Assert.IsFalse(PageRules.SameAround(nodes["page"], quiet, typed));
    }

    [TestMethod]
    public void A_table_row_detail_holds_parts_whose_states_count_too()
    {
        const string Page = """
            {"t":"table","id":"t","columns":[{"label":"A"}],"rows":[{"id":"r1","cells":[{"t":"text","value":"x"}],"detail":[{"t":"button","id":"inside","label":"Go"}]}]}
            """;
        Models.Node table = Read(Page);
        Assert.IsFalse(PageRules.SameAround(table, new Facts(), new Facts { States = { ["inside"] = "busy" } }));
    }

    // ---- matching children -------------------------------------------------------------------------------

    [TestMethod]
    public void Rows_added_at_the_end_of_a_list_pair_the_old_rows_with_themselves()
    {
        IReadOnlyList<Models.Node> old = Texts("a", "b", "c");
        IReadOnlyList<Models.Node> now = Texts("a", "b", "c", "d", "e");
        Pairing pairing = PageRules.Pair(old, now);
        CollectionAssert.AreEqual(new[] { 0, 1, 2, -1, -1 }, pairing.Old.ToArray());
        Assert.AreEqual(0, pairing.Gone.Count);
        Assert.IsFalse(pairing.Moved.Any(moved => moved));
        Assert.IsFalse(pairing.Refused);
    }

    [TestMethod]
    public void Rows_added_at_the_start_pair_the_old_rows_with_themselves_and_none_has_moved()
    {
        Pairing pairing = PageRules.Pair(Texts("b", "c"), Texts("a", "b", "c"));
        CollectionAssert.AreEqual(new[] { -1, 0, 1 }, pairing.Old.ToArray());
        Assert.IsFalse(pairing.Moved.Any(moved => moved));
    }

    [TestMethod]
    public void A_row_taken_out_is_gone_and_the_others_stand_where_they_were()
    {
        Pairing pairing = PageRules.Pair(Texts("a", "b", "c"), Texts("a", "c"));
        CollectionAssert.AreEqual(new[] { 0, 2 }, pairing.Old.ToArray());
        CollectionAssert.AreEqual(new[] { 1 }, pairing.Gone.ToArray());
    }

    [TestMethod]
    public void A_child_that_moved_is_reported_as_moved_and_not_as_gone_and_new()
    {
        Pairing pairing = PageRules.Pair(Texts("a", "b", "c", "d"), Texts("b", "c", "d", "a"));
        CollectionAssert.AreEqual(new[] { 1, 2, 3, 0 }, pairing.Old.ToArray());
        Assert.AreEqual(0, pairing.Gone.Count);
        CollectionAssert.AreEqual(new[] { false, false, false, true }, pairing.Moved.ToArray());

        Pairing swapped = PageRules.Pair(Texts("a", "b", "c"), Texts("c", "b", "a"));
        Assert.AreEqual(2, swapped.Moved.Count(moved => moved), "two of three change their order whatever way they are counted");
        Assert.AreEqual(0, swapped.Gone.Count);
    }

    [TestMethod]
    public void Children_with_no_id_are_matched_by_place_and_kind()
    {
        IReadOnlyList<Models.Node> old = [Read("""{"t":"text","value":"x"}"""), Read("""{"t":"badge","text":"y"}"""), Read("""{"t":"text","value":"z"}""")];
        IReadOnlyList<Models.Node> now = [Read("""{"t":"text","value":"x2"}"""), Read("""{"t":"text","value":"y2"}"""), Read("""{"t":"text","value":"z2"}""")];
        Pairing pairing = PageRules.Pair(old, now);
        CollectionAssert.AreEqual(new[] { 0, -1, 2 }, pairing.Old.ToArray(), "the second changed kind, so it is a new child");
        CollectionAssert.AreEqual(new[] { 1 }, pairing.Gone.ToArray());
    }

    [TestMethod]
    public void A_child_with_an_id_is_never_matched_with_one_that_has_another_id_or_another_kind()
    {
        Pairing byId = PageRules.Pair(
            [Read("""{"t":"text","id":"a","value":"x"}""")],
            [Read("""{"t":"text","id":"b","value":"x"}""")]);
        CollectionAssert.AreEqual(new[] { -1 }, byId.Old.ToArray());
        CollectionAssert.AreEqual(new[] { 0 }, byId.Gone.ToArray());

        Pairing byKind = PageRules.Pair(
            [Read("""{"t":"text","id":"a","value":"x"}""")],
            [Read("""{"t":"badge","id":"a","text":"x"}""")]);
        CollectionAssert.AreEqual(new[] { -1 }, byKind.Old.ToArray());
    }

    [TestMethod]
    public void Two_children_with_one_id_are_refused_so_the_container_is_drawn_again()
    {
        Assert.IsTrue(PageRules.Pair(Texts("a", "a"), Texts("a")).Refused);
        Assert.IsTrue(PageRules.Pair(Texts("a"), Texts("b", "b")).Refused);
        Assert.IsFalse(PageRules.Pair(Texts("a", "b"), Texts("a", "b")).Refused);
    }

    [TestMethod]
    public void A_row_of_a_table_is_the_same_when_it_writes_the_same_and_not_when_one_cell_changed()
    {
        const string Page = """
            {"t":"table","id":"t","columns":[{"label":"A"}],"rows":[{"id":"r1","cells":[{"t":"text","value":"VALUE"}]},{"id":"r2","cells":[{"t":"text","value":"x"}]}]}
            """;
        Models.Table one = (Models.Table)Read(Page.Replace("VALUE", "Rs 5", StringComparison.Ordinal));
        Models.Table again = (Models.Table)Read(Page.Replace("VALUE", "Rs 5", StringComparison.Ordinal));
        Models.Table other = (Models.Table)Read(Page.Replace("VALUE", "Rs 6", StringComparison.Ordinal));
        Assert.IsTrue(PageRules.SameRow(one.Rows[0], again.Rows[0]));
        Assert.IsFalse(PageRules.SameRow(one.Rows[0], other.Rows[0]));
        Assert.IsTrue(PageRules.SameRow(one.Rows[1], other.Rows[1]));
    }

    [TestMethod]
    public void A_state_of_a_button_in_a_row_of_a_table_is_seen_around_the_row()
    {
        const string Page = """
            {"t":"table","id":"t","columns":[{"label":"A"}],"rows":[{"id":"r1","cells":[{"t":"button","id":"del","label":"Delete"}]},{"id":"r2","cells":[{"t":"text","value":"x"}]}]}
            """;
        Models.Table table = (Models.Table)Read(Page);
        Facts busy = new() { States = { ["del"] = "disabled" } };
        Assert.IsFalse(PageRules.SameAroundRow(table.Rows[0], new Facts(), busy));
        Assert.IsTrue(PageRules.SameAroundRow(table.Rows[1], new Facts(), busy));
    }

    // ---- helpers -----------------------------------------------------------------------------------------

    private static IReadOnlyList<Models.Node> Texts(params string[] ids) =>
        [.. ids.Select(id => Read("{\"t\":\"text\",\"id\":\"" + id + "\",\"value\":\"" + id + "\"}"))];

    private static Models.Node Read(string json)
    {
        using JsonDocument document = Reading.Open(json);
        return Models.ReadNode(document.RootElement);
    }

    private static Dictionary<string, Models.Node> ById(Models.Node root)
    {
        Dictionary<string, Models.Node> found = [];
        foreach (Models.Node node in Tree.Walk(root))
        {
            if (Tree.IdOf(node) is { } id)
            {
                found[id] = node;
            }
        }

        return found;
    }

    private static IEnumerable<Models.Screen> ScreensOf(string file)
    {
        using JsonDocument document = Reading.Open(File.ReadAllText(file));
        if (document.RootElement.ValueKind != JsonValueKind.Object || !document.RootElement.TryGetProperty("screen", out _))
        {
            yield break;
        }

        Models.Envelope envelope = Models.ReadEnvelope(document.RootElement);
        if (envelope.Screen is not null)
        {
            yield return envelope.Screen;
        }
    }

    // A value for a plain property that is not the one it holds; none for a property that holds more than a plain value.
    private static (object? Value, bool Plain)? ChangedValue(Type type, object? current)
    {
        Type inner = Nullable.GetUnderlyingType(type) ?? type;
        if (inner == typeof(string))
        {
            return ((current as string) + "~", true);
        }

        if (inner == typeof(bool))
        {
            return (current is not true, true);
        }

        if (inner == typeof(int))
        {
            return ((current is int number ? number : 0) + 1, true);
        }

        if (inner == typeof(long))
        {
            return ((current is long big ? big : 0L) + 1, true);
        }

        if (inner == typeof(double))
        {
            return ((current is double real ? real : 0.0) + 1, true);
        }

        if (inner.IsEnum)
        {
            Array values = Enum.GetValues(inner);
            foreach (object value in values)
            {
                if (!Equals(value, current))
                {
                    return (value, true);
                }
            }
        }

        return null;
    }

    private sealed class Facts : IScreenFacts
    {
        internal Dictionary<string, string> States { get; } = [];

        internal Dictionary<string, string> Lists { get; } = [];

        internal Dictionary<string, string> Regions { get; } = [];

        internal Dictionary<(string, string), string> Fields { get; } = [];

        internal HashSet<string> Open { get; } = [];

        public string StatesOf(string id) => States.GetValueOrDefault(id, string.Empty);

        public string ListOf(string id) => Lists.GetValueOrDefault(id, string.Empty);

        public string RegionOf(string id) => Regions.GetValueOrDefault(id, string.Empty);

        public string FieldOf(string form, string field) => Fields.GetValueOrDefault((form, field), string.Empty);

        public bool IsOpen(string id) => Open.Contains(id);
    }
}
