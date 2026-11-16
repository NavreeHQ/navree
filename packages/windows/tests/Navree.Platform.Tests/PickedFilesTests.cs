using System.Security.Cryptography;
using System.Text;
using Navree.Design.Inputs;

namespace Navree.Platform.Tests;

/// <summary>The files a person chose for forms: held under tokens, checked, read when the request is written, and gone when they are gone.</summary>
[TestClass]
public sealed class PickedFilesTests
{
    private string folder = string.Empty;
    private string copies = string.Empty;

    [TestInitialize]
    public void Make()
    {
        folder = Path.Combine(Path.GetTempPath(), "navree-picked-" + Guid.NewGuid().ToString("N"));
        copies = Path.Combine(folder, "copies");
        _ = Directory.CreateDirectory(folder);
    }

    [TestCleanup]
    public void Remove()
    {
        try
        {
            Directory.Delete(folder, true);
        }
        catch (IOException)
        {
            // A temporary folder that cannot be deleted is left for the system to clear.
        }
    }

    private string Write(string name, byte[] bytes)
    {
        string path = Path.Combine(folder, name);
        File.WriteAllBytes(path, bytes);
        return path;
    }

    private static byte[] Bytes(int length, int seed)
    {
        byte[] bytes = new byte[length];
        new Random(seed).NextBytes(bytes);
        return bytes;
    }

    private static FieldRule Rule(string? accept = "image/*,.pdf", bool multiple = true, bool keep = false, long most = FileRules.MostBytes) => new(accept, multiple, keep, most);

    // A JPEG whose EXIF block holds a GPS directory with a latitude of three rationals (little endian), then a scan of 3 bytes.
    private static byte[] PhotoWithPlace()
    {
        byte[] tiff =
        [
            .. "II"u8.ToArray(), 42, 0, 8, 0, 0, 0,
            1, 0, 0x25, 0x88, 4, 0, 1, 0, 0, 0, 26, 0, 0, 0, 0, 0, 0, 0,
            1, 0, 2, 0, 5, 0, 3, 0, 0, 0, 44, 0, 0, 0, 0, 0, 0, 0,
            30, 0, 0, 0, 1, 0, 0, 0, 54, 0, 0, 0, 1, 0, 0, 0, 7, 0, 0, 0, 1, 0, 0, 0,
        ];
        byte[] exif = [.. "Exif\0\0"u8.ToArray(), .. tiff];
        int length = exif.Length + 2;
        return [0xFF, 0xD8, 0xFF, 0xE1, (byte)(length >> 8), (byte)length, .. exif, 0xFF, 0xDA, 0, 2, 7, 8, 9, 0xFF, 0xD9];
    }

    [TestMethod]
    public async Task A_file_is_held_under_a_token_and_what_is_sent_for_it_is_its_bytes_exactly()
    {
        PickedFiles files = new(copies);
        byte[] content = Bytes(3 * 1024 * 1024, 7);
        string path = Write("Harpreet Kaur - PAN.pdf", content);

        Picked picked = await files.AddAsync(path, Rule(), 0, CancellationToken.None);

        Assert.IsNull(picked.Refusal);
        Assert.IsNotNull(picked.File);
        Assert.AreEqual("Harpreet Kaur - PAN.pdf", picked.File.Name);
        Assert.AreEqual("application/pdf", picked.File.MediaType);
        Assert.AreEqual(content.Length, picked.File.Size);
        Assert.AreEqual(32, picked.File.Token.Length);
        Assert.IsFalse(picked.File.Token.Contains('\\', StringComparison.Ordinal) || picked.File.Token.Contains(Path.GetFileName(path), StringComparison.Ordinal), "the token says nothing of where the file is");
        Assert.IsTrue(files.Available(picked.File.Token));
        await using Stream stream = files.Open(picked.File.Token)!;
        using MemoryStream read = new();
        await stream.CopyToAsync(read);
        CollectionAssert.AreEqual(SHA256.HashData(content), SHA256.HashData(read.ToArray()));
    }

