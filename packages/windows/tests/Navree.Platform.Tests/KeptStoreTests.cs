namespace Navree.Platform.Tests;

/// <summary>What is kept between launches is protected for this person on this device, and goes when it is told to.</summary>
[TestClass]
public sealed class KeptStoreTests
{
    private static string Folder() => Path.Combine(Path.GetTempPath(), "navree-kept-" + Guid.NewGuid().ToString("N"));

    [TestMethod]
    public void A_value_is_kept_and_read_back_and_what_is_on_disk_is_not_the_value()
    {
        string folder = Folder();
        try
        {
            KeptStore store = new(folder, "Navree.Tests");
            store.Write("page:https://shop.example/", "{\"secret\":\"ਸਤ ਸ੍ਰੀ ਅਕਾਲ\"}");
            Assert.AreEqual("{\"secret\":\"ਸਤ ਸ੍ਰੀ ਅਕਾਲ\"}", store.Read("page:https://shop.example/"));
            Assert.IsNull(store.Read("other"));

            string file = Directory.GetFiles(folder, "*.kept").Single();
            Expect.That(!File.ReadAllText(file).Contains("secret", StringComparison.Ordinal), "the value is protected on disk");
            Expect.That((File.GetAttributes(file) & FileAttributes.NotContentIndexed) != 0, "and out of the index");
            Expect.That(!file.Contains("shop", StringComparison.Ordinal), "the name of the file does not say what is in it");
        }
        finally
        {
            Directory.Delete(folder, recursive: true);
        }
    }

    [TestMethod]
    public void A_store_opened_under_another_apps_name_reads_nothing_and_a_copy_that_does_not_open_is_nothing()
    {
        string folder = Folder();
        try
        {
            new KeptStore(folder, "One").Write("k", "v");
            Assert.IsNull(new KeptStore(folder, "Other").Read("k"));
            File.WriteAllBytes(Directory.GetFiles(folder, "*.kept").Single(), [1, 2, 3]);
            Assert.IsNull(new KeptStore(folder, "One").Read("k"));
        }
        finally
        {
            Directory.Delete(folder, recursive: true);
        }
    }

    [TestMethod]
    public void What_is_kept_opens_for_any_code_running_as_the_same_windows_user_that_knows_the_apps_name()
    {
        // What README says of the store: the protection is the Windows user's, and the app's name is no secret.
        string folder = Folder();
        try
        {
            new KeptStore(folder, "Navree.Tests").Write("cookies", "session=gurleen");
            byte[] onDisk = File.ReadAllBytes(Directory.GetFiles(folder, "*.kept").Single());
            string opened = System.Text.Encoding.UTF8.GetString(Dpapi.Unprotect(onDisk, System.Text.Encoding.UTF8.GetBytes("navree:Navree.Tests")));
            Assert.AreEqual("session=gurleen", opened);
        }
        finally
        {
            Directory.Delete(folder, recursive: true);
        }
    }

    [TestMethod]
    public void Clearing_takes_everything_and_a_write_after_it_survives()
    {
        string folder = Folder();
        try
        {
            KeptStore store = new(folder, "Navree.Tests");
            store.Write("a", "1");
            store.Write("b", "2");
            Expect.That(store.Count == 2, "two values are kept");

            // The order the core's effects arrive in: clear, then write.
            store.Clear();
            store.Write("c", "3");
            Assert.IsNull(store.Read("a"));
            Assert.AreEqual("3", store.Read("c"));

            store.Write("c", null);
            Assert.IsNull(store.Read("c"));
            store.Write("never-there", null);
        }
        finally
        {
            Directory.Delete(folder, recursive: true);
        }
    }

