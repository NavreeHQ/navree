using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace Navree.Shell;

/// <summary>
/// A pane at the side of the window: where a long form or a detail goes when the window is wide enough to keep what is
/// behind it in view. Over a narrow window the same thing is a dialog. It has a title, a way to close it, and
/// its own scrolling.
/// </summary>
internal sealed class SidePane : Grid
{
    private readonly TextBlock title = Look.Label(string.Empty, "SubtitleTextBlockStyle");
    private readonly ContentControl body = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch };
    private Action? closed;
    private object? before;

    internal SidePane()
    {
        HorizontalAlignment = HorizontalAlignment.Right;
        Width = Navree.Design.WindowRules.SidePane;
        Visibility = Visibility.Collapsed;
        Background = Look.Resource("SolidBackgroundFillColorBaseBrush");
        BorderBrush = Look.Resource("DividerStrokeColorDefaultBrush");
        BorderThickness = new Thickness(1, 0, 0, 0);
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        Grid header = new() { Padding = new Thickness(20, 12, 8, 8), ColumnSpacing = 8 };
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        title.VerticalAlignment = VerticalAlignment.Center;
        AutomationProperties.SetHeadingLevel(title, Microsoft.UI.Xaml.Automation.Peers.AutomationHeadingLevel.Level2);
        header.Children.Add(title);
        Button close = new() { Content = Look.Glyph("", 14), Padding = new Thickness(10) };
        AutomationProperties.SetName(close, "Close");
        ToolTipService.SetToolTip(close, "Close");
        close.Click += (_, _) => Close();
        Grid.SetColumn(close, 1);
        header.Children.Add(close);
        Children.Add(header);

        ScrollViewer scroll = new() { Content = body, Padding = new Thickness(20, 0, 20, 20) };
        Grid.SetRow(scroll, 1);
        Children.Add(scroll);
        KeyDown += (_, e) =>
        {
            if (e.Key == Windows.System.VirtualKey.Escape && IsShowing)
            {
                e.Handled = true;
                Close();
            }
        };
    }

    /// <summary>What the pane holds.</summary>
    internal UIElement? Content => body.Content as UIElement;

    /// <summary>Whether the pane is showing.</summary>
    internal bool IsShowing => Visibility == Visibility.Visible;

    /// <summary>Shows what is in it.</summary>
    internal void Show(string heading, UIElement content, Action onClosed)
    {
        title.Text = heading;
        body.Content = content;
        closed = onClosed;
        before = XamlRoot is null ? null : Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(XamlRoot);
        Visibility = Visibility.Visible;

        // The focus goes into the pane, as it does into a dialog, and the pane closes on Escape.
        DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, () =>
        {
            if (Microsoft.UI.Xaml.Input.FocusManager.FindFirstFocusableElement(body) is Control first)
            {
                _ = first.Focus(FocusState.Programmatic);
            }
        });
    }

    /// <summary>Says what the pane is about, as the overlay now says.</summary>
    /// <param name="heading">The words.</param>
    internal void Retitle(string heading) => title.Text = heading;

    /// <summary>Puts new content in the pane that is showing.</summary>
    internal void Replace(UIElement content) => body.Content = content;

    /// <summary>Takes the pane away without telling anyone: what asked for it has already moved on.</summary>
    internal void Hide()
    {
        Visibility = Visibility.Collapsed;
        body.Content = null;
        closed = null;
        if (before is Control back && back.XamlRoot is not null)
        {
            _ = back.Focus(FocusState.Programmatic);
        }

        before = null;
    }

    private void Close()
    {
        Action? tell = closed;
        Hide();
        tell?.Invoke();
    }
}
