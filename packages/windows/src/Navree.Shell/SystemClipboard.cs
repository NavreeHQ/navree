using Windows.ApplicationModel.DataTransfer;

namespace Navree.Shell;

/// <summary>Puts text on the clipboard.</summary>
internal static class SystemClipboard
{
    /// <summary>Puts text on the clipboard. A secret is kept out of the history and out of what syncs to other devices.</summary>
    internal static void Put(string text, bool sensitive)
    {
        DataPackage package = new();
        package.SetText(text);
        if (sensitive)
        {
            global::Windows.ApplicationModel.DataTransfer.Clipboard.SetContentWithOptions(
                package,
                new ClipboardContentOptions { IsAllowedInHistory = false, IsRoamable = false });
        }
        else
        {
            global::Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
        }

        global::Windows.ApplicationModel.DataTransfer.Clipboard.Flush();
    }
}