    [TestMethod]
    public void Taking_what_is_kept_for_a_person_leaves_the_keys_named_and_the_cache_and_never_reads_again_a_file_it_could_not_delete()
    {
        string folder = Folder();
        try
        {
            KeptStore store = new(folder, "Navree.Tests");
            store.Write("pending-notifications", "[{\"id\":\"call-simran\"}]");
            string reminder = Directory.GetFiles(folder, "*.kept").Single();
            store.Write("cookies", "session=manpreet");
            store.Write("snapshot", "{\"typed\":\"Jasleen's draft\"}");
            store.WriteCached("boot", "{\"app\":\"Charkha Supply Co.\"}");
            store.WriteCached("snapshot", "what the core keeps under the same word");

            // Another program holds the reminders' file (a scanner is reading it), so it cannot be deleted.
            using (FileStream other = new(reminder, FileMode.Open, FileAccess.Read, FileShare.Read))
            {
                store.ClearOwn("cookies");
                Expect.That(File.Exists(reminder), "the file another program holds was deleted: this test shows nothing");
                Assert.IsNull(store.Read("pending-notifications"), "a file of the person before that could not be deleted is still read");
            }

            Assert.AreEqual("session=manpreet", store.Read("cookies"), "a key that was to stay went");
            Assert.IsNull(store.Read("snapshot"), "what was kept for the person before stayed");
            Assert.AreEqual("{\"app\":\"Charkha Supply Co.\"}", store.ReadCached("boot"), "the core's cache went with what the client keeps for the person");
            Assert.AreEqual("what the core keeps under the same word", store.ReadCached("snapshot"));

            // A later start does not read the file either, and deletes it now that nothing holds it.
            KeptStore next = new(folder, "Navree.Tests");
            Assert.IsNull(next.Read("pending-notifications"), "the next start reads a file of the person before");
            Assert.AreEqual(0, next.DeleteSetAside());
            Expect.That(!File.Exists(reminder), "the file set aside is still on disk after it was let go");

            // What is kept under the same key afterwards is the next person's, and is read.
            next.Write("pending-notifications", "[{\"id\":\"stock-count\"}]");
            Assert.AreEqual("[{\"id\":\"stock-count\"}]", next.Read("pending-notifications"));
        }
        finally
        {
            Directory.Delete(folder, recursive: true);
        }
    }

    [TestMethod]
    public void Something_is_kept_for_a_person_when_a_value_of_the_clients_own_is_there_besides_the_keys_named()
    {
        string folder = Folder();
        try
        {
            KeptStore store = new(folder, "Navree.Tests");
            Expect.That(!store.HoldsOwn("cookies", "person"), "a store that was never written to holds something");

            // The session's cookies and the core's cache say nothing of a person the client keeps something for.
            store.Write("cookies", "session=manpreet");
            store.WriteCached("boot", "{\"app\":\"Charkha Supply Co.\"}");
            store.WriteCached("pending-notifications", "what the core keeps under the same word");
            Expect.That(!store.HoldsOwn("cookies", "person"), "the cookies, or the core's cache, are counted as something kept for a person");

            // A reminder that waits is.
            string[] before = Directory.GetFiles(folder, "*.kept");
            store.Write("pending-notifications", "[{\"id\":\"call-simran\"}]");
            string reminder = Directory.GetFiles(folder, "*.kept").Except(before, StringComparer.OrdinalIgnoreCase).Single();
            Expect.That(store.HoldsOwn("cookies", "person"), "a reminder that waits is not counted as something kept for a person");

            // So is one that cannot be opened here: it is there, and nothing says whose it is.
            File.WriteAllBytes(reminder, [1, 2, 3]);
            Assert.IsNull(store.Read("pending-notifications"));
            Expect.That(store.HoldsOwn("cookies", "person"), "a file that cannot be opened is not counted: what is kept beside it would be taken for the next person's");

            // A file that was set aside is never read again: it is nobody's any more, and does not count.
            using (FileStream other = new(reminder, FileMode.Open, FileAccess.Read, FileShare.Read))
            {
                store.ClearOwn("cookies");
                Expect.That(File.Exists(reminder), "the file another program holds was deleted: this test shows nothing");
                Expect.That(!store.HoldsOwn("cookies", "person"), "a file that was set aside is counted as something kept for a person");
            }
        }
        finally
        {
            Directory.Delete(folder, recursive: true);
        }
    }

    [TestMethod]
    public void The_default_folder_is_under_the_local_application_data_and_not_the_roaming_one()
    {
        string folder = KeptStore.DefaultFolder("Charkha");
        Expect.That(folder.StartsWith(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), StringComparison.OrdinalIgnoreCase), "local");
        Expect.That(!folder.StartsWith(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), StringComparison.OrdinalIgnoreCase)
            || Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData).Length == 0, "not roaming");
    }
}
