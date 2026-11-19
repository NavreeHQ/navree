using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Navree.Design;
using Navree.Platform;
using Navree.Presence;
using Navree.Runtime;
using Windows.Graphics;

namespace Navree.Shell;

/// <summary>
/// A screen of the app in a window of its own: another window over the same core, showing one screen of the state the core holds and kept up to
/// date with it. Everything the person does in it goes to the core as for any screen. Where a screen leads (a link to another) is the
/// app's one stack, which the main window shows, so the main window is brought forward when that changes.
/// </summary>
internal sealed class ScreenWindow : Window
{
    private readonly ShellHost host;
    private readonly long screenId;
    private readonly ScreenPresenter presenter;
    private readonly TitleBar titleBar = new() { IsBackButtonVisible = false, IsPaneToggleButtonVisible = false };
    private readonly Grid root = new();
    private readonly Action raiseMain;
    private readonly Dialogs dialogs;
    private long? frontWas;

    internal ScreenWindow(ShellHost host, long screenId, Action raiseMain, bool rightToLeft = false)
    {
        this.host = host;
        this.screenId = screenId;
        this.raiseMain = raiseMain;
        presenter = new ScreenPresenter(host);
        dialogs = new Dialogs(host, root, new SidePane()) { Own = screenId };
        Title = host.Options.AppName;
        SystemBackdrop = new MicaBackdrop { Kind = Microsoft.UI.Composition.SystemBackdrops.MicaKind.Base };
        ExtendsContentIntoTitleBar = true;
        titleBar.Title = host.Options.AppName;
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        root.Children.Add(titleBar);
        Grid.SetRow(presenter, 1);
        root.Children.Add(presenter);
        Content = root;
        SetTitleBar(titleBar);
        ApplyLeastSize();
        root.Loaded += (_, _) =>
        {
            ApplyLeastSize();
            if (root.XamlRoot is { } xamlRoot)
            {
                xamlRoot.Changed += (_, _) => ApplyLeastSize();
            }
        };

        if (rightToLeft)
        {
            root.FlowDirection = FlowDirection.RightToLeft;
            WindowFrame.SetRightToLeft(Microsoft.UI.Win32Interop.GetWindowFromWindowId(AppWindow.Id), true);
        }

        WindowVisibility.Follow(this, host);
        frontWas = host.State.Nav.Front;
        host.StateChanged += Show;
        Closed += (_, _) =>
        {
            host.StateChanged -= Show;
            dialogs.WindowClosed();

            // A web page that was shown in this window is closed with it.
            presenter.ReleaseWebs();
        };
        Show(host.State);
    }

    // The least width, in device pixels at the scale the window has now.
    private void ApplyLeastSize()
    {
        if (AppWindow.Presenter is OverlappedPresenter overlapped)
        {
            double scale = root.XamlRoot?.RasterizationScale ?? WindowFrame.ScaleOf(Microsoft.UI.Win32Interop.GetWindowFromWindowId(AppWindow.Id));
            overlapped.PreferredMinimumWidth = WindowRules.Raw(Navree.Presence.PlacementRules.SmallestWidth, scale);
        }
    }

    /// <summary>The screen this window shows.</summary>
    internal long ScreenId => screenId;

    /// <summary>Places the window and sizes it, in effective pixels, a step from the window it was opened from.</summary>
    internal void Place(Box from, Box work, int widthEpx, int heightEpx)
    {
        nint handle = Microsoft.UI.Win32Interop.GetWindowFromWindowId(AppWindow.Id);
        double scale = WindowFrame.ScaleOf(handle);
        int width = WindowRules.Raw(widthEpx, scale);
        int height = WindowRules.Raw(heightEpx, scale);
        (int left, int top) = PlacementRules.Cascade(from, work, width, height);
        AppWindow.MoveAndResize(new RectInt32(left, top, width, height));
    }

    private void Show(ViewState state)
    {
        if (!state.Screens.TryGetValue(screenId, out ScreenView? screen))
        {
            // The screen is no longer held by the core (it was closed or replaced): the window has nothing left to show.
            Close();
            return;
        }

        double width = root.ActualWidth > 0 ? root.ActualWidth : 840;
        if (CoverRules.Decide(state).BlankSecondaryWindow)
        {
            // While a locked cover stands over the main window, secondary windows show nothing of the person before and close any web view.
            titleBar.Subtitle = string.Empty;
            Title = host.Options.AppName;
            presenter.Show(null, WindowRules.ClassOf(width));
            presenter.ReleaseWebs();
            dialogs.Update(state, null, modalInFront: false);
            frontWas = state.Nav.Front;
            return;
        }

        string? title = screen.Title;
        titleBar.Subtitle = title ?? string.Empty;
        Title = title is null ? host.Options.AppName : title + " - " + host.Options.AppName;
        presenter.Show(screen, WindowRules.ClassOf(width));

        // The questions and sheets of this window's screen are shown in this window, not left waiting for an answer nobody can see.
        dialogs.Update(state, presenter.Context, modalInFront: false);

        // A link followed in this window leads the app's one stack somewhere else, which the main window shows.
        if (frontWas is not null && state.Nav.Front != frontWas && state.Nav.Front != screenId)
        {
            raiseMain();
        }

        frontWas = state.Nav.Front;
    }
}
