using System.Runtime.InteropServices;
using Navree.Design.Inputs;
using Navree.Runtime;

namespace Navree.Platform.Tests;

/// <summary>Whatever the picker or a drop throws, the field says it in words and holds what it held: nothing leaves the handler.</summary>
[TestClass]
public sealed class FileOffersTests
{
    private string folder = string.Empty;
    private string copies = string.Empty;

    [TestInitialize]
    public void Make()
    {
        folder = Path.Combine(Path.GetTempPath(), "navree-offers-" + Guid.NewGuid().ToString("N"));
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

    private string Write(string name, int length)
    {
        string path = Path.Combine(folder, name);
        byte[] bytes = new byte[length];
        new Random(length).NextBytes(bytes);
        bytes[0] = 0x25;
        File.WriteAllBytes(path, bytes);
        return path;
    }

    private static FieldRule Rule(long most = FileRules.MostBytes) => new(null, true, false, most);

    // The ways a source of files fails: at once, later, and with what the system throws for a thing it cannot read.
    private static IEnumerable<Func<Task<T>>> Failing<T>() =>
    [
        () => throw new InvalidOperationException("the picker could not be opened"),
        () => Task.FromException<T>(new COMException("the drop holds nothing that can be read", unchecked((int)0x80040064))),
        async () =>
        {
            await Task.Yield();
            throw new UnauthorizedAccessException();
        },
        () => throw new NullReferenceException(),
        () => Task.FromCanceled<T>(new CancellationToken(true)),
    ];

    [TestMethod]
    public async Task A_picker_that_fails_in_any_way_gives_no_file_and_words_and_throws_nothing()
    {
        foreach (Func<Task<IReadOnlyList<string>>> pick in Failing<IReadOnlyList<string>>())
        {
            Dropped chosen = await FileOffers.ChosenAsync(pick);
            Assert.AreEqual(0, chosen.Paths.Count);
            CollectionAssert.AreEqual(new[] { FileRules.PickerFailedWords }, chosen.Words.ToArray());
        }

        // A picker the person closed without choosing says nothing; one that gives no list at all is the same.
        Dropped closed = await FileOffers.ChosenAsync(() => Task.FromResult<IReadOnlyList<string>>([]));
        Assert.AreEqual(0, closed.Paths.Count + closed.Words.Count);
        Dropped none = await FileOffers.ChosenAsync(() => Task.FromResult<IReadOnlyList<string>>(null!));
        Assert.AreEqual(0, none.Paths.Count + none.Words.Count);
    }

    [TestMethod]
    public async Task A_drop_that_cannot_be_read_in_any_way_gives_no_file_and_words_and_throws_nothing()
    {
        foreach (Func<Task<IReadOnlyList<DroppedItem>>> read in Failing<IReadOnlyList<DroppedItem>>())
        {
            Dropped dropped = await FileOffers.DroppedAsync(read);
            Assert.AreEqual(0, dropped.Paths.Count);
            CollectionAssert.AreEqual(new[] { FileRules.DropFailedWords }, dropped.Words.ToArray());
        }

        // A drop that reads: the file with a path is offered, and a folder and a thing with no path here are said in words.
        string bill = Write("Bill of 16 November.pdf", 300);
        Dropped mixed = await FileOffers.DroppedAsync(() => Task.FromResult<IReadOnlyList<DroppedItem>>(
        [
            new DroppedItem("Bill of 16 November.pdf", bill, false),
            new DroppedItem("Bills", Path.Combine(folder, "Bills"), true),
            new DroppedItem("IMG_0042.JPG", null, false),
        ]));
        CollectionAssert.AreEqual(new[] { bill }, mixed.Paths.ToArray());
        Assert.AreEqual(2, mixed.Words.Count, string.Join(" | ", mixed.Words));
    }

    [TestMethod]
    public async Task A_file_chosen_in_a_picker_that_was_opened_before_the_session_ended_is_not_held_for_whoever_comes_next()
    {
        PickedFiles files = new(copies);

        // The person opens the picker. While it is open the session ends (the server signs them out); then they choose a file.
        long opened = files.Session;
        files.Clear();
        Offered late = await FileOffers.AddAsync(files, [Write("Lease.pdf", 200)], Rule(), [], multiple: true, session: opened);
        Assert.AreEqual(0, late.Added.Count, "a file chosen in a picker opened before the session ended is held for whoever comes next");
        Assert.AreEqual(0, late.Held.Count);
        Assert.AreEqual(1, late.Words.Count, string.Join(" | ", late.Words));
        StringAssert.Contains(late.Words[0], "has ended");

        // A picker opened since is of the session now, and so is a drop.
        Offered now = await FileOffers.AddAsync(files, [Write("Bill.pdf", 200)], Rule(), [], multiple: true, session: files.Session);
        Assert.AreEqual(1, now.Added.Count, string.Join(" | ", now.Words));
        Offered dropped = await FileOffers.AddAsync(files, [Write("Receipt.pdf", 200)], Rule(), now.Held, multiple: true);
        Assert.AreEqual(2, dropped.Held.Count, string.Join(" | ", dropped.Words));
    }

    [TestMethod]
    public async Task Files_that_cannot_be_read_are_said_in_words_and_the_field_holds_what_it_held()
    {
        PickedFiles files = new(copies);
        Offered first = await FileOffers.AddAsync(files, [Write("Lease.pdf", 200)], Rule(), [], multiple: true);
        Assert.AreEqual(1, first.Held.Count);

        // A file that is gone, a path that is no path, a path of nothing, and a file larger than the field takes.
        Offered offered = await FileOffers.AddAsync(
            files,
            [Path.Combine(folder, "gone.pdf"), "C:\\papers\\a\0b.pdf", string.Empty, null!, Write("Too large.pdf", 5000)],
            Rule(most: 1000),
            first.Held,
            multiple: true,
            said: ["A folder cannot be added."]);

        Assert.AreEqual(0, offered.Added.Count);
        CollectionAssert.AreEqual(first.Held.ToArray(), offered.Held.ToArray(), "the field no longer holds what it held");
        Assert.AreEqual(6, offered.Words.Count, string.Join(" | ", offered.Words));
        Assert.AreEqual("A folder cannot be added.", offered.Words[0]);
        Expect.That(files.Available(first.Held[0].Token), "the file the field held was let go");
    }

    [TestMethod]
    public async Task A_list_of_files_that_fails_while_it_is_read_keeps_what_was_added_before_it_failed()
    {
        PickedFiles files = new(copies);
        string lease = Write("Lease.pdf", 200);

        static IEnumerable<string> Breaking(string good)
        {
            yield return good;
            throw new COMException("the dropped things went away");
        }

        Offered offered = await FileOffers.AddAsync(files, Breaking(lease), Rule(), [], multiple: true);
        Assert.AreEqual(1, offered.Added.Count);
        Assert.AreEqual(1, offered.Held.Count);
        CollectionAssert.AreEqual(new[] { FileRules.DropFailedWords }, offered.Words.ToArray());
    }

    [TestMethod]
    public async Task A_field_that_takes_one_file_holds_the_last_that_fits_and_lets_go_of_the_others()
    {
        PickedFiles files = new(copies);
        Offered before = await FileOffers.AddAsync(files, [Write("Old.pdf", 100)], Rule(), [], multiple: false);
        FileRef old = before.Held.Single();

        Offered offered = await FileOffers.AddAsync(files, [Write("First.pdf", 200), Write("Second.pdf", 300)], Rule(), before.Held, multiple: false);
        FileRef held = offered.Held.Single();
        Assert.AreEqual("Second.pdf", held.Name);
        Assert.AreEqual(1, offered.Added.Count);
        Expect.That(files.Available(held.Token), "the file the field holds cannot be sent");
        Expect.That(!files.Available(old.Token), "the file the field held before is still held");

        // Nothing that fits: the field holds what it held.
        Offered none = await FileOffers.AddAsync(files, [Path.Combine(folder, "gone.pdf")], Rule(), offered.Held, multiple: false);
        Assert.AreEqual(held, none.Held.Single());
        Expect.That(files.Available(held.Token), "the file the field held was let go though nothing took its place");
    }
}
