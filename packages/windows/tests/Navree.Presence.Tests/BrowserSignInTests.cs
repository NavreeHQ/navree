namespace Navree.Presence.Tests;

/// <summary>Signing in through the system's browser on Windows, and what is done with an address that arrives on the app's scheme.</summary>
[TestClass]
public sealed class BrowserSignInTests
{
    // As the core starts a sign-in for a Windows app the server lists: the state it made, and the callback the shell named.
    private const string State = "ISIjJCUmJygpKissLS4vMA";
    private const string Start = "https://shop.example/auth/app/sign-in?platform=windows&app=CharkhaSupply_8wekyb3d8bbwe&code_challenge=658WgAyQKf_KhWlXY9I8Os5xARz0DpNUrNgQIF4lD4c&code_challenge_method=S256&state=" + State + "&redirect_uri=charkha%3A%2Fsigned-in";

    private static readonly string[] NotASchemeOfTheApps = [string.Empty, " ", "charkha:/signed-in", "charkha://", "https", "http", "9charkha", "char kha", "https://shop.example/app/signed-in"];

    [TestMethod]
    public void Signing_in_through_the_browser_is_offered_and_every_run_offers_the_apps_scheme()
    {
        Expect.That(BrowserSignIn.Offered, "a sign-in in the browser is not offered");
        foreach ((bool identity, bool elevated) in new[] { (false, false), (true, false), (false, true), (true, true) })
        {
            FeatureOffer scheme = Capabilities.For(identity, elevated).Single(offer => offer.Feature == SystemFeature.ProtocolReturn);
            Expect.That(scheme.Offered, "the app's scheme is not offered with identity " + identity + ", elevated " + elevated);
            Assert.IsNull(scheme.Reason);
            Assert.AreEqual(identity ? "the package's own scheme" : "a scheme registered for this person at run time", scheme.Means);
            Expect.That(!scheme.NeedsIdentity, "the scheme is said to need a package, and a run with none registers it for this person");
        }
    }

    [TestMethod]
    public void A_run_can_catch_a_return_only_when_it_is_offered_and_the_app_has_a_scheme_of_its_own()
    {
        foreach (string scheme in new[] { "charkha", "CHARKHA", "com.charkha.supply", "charkha-supply+in" })
        {
            Expect.That(BrowserSignIn.ComesBack(offered: true, scheme), scheme + " is not taken for a scheme of the app's own");
            Expect.That(!BrowserSignIn.ComesBack(offered: false, scheme), scheme + " comes back with signing in through the browser turned off");
        }

        Expect.That(!BrowserSignIn.ComesBack(offered: true, null), "an app with no scheme can catch a return");
        foreach (string text in NotASchemeOfTheApps)
        {
            Expect.That(!BrowserSignIn.ComesBack(offered: true, text), "'" + text + "' is taken for a scheme of the app's own");
        }
    }

    [TestMethod]
    public void The_core_is_told_of_a_sign_in_session_only_by_a_run_that_can_catch_its_return()
    {
        foreach ((bool identity, bool elevated) in new[] { (false, false), (true, false), (false, true), (true, true) })
        {
            IReadOnlyList<FeatureOffer> offers = Capabilities.For(identity, elevated);
            Expect.That(Capabilities.ProtocolCaps(offers, "charkha").Contains("auth_session"), "a run with the app's scheme does not tell the core it has a sign-in session");
            Expect.That(!Capabilities.ProtocolCaps(offers).Contains("auth_session"), "a run with no scheme tells the core it has a sign-in session");
            Expect.That(!Capabilities.ProtocolCaps(offers, null).Contains("auth_session"), "a run with no scheme tells the core it has a sign-in session");
            foreach (string text in NotASchemeOfTheApps)
            {
                Expect.That(!Capabilities.ProtocolCaps(offers, text).Contains("auth_session"), "'" + text + "' made the run tell the core it has a sign-in session");
            }
        }

        // A run that does not offer the return (signing in through the browser turned off) tells the core nothing of it, whatever its scheme.
        IReadOnlyList<FeatureOffer> off =
        [
            .. Capabilities.For(false, false).Select(offer => offer.Feature == SystemFeature.ProtocolReturn ? offer with { Offered = false, Reason = "turned off" } : offer),
        ];
        Expect.That(!Capabilities.ProtocolCaps(off, "charkha").Contains("auth_session"), "the core is told the build has a sign-in session though the return is not offered");
    }

    [TestMethod]
    public void A_sign_in_the_core_asks_for_is_started_unless_signing_in_through_the_browser_is_turned_off()
    {
        Assert.IsNull(BrowserSignIn.WhyNotStart(BrowserSignIn.Offered), "a sign-in in the browser would not start");
        Assert.IsNull(BrowserSignIn.WhyNotStart(offered: true));
        Assert.IsNotNull(BrowserSignIn.WhyNotStart(offered: false), "a sign-in in the browser would start though it is turned off");
    }

