using Microsoft.UI;
using Microsoft.Windows.Storage.Pickers;

namespace Navree.Shell;

/// <summary>What a file field asks the picker for.</summary>
/// <param name="Types">The extensions to show, each with its dot, or <c>*</c> for every file.</param>
/// <param name="Multiple">Whether more than one file may be chosen.</param>
/// <param name="Title">The field's label, which titles the picker.</param>
public sealed record FilePickRequest(IReadOnlyList<string> Types, bool Multiple, string Title);

/// <summary>Where the files a person chooses for a field come from. The system's own picker in the product; a run that must not open one gives its own.</summary>
public interface IFilePicker
{
    /// <summary>Asks the person for files.</summary>
    /// <param name="request">What the field takes.</param>
    /// <param name="window">The top-level window the picker belongs to.</param>
    /// <returns>The paths chosen; none when the person cancelled.</returns>
    Task<IReadOnlyList<string>> PickAsync(FilePickRequest request, WindowId window);
}

/// <summary>
/// The system's file picker for a desktop app of the Windows App SDK: <c>Microsoft.Windows.Storage.Pickers.FileOpenPicker</c>, made with the
/// <c>WindowId</c> of the window it belongs to (no window handle is passed to it by hand), which answers the paths of the files chosen and
/// an empty list when the person closes it without choosing.
/// </summary>
internal sealed class SystemFilePicker : IFilePicker
{
    public async Task<IReadOnlyList<string>> PickAsync(FilePickRequest request, WindowId window)
    {
        ArgumentNullException.ThrowIfNull(request);
        FileOpenPicker picker = new(window);
        foreach (string type in request.Types)
        {
            picker.FileTypeFilter.Add(type);
        }

        if (request.Title.Length > 0)
        {
            picker.Title = request.Title;
        }

        if (request.Multiple)
        {
            IReadOnlyList<PickFileResult> chosen = await picker.PickMultipleFilesAsync();
            return [.. chosen.Select(file => file.Path)];
        }

        PickFileResult? one = await picker.PickSingleFileAsync();
        return one is null ? [] : [one.Path];
    }
}
