using System.Text.Json;
using Navree.Protocol;

namespace Navree.Design.Tests;

/// <summary>The placement rules, the words, the tree reading: plain functions, each with its test.</summary>
[TestClass]
public sealed class RulesTests
{
    [TestMethod]
    [DataRow(320.0, WidthClass.Compact)]
    [DataRow(500.0, WidthClass.Compact)]
    [DataRow(640.0, WidthClass.Compact)]
    [DataRow(641.0, WidthClass.Medium)]
    [DataRow(1007.0, WidthClass.Medium)]
    [DataRow(1008.0, WidthClass.Expanded)]
    [DataRow(1920.0, WidthClass.Expanded)]
    public void A_window_is_compact_medium_or_expanded_by_the_navigation_panes_own_breakpoints(double width, WidthClass expected)
    {
        Assert.AreEqual(expected, WindowRules.ClassOf(width));
    }

    [TestMethod]
    public void A_list_and_its_detail_share_the_window_only_when_it_is_wide_and_there_is_a_detail()
    {
        Assert.IsTrue(WindowRules.TwoPanes(WidthClass.Expanded, 2));
        Assert.IsTrue(WindowRules.TwoPanes(WidthClass.Expanded, 3));
        Assert.IsFalse(WindowRules.TwoPanes(WidthClass.Expanded, 1));
        Assert.IsFalse(WindowRules.TwoPanes(WidthClass.Medium, 2));
        Assert.IsFalse(WindowRules.TwoPanes(WidthClass.Compact, 5));
    }

    [TestMethod]
    public void A_list_has_a_detail_beside_it_but_a_screen_that_lays_out_panes_of_its_own_stands_alone()
    {
        Models.Node list = new Models.Stack { Children = [new Models.List()] };
        Models.Node withForm = new Models.Stack { Children = [new Models.List(), new Models.Form { Action = "/search", Children = [] }] };
        Models.Node split = new Models.Split { Primary = new Models.Stack(), Detail = new Models.Stack() };

        Assert.IsTrue(WindowRules.TwoPanes(WidthClass.Expanded, 2, list));
        Assert.IsTrue(WindowRules.TwoPanes(WidthClass.Expanded, 2, new Models.Table { Columns = [], Rows = [] }));
        Assert.IsFalse(WindowRules.TwoPanes(WidthClass.Expanded, 2, split));
        Assert.IsFalse(WindowRules.TwoPanes(WidthClass.Expanded, 2, withForm));
        Assert.IsFalse(WindowRules.TwoPanes(WidthClass.Expanded, 2, null));
        Assert.IsFalse(WindowRules.TwoPanes(WidthClass.Expanded, 1, list));
        Assert.IsFalse(WindowRules.TwoPanes(WidthClass.Medium, 2, list));
        Assert.IsTrue(WindowRules.IsCollection(new Models.Pane { Role = Models.PaneRole.List }));
        Assert.IsFalse(WindowRules.IsCollection(new Models.Pane { Role = Models.PaneRole.Detail }));
    }

    [TestMethod]
    public void A_long_form_is_a_pane_at_the_side_only_when_the_window_is_wide_and_everything_else_is_a_dialog()
    {
        Assert.IsTrue(WindowRules.UsesSidePane(WidthClass.Expanded, true));
        Assert.IsFalse(WindowRules.UsesSidePane(WidthClass.Expanded, false));
        Assert.IsFalse(WindowRules.UsesSidePane(WidthClass.Medium, true));
        Assert.IsFalse(WindowRules.UsesSidePane(WidthClass.Compact, true));
    }

    [TestMethod]
    public void A_stack_across_that_holds_a_field_is_laid_out_down_in_a_narrow_window_only()
    {
        Models.Node[] withField = [new Models.Field { Input = new Models.TextInput(), Name = "units" }, new Models.Button { Label = "Restock" }];
        Models.Node[] words = [new Models.Text { Value = "Kitchen" }, new Models.Text { Value = "Reorder at 14" }];
        Assert.IsTrue(WindowRules.FoldsDown(WidthClass.Compact, withField));
        Assert.IsFalse(WindowRules.FoldsDown(WidthClass.Medium, withField));
        Assert.IsFalse(WindowRules.FoldsDown(WidthClass.Compact, words));
        Assert.IsFalse(WindowRules.FoldsDown(WidthClass.Compact, [new Models.Field { Input = new Models.TextInput(), Name = "units" }]));
    }

