using System.Text.Json;

namespace Navree.Runtime.Tests;

/// <summary>The client reads every notice the core names, by the core's own list.</summary>
[TestClass]
public sealed class NoticeTests
{
    // The words of a notice, as the core's schema lists them.
    private static string[] CoreNotices()
    {
        using JsonDocument schema = JsonDocument.Parse(File.ReadAllText(Path.Combine(Repository.Root, "crates", "navree-native-core", "schema", "navree-native-core.schema.json")));
        string[] notices = [.. schema.RootElement.GetProperty("$defs").GetProperty("Message").GetProperty("properties").GetProperty("notice").GetProperty("enum").EnumerateArray().Select(word => word.GetString()!)];
        Assert.IsTrue(notices.Length >= 10, "the core's notices were not found in its schema");
        return notices;
    }

    [TestMethod]
    public void Every_notice_the_core_names_is_read_as_itself_and_named_back_by_the_cores_word()
    {
        foreach (string word in CoreNotices())
        {
            Notice read = ViewReading.ReadNotice(JsonSerializer.SerializeToElement(word));
            Expect.That(read != Notice.Unknown, "the core's notice " + word + " is read as one this client does not know");
            Assert.AreEqual(word, ViewReading.NoticeWord(read));
        }
    }

    [TestMethod]
    public void A_change_that_was_saved_while_the_screen_was_not_brought_up_to_date_is_a_notice_of_its_own()
    {
        Assert.AreEqual(Notice.SavedNotRefreshed, ViewReading.ReadNotice(JsonSerializer.SerializeToElement("saved_not_refreshed")));
        Assert.AreEqual("saved_not_refreshed", ViewReading.NoticeWord(Notice.SavedNotRefreshed));
    }

    [TestMethod]
    public void A_word_this_build_does_not_know_is_no_notice_and_has_no_word()
    {
        Assert.AreEqual(Notice.Unknown, ViewReading.ReadNotice(JsonSerializer.SerializeToElement("from_the_future")));
        Assert.AreEqual(Notice.Unknown, ViewReading.ReadNotice(JsonSerializer.SerializeToElement(7)));
        Assert.AreEqual(string.Empty, ViewReading.NoticeWord(Notice.Unknown));
    }
}