    [TestMethod]
    public async Task A_file_of_a_wrong_kind_a_file_that_is_too_large_and_a_file_that_takes_the_field_over_the_most_are_refused_and_held_nowhere()
    {
        PickedFiles files = new(copies);
        string text = Write("notes.txt", Bytes(100, 1));
        string big = Write("scan.pdf", Bytes(6000, 2));
        string small = Write("page.pdf", Bytes(3000, 3));

        Picked wrong = await files.AddAsync(text, Rule(most: 5000), 0, CancellationToken.None);
        Picked large = await files.AddAsync(big, Rule(most: 5000), 0, CancellationToken.None);
        Picked together = await files.AddAsync(small, Rule(most: 5000), 3000, CancellationToken.None);
        Picked fits = await files.AddAsync(small, Rule(most: 5000), 2000, CancellationToken.None);

        Assert.AreEqual("notes.txt is not a kind of file this field takes (it takes pictures and .pdf), so it was not added.", wrong.Refusal);
        Assert.AreEqual("scan.pdf is larger than 4.9 KB and was not added.", large.Refusal);
        Assert.AreEqual("Adding page.pdf would take the files of this field over 4.9 KB, so it was not added.", together.Refusal);
        Assert.IsNull(wrong.File);
        Assert.IsNull(large.File);
        Assert.IsNull(together.File);
        Assert.IsNotNull(fits.File);
    }

    [TestMethod]
    public async Task A_file_deleted_after_it_was_chosen_is_gone_and_is_left_out_not_sent_empty()
    {
        PickedFiles files = new(copies);
        List<string> missing = [];
        files.WentMissing += tokens => missing.AddRange(tokens);
        string path = Write("lease.pdf", Bytes(2000, 4));
        Picked picked = await files.AddAsync(path, Rule(), 0, CancellationToken.None);
        string token = picked.File!.Token;

        File.Delete(path);

        Assert.IsFalse(files.Available(token));
        Assert.IsNull(files.Open(token), "no stream, so the request has no part for it and never an empty one");
        CollectionAssert.AreEqual(new[] { token }, missing);
        Assert.IsNull(files.Open("0123456789abcdef0123456789abcdef"), "a token that was never given opens nothing");
    }

    [TestMethod]
    public async Task A_file_that_is_a_folder_or_is_not_there_is_refused_with_words()
    {
        PickedFiles files = new(copies);
        Picked absent = await files.AddAsync(Path.Combine(folder, "nowhere.pdf"), Rule(), 0, CancellationToken.None);
        Assert.AreEqual("nowhere.pdf could not be read, so it was not added.", absent.Refusal);
    }

    [TestMethod]
    [DataRow("")]
    [DataRow("   ")]
    [DataRow("Lohri at Jalandhar.jpg")]
    [DataRow("https://charkha.example/photos/lohri.jpg")]
    public async Task An_item_with_no_path_on_this_computer_is_refused_with_words(string path)
    {
        PickedFiles files = new(copies);

        Picked picked = await files.AddAsync(path, Rule(accept: null), 0, CancellationToken.None);

        Assert.IsNull(picked.File);
        StringAssert.Contains(picked.Refusal, "is not a file on this computer");
    }

    [TestMethod]
    public async Task A_photo_has_its_place_taken_out_in_a_copy_that_is_what_is_sent_and_the_original_is_left_alone()
    {
        PickedFiles files = new(copies);
        byte[] photo = PhotoWithPlace();
        string path = Write("Ludhiana-gate.jpg", photo);

        Picked picked = await files.AddAsync(path, Rule(), 0, CancellationToken.None);

        Assert.IsNull(picked.Refusal);
        CollectionAssert.AreEqual(photo, File.ReadAllBytes(path), "the person's own file is never changed");
        await using Stream stream = files.Open(picked.File!.Token)!;
        using MemoryStream sent = new();
        await stream.CopyToAsync(sent);
        Assert.AreEqual(photo.Length, sent.Length);
        Assert.IsFalse(sent.ToArray().AsSpan().IndexOf(new byte[] { 30, 0, 0, 0, 1, 0, 0, 0, 54 }) >= 0, "the latitude is not in what is sent");
        Assert.AreEqual(1, Directory.GetFiles(copies).Length);

        files.Release(picked.File.Token);
        Assert.AreEqual(0, Directory.GetFiles(copies).Length, "a copy goes when its file is taken out of the field");
    }

