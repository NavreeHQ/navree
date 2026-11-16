using Navree.Design.Inputs;
using Navree.Runtime;

namespace Navree.Platform;

/// <summary>What came of offering files to a field.</summary>
/// <param name="Held">The files the field holds now.</param>
/// <param name="Added">The files that were added.</param>
/// <param name="Words">What is said at the field: why a file was not added, or nothing.</param>
public sealed record Offered(IReadOnlyList<FileRef> Held, IReadOnlyList<FileRef> Added, IReadOnlyList<string> Words);

/// <summary>
/// What a file field does with the files a person chose or dropped, with nothing of a window in it. Where the files come from is a source
/// that can fail in any way (a picker that cannot be opened, a dropped thing the system cannot read), and nothing it throws goes further:
/// the handler of a press or of a drop is the system's own, and an error that leaves it ends the app. What failed is said in words at the
/// field, and the files the field held stay as they were.
/// </summary>
public static class FileOffers
{
    /// <summary>Asks a picker for files.</summary>
    /// <param name="pick">Opens the picker and gives the paths chosen. It may throw anything.</param>
    /// <returns>The paths; when the picker failed, none, and the words to say.</returns>
    public static async Task<Dropped> ChosenAsync(Func<Task<IReadOnlyList<string>>> pick)
    {
        ArgumentNullException.ThrowIfNull(pick);
        try
        {
            return new Dropped(await pick().ConfigureAwait(false) ?? [], []);
        }
        catch (Exception)
        {
            return new Dropped([], [FileRules.PickerFailedWords]);
        }
    }

    /// <summary>Reads what was dropped on a field.</summary>
    /// <param name="read">Reads the dropped things from the system. It may throw anything.</param>
    /// <returns>
    /// The paths of the files that are on this computer, and the words for the things that are not (<see cref="FileRules.Sort"/>); when the
    /// drop could not be read at all, none, and the words to say.
    /// </returns>
    public static async Task<Dropped> DroppedAsync(Func<Task<IReadOnlyList<DroppedItem>>> read)
    {
        ArgumentNullException.ThrowIfNull(read);
        try
        {
            return FileRules.Sort(await read().ConfigureAwait(false) ?? []);
        }
        catch (Exception)
        {
            return new Dropped([], [FileRules.DropFailedWords]);
        }
    }

    /// <summary>
    /// Offers files to a field: each is checked, the ones that fit are added, and the others are said in words. A field that takes one file
    /// holds the last one that fits, and the one it held before is let go. It never throws.
    /// </summary>
    /// <param name="files">The registry of chosen files.</param>
    /// <param name="paths">Where the files are.</param>
    /// <param name="rule">What the field takes.</param>
    /// <param name="before">The files the field holds.</param>
    /// <param name="multiple">Whether the field takes more than one.</param>
    /// <param name="said">Words already to say, for things that could not be offered.</param>
    /// <returns>What the field holds now, what was added, and what is said.</returns>
    public static async Task<Offered> AddAsync(PickedFiles files, IEnumerable<string> paths, FieldRule rule, IReadOnlyList<FileRef> before, bool multiple, IReadOnlyList<string>? said = null)
    {
        ArgumentNullException.ThrowIfNull(files);
        ArgumentNullException.ThrowIfNull(paths);
        ArgumentNullException.ThrowIfNull(rule);
        ArgumentNullException.ThrowIfNull(before);
        List<FileRef> list = multiple ? [.. before] : [];
        long bytes = list.Sum(file => file.Size);
        List<string> words = [.. said ?? []];
        List<FileRef> added = [];

        // The session the person chose them in: a file still being looked at when that session ends is not held for whoever comes next.
        long chosenIn = files.Session;
        try
        {
            foreach (string path in paths)
            {
                long total = bytes;
                Picked result;
                try
                {
                    result = await Task.Run(() => files.AddAsync(path, rule, total, chosenIn, CancellationToken.None)).ConfigureAwait(false);
                }
                catch (Exception)
                {
                    result = new Picked(null, FileRules.DisplayName(path ?? string.Empty) + " could not be read, so it was not added.");
                }

                if (result.File is { } file)
                {
                    if (!multiple && added.Count > 0)
                    {
                        // A field that takes one file holds the last chosen.
                        files.Release(added[0].Token);
                        added.Clear();
                        list.Clear();
                        bytes = 0;
                    }

                    added.Add(file);
                    list.Add(file);
                    bytes += file.Size;
                }
                else if (result.Refusal is { } why)
                {
                    words.Add(why);
                }
            }
        }
        catch (Exception)
        {
            // The list of paths itself could not be read through: what was added so far stands, and the rest is said.
            words.Add(FileRules.DropFailedWords);
        }

        if (added.Count == 0)
        {
            return new Offered(before, added, words);
        }

        if (!multiple)
        {
            foreach (FileRef replaced in before)
            {
                files.Release(replaced.Token);
            }
        }

        return new Offered(list, added, words);
    }
}
