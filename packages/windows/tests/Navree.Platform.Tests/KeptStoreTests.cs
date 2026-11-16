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
    public void The_default_folder_is_under_the_local_application_data_and_not_the_roaming_one()
    {
        string folder = KeptStore.DefaultFolder("Charkha");
        Expect.That(folder.StartsWith(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), StringComparison.OrdinalIgnoreCase), "local");
        Expect.That(!folder.StartsWith(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), StringComparison.OrdinalIgnoreCase)
            || Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData).Length == 0, "not roaming");
    }
}
