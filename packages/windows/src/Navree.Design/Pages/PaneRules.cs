namespace Navree.Design.Pages;

/// <summary>When a sheet at the side (or a dialog) is waiting for what it is to show.</summary>
public static class PaneRules
{
    /// <summary>
    /// A sheet is opened first and filled when the server's answer comes, so a sheet that holds nothing yet is waiting for its body, and says so with the
    /// same indication a page that is loading gives (an indeterminate bar), until something is in it. A sheet that holds children or actions is not waiting.
    /// </summary>
    /// <param name="children">How many parts the sheet holds.</param>
    /// <param name="actions">How many actions it holds.</param>
    /// <returns>True while the sheet is waiting for its body.</returns>
    public static bool WaitsForItsBody(int children, int actions) => children == 0 && actions == 0;
}
