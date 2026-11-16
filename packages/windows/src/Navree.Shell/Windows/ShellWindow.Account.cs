using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

// The person the app knows: a picture of them at the right of the title bar that opens their name, their address and what the server put in
// their menu (their profile, signing out). Nothing is shown while the app does not know who is using it.
public sealed partial class ShellWindow
{
    private string accountShown = string.Empty;

    private void ShowAccount(Models.User? user)
    {
        // The same person with the same menu, read afresh, leaves the menu as it is: it may be open.
        string key = AccountRules.Key(user);
        if (key == accountShown)
        {
            return;
        }

        accountShown = key;
        if (user is null)
        {
            titleBar.RightHeader = null;
            return;
        }

        MenuFlyout menu = new();

        // The name and the address are items a screen reader reaches (a disabled item is skipped); pressing one only closes the menu.
        MenuFlyoutItem who = new() { Text = user.Name };
        AutomationProperties.SetName(who, AccountRules.SignedInAs(user));
        menu.Items.Add(who);
        if (!string.IsNullOrEmpty(user.Email))
        {
            MenuFlyoutItem address = new() { Text = user.Email };
            AutomationProperties.SetName(address, "Email address, " + user.Email);
            menu.Items.Add(address);
        }

        if (user.Menu.Count > 0 || host.OffersOwnSignOut)
        {
            menu.Items.Add(new MenuFlyoutSeparator());
        }

        AddItems(menu.Items, user.Menu);
        if (host.OffersOwnSignOut)
        {
            MenuFlyoutItem signOut = new() { Text = "Sign out" };
            signOut.Click += (_, _) => _ = host.SignOutHereAsync();
            menu.Items.Add(signOut);
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

    // What the server put in the menu: a link, a button, a divider and a menu inside the menu; anything else it put there is left out.
    private void AddItems(IList<MenuFlyoutItemBase> items, IEnumerable<Models.Node> nodes)
    {
        foreach (Models.Node node in nodes)
        {
            switch (node)
            {
                case Models.Link link:
                    items.Add(AccountItem(link.Label, link.Action, null, link.Id));
                    break;
                case Models.Button button when button.Action is not null:
                    items.Add(AccountItem(button.Label, button.Action, button.Ask, button.Id));
                    break;
                case Models.Divider:
                    items.Add(new MenuFlyoutSeparator());
                    break;
                case Models.Menu inner:
                    MenuFlyoutSubItem sub = new() { Text = inner.Label };
                    AddItems(sub.Items, inner.Items);
                    items.Add(sub);
                    break;
                default:
                    break;
            }
        }
    }

    private MenuFlyoutItem AccountItem(string label, Models.Action action, Models.Question? ask, string? from)
    {
        MenuFlyoutItem item = new() { Text = label };
        item.Click += (_, _) => host.Send(Events.Act(host.State.Nav.Front, action, null, null, null, null, ask, from));
        return item;
    }
}
