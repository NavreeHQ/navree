using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Navree.Presence;

namespace Navree.Shell;

/// <summary>
/// One card of the settings page, drawn as the system's own Settings app draws its cards: a quiet panel with a thin edge and 4 pixel
/// corners, the title and a line under it at the start, the control at the end. The toolkit's own settings cards are not used.
/// </summary>
internal sealed class SettingsCardView : ContentControl
{
    internal SettingsCardView(string title, string description, FrameworkElement? control)
    {
        Title = title;
        IsTabStop = false;
        HorizontalContentAlignment = HorizontalAlignment.Stretch;
        Grid grid = new() { Padding = new Thickness(16, 12, 16, 12), ColumnSpacing = 16, MinHeight = 56 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        StackPanel text = new() { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
        text.Children.Add(Look.Label(title, "BodyTextBlockStyle"));
        if (description.Length > 0)
        {
            text.Children.Add(Look.Label(description, "CaptionTextBlockStyle", muted: true));
        }

        grid.Children.Add(text);
        if (control is not null)
        {
            Grid.SetColumn(control, 1);
            control.VerticalAlignment = VerticalAlignment.Center;
            grid.Children.Add(control);
        }

        Content = new Border
        {
            Background = Look.Resource("CardBackgroundFillColorDefaultBrush"),
            BorderBrush = Look.Resource("CardStrokeColorDefaultBrush"),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(4),
            Child = grid,
        };
    }

    /// <summary>The card's title, which is also what assistive technology calls it.</summary>
    internal string Title { get; }

    /// <inheritdoc />
    protected override AutomationPeer OnCreateAutomationPeer() => new SettingsCardPeer(this);
}

/// <summary>What assistive technology is told of a settings card: a group with the card's title.</summary>
internal sealed class SettingsCardPeer(SettingsCardView owner) : FrameworkElementAutomationPeer(owner)
{
    /// <inheritdoc />
    protected override string GetClassNameCore() => nameof(SettingsCardView);

    /// <inheritdoc />
    protected override AutomationControlType GetAutomationControlTypeCore() => AutomationControlType.Group;

    /// <inheritdoc />
    protected override string GetNameCore() => owner.Title;
}

/// <summary>
/// The settings page: what the app does in Windows (the icon in the notification area, opening at sign-in, the count on the taskbar,
/// notifications), then About, last, with what this build can do here and what it cannot and why.
/// </summary>
internal sealed class SettingsPage : ScrollViewer
{
    private readonly SystemPlace place;
    private readonly StackPanel panel = new() { Spacing = 4, MaxWidth = 1000, HorizontalAlignment = HorizontalAlignment.Left, Padding = new Thickness(36, 20, 36, 36) };
    private bool refreshing;

    internal SettingsPage(SystemPlace place)
    {
        this.place = place;
        VerticalScrollBarVisibility = ScrollBarVisibility.Auto;
        HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled;
        Content = panel;
        AutomationProperties.SetName(this, "Settings");
        place.Changed += Refresh;
        Unloaded += (_, _) => place.Changed -= Refresh;
        Refresh();
    }

    /// <summary>Draws the page again from what the person chose and what the build can do.</summary>
    internal void Refresh()
    {
        refreshing = true;
        try
        {
            panel.Children.Clear();
            TextBlock title = Look.Label("Settings", "TitleTextBlockStyle");
            AutomationProperties.SetHeadingLevel(title, AutomationHeadingLevel.Level1);
            title.Margin = new Thickness(0, 0, 0, 12);
            panel.Children.Add(title);

            string? group = null;
            foreach (Navree.Presence.SettingsCard card in place.Cards())
            {
                if (card.Group != group)
                {
                    group = card.Group;
                    Heading(group);
                }

                panel.Children.Add(Card(card));
            }

            Heading("About");
            foreach ((string line, string text) in place.AboutLines())
            {
                panel.Children.Add(new SettingsCardView(line, text, null));
            }
        }
        finally
        {
            refreshing = false;
        }
    }

    private void Heading(string text)
    {
        TextBlock heading = Look.Label(text, "BodyStrongTextBlockStyle");
        heading.Margin = new Thickness(0, 20, 0, 4);
        AutomationProperties.SetHeadingLevel(heading, AutomationHeadingLevel.Level2);
        panel.Children.Add(heading);
    }

    private SettingsCardView Card(Navree.Presence.SettingsCard card)
    {
        FrameworkElement? control = null;
        switch (card.Kind)
        {
            case CardKind.Switch:
                ToggleSwitch toggle = new() { IsOn = card.On, IsEnabled = card.Enabled, OnContent = "On", OffContent = "Off", MinWidth = 0 };
                AutomationProperties.SetName(toggle, card.Title);
                AutomationProperties.SetHelpText(toggle, card.Description);
                toggle.Toggled += (_, _) =>
                {
                    if (!refreshing)
                    {
                        place.Choose(card.Id, toggle.IsOn);
                    }
                };
                control = toggle;
                break;
            case CardKind.Button:
                Button button = new() { Content = card.Button ?? "Open", IsEnabled = card.Enabled };
                AutomationProperties.SetName(button, card.Title);
                button.Click += (_, _) => place.Invoke(card.Id);
                control = button;
                break;
            default:
                break;
        }

        return new SettingsCardView(card.Title, card.Description, control);
    }
}