    [TestMethod]
    public void A_sign_in_comes_back_on_the_apps_own_scheme_and_no_other_way()
    {
        Assert.IsNull(BrowserSignIn.WhyNotReturn(SignInReturn.Parse("charkha")!));
        Assert.IsNull(BrowserSignIn.WhyNotReturn(SignInReturn.Parse("charkha:/signed-in")!));
        Assert.IsNull(BrowserSignIn.WhyNotReturn(SignInReturn.Parse("charkha://signed-in")!));

        // A port of this machine and a link of the site are not ways back the server gives a Windows app: neither sign-in is started, and
        // no port is listened on.
        foreach (string other in new[] { "http://127.0.0.1:5123/signed-in", "http://127.0.0.1/signed-in", "http://localhost:9/cb", "http://[::1]:5123/cb", "https://shop.example/app/signed-in" })
        {
            SignInReturn comesBack = SignInReturn.Parse(other) ?? throw new AssertFailedException(other + " was not read");
            Assert.AreNotEqual(ReturnKind.Scheme, comesBack.Kind, other);
            Assert.IsNotNull(BrowserSignIn.WhyNotReturn(comesBack), "a sign-in that comes back on " + other + " would start");
        }
    }

    [TestMethod]
    public void The_return_the_server_sends_ends_only_the_sign_in_that_started_with_its_state()
    {
        SignInReturn comesBack = SignInReturn.Parse("charkha:/signed-in") ?? throw new AssertFailedException("the callback was not read");
        Assert.IsNull(BrowserSignIn.WhyNotReturn(comesBack));
        Expect.That(SignInRules.MayStart(Start), "the start address is not one a browser may be sent to");
        Assert.AreEqual(State, SignInRules.QueryValue(Start, "state"));

        SignInTracker tracker = new();
        DateTimeOffset now = new(2026, 11, 25, 10, 0, 0, TimeSpan.FromMinutes(330));
        int ticket = tracker.Begin(comesBack, now, TimeSpan.FromMinutes(10), SignInRules.QueryValue(Start, "state"));

        // Another state, no state, and this sign-in's state on another scheme: none is its return, and the sign-in stays open.
        Assert.AreEqual(SignInOffer.NotTheReturn, tracker.Offer("charkha:/signed-in?code=OTHER00000&state=not-the-state", out _));
        Assert.AreEqual(SignInOffer.NotTheReturn, tracker.Offer("charkha:/signed-in?code=OTHER00000", out _));
        Assert.AreEqual(SignInOffer.NotTheReturn, tracker.Offer("other:/signed-in?code=OTHER00000&state=" + State, out _));
        Expect.That(tracker.IsOpen, "an address that is not its return ended the sign-in");

        // Its own return ends it, once.
        Assert.AreEqual(SignInOffer.Taken, tracker.Offer("charkha:/signed-in?code=Tq7uX0b2c9&state=" + State, out int taken));
        Assert.AreEqual(ticket, taken);
        Expect.That(!tracker.IsOpen, "the sign-in is still open after its return");
        Assert.AreEqual(SignInOffer.NoneOpen, tracker.Offer("charkha:/signed-in?code=Tq7uX0b2c9&state=" + State, out _));
    }

    [TestMethod]
    public void Every_arrival_goes_to_the_core_as_it_came_which_decides()
    {
        // The core takes a return only while the sign-in it opened is open, on the way back it named and with its state; anything else it refuses.
        const string other = "charkha:/signed-in?code=OTHER00000&state=not-the-state";
        Assert.AreEqual(Arrival.SignInDone, BrowserSignIn.Route(BrowserSignIn.Offered, signInOpen: true, other));
        Assert.AreEqual(Arrival.OpenLink, BrowserSignIn.Route(BrowserSignIn.Offered, signInOpen: false, other));
        Assert.AreEqual(Arrival.SignInDone, BrowserSignIn.Route(offered: true, signInOpen: true, other));
        Assert.AreEqual(Arrival.OpenLink, BrowserSignIn.Route(offered: true, signInOpen: false, other));
        Assert.AreEqual(Arrival.Dropped, BrowserSignIn.Route(offered: true, signInOpen: true, string.Empty));
        Assert.AreEqual(Arrival.Dropped, BrowserSignIn.Route(offered: true, signInOpen: false, " "));
    }

    [TestMethod]
    public void Turned_off_an_address_on_the_apps_scheme_is_neither_opened_nor_taken_as_a_return()
    {
        string[] arriving =
        [
            "charkha://signed-in?code=OTHER00000&state=" + State,
            "charkha:/signed-in?code=OTHER00000&state=" + State,
            "charkha://signed-in?code=OTHER00000",
            "charkha://orders/7",
            "charkha:/signed-in",
        ];
        foreach (string address in arriving)
        {
            Assert.AreEqual(Arrival.Dropped, BrowserSignIn.Route(offered: false, signInOpen: true, address), address + " with a sign-in open");
            Assert.AreEqual(Arrival.Dropped, BrowserSignIn.Route(offered: false, signInOpen: false, address), address + " with none open");
        }
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
