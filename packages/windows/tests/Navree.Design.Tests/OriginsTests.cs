using System.Text.Json;

namespace Navree.Design.Tests;

/// <summary>"Is this ours?", proved with the vectors every client of the core runs, and with the rules beside them.</summary>
[TestClass]
public sealed class OriginsTests
{
    [TestMethod]
    public void Every_vector_of_the_core_is_answered_as_the_core_says()
    {
        string path = Path.Combine(Repository.Root, "crates/navree-native-core/vectors/addresses.json");
        using JsonDocument document = JsonDocument.Parse(File.ReadAllText(path));
        JsonElement ours = document.RootElement.GetProperty("ours");
        Expect.That(ours.GetArrayLength() >= 30, "the vectors were found");
        foreach (JsonElement vector in ours.EnumerateArray())
        {
            string origin = vector.GetProperty("origin").GetString()!;
            string[] also = [.. vector.GetProperty("also").EnumerateArray().Select(item => item.GetString()!)];
            string address = vector.GetProperty("address").GetString()!;

            // A development build that allows plain http to the machine itself: the vectors' machine is the Android emulator's.
            Origins origins = new(origin, also, loopbackHttp: origin.StartsWith("http://", StringComparison.Ordinal), loopbackHosts: ["10.0.2.2"]);
            Assert.AreEqual(vector.GetProperty("ours").GetBoolean(), origins.Ours(address), address);
        }
    }

    [TestMethod]
    public void An_origin_that_is_not_https_is_not_usable_unless_the_build_allows_the_machine_itself()
    {
        Assert.IsFalse(new Origins("http://shop.example").Usable);
        Assert.IsFalse(new Origins("http://localhost:8095").Usable);
        Assert.IsTrue(new Origins("http://localhost:8095", loopbackHttp: true).Usable);
        Assert.IsFalse(new Origins("http://shop.example", loopbackHttp: true).Usable);
        Assert.IsFalse(new Origins("https://shop.example/path").Usable);
    }

    [TestMethod]
    public void An_origin_that_does_not_read_refuses_everything()
    {
        Origins origins = new("not an origin");
        Assert.IsFalse(origins.Usable);
        Assert.IsFalse(origins.Ours("https://shop.example/"));
    }

    [TestMethod]
    public void A_pattern_needs_two_labels_under_it_and_a_refused_entry_is_named()
    {
        Origins origins = new("https://shop.example", ["https://*.com", "https://*.cdn.example", "http://plain.example"]);
        CollectionAssert.AreEqual(new[] { "https://*.com", "http://plain.example" }, origins.Refused.ToArray());
        Assert.IsTrue(origins.Ours("https://img.cdn.example/a.png"));
        Assert.IsFalse(origins.Ours("https://cdn.example/a.png"));
        Assert.IsFalse(origins.Ours("https://img.cdn.example.evil.example/a.png"));
    }

    [TestMethod]
    public void Only_six_schemes_are_handed_to_the_system()
    {
        foreach (string yes in new[] { "https://shop.example/x", "http://shop.example/", "mailto:help@shop.example", "tel:+911234567890", "sms:+911234567890", "geo:30.9,75.85" })
        {
            Assert.IsTrue(Origins.HandedToSystem(yes), yes);
        }

        foreach (string no in new[] { "file:///c:/x", "javascript:alert(1)", "ms-settings:", "https://a@b.example/", "https://shop.example/a b", "data:text/html,x", "ftp://x.example/", string.Empty, "https://shop.example\\x" })
        {
            Assert.IsFalse(Origins.HandedToSystem(no), no);
        }
    }

    [TestMethod]
    public void A_sign_in_returns_only_by_the_callback_it_named()
    {
        Origins origins = new("https://shop.example");
        Assert.IsTrue(origins.SignInReturn("shop://signed-in?code=1", "shop"));
        Assert.IsFalse(origins.SignInReturn("other://signed-in", "shop"));
        Assert.IsTrue(origins.SignInReturn("https://shop.example/auth/back?code=1", "https://shop.example/auth/back"));
        Assert.IsFalse(origins.SignInReturn("https://shop.example/auth/other", "https://shop.example/auth/back"));
        Assert.IsFalse(origins.SignInReturn("https://evil.example/auth/back", "https://shop.example/auth/back"));
        Assert.IsTrue(origins.SignInReturn("https://shop.example/anything", null));
        Assert.IsFalse(origins.SignInReturn("https://evil.example/anything", null));
    }
}
