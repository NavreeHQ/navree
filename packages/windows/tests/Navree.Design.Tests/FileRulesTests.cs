using Navree.Design.Inputs;

namespace Navree.Design.Tests;

/// <summary>What is offered to a file field when things are dropped on it: files on this computer, and words for the rest.</summary>
[TestClass]
public sealed class DroppedTests
{
    [TestMethod]
    public void Files_on_this_computer_are_offered_and_a_folder_or_an_item_with_no_path_is_refused_with_words()
    {
        Dropped dropped = FileRules.Sort(
        [
            new DroppedItem("Harpreet Kaur - PAN.pdf", @"C:\Users\harpreet\Documents\Harpreet Kaur - PAN.pdf", false),
            new DroppedItem("Photo from the phone.jpg", string.Empty, false),
            new DroppedItem("lohri.jpg", "https://charkha.example/photos/lohri.jpg", false),
            new DroppedItem("Bills", @"C:\Users\harpreet\Documents\Bills", true),
            new DroppedItem("Shop lease, Ludhiana.pdf", @"\\office\papers\Shop lease, Ludhiana.pdf", false),
        ]);

        CollectionAssert.AreEqual(new[] { @"C:\Users\harpreet\Documents\Harpreet Kaur - PAN.pdf", @"\\office\papers\Shop lease, Ludhiana.pdf" }, dropped.Paths.ToArray());
        CollectionAssert.AreEqual(
            new[]
            {
                "Photo from the phone.jpg is not a file on this computer (it may be on a phone or a link from a browser), so it was not added. Save it to a folder first and choose it from there.",
                "lohri.jpg is not a file on this computer (it may be on a phone or a link from a browser), so it was not added. Save it to a folder first and choose it from there.",
                "Folders cannot be added; choose the files in it.",
            },
            dropped.Words.ToArray());
    }

    [TestMethod]
    public void A_drop_of_nothing_offers_nothing_and_says_nothing()
    {
        Dropped dropped = FileRules.Sort([]);
        Assert.AreEqual(0, dropped.Paths.Count);
        Assert.AreEqual(0, dropped.Words.Count);
    }
}

/// <summary>What a file field takes, the words it refuses a file with, and the words said of a file.</summary>
[TestClass]
public sealed class FileRulesTests
{
    private const string Papers = "image/*,.pdf";

    [TestMethod]
    [DataRow("receipt.jpg", true)]
    [DataRow("Aadhaar scan.PNG", true)]
    [DataRow("invoice.pdf", true)]
    [DataRow("INVOICE.PDF", true)]
    [DataRow("notes.txt", false)]
    [DataRow("sheet.xlsx", false)]
    [DataRow("noextension", false)]
    [DataRow("clip.mp4", false)]
    public void The_protocols_own_form_example_takes_pictures_and_pdf_files_and_nothing_else(string name, bool taken)
    {
        Assert.AreEqual(taken, FileRules.Takes(Papers, name, FileRules.MediaTypeOf(name)), name);
    }

    [TestMethod]
    public void A_field_that_names_no_kind_takes_every_file_and_a_kind_named_in_full_takes_that_kind()
    {
        Assert.IsTrue(FileRules.Takes(null, "anything.bin", FileRules.Unknown));
        Assert.IsTrue(FileRules.Takes("  ", "anything.bin", FileRules.Unknown));
        Assert.IsTrue(FileRules.Takes("application/pdf", "bill.pdf", "application/pdf"));
        Assert.IsFalse(FileRules.Takes("application/pdf", "bill.docx", FileRules.MediaTypeOf("bill.docx")));
        Assert.IsFalse(FileRules.Takes("image/*", "mystery", FileRules.Unknown), "a file whose kind is not known is not taken as a picture");
    }

    [TestMethod]
    public void The_pickers_types_follow_the_kinds_the_field_names_and_fall_back_to_every_file()
    {
        CollectionAssert.AreEqual(new[] { ".pdf" }, FileRules.PickerTypes(".pdf").ToArray());
        string[] papers = [.. FileRules.PickerTypes(Papers)];
        CollectionAssert.IsSubsetOf(new[] { ".jpg", ".jpeg", ".png", ".webp", ".pdf" }, papers);
        Assert.IsFalse(papers.Contains(".mp4"));
        CollectionAssert.AreEqual(new[] { "*" }, FileRules.PickerTypes(null).ToArray());
        CollectionAssert.AreEqual(new[] { "*" }, FileRules.PickerTypes("application/x-nobody-knows").ToArray(), "a kind this client has no extensions for shows every file, and is checked after it is chosen");
    }

