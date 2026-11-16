using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Protocol.Tests;

/// <summary>What the client says it cannot draw is what it cannot draw, and the core acts on it.</summary>
[TestClass]
public sealed class UndrawableTests
{
    private static readonly string[] Caps = ["clipboard", "open_url"];

    // The kinds the renderer's sources name anywhere: a kind they never name has no drawing.
    private static HashSet<string> NamedByTheRenderer()
    {
        string folder = Path.Combine(Repository.Root, "packages", "windows", "src", "Navree.Shell");
        HashSet<string> named = [];
        foreach (string file in Directory.GetFiles(folder, "NodeRenderer*.cs"))
        {
            foreach (Match match in Regex.Matches(File.ReadAllText(file), @"\bModels\.(\w+)"))
            {
                named.Add(match.Groups[1].Value);
            }
        }

        return named;
    }

    private static IEnumerable<Type> KindsOf(Type family) =>
        typeof(Models).GetNestedTypes().Where(type => type.IsSealed && family.IsAssignableFrom(type) && type != family);

    [TestMethod]
    public void The_kinds_the_renderer_never_names_are_the_kinds_the_client_says_it_cannot_draw()
    {
        HashSet<string> named = NamedByTheRenderer();
        Expect.That(named.Contains("Stack") && named.Contains("TextInput"), "the renderer's sources were not read: " + string.Join(", ", named.Take(5)));

        // The core always sets aside a kind it does not know and an input of an unknown kind, so neither is listed.
        string[] never = [nameof(Models.NodeUnknown), nameof(Models.InputUnknown)];
        string[] withoutDrawing =
        [
            .. KindsOf(typeof(Models.Node)).Concat(KindsOf(typeof(Models.Input)))
                .Where(type => !named.Contains(type.Name) && !never.Contains(type.Name))
                .Select(type => type.Name)
                .Order(StringComparer.Ordinal),
        ];
        string[] said = [.. Undrawable.Named.Keys.Select(type => type.Name).Order(StringComparer.Ordinal)];
        CollectionAssert.AreEqual(withoutDrawing, said, "a kind with no drawing is missing from the list, or a kind that is drawn is on it");
    }

    [TestMethod]
    public void Every_kind_on_the_list_has_the_name_the_core_knows_it_by()
    {
        foreach ((Type type, string name) in Undrawable.Named)
        {
            string expected = typeof(Models.Input).IsAssignableFrom(type)
                ? "input:" + type.Name[..^"Input".Length].ToLowerInvariant()
                : type.Name == nameof(Models.WebRegion) ? "web" : throw new AssertFailedException(type.Name + " has no name");
            Assert.AreEqual(expected, name);
        }

        CollectionAssert.AreEquivalent(Undrawable.Named.Values.ToArray(), Undrawable.Kinds.ToArray());
    }

    [TestMethod]
    public void The_configuration_carries_the_list_and_says_what_this_client_is()
    {
        ClientSettings settings = new(Dark: true, MoreContrast: false, TextPercent: 125, Locale: "en-IN", TimeZone: "Asia/Kolkata");
        byte[] json = ClientConfig.Json("https://shop.example", ["https://*.shop.example"], "com.charkha.supply", settings, "10.0.26100.0", "1.2.3", Caps, "navree-demo", false, Undrawable.Kinds);
        using JsonDocument document = JsonDocument.Parse(json);
        JsonElement root = document.RootElement;
        CollectionAssert.AreEqual(Undrawable.Kinds.ToArray(), root.GetProperty("undrawable").EnumerateArray().Select(item => item.GetString()).ToArray());
        JsonElement client = root.GetProperty("client");
        Assert.AreEqual("windows", client.GetProperty("platform").GetString());
        Assert.AreEqual("desktop", client.GetProperty("form").GetString());
        Assert.AreEqual("dark", client.GetProperty("scheme").GetString());
        Assert.AreEqual(125, client.GetProperty("text_percent").GetInt32());
        Assert.AreEqual("Asia/Kolkata", client.GetProperty("tz").GetString());
        Assert.AreEqual("navree-demo", root.GetProperty("sign_in").GetProperty("callback").GetString());
    }

