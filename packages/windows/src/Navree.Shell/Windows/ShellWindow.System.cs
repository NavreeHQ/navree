using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Presence;
using Windows.System;

namespace Navree.Shell;

// What the main window does beside showing the screen: the settings page as the last item of the navigation, a window of the front screen
// of its own, the full screen, the frame laid out from the right in a right-to-left language, and the offer to start again when the core stopped.
public sealed partial class ShellWindow
{
    private readonly Border settingsHost = new() { Visibility = Visibility.Collapsed };
    private SettingsPage? settingsPage;
    private bool settingsOpen;
    private bool rightToLeft;
    private InfoBar? stoppedBar;
    private OverlappedPresenter? windowed;

    /// <summary>Whether the window is laid out from the right.</summary>
    public bool IsRightToLeft => rightToLeft;

    /// <summary>Whether the settings page is in front.</summary>
    public bool SettingsOpen => settingsOpen;

    /// <summary>Whether the window takes the whole screen.</summary>
    public bool IsFullScreen => AppWindow.Presenter.Kind == AppWindowPresenterKind.FullScreen;

    // Called once, at the end of the constructor.
    private void InitializeSystem()
    {
        content.Children.Insert(2, settingsHost);
        navigation.IsSettingsVisible = true;

        // The window may be made no smaller than snap layouts allow; the maximise button stays the system's.
        if (AppWindow.Presenter is OverlappedPresenter presenter)
        {
            (int smallestWidth, int smallestHeight) = PlacementRules.Smallest(WindowFrame.ScaleOf(WinRT.Interop.WindowNative.GetWindowHandle(this)));
            presenter.PreferredMinimumWidth = smallestWidth;
            presenter.PreferredMinimumHeight = smallestHeight;
        }

        navigation.ItemInvoked += OnItemInvoked;
        root.KeyboardAccelerators.Add(Accelerator(VirtualKey.N, VirtualKeyModifiers.Control, () => host.Place?.OpenWindow()));
        root.KeyboardAccelerators.Add(Accelerator((VirtualKey)0xBC, VirtualKeyModifiers.Control, ShowSettings));
        root.KeyboardAccelerators.Add(Accelerator(VirtualKey.F11, VirtualKeyModifiers.None, ToggleFullScreen));
        host.CoreStopped += OfferRestart;
        if (host.HasStopped)
        {
            OfferRestart();
        }
    }

    private void OnItemInvoked(NavigationView sender, NavigationViewItemInvokedEventArgs args)
    {
        if (args.IsSettingsInvoked)
        {
            ShowSettings();
        }
        else if (settingsOpen)
        {
            HideSettings();
        }
    }

    /// <summary>Shows the settings page in place of the screen, and selects the navigation's last item.</summary>
    public void ShowSettings()
    {
        if (host.Place is null)
        {
            return;
        }

        settingsPage ??= new SettingsPage(host.Place);
        settingsHost.Child = settingsPage;
        settingsHost.Background = Look.Resource("SolidBackgroundFillColorBaseBrush");
        settingsHost.Visibility = Visibility.Visible;
        settingsOpen = true;
        updatingSelection = true;
        try
        {
            navigation.SelectedItem = navigation.SettingsItem;
        }
        finally
        {
            updatingSelection = false;
        }

        titleBar.IsBackButtonEnabled = true;
    }

    /// <summary>Takes the settings page away and shows the screen again.</summary>
    public void HideSettings()
    {
        if (!settingsOpen)
        {
            return;
        }

        settingsOpen = false;
        settingsHost.Visibility = Visibility.Collapsed;
        Show(host.State);
    }

    /// <summary>Takes the whole screen, or gives it back.</summary>
    public void ToggleFullScreen()
    {
        if (IsFullScreen)
        {
            AppWindow.SetPresenter(windowed ?? OverlappedPresenter.Create());
        }
        else
        {
            windowed = AppWindow.Presenter as OverlappedPresenter;
            AppWindow.SetPresenter(AppWindowPresenterKind.FullScreen);
        }
    }

    // The window's frame (the caption buttons, the title's side) is the window's own, not the content's: it is laid out from the right too.
    private void MirrorFrame(bool value)
    {
        rightToLeft = value;
        WindowFrame.SetRightToLeft(WinRT.Interop.WindowNative.GetWindowHandle(this), value);
    }

    // The core said it has stopped for good: nothing it held can be used, and the only way on is to start the app again.
    private void OfferRestart()
    {
        if (stoppedBar is not null)
        {
            return;
        }

        Button again = new() { Content = "Restart" };
        again.Click += (_, _) => host.Restart();
        stoppedBar = new InfoBar
        {
            IsOpen = true,
            IsClosable = false,
            Severity = InfoBarSeverity.Error,
            Title = "The app stopped",
            Message = "Something went wrong inside the app and it cannot go on. Restart it to carry on.",
            ActionButton = again,
        };
        messages.Children.Add(stoppedBar);
        Announce("The app stopped. Restart it to carry on.");
    }
}
