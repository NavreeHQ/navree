using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Navree.Design;
using Navree.Presence;
using Windows.Graphics;

namespace Navree.Shell;

/// <summary>
/// The small window by the notification-area icon: the app's name and what waits, its places, and three commands (open the window, settings,
/// quit). It is a transient surface, so it is made of acrylic, has no title bar, stays out of Alt+Tab and the taskbar, and goes away when
/// anything else is chosen.
/// </summary>
internal sealed class TrayWindow : Window
{
    private const double WidthEpx = 320;
    private const double RowEpx = 44;
    private const double ChromeEpx = 168;
    private readonly SystemPlace place;
    private readonly string app;
    private readonly TextBlock title;
    private readonly TextBlock waiting;
    private readonly ListView places = new() { SelectionMode = ListViewSelectionMode.None, IsItemClickEnabled = true };
    private int rows;
    private long hiddenAt;

    internal TrayWindow(SystemPlace place, string app)
    {
        this.place = place;
        this.app = app;
        Title = app;
        SystemBackdrop = new DesktopAcrylicBackdrop();
        AppWindow.SetPresenter(OverlappedPresenter.CreateForContextMenu());
        AppWindow.IsShownInSwitchers = false;
        if (AppWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.IsAlwaysOnTop = true;
        }

        title = Look.Label(app, "BodyStrongTextBlockStyle");
        waiting = Look.Label(string.Empty, "CaptionTextBlockStyle", muted: true);
        AutomationProperties.SetHeadingLevel(title, Microsoft.UI.Xaml.Automation.Peers.AutomationHeadingLevel.Level1);
        AutomationProperties.SetName(places, "Places");
        places.ItemClick += (_, e) =>
        {
            if (e.ClickedItem is FrameworkElement { Tag: int index })
            {
                place.OpenDestination(index);
            }
        };

        Button open = new() { Content = "Open in window", HorizontalAlignment = HorizontalAlignment.Stretch };
        if (Application.Current.Resources.TryGetValue("AccentButtonStyle", out object? accent) && accent is Style accentStyle)
        {
            open.Style = accentStyle;
        }

        open.Click += (_, _) => place.OpenMainWindow();
        Button settings = new() { Content = "Settings", HorizontalAlignment = HorizontalAlignment.Stretch };
        settings.Click += (_, _) => place.OpenSettings();
        Button quit = new() { Content = "Quit " + app, HorizontalAlignment = HorizontalAlignment.Stretch };
        quit.Click += (_, _) => place.Quit();

        StackPanel commands = new() { Spacing = 8, Margin = new Thickness(0, 8, 0, 0) };
        commands.Children.Add(open);
        Grid second = new() { ColumnSpacing = 8 };
        second.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        second.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(quit, 1);
        second.Children.Add(settings);
        second.Children.Add(quit);
        commands.Children.Add(second);

        StackPanel header = new() { Spacing = 2, Margin = new Thickness(4, 0, 4, 8) };
        header.Children.Add(title);
        header.Children.Add(waiting);

        Grid root = new() { Padding = new Thickness(12), RowSpacing = 0 };
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.Children.Add(header);
        Grid.SetRow(places, 1);
        root.Children.Add(places);
        Grid.SetRow(commands, 2);
        root.Children.Add(commands);
        Content = root;
        root.KeyDown += (_, e) =>
        {
            if (e.Key == Windows.System.VirtualKey.Escape)
            {
                HideNow();
                e.Handled = true;
            }
        };

        Activated += (_, e) =>
        {
            if (e.WindowActivationState == WindowActivationState.Deactivated)
            {
                HideNow();
            }
        };
    }

    /// <summary>Shows what the window holds: the places, and how many things wait.</summary>
    internal void Refresh(IReadOnlyList<PlaceDestination> destinations, long? count)
    {
        title.Text = app;
        waiting.Text = BadgeRules.Label(count) is string said ? said + " waiting" : "Nothing waiting";
        places.Items.Clear();
        for (int at = 0; at < destinations.Count; at++)
        {
            PlaceDestination destination = destinations[at];
            Grid row = new() { Height = RowEpx - 4, Tag = at, ColumnSpacing = 12 };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            FontIcon icon = Look.Glyph(SystemGlyphs.For(null, destination.Label));
            row.Children.Add(icon);
            TextBlock label = Look.Label(destination.Label, "BodyTextBlockStyle");
            label.VerticalAlignment = VerticalAlignment.Center;
            Grid.SetColumn(label, 1);
            row.Children.Add(label);
            if (int.TryParse(destination.Badge, out int number) && number > 0)
            {
                TextBlock badge = Look.Label(BadgeRules.Label(number) ?? string.Empty, "CaptionTextBlockStyle", muted: true);
                badge.VerticalAlignment = VerticalAlignment.Center;
                Grid.SetColumn(badge, 2);
                row.Children.Add(badge);
                AutomationProperties.SetName(row, destination.Label + ", " + number);
            }
            else
            {
                AutomationProperties.SetName(row, destination.Label);
            }

            places.Items.Add(row);
        }

        rows = destinations.Count;
    }

    /// <summary>
    /// Shows the window by the icon, or takes it away when it is showing. A click on the icon takes the window's focus first, which hides
    /// it, so a click that comes right after the window hid itself is the one that meant to close it.
    /// </summary>
    internal void Toggle(Box icon)
    {
        if (IsShown)
        {
            HideNow();
            return;
        }

        if (Environment.TickCount64 - hiddenAt < 400)
        {
            return;
        }

        ShowNear(icon);
    }

    private void ShowNear(Box icon)
    {
        RectInt32 area = DisplayArea.GetFromPoint(new PointInt32(icon.CentreX, icon.CentreY), DisplayAreaFallback.Nearest).WorkArea;
        nint handle = Microsoft.UI.Win32Interop.GetWindowFromWindowId(AppWindow.Id);
        double scale = WindowFrame.ScaleOf(handle);
        int width = (int)(WidthEpx * scale);
        int height = (int)((ChromeEpx + RowEpx * Math.Min(Math.Max(rows, 1), 8)) * scale);
        (int left, int top) = TrayRules.Place(icon, width, height, new Box(area.X, area.Y, area.X + area.Width, area.Y + area.Height));
        AppWindow.MoveAndResize(new RectInt32(left, top, width, height));
        AppWindow.Show();
        Activate();
        WindowFrame.Raise(handle);
    }

    /// <summary>Whether the window is showing.</summary>
    internal bool IsShown => AppWindow.IsVisible;

    /// <summary>Takes the window away.</summary>
    internal void HideNow()
    {
        hiddenAt = Environment.TickCount64;
        AppWindow.Hide();
    }
}
