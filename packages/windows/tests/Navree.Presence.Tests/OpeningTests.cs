namespace Navree.Presence.Tests;

[TestClass]
public sealed class OpeningTests
{
    private static readonly PlaceDestination[] Places =
    [
        new("Dashboard", "/", null),
        new("Orders", "/orders", "3"),
        new("Inventory", "/inventory", null),
    ];

    [TestMethod]
    public void OnlyAPathOnTheAppsOwnOriginIsTakenFromOutside()
    {
        Assert.IsTrue(OpeningRules.IsAppPath("/"));
        Assert.IsTrue(OpeningRules.IsAppPath("/orders/1?tab=items#top"));
        Assert.IsFalse(OpeningRules.IsAppPath(null));
        Assert.IsFalse(OpeningRules.IsAppPath(string.Empty));
        Assert.IsFalse(OpeningRules.IsAppPath("orders"));
        Assert.IsFalse(OpeningRules.IsAppPath("//evil.example/x"));
        Assert.IsFalse(OpeningRules.IsAppPath("/\\evil.example"));
        Assert.IsFalse(OpeningRules.IsAppPath("https://evil.example/"));
        Assert.IsFalse(OpeningRules.IsAppPath("/a\nb"));
        Assert.IsFalse(OpeningRules.IsAppPath("/a\0b"));
        Assert.IsFalse(OpeningRules.IsAppPath("/a\\b"));
        Assert.IsFalse(OpeningRules.IsAppPath("/" + new string('a', OpeningRules.Longest)));
    }

    [TestMethod]
    public void AnAddressThatIsADestinationSelectsIt()
    {
        OpenAction action = OpeningRules.Decide("/orders", Places);
        Assert.AreEqual(OpenKind.Select, action.Kind);
        Assert.AreEqual(1, action.Index);
        Assert.AreEqual(OpenKind.Select, OpeningRules.Decide("/orders/", Places).Kind);
        Assert.AreEqual(OpenKind.Select, OpeningRules.Decide("/orders?sort=total", Places).Kind);
        Assert.AreEqual(0, OpeningRules.Decide("/", Places).Index);
    }

    [TestMethod]
    public void AnyOtherPathOfTheAppIsALinkTheCoreDecidesOn()
    {
        OpenAction action = OpeningRules.Decide("/orders/1", Places);
        Assert.AreEqual(OpenKind.Link, action.Kind);
        Assert.AreEqual("/orders/1", action.Href);
    }

    [TestMethod]
    public void WhatIsNotAPathOfTheAppDoesNothing()
    {
        Assert.AreEqual(OpenKind.Nothing, OpeningRules.Decide("https://evil.example/", Places).Kind);
        Assert.AreEqual(OpenKind.Nothing, OpeningRules.Decide("//evil.example", Places).Kind);
        Assert.AreEqual(OpenKind.Nothing, OpeningRules.Decide(null, Places).Kind);
    }

    [TestMethod]
    public void AJumpListEntryCarriesTheAddressPercentEncodedAndReadsBackTheSame()
    {
        string? arguments = LaunchArguments.ForJumpList("/orders?status=open&sort=total");
        Assert.IsNotNull(arguments);
        Assert.IsFalse(arguments.Contains(' '));
        Assert.IsFalse(arguments.Contains('"'));
        LaunchRequest request = LaunchArguments.Read(LaunchArguments.Split(arguments), null);
        Assert.AreEqual("/orders?status=open&sort=total", request.OpenHref);
    }

    [TestMethod]
    public void AJumpListEntryIsMadeOnlyOfAPathOfTheApp()
    {
        Assert.IsNull(LaunchArguments.ForJumpList("https://evil.example/"));
        Assert.IsNull(LaunchArguments.ForJumpList("//evil.example"));
    }

    [TestMethod]
    public void TheCommandLineIsReadForTheFlagsThisBuildKnows()
    {
        LaunchRequest request = LaunchArguments.Read(["--background", "--new-window", "--open", "/inventory", "--unknown"], null);
        Assert.IsTrue(request.Background);
        Assert.IsTrue(request.NewWindow);
        Assert.AreEqual("/inventory", request.OpenHref);
        Assert.IsNull(request.Return);
        Assert.AreEqual(LaunchRequest.Plain, LaunchArguments.Read([], null));
    }

    [TestMethod]
    public void AnAddressInTheCommandLineThatIsNotAPathIsLeftOut()
    {
        Assert.IsNull(LaunchArguments.Read(["--open=https%3A%2F%2Fevil.example%2F"], null).OpenHref);
        Assert.IsNull(LaunchArguments.Read(["--open", "//evil.example"], null).OpenHref);
        Assert.IsNull(LaunchArguments.Read(["--open"], null).OpenHref);
    }

    [TestMethod]
    public void AnAddressOnTheAppsOwnSchemeIsTheReturnOfASignIn()
    {
        LaunchRequest request = LaunchArguments.Read(["navree-demo://signed-in?code=1&state=2"], "navree-demo");
        Assert.AreEqual("navree-demo://signed-in?code=1&state=2", request.Return);
        Assert.IsNull(LaunchArguments.Read(["other://signed-in"], "navree-demo").Return);
        Assert.IsNull(LaunchArguments.Read(["navree-demo://x"], null).Return);
    }

    [TestMethod]
    public void ALineOfArgumentsIsSplitAsTheSystemSplitsACommandLine()
    {
        CollectionAssert.AreEqual(new[] { "a", "b c", "d" }, LaunchArguments.Split("a \"b c\"   d").ToArray());
        CollectionAssert.AreEqual(new[] { "say \"hi\"" }, LaunchArguments.Split("\"say \\\"hi\\\"\"").ToArray());
        CollectionAssert.AreEqual(Array.Empty<string>(), LaunchArguments.Split("   ").ToArray());
        CollectionAssert.AreEqual(Array.Empty<string>(), LaunchArguments.Split(null).ToArray());
        CollectionAssert.AreEqual(new[] { string.Empty }, LaunchArguments.Split("\"\"").ToArray());
    }
}
