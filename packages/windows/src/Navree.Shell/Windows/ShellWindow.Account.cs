using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

// The person the app knows: a picture of them at the right of the title bar that opens their name, their address and what the server put in
// their menu (their profile, signing out). Nothing is shown while the app does not know who is using it.
public sealed partial class ShellWindow
{
    private Models.User? accountShown;

    private void ShowAccount(Models.User? user)
    {
        if (ReferenceEquals(user, accountShown))
        {
            return;
        }

        accountShown = user;
        if (user is null)
        {
            titleBar.RightHeader = null;
            return;
        }

        MenuFlyout menu = new();
        menu.Items.Add(new MenuFlyoutItem { Text = user.Name, IsEnabled = false });
        if (!string.IsNullOrEmpty(user.Email))
        {
            menu.Items.Add(new MenuFlyoutItem { Text = user.Email, IsEnabled = false });
        }

        if (user.Menu.Count > 0)
        {
            menu.Items.Add(new MenuFlyoutSeparator());
        }

        foreach (Models.Node node in user.Menu)
        {
            switch (node)
            {
                case Models.Link link:
                    menu.Items.Add(AccountItem(link.Label, link.Action, null, link.Id));
                    break;
                case Models.Button button when button.Action is not null:
                    menu.Items.Add(AccountItem(button.Label, button.Action, button.Ask, button.Id));
                    break;
                case Models.Divider:
                    menu.Items.Add(new MenuFlyoutSeparator());
                    break;
                default:
                    break;
            }
        }

        Button account = new()
        {
            Content = new PersonPicture { DisplayName = user.Name, Width = 28, Height = 28 },
            Padding = new Thickness(4),
            Background = new Microsoft.UI.Xaml.Media.SolidColorBrush(Microsoft.UI.Colors.Transparent),
            BorderThickness = new Thickness(0),
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(0, 0, 8, 0),
            Flyout = menu,
        };
        AutomationProperties.SetName(account, "Account, " + user.Name);
        ToolTipService.SetToolTip(account, user.Name);
        titleBar.RightHeader = account;
    }

    private MenuFlyoutItem AccountItem(string label, Models.Action action, Models.Question? ask, string? from)
    {
        MenuFlyoutItem item = new() { Text = label };
        item.Click += (_, _) => host.Send(Events.Act(host.State.Nav.Front, action, null, null, null, null, ask, from));
        return item;
    }
}