    // The core, given that configuration, shows a page that holds a range field in the web view and not with a hole.
    [TestMethod]
    public async Task The_core_shows_a_page_with_a_part_the_client_cannot_draw_in_the_web_view()
    {
        string made = Path.Combine(Repository.Root, "crates", "navree-native-core", "transcripts", "fixtures");
        string shell = File.ReadAllText(Path.Combine(made, "store", "shell.json"));

        // The protocol's own page with a form, its photos field made a kind that this client still cannot draw.
        JsonNode page = JsonNode.Parse(File.ReadAllText(Path.Combine(made, "made", "return-new.json")))!;
        JsonArray fields = page["screen"]!["body"]!["children"]!.AsArray();
        fields[1]!["input"] = JsonNode.Parse("""{"t":"range","from":{"name":"from"},"to":{"name":"to"}}""");
        string pageText = page.ToJsonString();
        foreach (bool listed in new[] { true, false })
        {
            ClientSettings settings = new(false, false, 100, "en-IN", null);
            byte[] config = ClientConfig.Json("https://shop.example", [], null, settings, "10.0", "1.0.0", Caps, null, false, listed ? Undrawable.Kinds : []);
            List<Effect> effects = [];
            using CoreClient client = new(config, update => effects.AddRange(update.Effects));
            await client.SendAsync(Events.Start);
            await client.SendAsync(Events.Response(1, 200, "application/vnd.navree.native+json", null, [], shell));
            FetchEffect first = effects.OfType<FetchEffect>().Last(fetch => fetch.Request == 2);
            Update shown = await client.SendAsync(Events.Response(first.Request, 200, "application/vnd.navree.native+json", null, [], pageText));
            ScreenContent? content = shown.State.FrontScreen?.Content;
            if (listed)
            {
                WebContent web = content as WebContent ?? throw new AssertFailedException("the page was drawn, not shown in the web view: " + content?.GetType().Name);
                Expect.That(web.Why.Any(why => why.Contains("input:range", StringComparison.Ordinal)), "why: " + string.Join(", ", web.Why));
            }
            else
            {
                Expect.That(content is not WebContent, "without the list the page is drawn by the client");
            }
        }
    }

    // The protocol's own form example holds a colour field and a file field; this client draws both, so the core does not send it to the web view.
    [TestMethod]
    public async Task The_protocols_form_example_with_a_colour_field_and_a_file_field_is_drawn_by_the_client()
    {
        CollectionAssert.DoesNotContain(Undrawable.Kinds.ToArray(), "input:colour");
        CollectionAssert.DoesNotContain(Undrawable.Kinds.ToArray(), "input:file");
        string shell = File.ReadAllText(Path.Combine(Repository.Root, "crates", "navree-native-core", "transcripts", "fixtures", "store", "shell.json"));
        string form = File.ReadAllText(Path.Combine(Repository.Root, "crates", "navree-native", "corpus", "screen-form.json"));
        byte[] config = ClientConfig.Json("https://shop.example", [], null, new ClientSettings(false, false, 100, "en-IN", null), "10.0", "1.0.0", Caps, null, false, Undrawable.Kinds);
        List<Effect> effects = [];
        using CoreClient client = new(config, update => effects.AddRange(update.Effects));
        await client.SendAsync(Events.Start);
        await client.SendAsync(Events.Response(1, 200, "application/vnd.navree.native+json", null, [], shell));
        FetchEffect first = effects.OfType<FetchEffect>().Last(fetch => fetch.Request == 2);
        Update shown = await client.SendAsync(Events.Response(first.Request, 200, "application/vnd.navree.native+json", null, [], form));
        Models.Screen screen = shown.State.FrontScreen?.Content is NativeContent native ? native.Screen : throw new AssertFailedException("the form was not drawn: " + shown.State.FrontScreen?.Content?.GetType().Name);
        Expect.That(Navree.Design.Tree.Nodes(screen).OfType<Models.Field>().Select(field => field.Input).OfType<Models.ColourInput>().Any(), "the colour field is in the tree");
        Expect.That(Navree.Design.Tree.Nodes(screen).OfType<Models.Field>().Select(field => field.Input).OfType<Models.FileInput>().Any(), "the file field is in the tree");
    }
}
