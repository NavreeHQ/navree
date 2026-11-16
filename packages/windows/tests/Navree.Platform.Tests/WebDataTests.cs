namespace Navree.Platform.Tests;

/// <summary>What the web view leaves on the device goes when the session ends, whether or not a web view is running.</summary>
[TestClass]
public sealed class WebDataTests
{
    private static string Folder()
    {
        string folder = Path.Combine(Path.GetTempPath(), "navree-web-data-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Path.Combine(folder, "EBWebView", "Default", "Local Storage"));
        File.WriteAllText(Path.Combine(folder, "EBWebView", "Default", "Local Storage", "000003.log"), "navree_wipe_probe");
        return folder;
    }

    [TestMethod]
    public async Task A_folder_with_data_in_it_is_taken_away_whole()
    {
        string folder = Folder();
        Assert.IsTrue(await WebData.DeleteAsync(folder, TimeSpan.FromSeconds(2)));
        Assert.IsFalse(Directory.Exists(folder));
    }

    [TestMethod]
    public async Task A_folder_that_is_not_there_is_already_gone()
    {
        Assert.IsTrue(await WebData.DeleteAsync(Path.Combine(Path.GetTempPath(), "navree-web-data-none"), TimeSpan.FromSeconds(1)));
    }

    [TestMethod]
    public async Task A_file_held_by_a_leaving_process_is_waited_for_and_then_the_folder_goes()
    {
        string folder = Folder();
        string held = Path.Combine(folder, "EBWebView", "Default", "Local Storage", "000003.log");
        FileStream holding = new(held, FileMode.Open, FileAccess.Read, FileShare.None);
        _ = Task.Run(async () =>
        {
            await Task.Delay(400);
            await holding.DisposeAsync();
        });
        Assert.IsTrue(await WebData.DeleteAsync(folder, TimeSpan.FromSeconds(10), TimeSpan.FromMilliseconds(50)));
        Assert.IsFalse(Directory.Exists(folder));
    }

    [TestMethod]
    public async Task A_file_that_stays_held_is_a_folder_that_stays_and_says_so()
    {
        string folder = Folder();
        string held = Path.Combine(folder, "EBWebView", "Default", "Local Storage", "000003.log");
        await using (FileStream holding = new(held, FileMode.Open, FileAccess.Read, FileShare.None))
        {
            Assert.IsFalse(await WebData.DeleteAsync(folder, TimeSpan.FromMilliseconds(300), TimeSpan.FromMilliseconds(50)));
            Assert.IsTrue(Directory.Exists(folder));
        }

        Assert.IsTrue(await WebData.DeleteAsync(folder, TimeSpan.FromSeconds(2)));
    }

    [TestMethod]
    public void The_web_view_makes_its_own_blank_documents_and_blobs_and_inline_data_is_not_a_page()
    {
        Assert.IsTrue(WebData.IsOwnDocument("about:blank"));
        Assert.IsTrue(WebData.IsOwnDocument("blob:https://shop.example/1f2e"));
        Assert.IsFalse(WebData.IsOwnDocument("data:text/html,<script>alert(1)</script>"));
        Assert.IsFalse(WebData.IsOwnDocument("https://shop.example/"));
        Assert.IsTrue(WebData.IsOwnRequest("data:image/png;base64,AAAA"));
        Assert.IsFalse(WebData.IsOwnRequest("https://evil.example/"));
    }

    [TestMethod]
    public void A_request_of_the_web_view_goes_only_to_the_apps_origin_and_none_goes_to_the_network_while_signing_out()
    {
        Navree.Design.Origins shop = new("https://shop.example");
        Assert.AreEqual(WebAnswer.Ours, WebData.Answer("https://shop.example/orders", shop.Ours, wiping: false));
        Assert.AreEqual(WebAnswer.Refused, WebData.Answer("https://other.example/collect", shop.Ours, wiping: false));
        Assert.AreEqual(WebAnswer.Refused, WebData.Answer("http://shop.example/orders", shop.Ours, wiping: false));
        Assert.AreEqual(WebAnswer.Refused, WebData.Answer("https://shop.example/orders", shop.Ours, wiping: true));
        Assert.AreEqual(WebAnswer.AsItIs, WebData.Answer("about:blank", shop.Ours, wiping: true));
        Assert.AreEqual(WebAnswer.AsItIs, WebData.Answer("blob:https://shop.example/1f2e", shop.Ours, wiping: false));
    }
}