    [TestMethod]
    [DataRow(0L, "0 bytes")]
    [DataRow(1L, "1 byte")]
    [DataRow(1023L, "1023 bytes")]
    [DataRow(1024L, "1 KB")]
    [DataRow(1536L, "1.5 KB")]
    [DataRow(348160L, "340 KB")]
    [DataRow(1_258_291L, "1.2 MB")]
    [DataRow(104_857_600L, "100 MB")]
    [DataRow(5_368_709_120L, "5 GB")]
    public void A_size_is_said_as_a_person_reads_it(long bytes, string words)
    {
        Assert.AreEqual(words, FileRules.Size(bytes));
    }

    [TestMethod]
    public void The_kinds_a_field_takes_are_said_in_plain_words()
    {
        Assert.AreEqual("pictures and .pdf", FileRules.KindsInWords(Papers));
        Assert.AreEqual("pictures, videos and .zip", FileRules.KindsInWords("image/*, video/*, .zip"));
        Assert.AreEqual(".pdf", FileRules.KindsInWords(".pdf"));
        Assert.AreEqual(string.Empty, FileRules.KindsInWords(null));
    }

    [TestMethod]
    public void A_file_of_a_wrong_kind_is_refused_with_words_that_name_what_the_field_takes()
    {
        Refusal? refusal = FileRules.Refuse("ledger.xlsx", FileRules.MediaTypeOf("ledger.xlsx"), 2048, 0, Papers, FileRules.MostBytes);
        Assert.IsNotNull(refusal);
        Assert.AreEqual("ledger.xlsx is not a kind of file this field takes (it takes pictures and .pdf), so it was not added.", refusal.Words);
    }

    [TestMethod]
    public void A_file_over_the_most_and_a_file_that_takes_the_field_over_it_are_refused_and_one_that_fits_is_not()
    {
        const long Most = 100L * 1024 * 1024;
        Refusal? alone = FileRules.Refuse("film.pdf", "application/pdf", Most + 1, 0, Papers, Most);
        Assert.AreEqual("film.pdf is larger than 100 MB and was not added.", alone?.Words);

        Refusal? together = FileRules.Refuse("page2.pdf", "application/pdf", 30L * 1024 * 1024, 80L * 1024 * 1024, Papers, Most);
        Assert.AreEqual("Adding page2.pdf would take the files of this field over 100 MB, so it was not added.", together?.Words);

        Assert.IsNull(FileRules.Refuse("page1.pdf", "application/pdf", Most, 0, Papers, Most), "exactly the most fits");
        Assert.IsNull(FileRules.Refuse("page3.pdf", "application/pdf", 20L * 1024 * 1024, 80L * 1024 * 1024, Papers, Most));
        Assert.AreEqual("a.pdf is larger than 5 MB and was not added.", FileRules.Refuse("a.pdf", "application/pdf", 6L * 1024 * 1024, 0, Papers, 5L * 1024 * 1024)?.Words, "the most is the client's option");
    }

    [TestMethod]
    public void A_screen_reader_is_told_each_files_name_and_size_and_how_many_are_chosen()
    {
        Assert.AreEqual("Gurpreet Kaur - PAN.pdf, 340 KB", FileRules.Spoken("Gurpreet Kaur - PAN.pdf", 348160));
        Assert.AreEqual("no file chosen", FileRules.Count(0));
        Assert.AreEqual("1 file chosen", FileRules.Count(1));
        Assert.AreEqual("3 files chosen", FileRules.Count(3));
    }

    [TestMethod]
    public void A_name_is_the_last_part_of_a_path_whichever_slash_is_used()
    {
        Assert.AreEqual("a.pdf", FileRules.DisplayName(@"C:\Users\Harpreet\Documents\a.pdf"));
        Assert.AreEqual("b.png", FileRules.DisplayName("/home/harpreet/b.png"));
        Assert.AreEqual("c.txt", FileRules.DisplayName("c.txt"));
    }
}
