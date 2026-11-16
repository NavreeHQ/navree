using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;

namespace Navree.Shell;

/// <summary>Tells the host when a window of the app is shown and when it is not (minimised, hidden or closed), so the core is told the app went to the background and came back.</summary>
internal static class WindowVisibility
{
    /// <summary>Follows a window for the life of the window.</summary>
    /// <param name="window">The window.</param>
    /// <param name="host">The app's host.</param>
    internal static void Follow(Window window, ShellHost host)
    {
        window.AppWindow.Changed += (sender, args) =>
        {
            if (args.DidVisibilityChange || args.DidPresenterChange)
            {
                host.WindowShown(window, sender.IsVisible && (sender.Presenter as OverlappedPresenter)?.State != OverlappedPresenterState.Minimized);
            }
        };
        window.Closed += (_, _) => host.WindowShown(window, false);
    }
}