    [TestMethod]
    public async Task A_field_that_keeps_the_place_sends_the_photo_as_it_is_without_a_copy()
    {
        PickedFiles files = new(copies);
        byte[] photo = PhotoWithPlace();
        string path = Write("Amritsar-gate.jpg", photo);

        Picked picked = await files.AddAsync(path, Rule(keep: true), 0, CancellationToken.None);

        await using Stream stream = files.Open(picked.File!.Token)!;
        using MemoryStream sent = new();
        await stream.CopyToAsync(sent);
        CollectionAssert.AreEqual(photo, sent.ToArray());
        Assert.IsFalse(Directory.Exists(copies) && Directory.GetFiles(copies).Length > 0);
    }

    [TestMethod]
    public async Task A_photo_whose_place_cannot_be_taken_out_is_refused_with_words()
    {
        PickedFiles files = new(copies);
        string path = Write("iPhone-photo.heic", [0, 0, 0, 24, .. "ftypheic"u8.ToArray(), 0, 0, 0, 0, .. "mif1heic"u8.ToArray()]);

        Picked picked = await files.AddAsync(path, Rule(), 0, CancellationToken.None);

        Assert.IsNull(picked.File);
        StringAssert.StartsWith(picked.Refusal, "iPhone-photo.heic: The place it was taken cannot be taken out of a HEIC picture here");
        Assert.AreEqual("image/heic", FileRules.MediaTypeOf("iPhone-photo.heic"));
    }

    private static async Task<byte[]> SentAsync(PickedFiles files, Picked picked)
    {
        await using Stream stream = files.Open(picked.File!.Token)!;
        using MemoryStream sent = new();
        await stream.CopyToAsync(sent);
        return sent.ToArray();
    }

    private static readonly byte[] Latitude = [30, 0, 0, 0, 1, 0, 0, 0, 54];

    [TestMethod]
    [DataRow("Golden Temple, Amritsar.jfif")]
    [DataRow("Golden Temple, Amritsar.JFIF")]
    [DataRow("IMG_20261116_101500")]
    [DataRow("Golden Temple, Amritsar.txt")]
    [DataRow("Golden Temple, Amritsar.pdf")]
    public async Task A_photo_is_known_by_its_bytes_whatever_its_name_says_and_its_place_is_taken_out(string name)
    {
        PickedFiles files = new(copies);
        byte[] photo = PhotoWithPlace();

        Picked picked = await files.AddAsync(Write(name, photo), Rule(accept: null), 0, CancellationToken.None);

        Assert.IsNull(picked.Refusal, picked.Refusal);
        byte[] sent = await SentAsync(files, picked);
        Assert.AreEqual(photo.Length, sent.Length);
        Assert.IsTrue(sent.AsSpan().IndexOf(Latitude) < 0, "the latitude is not in what is sent");
        Assert.IsTrue(photo.AsSpan().IndexOf(Latitude) >= 0);
    }

    [TestMethod]
    public async Task A_photo_named_jfif_or_with_no_name_of_its_kind_is_sent_as_a_jpeg_picture()
    {
        PickedFiles files = new(copies);

        Picked jfif = await files.AddAsync(Write("Patiala fort.jfif", PhotoWithPlace()), Rule(accept: "image/*"), 0, CancellationToken.None);
        Picked bare = await files.AddAsync(Write("IMG_20261116_101500", PhotoWithPlace()), Rule(accept: null), 0, CancellationToken.None);

        Assert.AreEqual("image/jpeg", jfif.File?.MediaType, jfif.Refusal);
        Assert.AreEqual("image/jpeg", bare.File?.MediaType, bare.Refusal);
    }

    [TestMethod]
    [DataRow("heic", "IMG_4410")]
    [DataRow("avif", "IMG_4410.bin")]
    [DataRow("mif1", "IMG_4410.jpg")]
    public async Task A_picture_of_the_heif_family_is_known_by_its_brand_and_refused_whatever_its_name_says(string brand, string name)
    {
        PickedFiles files = new(copies);
        byte[] heif = [0, 0, 0, 24, .. "ftyp"u8.ToArray(), .. Encoding.ASCII.GetBytes(brand), 0, 0, 0, 0, .. "mif1"u8.ToArray(), .. Encoding.ASCII.GetBytes(brand), 0, 0, 0, 8, .. "free"u8.ToArray()];

        Picked picked = await files.AddAsync(Write(name, heif), Rule(accept: null), 0, CancellationToken.None);

        Assert.IsNull(picked.File, "a picture whose place cannot be taken out is not held");
        StringAssert.StartsWith(picked.Refusal, name + ": The place it was taken cannot be taken out of ");
    }

