using Navree.Design;

namespace Navree.Protocol.Tests;

/// <summary>What the core is told when the person's system changes, and when the app goes to the background and comes back.</summary>
[TestClass]
public sealed class ClientSettingsTests
{
    private static readonly ClientSettings Told = new(Dark: false, MoreContrast: false, TextPercent: 100, Locale: "en-IN", TimeZone: "Asia/Kolkata");

    [TestMethod]
    public void Nothing_that_is_the_same_is_told_again()
    {
        Assert.IsTrue((Told with { }).Since(Told).IsEmpty);
    }

    [TestMethod]
    public void Each_change_is_told_alone_and_what_did_not_change_is_left_out()
    {
        SettingsChange dark = (Told with { Dark = true }).Since(Told);
        Assert.AreEqual(true, dark.Dark);
        Assert.IsNull(dark.Contrast);
        Assert.IsNull(dark.TextPercent);

        Assert.AreEqual("more", (Told with { MoreContrast = true }).Since(Told).Contrast);
        Assert.AreEqual("normal", Told.Since(Told with { MoreContrast = true }).Contrast);
        Assert.AreEqual(150, (Told with { TextPercent = 150 }).Since(Told).TextPercent);
        Assert.AreEqual("pa-IN", (Told with { Locale = "pa-IN" }).Since(Told).Locale);
        Assert.AreEqual("Asia/Colombo", (Told with { TimeZone = "Asia/Colombo" }).Since(Told).TimeZone);
    }

    [TestMethod]
    public void Several_changes_at_once_are_told_together()
    {
        SettingsChange change = new ClientSettings(true, true, 125, "en-IN", "Asia/Kolkata").Since(Told);
        Assert.AreEqual(true, change.Dark);
        Assert.AreEqual("more", change.Contrast);
        Assert.AreEqual(125, change.TextPercent);
        Assert.IsNull(change.Locale);
        Assert.IsFalse(change.IsEmpty);
    }

    [TestMethod]
    public void The_app_goes_to_the_background_when_its_last_window_is_not_shown_and_comes_back_with_the_first_that_is()
    {
        ForegroundTracker app = new();
        object first = new();
        object second = new();
        Assert.IsTrue(app.InFront);
        Assert.IsNull(app.Moved(first, true), "a window shown at the start moves nothing");
        Assert.IsNull(app.Moved(second, true), "a second window moves nothing");
        Assert.IsNull(app.Moved(first, false), "one window left shown keeps the app in front");
        Assert.AreEqual(false, app.Moved(second, false));
        Assert.IsFalse(app.InFront);
        Assert.IsNull(app.Moved(second, false), "a window that says the same twice moves nothing");
        Assert.AreEqual(true, app.Moved(first, true));
        Assert.IsTrue(app.InFront);
    }
}
