namespace Navree.Presence.Tests;

[TestClass]
public sealed class CapabilityTests
{
    private static FeatureOffer Offer(IReadOnlyList<FeatureOffer> offers, SystemFeature feature) => offers.Single(item => item.Feature == feature);

    [TestMethod]
    public void EveryFeatureHasOneOfferInAFixedOrder()
    {
        IReadOnlyList<FeatureOffer> offers = Capabilities.For(hasIdentity: false, elevated: false);
        Assert.AreEqual(Enum.GetValues<SystemFeature>().Length, offers.Count);
        CollectionAssert.AreEqual(Enum.GetValues<SystemFeature>(), offers.Select(item => item.Feature).ToArray());
    }

    [TestMethod]
    public void AJumpListNeedsPackageIdentityAndIsNeverOfferedWithout()
    {
        FeatureOffer without = Offer(Capabilities.For(false, false), SystemFeature.JumpList);
        FeatureOffer with = Offer(Capabilities.For(true, false), SystemFeature.JumpList);
        Assert.IsFalse(without.Offered);
        Assert.IsTrue(without.NeedsIdentity);
        Assert.IsNotNull(without.Reason);
        Assert.IsTrue(with.Offered);
        Assert.IsNull(with.Reason);
    }

    [TestMethod]
    public void TheTaskbarCountIsAnOverlayWithoutIdentityAndTheSystemsBadgeWith()
    {
        Assert.AreEqual("an overlay icon on the taskbar button", Offer(Capabilities.For(false, false), SystemFeature.TaskbarCount).Means);
        Assert.AreEqual("the taskbar badge", Offer(Capabilities.For(true, false), SystemFeature.TaskbarCount).Means);
        Assert.IsTrue(Offer(Capabilities.For(false, false), SystemFeature.TaskbarCount).Offered);
        Assert.IsTrue(Offer(Capabilities.For(true, false), SystemFeature.TaskbarCount).Offered);
    }

    [TestMethod]
    public void OpeningAtSignInIsOfferedEitherWayWithADifferentMeans()
    {
        FeatureOffer plain = Offer(Capabilities.For(false, false), SystemFeature.StartAtSignIn);
        FeatureOffer packaged = Offer(Capabilities.For(true, false), SystemFeature.StartAtSignIn);
        Expect.That(plain.Offered && packaged.Offered, "both ways are offered");
        Assert.AreNotEqual(plain.Means, packaged.Means);
    }

    [TestMethod]
    public void NotificationsAreNotOfferedToAnAppThatRunsAsAnAdministrator()
    {
        FeatureOffer elevated = Offer(Capabilities.For(true, true), SystemFeature.Notifications);
        Assert.IsFalse(elevated.Offered);
        Expect.That(elevated.Reason!.Contains("administrator", StringComparison.Ordinal), "the reason names the cause");
        Assert.IsTrue(Offer(Capabilities.For(true, false), SystemFeature.Notifications).Offered);
    }

    [TestMethod]
    public void WindowsHelloAndPasskeysAreNeverOfferedForNoBuildHasProvedThem()
    {
        Assert.IsFalse(Offer(Capabilities.For(true, false), SystemFeature.Passkeys).Offered);
        Assert.IsFalse(Offer(Capabilities.For(false, false), SystemFeature.Passkeys).Offered);
    }

    [TestMethod]
    public void TheProtocolCapsListOnlyWhatIsCarriedOut()
    {
        IReadOnlyList<string> plain = Capabilities.ProtocolCaps(Capabilities.For(false, false));
        CollectionAssert.AreEqual(new[] { "clipboard", "open_url", "open_settings", "share", "local_notification" }, plain.ToArray());
        IReadOnlyList<string> elevated = Capabilities.ProtocolCaps(Capabilities.For(false, true));
        Expect.That(!elevated.Contains("local_notification"), "an elevated run lists no local notification");
        Expect.That(elevated.Contains("share"), "it still lists the share sheet");

        // A sign-in session is listed last, and only for an app with a scheme of its own, on which the sign-in comes back.
        IReadOnlyList<string> withScheme = Capabilities.ProtocolCaps(Capabilities.For(false, false), "charkha");
        CollectionAssert.AreEqual(new[] { "clipboard", "open_url", "open_settings", "share", "local_notification", "auth_session" }, withScheme.ToArray());
    }

    [TestMethod]
    public void HasSaysWhetherAFeatureIsOffered()
    {
        IReadOnlyList<FeatureOffer> offers = Capabilities.For(false, false);
        Assert.IsTrue(Capabilities.Has(offers, SystemFeature.Share));
        Assert.IsFalse(Capabilities.Has(offers, SystemFeature.JumpList));
    }

    [TestMethod]
    public void ThePackageQuestionIsAnsweredByTheSystemsErrorCode()
    {
        Assert.IsTrue(PackageIdentity.Decides(122));
        Assert.IsFalse(PackageIdentity.Decides(15700));
        Assert.IsFalse(PackageIdentity.Decides(0));
    }

    [TestMethod]
    public void ATestHostHasNoPackageIdentity()
    {
        Assert.IsFalse(PackageIdentity.Has());
    }
}
