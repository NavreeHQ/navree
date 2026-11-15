using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Navree.Design;
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
    private long? frontWas;

    internal ScreenWindow(ShellHost host, long screenId, Action raiseMain, bool rightToLeft = false)
    {
        this.host = host;
        this.screenId = screenId;
        this.raiseMain = raiseMain;
        presenter = new ScreenPresenter(host);
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

        frontWas = host.State.Nav.Front;
        host.StateChanged += Show;
        Closed += (_, _) => host.StateChanged -= Show;
        Show(host.State);
    }

    // The least width, in device pixels at the scale the window has now.
    private void ApplyLeastSize()
    {
        if (AppWindow.Presenter is OverlappedPresenter overlapped)
        {
            double scale = root.XamlRoot?.RasterizationScale ?? WindowFrame.ScaleOf(Microsoft.UI.Win32Interop.GetWindowFromWindowId(AppWindow.Id));
            overlapped.PreferredMinimumWidth = WindowRules.Raw(WindowRules.SmallestWindow, scale);
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

        string? title = screen.Title;
        titleBar.Subtitle = title ?? string.Empty;
        Title = title is null ? host.Options.AppName : title + " - " + host.Options.AppName;
        double width = root.ActualWidth > 0 ? root.ActualWidth : 840;
        presenter.Show(screen, WindowRules.ClassOf(width));

        // A link followed in this window leads the app's one stack somewhere else, which the main window shows.
        if (frontWas is not null && state.Nav.Front != frontWas && state.Nav.Front != screenId)
        {
            raiseMain();
        }

        frontWas = state.Nav.Front;
    }
}
