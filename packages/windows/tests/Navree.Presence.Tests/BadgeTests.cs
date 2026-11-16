namespace Navree.Presence.Tests;

[TestClass]
public sealed class BadgeTests
{
    [TestMethod]
    public void NoCountOrZeroOrLessClearsTheBadge()
    {
        Assert.IsNull(BadgeRules.Shown(null));
        Assert.IsNull(BadgeRules.Shown(0));
        Assert.IsNull(BadgeRules.Shown(-3));
        Assert.IsNull(BadgeRules.Label(null));
        Assert.AreEqual(0u, BadgeRules.ForSystem(0));
        Assert.IsNull(BadgeRules.OverlayText(0));
    }

    [TestMethod]
    public void ACountIsItsDigitsUpToNinetyNineAndThenNinetyNinePlus()
    {
        Assert.AreEqual("1", BadgeRules.Label(1));
        Assert.AreEqual("99", BadgeRules.Label(99));
        Assert.AreEqual("99+", BadgeRules.Label(100));
        Assert.AreEqual("99+", BadgeRules.Label(100000));
    }

    [TestMethod]
    public void TheSystemsBadgeIsGivenOnePastTheLargestSoItWritesThePlusItself()
    {
        Assert.AreEqual(7u, BadgeRules.ForSystem(7));
        Assert.AreEqual(99u, BadgeRules.ForSystem(99));
        Assert.AreEqual(100u, BadgeRules.ForSystem(100));
        Assert.AreEqual(100u, BadgeRules.ForSystem(5000000000));
    }

    [TestMethod]
    public void AnOverlayHoldsTwoCharactersSoALargeCountIsNinePlus()
    {
        Assert.AreEqual("7", BadgeRules.OverlayText(7));
        Assert.AreEqual("42", BadgeRules.OverlayText(42));
        Assert.AreEqual("9+", BadgeRules.OverlayText(100));
    }

    [TestMethod]
    public void AScreenReaderIsToldTheAppAndTheCount()
    {
        Assert.AreEqual("Navree Demo, 7 unread", BadgeRules.Description("Navree Demo", 7));
        Assert.AreEqual("Navree Demo", BadgeRules.Description("Navree Demo", null));
        Assert.AreEqual("Navree Demo, 99+ unread", BadgeRules.Description("Navree Demo", 400));
    }

    [TestMethod]
    public void AnOverlayIsASquareOfStraightPixelsWithItsCornersClear()
    {
        byte[] pixels = IconArt.Overlay("7", 16);
        Assert.AreEqual(16 * 16 * 4, pixels.Length);
        Assert.AreEqual(0, pixels[3], "the top left corner is clear");
        Assert.AreEqual(0, pixels[(15 * 16 + 15) * 4 + 3], "the bottom right corner is clear");
        bool anyRed = false;
        bool anyWhite = false;
        for (int at = 0; at < pixels.Length; at += 4)
        {
            if (pixels[at + 3] == 0xFF && pixels[at + 2] == 0xC4 && pixels[at + 1] == 0x2B && pixels[at] == 0x1C)
            {
                anyRed = true;
            }

            if (pixels[at + 3] == 0xFF && pixels[at] == 0xFF && pixels[at + 1] == 0xFF && pixels[at + 2] == 0xFF)
            {
                anyWhite = true;
            }
        }

        Expect.That(anyRed, "the disc is red");
        Expect.That(anyWhite, "the count is written in white");
    }

    [TestMethod]
    public void TheCharactersAreEnlargedToTheRoomAndNeverBelowOne()
    {
        Assert.AreEqual(2, IconArt.Scale(16, 2));
        Assert.AreEqual(2, IconArt.Scale(16, 1));
        Assert.AreEqual(4, IconArt.Scale(32, 2));
        Assert.AreEqual(1, IconArt.Scale(8, 3));
    }

    [TestMethod]
    public void TwoCountsDrawTwoDifferentPictures()
    {
        CollectionAssert.AreNotEqual(IconArt.Overlay("7", 16), IconArt.Overlay("8", 16));
        CollectionAssert.AreEqual(IconArt.Overlay("42", 16), IconArt.Overlay("42", 16));
    }

    [TestMethod]
    public void TheAppsOwnIconIsARoundedSquareWithAnN()
    {
        byte[] pixels = IconArt.App(32);
        Assert.AreEqual(32 * 32 * 4, pixels.Length);
        Assert.AreEqual(0, pixels[3], "the corner is clear");
        Assert.AreEqual(0xFF, pixels[(16 * 32 + 3) * 4 + 3], "the body is solid");
    }

    [TestMethod]
    public void AnIconCanBeMadeFromPixelsAndGivenBack()
    {
        nint icon = IconHandle.Create(IconArt.Overlay("3", 16), 16);
        Assert.AreNotEqual(0, icon);
        IconHandle.Destroy(icon);
        IconHandle.Destroy(0);
    }

    [TestMethod]
    public void PixelsThatAreNotASquareOfThatSideAreRefused()
    {
        Assert.ThrowsExactly<ArgumentException>(() =>
        {
            _ = IconHandle.Create(new byte[10], 16);
        });
    }
}