    [TestMethod]
    [DataRow(new byte[] { 0xFF, 0x0A, 0xFA, 0x7F, 0x01, 0x90 }, "JPEG XL")]
    [DataRow(new byte[] { 0, 0, 0, 0x0C, 0x4A, 0x58, 0x4C, 0x20, 0x0D, 0x0A, 0x87, 0x0A, 0, 0, 0, 0 }, "JPEG XL")]
    [DataRow(new byte[] { 0, 0, 0, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A, 0, 0, 0, 0 }, "JPEG 2000")]
    [DataRow(new byte[] { 0x49, 0x49, 0x2B, 0x00, 0x08, 0x00, 0x00, 0x00 }, "BigTIFF")]
    public async Task A_picture_of_a_kind_the_place_cannot_be_taken_out_of_is_refused_by_its_bytes_even_with_no_name_of_its_kind(byte[] bytes, string kind)
    {
        PickedFiles files = new(copies);

        Picked picked = await files.AddAsync(Write("from-the-phone", bytes), Rule(accept: null), 0, CancellationToken.None);

        Assert.IsNull(picked.File);
        StringAssert.Contains(picked.Refusal, kind);
    }

    [TestMethod]
    public async Task A_motion_photo_is_refused_with_words_and_a_png_whose_place_went_is_sent_at_its_new_size()
    {
        PickedFiles files = new(copies);
        byte[] video = [0, 0, 0, 20, .. "ftyp"u8.ToArray(), .. "isom"u8.ToArray(), 0, 0, 2, 0, .. "mp41"u8.ToArray()];
        byte[] packet = Encoding.UTF8.GetBytes("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:Description photoshop:City=\"Chandigarh\"/></x:xmpmeta>");
        byte[] png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, .. Chunk("IHDR", [0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]), .. Chunk("tEXt", [.. "XML:com.adobe.xmp"u8.ToArray(), 0, .. packet]), .. Chunk("IDAT", [1, 2, 3]), .. Chunk("IEND", [])];

        Picked motion = await files.AddAsync(Write("PXL_20261116_101500.MP.jpg", [.. PhotoWithPlace(), .. video]), Rule(accept: null), 0, CancellationToken.None);
        Picked picture = await files.AddAsync(Write("Sukhna Lake.png", png), Rule(accept: null), 0, CancellationToken.None);

