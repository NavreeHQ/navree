using System.Text;
using System.Text.Json;
using Navree.Protocol;

namespace Navree.Design;

/// <summary>What the person's menu holds and when the client offers a way out of its own.</summary>
public static class AccountRules
{
    /// <summary>Whether the menu the server gave the person already has a way to sign out: an item that says so, or one that posts to a sign-out address.</summary>
    /// <param name="menu">The items of the person's menu.</param>
    /// <returns>True when the server's menu signs the person out.</returns>
    public static bool OffersSignOut(IEnumerable<Models.Node> menu)
    {
        ArgumentNullException.ThrowIfNull(menu);
        foreach (Models.Node node in menu)
        {
            switch (node)
            {
                case Models.Button button when Says(button.Label) || Posts(button.Action):
                case Models.Link link when Says(link.Label) || Posts(link.Action):
                    return true;
                case Models.Menu inner when OffersSignOut(inner.Items):
                    return true;
                default:
                    break;
            }
        }

        return false;
    }

    /// <summary>What a person's name and address are said as, once: "Signed in as Harpreet Kaur, harpreet@example.in".</summary>
    /// <param name="user">The person.</param>
    /// <returns>The words a screen reader reads.</returns>
    public static string SignedInAs(Models.User user)
    {
        ArgumentNullException.ThrowIfNull(user);
        return string.IsNullOrEmpty(user.Email) ? "Signed in as " + user.Name : "Signed in as " + user.Name + ", " + user.Email;
    }

    /// <summary>What says that one person with one menu is the same as another, by what they hold and not by the object that holds it.</summary>
    /// <param name="user">The person, or nobody.</param>
    /// <returns>A key that is equal for equal people and menus.</returns>
    public static string Key(Models.User? user)
    {
        if (user is null)
        {
            return string.Empty;
        }

        using MemoryStream stream = new();
        using (Utf8JsonWriter writer = new(stream))
        {
            Models.WriteUser(writer, user);
        }

        return Encoding.UTF8.GetString(stream.ToArray());
    }

    private static bool Says(string label) =>
        label.Contains("sign out", StringComparison.OrdinalIgnoreCase)
        || label.Contains("sign-out", StringComparison.OrdinalIgnoreCase)
        || label.Contains("log out", StringComparison.OrdinalIgnoreCase)
        || label.Contains("logout", StringComparison.OrdinalIgnoreCase);

    private static bool Posts(Models.Action? action) =>
        action is Models.ActionRequest { Method: Models.Method.Post } request
        && (request.Href.EndsWith("/sign-out", StringComparison.OrdinalIgnoreCase)
            || request.Href.EndsWith("/logout", StringComparison.OrdinalIgnoreCase)
            || request.Href.EndsWith("/sign_out", StringComparison.OrdinalIgnoreCase));
}
