using System.Text;
using System.Text.Json;

namespace Navree.Core.Tests;

/// <summary>The bridge against the real library: what is sent goes through the core's DLL and comes back.</summary>
[TestClass]
public sealed class RoundTripTests
{
    private static readonly byte[] Config =
        """{"origin":"https://shop.example","client":{"platform":"windows","form":"desktop","vocab":1,"text_percent":100}}"""u8.ToArray();

    [TestMethod]
    public void A_core_is_made_started_and_read()
    {
        using NavreeCore core = new(Config);

        using JsonDocument update = JsonDocument.Parse(core.Send("""{"t":"start"}"""u8));
        JsonElement effect = update.RootElement.GetProperty("effects")[0];
        Assert.AreEqual("fetch", effect.GetProperty("t").GetString());
        Assert.AreEqual("https://shop.example/_navree/native/shell", effect.GetProperty("url").GetString());

        using JsonDocument nav = JsonDocument.Parse(core.View("""{"part":"nav"}"""u8));
        Assert.AreEqual("starting", nav.RootElement.GetProperty("stage").GetString());
    }

    [TestMethod]
    public void The_whole_view_state_is_read_with_no_query()
    {
        using NavreeCore core = new(Config);
        _ = core.Send("""{"t":"start"}"""u8);

        using JsonDocument all = JsonDocument.Parse(core.View());
        Assert.AreEqual("starting", all.RootElement.GetProperty("stage").GetString());
    }

    [TestMethod]
    public void The_client_the_core_describes_to_the_server_is_this_one()
    {
        using NavreeCore core = new(Config);

        using JsonDocument update = JsonDocument.Parse(core.Send("""{"t":"start"}"""u8));
        JsonElement headers = update.RootElement.GetProperty("effects")[0].GetProperty("headers");
        string? client = null;
        foreach (JsonElement header in headers.EnumerateArray())
        {
            if (header[0].GetString() == "x-navree-client")
            {
                client = header[1].GetString();
            }
        }

        Assert.IsNotNull(client);
        Assert.Contains("platform=windows; form=desktop", client, StringComparison.Ordinal);
    }

    [TestMethod]
    public void A_configuration_that_does_not_read_is_refused()
    {
        Assert.ThrowsExactly<ArgumentException>(() =>
        {
            new NavreeCore("{not json"u8).Dispose();
        });
    }

    [TestMethod]
    public void An_event_the_core_does_not_know_is_an_error_answer_not_a_crash()
    {
        using NavreeCore core = new(Config);

        string answer = Encoding.UTF8.GetString(core.Send("""{"t":"teleport"}"""u8));
        Assert.Contains("\"error\"", answer, StringComparison.Ordinal);
    }

    [TestMethod]
    public void After_the_core_has_ended_a_call_is_refused()
    {
        NavreeCore core = new(Config);
        core.Dispose();
        core.Dispose();

        Assert.ThrowsExactly<ObjectDisposedException>(() => core.Send("""{"t":"start"}"""u8));
        Assert.ThrowsExactly<ObjectDisposedException>(() => core.View());
    }
}