        Assert.IsNull(motion.File);
        StringAssert.Contains(motion.Refusal, "video");
        byte[] sent = await SentAsync(files, picture);
        Assert.AreEqual(sent.Length, picture.File!.Size);
        Assert.AreEqual(png.Length - 12 - 18 - packet.Length, sent.Length);
        Assert.IsFalse(Encoding.UTF8.GetString(sent).Contains("Chandigarh", StringComparison.Ordinal));
    }

    private static byte[] Chunk(string kind, byte[] data)
    {
        byte[] whole = new byte[12 + data.Length];
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(whole, (uint)data.Length);
        Encoding.ASCII.GetBytes(kind).CopyTo(whole, 4);
        data.CopyTo(whole, 8);
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(whole.AsSpan(8 + data.Length), PlaceRules.Crc32(whole.AsSpan(4, 4 + data.Length)));
        return whole;
    }

    [TestMethod]
    public async Task A_video_and_a_paper_are_not_pictures_and_are_sent_as_they_are()
    {
        PickedFiles files = new(copies);
        byte[] video = [0, 0, 0, 20, .. "ftyp"u8.ToArray(), .. "isom"u8.ToArray(), 0, 0, 2, 0, .. "mp41"u8.ToArray(), 0, 0, 0, 8, .. "free"u8.ToArray()];
        byte[] paper = [.. "%PDF-1.7\n"u8.ToArray(), .. Bytes(500, 9)];

        Picked clip = await files.AddAsync(Write("Lohri at Jalandhar", video), Rule(accept: null), 0, CancellationToken.None);
        Picked bill = await files.AddAsync(Write("bill", paper), Rule(accept: null), 0, CancellationToken.None);

        CollectionAssert.AreEqual(video, await SentAsync(files, clip));
        CollectionAssert.AreEqual(paper, await SentAsync(files, bill));
    }

    [TestMethod]
    public async Task Clearing_lets_go_of_every_file_deletes_the_copies_and_a_copy_left_by_an_earlier_run_goes_when_the_registry_is_made()
    {
        PickedFiles files = new(copies);
        Picked picked = await files.AddAsync(Write("Moga.jpg", PhotoWithPlace()), Rule(), 0, CancellationToken.None);
        Assert.AreEqual(1, Directory.GetFiles(copies).Length);

        files.Clear();

        Assert.AreEqual(0, Directory.GetFiles(copies).Length);
        Assert.IsNull(files.Open(picked.File!.Token));

        File.WriteAllText(Path.Combine(copies, "left-by-a-run-that-ended"), "x", Encoding.UTF8);
        _ = new PickedFiles(copies);
        Assert.AreEqual(0, Directory.GetFiles(copies).Length);
    }

    private int CopiesOnDisk() => Directory.Exists(copies) ? Directory.GetFiles(copies).Length : 0;

    [TestMethod]
    public async Task A_photoshop_document_is_not_sent_from_a_field_that_does_not_keep_the_place_and_is_sent_as_it_is_from_one_that_does()
    {
        PickedFiles files = new(copies);

        // A Photoshop document: its first bytes say so, and it can hold the place of the photo it was made from.
        byte[] document = [.. "8BPS"u8.ToArray(), 0, 1, 0, 0, 0, 0, 0, 0, 0, 3, .. PhotoWithPlace()];
        string path = Write("Wagah border.psd", document);

        Picked refused = await files.AddAsync(path, Rule(accept: null), 0, CancellationToken.None);
        Assert.IsNull(refused.File, "a Photoshop document went out of a field that does not keep the place");
        StringAssert.Contains(refused.Refusal, "Photoshop");
        Assert.AreEqual(0, CopiesOnDisk());

        // Named as a paper, it is the same file.
        Picked renamed = await files.AddAsync(Write("Wagah border.pdf", document), Rule(accept: null), 0, CancellationToken.None);
        Assert.IsNull(renamed.File, "a Photoshop document named as a paper went out of a field that does not keep the place");

        // A field that keeps the place sends it as it is.
        Picked kept = await files.AddAsync(path, Rule(accept: null, keep: true), 0, CancellationToken.None);
        Assert.IsNotNull(kept.File);
        CollectionAssert.AreEqual(document, await SentAsync(files, kept));
    }

    [TestMethod]
    public async Task A_gimp_document_and_a_jpeg_xr_picture_are_not_sent_from_a_field_that_does_not_keep_the_place_and_a_file_nothing_knows_is()
    {
        PickedFiles files = new(copies);

        // A GIMP document, by its first bytes, under its own name and under a paper's.
        byte[] document = [.. "gimp xcf v011"u8.ToArray(), 0, .. PhotoWithPlace()];
        string path = Write("Rock Garden.xcf", document);
        Picked refused = await files.AddAsync(path, Rule(accept: null), 0, CancellationToken.None);
        Assert.IsNull(refused.File, "a GIMP document went out of a field that does not keep the place");
        StringAssert.Contains(refused.Refusal, "GIMP");
        Picked renamed = await files.AddAsync(Write("Rock Garden.pdf", document), Rule(accept: null), 0, CancellationToken.None);
        Assert.IsNull(renamed.File, "a GIMP document named as a paper went out of a field that does not keep the place");

        // A JPEG XR picture.
        byte[] xr = [0x49, 0x49, 0xBC, 1, 0x20, 0, 0, 0, .. PhotoWithPlace()];
        Picked picture = await files.AddAsync(Write("Rock Garden.dat", xr), Rule(accept: null), 0, CancellationToken.None);
        Assert.IsNull(picture.File, "a JPEG XR picture went out of a field that does not keep the place");
        StringAssert.Contains(picture.Refusal, "JPEG XR");

        // A document of an editor whose bytes this client does not know, by its name.
        byte[] other = [.. "KDocument-1"u8.ToArray(), .. PhotoWithPlace()];
        Picked named = await files.AddAsync(Write("Rock Garden.kra", other), Rule(accept: null), 0, CancellationToken.None);
        Assert.IsNull(named.File, "a file named as an editor's document went out of a field that does not keep the place");
        Assert.AreEqual(0, CopiesOnDisk());

        // A field that keeps the place sends each as it is.
        Picked kept = await files.AddAsync(path, Rule(accept: null, keep: true), 0, CancellationToken.None);
        Assert.IsNotNull(kept.File);
        CollectionAssert.AreEqual(document, await SentAsync(files, kept));

        // The same bytes under a name that says nothing are known neither way: the file is sent as it is, and is not looked into.
        Picked unknown = await files.AddAsync(Write("Rock Garden.bin", other), Rule(accept: null), 0, CancellationToken.None);
        Assert.IsNotNull(unknown.File, "a file nothing says is a picture was refused");
        CollectionAssert.AreEqual(other, await SentAsync(files, unknown));
    }

    [TestMethod]
    public async Task A_photo_chosen_in_a_session_that_has_ended_is_not_held_and_leaves_no_copy()
    {
        PickedFiles files = new(copies);
        string path = Write("Moga.jpg", PhotoWithPlace());
        long chosenIn = files.Session;

        // The person signs out while the photo they chose is still waiting to be looked at.
        files.Clear();
        Picked late = await files.AddAsync(path, Rule(), 0, chosenIn, CancellationToken.None);

        Assert.IsNull(late.File, "a file chosen in a session that has ended is held");
        StringAssert.Contains(late.Refusal, "Moga.jpg");
        Assert.AreEqual(0, CopiesOnDisk(), "a copy of a photo of the person who left is on disk");
        Assert.AreNotEqual(chosenIn, files.Session);

        // A file with no copy (its field keeps the place) is not held for the session that ended either.
        Picked kept = await files.AddAsync(path, Rule(keep: true), 0, chosenIn, CancellationToken.None);
        Assert.IsNull(kept.File, "a file chosen in a session that has ended is held");

        // What the next person chooses is held as before.
        Picked next = await files.AddAsync(path, Rule(), 0, CancellationToken.None);
        Assert.IsNotNull(next.File);
        Assert.AreEqual(1, CopiesOnDisk());
        Expect.That(files.Available(next.File.Token), "the next person's file is not there to be sent");
    }

    [TestMethod]
    public async Task The_end_of_a_session_is_not_said_to_be_done_while_a_copy_of_a_photo_is_still_on_disk()
    {
        PickedFiles files = new(copies);
        Picked picked = await files.AddAsync(Write("Moga.jpg", PhotoWithPlace()), Rule(), 0, CancellationToken.None);
        Assert.IsNotNull(picked.File);
        string copy = Directory.GetFiles(copies).Single();

        // Another program holds the copy in a way that does not let it be deleted.
        using (FileStream other = new(copy, FileMode.Open, FileAccess.Read, FileShare.Read))
        {
            _ = await Assert.ThrowsExactlyAsync<IOException>(() => files.ClearAsync(), "the files' step said it was done while a copy of a photo was on disk");
            Assert.IsNull(files.Open(picked.File.Token), "a file of the session that ended is still handed to a request");
            Assert.AreEqual(1, CopiesOnDisk());
        }

        // Once it is let go the copy goes: at the next end of a session, or when the registry is made at the next start.
        _ = new PickedFiles(copies);
        Assert.AreEqual(0, CopiesOnDisk());
        await files.ClearAsync().WaitAsync(TimeSpan.FromSeconds(10));
    }

    [TestMethod]
    public async Task A_copy_still_being_made_when_the_session_ends_is_gone_when_the_clear_is_done_and_does_not_come_back()
    {
        PickedFiles files = new(copies);
        string path = Write("Anandpur Sahib.jpg", PhotoWithPlace());
        for (int round = 0; round < 60; round++)
        {
            // The photo is being read on another thread when the session ends: the two cross at a different moment in each round.
            long chosenIn = files.Session;
            Task<Picked> adding = Task.Run(() => files.AddAsync(path, Rule(), 0, chosenIn, CancellationToken.None));
            if (round % 3 == 1)
            {
                await Task.Yield();
            }
            else if (round % 3 == 2)
            {
                await Task.Delay(1);
            }

            await files.ClearAsync().WaitAsync(TimeSpan.FromSeconds(10));
            Assert.AreEqual(0, CopiesOnDisk(), "round " + round + ": a copy is on disk when the clear is done");

            Picked picked = await adding.WaitAsync(TimeSpan.FromSeconds(10));
            Assert.AreEqual(0, CopiesOnDisk(), "round " + round + ": a copy of the session that ended was written after the clear");
            Expect.That(picked.File is null || !files.Available(picked.File.Token), "round " + round + ": a file of the session that ended is still held");
        }
    }
}
