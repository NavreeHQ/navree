namespace Navree.Presence.Tests;

/// <summary>Signing in through the system's browser on Windows, and what is done with an address that arrives on the app's scheme.</summary>
[TestClass]
public sealed class BrowserSignInTests
{
    [TestMethod]
    public void Signing_in_through_the_browser_is_not_offered_and_no_run_offers_the_apps_scheme()
    {
        Expect.That(!BrowserSignIn.Offered, "a sign-in in the browser is offered");
        foreach ((bool identity, bool elevated) in new[] { (false, false), (true, false), (false, true), (true, true) })
        {
            FeatureOffer scheme = Capabilities.For(identity, elevated).Single(offer => offer.Feature == SystemFeature.ProtocolReturn);
            Expect.That(!scheme.Offered, "the app's scheme is offered with identity " + identity + ", elevated " + elevated);
            Expect.That(!string.IsNullOrEmpty(scheme.Reason), "the settings page has no words for why it is not");
            Expect.That(!Capabilities.ProtocolCaps(Capabilities.For(identity, elevated)).Contains("auth_session"), "the core is told the build has a sign-in session");
        }
    }

    [TestMethod]
    public void A_sign_in_the_core_asks_for_is_not_started_while_it_is_not_offered()
    {
        Assert.IsNotNull(BrowserSignIn.WhyNotStart(offered: false), "a sign-in in the browser would start");
        Assert.IsNull(BrowserSignIn.WhyNotStart(offered: true));
    }

    [TestMethod]
    public void An_address_on_the_apps_scheme_is_neither_opened_nor_taken_as_a_return_while_sign_in_through_the_browser_is_not_offered()
    {
        string[] arriving =
        [
            "charkha://signed-in?code=PLANTED000&state=ISIjJCUmJygpKissLS4vMA",
            "charkha://signed-in?code=PLANTED000",
            "charkha://orders/7",
            "charkha:/signed-in",
            "http://127.0.0.1:5123/cb?code=PLANTED000",
        ];
        foreach (string address in arriving)
        {
            Assert.AreEqual(Arrival.Dropped, BrowserSignIn.Route(offered: false, signInOpen: true, address), address + " with a sign-in open");
            Assert.AreEqual(Arrival.Dropped, BrowserSignIn.Route(offered: false, signInOpen: false, address), address + " with none open");
        }
    }

    [TestMethod]
    public void Once_offered_every_arrival_goes_to_the_core_as_it_came_which_decides()
    {
        // The core takes a return only while the sign-in it opened is open, on the way back it named and with its state; anything else it refuses.
        const string planted = "charkha://signed-in?code=PLANTED000&state=not-the-state";
        Assert.AreEqual(Arrival.SignInDone, BrowserSignIn.Route(offered: true, signInOpen: true, planted));
        Assert.AreEqual(Arrival.OpenLink, BrowserSignIn.Route(offered: true, signInOpen: false, planted));
        Assert.AreEqual(Arrival.Dropped, BrowserSignIn.Route(offered: true, signInOpen: true, string.Empty));
    }

    [TestMethod]
    public void The_apps_id_given_to_the_hand_over_is_the_package_family_name_when_packaged_and_the_configured_id_when_not()
    {
        Assert.AreEqual("CharkhaSupply_8wekyb3d8bbwe", BrowserSignIn.AppId("CharkhaSupply_8wekyb3d8bbwe", "com.charkha.supply"));
        Assert.AreEqual("com.charkha.supply", BrowserSignIn.AppId(null, "com.charkha.supply"));
        Assert.AreEqual("com.charkha.supply", BrowserSignIn.AppId(string.Empty, "com.charkha.supply"));
        Assert.IsNull(BrowserSignIn.AppId(null, null));
    }
}