    [TestMethod]
    public void The_core_is_told_the_class_by_its_own_words_and_the_smallest_window_leaves_snap_layouts_working()
    {
        Assert.AreEqual("compact", WindowRules.Word(WidthClass.Compact));
        Assert.AreEqual("medium", WindowRules.Word(WidthClass.Medium));
        Assert.AreEqual("expanded", WindowRules.Word(WidthClass.Expanded));
        Expect.That(WindowRules.SmallestWindow <= 500, "the smallest window is at most 500 effective pixels wide");
        Assert.AreEqual(8.0, WindowRules.Space(2));
    }

    [TestMethod]
    public void The_core_names_nine_notices_and_each_has_words_that_a_name_it_does_not_know_lacks()
    {
        foreach (string notice in new[] { "offline", "timeout", "failed", "unsaved", "loading", "copied", "fields_need_attention", "loaded", "second_step" })
        {
            Assert.IsNotNull(Words.Notice(notice), notice);
        }

        Assert.IsNull(Words.Notice("from_the_future"));
        Assert.AreEqual("You are offline", Words.Failure("offline").Title);
        Assert.AreEqual("Something went wrong", Words.Failure("anything else").Title);
    }

    [TestMethod]
    public void An_icon_the_page_names_gets_windows_own_glyph_and_a_name_that_maps_nowhere_the_label_or_a_page()
    {
        Assert.AreEqual("", SystemGlyphs.For("home"));
        Assert.AreEqual("", SystemGlyphs.For("Orders"));
        Assert.AreEqual("", SystemGlyphs.For("unheard-of", "Inventory and stock"));
        Assert.AreEqual("", SystemGlyphs.For("unheard-of", "Zebra"));
        Assert.AreEqual("", SystemGlyphs.Of(Models.Standard.Share));
    }

    [TestMethod]
    public void Walking_a_tree_yields_every_node_depth_first_and_finds_what_an_unknown_kind_keeps()
    {
        string path = Path.Combine(Repository.Root, "crates/navree-native/corpus/from-the-future.json");
        using JsonDocument document = Reading.Open(File.ReadAllText(path));
        Models.Envelope envelope = Models.ReadEnvelope(document.RootElement);
        Models.Screen screen = envelope.Screen!;
        List<Models.Node> nodes = [.. Tree.Nodes(screen)];

        Models.NodeUnknown[] unknown = [.. nodes.OfType<Models.NodeUnknown>()];
        CollectionAssert.AreEqual(new[] { "hologram", "hologram" }, unknown.Select(node => node.Tag).ToArray());
        Assert.IsInstanceOfType<Models.Image>(unknown[0].Alt);
        Assert.IsNull(unknown[1].Alt);
        Assert.AreEqual("shelves-3d", Tree.IdOf(unknown[0]));

        Models.Button teleport = nodes.OfType<Models.Button>().Single(button => button.Label == "Teleport");
        Assert.IsInstanceOfType<Models.ActionUnknown>(teleport.Action);
        Models.Button celebrate = nodes.OfType<Models.Button>().Single(button => button.Label == "Celebrate");
        Models.ActionCommands commands = (Models.ActionCommands)celebrate.Action!;
        Assert.IsInstanceOfType<Models.CommandUnknown>(commands.List[0]);
        Assert.IsInstanceOfType<Models.CommandToast>(commands.List[^1]);
        Assert.IsFalse(Tree.Writes(teleport.Action));
    }

    [TestMethod]
    public void A_form_goes_by_its_own_id_or_by_at_and_its_address_and_a_forms_fields_are_found_through_what_holds_them()
    {
        Models.Field name = new() { Input = new Models.TextInput(), Name = "name", Id = "name-field" };
        Models.Form form = new()
        {
            Action = "/orders",
            Children = [new Models.Stack { Children = [name] }, new Models.Form { Action = "/inner", Children = [new Models.Field { Input = new Models.TextInput(), Name = "inner" }] }],
        };
        Assert.AreEqual("@/orders", Tree.FormId(form));
        Assert.AreEqual("checkout", Tree.FormId(new Models.Form { Action = "/x", Id = "checkout" }));
        CollectionAssert.AreEqual(new[] { "name" }, Tree.FieldsOf(form).Select(field => field.Name).ToArray());
    }
}
