using Navree.Design;
using Navree.Protocol;

namespace Navree.Protocol.Tests;

/// <summary>The person's menu: when the server's already signs them out, how they are named to a screen reader, and when two menus are the same.</summary>
[TestClass]
public sealed class AccountRulesTests
{
    private static Models.Node Item(string label, string href, Models.Method method = Models.Method.Get) =>
        new Models.Button { Label = label, Action = new Models.ActionRequest { Href = href, Method = method } };

    [TestMethod]
    public void A_menu_with_an_item_that_says_sign_out_or_posts_to_a_sign_out_address_has_one()
    {
        Assert.IsTrue(AccountRules.OffersSignOut([Item("Profile", "/profile"), Item("Sign out", "/anywhere")]));
        Assert.IsTrue(AccountRules.OffersSignOut([Item("Leave", "/auth/sign-out", Models.Method.Post)]));
        Assert.IsTrue(AccountRules.OffersSignOut([new Models.Menu { Label = "More", Items = [Item("Log out", "/x")] }]));
    }

    [TestMethod]
    public void A_menu_with_neither_does_not_and_a_get_of_a_sign_out_page_is_not_signing_out()
    {
        Assert.IsFalse(AccountRules.OffersSignOut([]));
        Assert.IsFalse(AccountRules.OffersSignOut([Item("Profile", "/profile"), Item("Settings", "/settings")]));
        Assert.IsFalse(AccountRules.OffersSignOut([Item("Open", "/sign-out", Models.Method.Get)]));
    }

    [TestMethod]
    public void A_screen_reader_reads_the_name_and_the_address_as_one_sentence()
    {
        Assert.AreEqual("Signed in as Harpreet Kaur, harpreet@example.in", AccountRules.SignedInAs(new Models.User { Name = "Harpreet Kaur", Email = "harpreet@example.in" }));
        Assert.AreEqual("Signed in as Gurpreet Singh", AccountRules.SignedInAs(new Models.User { Name = "Gurpreet Singh" }));
    }

    [TestMethod]
    public void A_person_read_afresh_is_the_same_person_and_a_changed_menu_is_not()
    {
        Models.User one = new() { Name = "Harpreet Kaur", Menu = [Item("Profile", "/profile")] };
        Models.User again = new() { Name = "Harpreet Kaur", Menu = [Item("Profile", "/profile")] };
        Models.User changed = new() { Name = "Harpreet Kaur", Menu = [Item("Profile", "/me")] };
        Assert.IsFalse(ReferenceEquals(one, again));
        Assert.AreEqual(AccountRules.Key(one), AccountRules.Key(again));
        Assert.AreNotEqual(AccountRules.Key(one), AccountRules.Key(changed));
        Assert.AreEqual(string.Empty, AccountRules.Key(null));
    }
}
