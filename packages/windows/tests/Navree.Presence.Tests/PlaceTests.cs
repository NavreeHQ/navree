using Microsoft.Win32;

namespace Navree.Presence.Tests;

[TestClass]
public sealed class TrayTests
{
    private static readonly PlaceDestination[] Places = [new("Dashboard", "/", null), new("Orders", "/orders", "3"), new("Inventory", "/inventory", "New")];

    [TestMethod]
    public void TheMenuHoldsTheWindowThePlacesSettingsAndQuit()
    {
        IReadOnlyList<TrayEntry> menu = TrayRules.Menu(Places, "Navree Demo");
        CollectionAssert.AreEqual(
            new[] { "Open Navree Demo", string.Empty, "Dashboard", "Orders  (3)", "Inventory", string.Empty, "Settings", "Quit Navree Demo" },
            menu.Select(entry => entry.Label).ToArray());
        CollectionAssert.AreEqual(
            new[] { TrayCommand.OpenWindow, TrayCommand.Separator, TrayCommand.Destination, TrayCommand.Destination, TrayCommand.Destination, TrayCommand.Separator, TrayCommand.Settings, TrayCommand.Quit },
            menu.Select(entry => entry.Command).ToArray());
        Assert.AreEqual(1, menu[3].Index);
    }

    [TestMethod]
    public void WithNoPlacesYetTheMenuHasOnlyTheWindowSettingsAndQuit()
    {
        IReadOnlyList<TrayEntry> menu = TrayRules.Menu([], "Navree Demo");
        CollectionAssert.AreEqual(
            new[] { TrayCommand.OpenWindow, TrayCommand.Separator, TrayCommand.Settings, TrayCommand.Quit },
            menu.Select(entry => entry.Command).ToArray());
    }

    [TestMethod]
    public void ATooltipNamesTheAppAndTheCountAndFitsWhatTheSystemHolds()
    {
        Assert.AreEqual("Navree Demo, 7 unread", TrayRules.Tooltip("Navree Demo", 7));
        Assert.AreEqual("Navree Demo", TrayRules.Tooltip("Navree Demo", null));
        Assert.AreEqual(TrayRules.TooltipLongest, TrayRules.Tooltip(new string('n', 400), 5).Length);
    }

    [TestMethod]
    public void TheSmallWindowStandsAboveAnIconInTheLowerHalfCentredOnItAndInsideTheWorkArea()
    {
        Box work = new(0, 0, 1920, 1040);
        Box icon = new(1700, 1048, 1716, 1064);
        (int left, int top) = TrayRules.Place(icon, 320, 400, work);
        Assert.AreEqual(1708 - 160, left);
        Assert.AreEqual(work.Bottom - 400 - TrayRules.Gap, top, "above the icon, and no lower than the work area leaves room for");
    }

    [TestMethod]
    public void TheSmallWindowIsKeptInsideTheWorkAreaAtTheRightEdge()
    {
        Box work = new(0, 0, 1920, 1040);
        Box icon = new(1890, 1048, 1906, 1064);
        (int left, _) = TrayRules.Place(icon, 320, 400, work);
        Assert.AreEqual(1920 - 320 - TrayRules.Gap, left);
    }

    [TestMethod]
    public void TheSmallWindowStandsBelowAnIconAtTheTopOfTheScreen()
    {
        Box work = new(0, 40, 1920, 1080);
        Box icon = new(1700, 0, 1716, 32);
        (_, int top) = TrayRules.Place(icon, 320, 400, work);
        Assert.AreEqual(work.Top + TrayRules.Gap, top, "below the icon, and no higher than the work area");
    }

    [TestMethod]
    public void AWindowTallerThanTheRoomIsPutAtTheTopOfTheWorkArea()
    {
        Box work = new(0, 0, 1920, 300);
        Box icon = new(1700, 400, 1716, 416);
        (_, int top) = TrayRules.Place(icon, 320, 900, work);
        Assert.AreEqual(TrayRules.Gap, top);
    }

    [TestMethod]
    public void ClosingTheWindowOnlyHidesItWhenTheIconIsThereAndThePersonWantsThat()
    {
        Assert.IsTrue(TrayRules.ClosingHides(true, true));
        Assert.IsFalse(TrayRules.ClosingHides(true, false));
        Assert.IsFalse(TrayRules.ClosingHides(false, true));
    }

    [TestMethod]
    public void TheIconCanBeMadeAndGivenBackWhateverTheSystemSaysOfTheNotificationArea()
    {
        using TrayIcon icon = new();
        nint handle = IconHandle.Create(IconArt.App(16), 16);
        bool added = icon.Show("Navree test", handle);
        Assert.AreEqual(added, icon.IsAdded);
        icon.Hide();
        Assert.IsFalse(icon.IsAdded);
        IconHandle.Destroy(handle);
    }

    [TestMethod]
    public void AScreenCopyIsAPictureOfTheAskedSizeOrNothingWhenThereIsNoScreen()
    {
        byte[]? pixels = ScreenCapture.Grab(new Box(0, 0, 8, 6));
        Expect.That(pixels is null || pixels.Length == 8 * 6 * 4, "the pixels are the size asked for");
        Assert.IsNull(ScreenCapture.Grab(new Box(0, 0, 0, 6)));
        Assert.IsNull(ScreenCapture.Grab(new Box(0, 0, 99999, 6)));
    }
}

[TestClass]
public sealed class StartupTests
{
    private const string Exe = @"C:\Program Files\Navree Demo\Navree.Demo.exe";

    [TestMethod]
    public void TheCommandIsTheQuotedPathAndTheFlagThatSaysTheSystemStartedIt()
    {
        Assert.AreEqual("\"" + Exe + "\" --background", StartupRules.Command(Exe));
    }

    [TestMethod]
    public void AnEntryTooLongForTheListOrWithAQuoteInItsPathIsNotMade()
    {
        Assert.IsNull(StartupRules.Command(@"C:\" + new string('a', 300) + ".exe"));
        Assert.IsNull(StartupRules.Command("C:\\a\"b.exe"));
        Assert.IsNull(StartupRules.Command(string.Empty));
    }

    [TestMethod]
    public void AValueIsTheAppsOwnWhenItStartsWithTheAppsQuotedPath()
    {
        Assert.IsTrue(StartupRules.IsOurs("\"" + Exe + "\" --background", Exe));
        Assert.IsTrue(StartupRules.IsOurs("\"" + Exe.ToUpperInvariant() + "\"", Exe));
        Assert.IsFalse(StartupRules.IsOurs("\"C:\\other\\app.exe\" --background", Exe));
        Assert.IsFalse(StartupRules.IsOurs(null, Exe));
    }

    [TestMethod]
    public void WindowsKeepsWhetherThePersonTurnedAnEntryOffInTheFirstByte()
    {
        Assert.IsFalse(StartupRules.IsDisabled(null));
        Assert.IsFalse(StartupRules.IsDisabled([]));
        Assert.IsFalse(StartupRules.IsDisabled([0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        Assert.IsFalse(StartupRules.IsDisabled([0x06, 0, 0, 0]));
        Assert.IsTrue(StartupRules.IsDisabled([0x03, 0, 0, 0]));
        Assert.IsTrue(StartupRules.IsDisabled([0x07, 0, 0, 0]));
    }

    [TestMethod]
    public void AStartupTasksNumbersAreMappedAndOnlyOffAndOnAreTheAppsToChange()
    {
        Assert.AreEqual(StartupState.Off, StartupRules.FromTask(0));
        Assert.AreEqual(StartupState.DisabledByUser, StartupRules.FromTask(1));
        Assert.AreEqual(StartupState.On, StartupRules.FromTask(2));
        Assert.AreEqual(StartupState.ByPolicy, StartupRules.FromTask(3));
        Assert.AreEqual(StartupState.ByPolicy, StartupRules.FromTask(4));
        Assert.AreEqual(StartupState.Off, StartupRules.FromTask(99));
        Assert.IsTrue(StartupRules.AppMayChange(StartupState.On));
        Assert.IsTrue(StartupRules.AppMayChange(StartupState.Off));
        Assert.IsFalse(StartupRules.AppMayChange(StartupState.DisabledByUser));
        Assert.IsFalse(StartupRules.AppMayChange(StartupState.ByPolicy));
    }

    [TestMethod]
    public void TheRunKeyIsPutAndTakenAndThePersonsOwnChoiceIsRead()
    {
        string root = @"Software\Navree\Tests\" + Guid.NewGuid().ToString("N");
        string run = root + @"\Run";
        string approved = root + @"\Approved";
        try
        {
            RunKey key = new("NavreeTest", run, approved);
            Assert.AreEqual(StartupState.Off, key.State(Exe));
            Assert.IsTrue(key.Set(Exe, true));
            Assert.AreEqual(StartupState.On, key.State(Exe));
            using (RegistryKey values = Registry.CurrentUser.OpenSubKey(run)!)
            {
                Assert.AreEqual("\"" + Exe + "\" --background", values.GetValue("NavreeTest"));
            }

            using (RegistryKey choices = Registry.CurrentUser.CreateSubKey(approved))
            {
                choices.SetValue("NavreeTest", new byte[] { 0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 }, RegistryValueKind.Binary);
            }

            Assert.AreEqual(StartupState.DisabledByUser, key.State(Exe));
            Assert.IsTrue(key.Set(Exe, false));
            Assert.AreEqual(StartupState.Off, key.State(Exe));
            Assert.IsTrue(key.Set(Exe, false));
        }
        finally
        {
            Registry.CurrentUser.DeleteSubKeyTree(@"Software\Navree\Tests", throwOnMissingSubKey: false);
        }
    }

    [TestMethod]
    public void AnEntryOfAnotherProgramUnderTheSameNameIsNotTheAppsOn()
    {
        string root = @"Software\Navree\Tests\" + Guid.NewGuid().ToString("N");
        try
        {
            using (RegistryKey values = Registry.CurrentUser.CreateSubKey(root + @"\Run"))
            {
                values.SetValue("NavreeTest", "\"C:\\other\\app.exe\" --x");
            }

            RunKey key = new("NavreeTest", root + @"\Run", root + @"\Approved");
            Assert.AreEqual(StartupState.Off, key.State(Exe));
        }
        finally
        {
            Registry.CurrentUser.DeleteSubKeyTree(@"Software\Navree\Tests", throwOnMissingSubKey: false);
        }
    }
}

[TestClass]
public sealed class SettingsTests
{
    private static SettingsCard Card(IReadOnlyList<SettingsCard> cards, string id) => cards.Single(card => card.Id == id);

    [TestMethod]
    public void NothingIsOnUntilThePersonTurnsItOn()
    {
        Assert.IsFalse(Choices.Initial.NotificationAreaIcon);
        IReadOnlyList<SettingsCard> cards = SettingsRules.Cards(Capabilities.For(false, false), Choices.Initial, StartupState.Off);
        Assert.IsFalse(Card(cards, CardIds.Tray).On);
        Assert.IsFalse(Card(cards, CardIds.Startup).On);
    }

    [TestMethod]
    public void KeepingTheAppRunningNeedsTheIcon()
    {
        IReadOnlyList<SettingsCard> without = SettingsRules.Cards(Capabilities.For(false, false), Choices.Initial, StartupState.Off);
        Assert.IsFalse(Card(without, CardIds.KeepRunning).Enabled);
        Assert.IsFalse(Card(without, CardIds.KeepRunning).On);
        IReadOnlyList<SettingsCard> with = SettingsRules.Cards(Capabilities.For(false, false), Choices.Initial with { NotificationAreaIcon = true }, StartupState.Off);
        Assert.IsTrue(Card(with, CardIds.KeepRunning).Enabled);
        Assert.IsTrue(Card(with, CardIds.KeepRunning).On);
    }

    [TestMethod]
    public void OpeningAtSignInThePersonTurnedOffInWindowsCanOnlyBeTurnedOnThereAndTheCardSaysSo()
    {
        IReadOnlyList<SettingsCard> cards = SettingsRules.Cards(Capabilities.For(false, false), Choices.Initial, StartupState.DisabledByUser);
        SettingsCard startup = Card(cards, CardIds.Startup);
        Assert.IsFalse(startup.Enabled);
        Expect.That(startup.Description.Contains("Windows", StringComparison.Ordinal), "the card says where it was turned off");
        Assert.AreEqual(CardKind.Button, Card(cards, CardIds.StartupSettings).Kind);
    }

    [TestMethod]
    public void OpeningAtSignInThatAnOrganisationDecidesCannotBeChanged()
    {
        SettingsCard startup = Card(SettingsRules.Cards(Capabilities.For(true, false), Choices.Initial, StartupState.ByPolicy), CardIds.Startup);
        Assert.IsFalse(startup.Enabled);
        Assert.IsTrue(startup.On);
    }

    [TestMethod]
    public void ARunThatCannotShowNotificationsSaysWhyInPlaceOfAButton()
    {
        IReadOnlyList<SettingsCard> cards = SettingsRules.Cards(Capabilities.For(false, true), Choices.Initial, StartupState.Off);
        SettingsCard notifications = Card(cards, CardIds.NotificationSettings);
        Assert.AreEqual(CardKind.Info, notifications.Kind);
        Expect.That(notifications.Description.Contains("administrator", StringComparison.Ordinal), "the reason is the administrator's rights");
        Assert.AreEqual(CardKind.Button, Card(SettingsRules.Cards(Capabilities.For(false, false), Choices.Initial, StartupState.Off), CardIds.NotificationSettings).Kind);
    }

    [TestMethod]
    public void TheCardsStandUnderTheirHeadingsInOrder()
    {
        IReadOnlyList<SettingsCard> cards = SettingsRules.Cards(Capabilities.For(false, false), Choices.Initial, StartupState.Off);
        CollectionAssert.AreEqual(
            new[] { SettingsRules.General, SettingsRules.General, SettingsRules.General, SettingsRules.General, SettingsRules.Notifications },
            cards.Select(card => card.Group).ToArray());
    }

    [TestMethod]
    public void TheAboutSectionSaysWhatTheBuildIsAndWhatItCanDoHere()
    {
        IReadOnlyList<(string Title, string Line)> lines = SettingsRules.About("Navree Demo", "1.2.3", false, Capabilities.For(false, false));
        Assert.AreEqual("Version 1.2.3", lines[0].Line);
        Expect.That(lines[1].Line.StartsWith("No", StringComparison.Ordinal), "it says it has no package");
        Expect.That(lines.Any(line => line.Title == "Jump list" && line.Line.StartsWith("Not on offer", StringComparison.Ordinal)), "the jump list is not on offer without a package");
        Expect.That(lines.Any(line => line.Title == "Share" && line.Line.StartsWith("On offer", StringComparison.Ordinal)), "the share sheet is");
        Assert.AreEqual(2 + Enum.GetValues<SystemFeature>().Length, lines.Count);
    }

    [TestMethod]
    public void WhatThePersonChoseIsWrittenAndReadBack()
    {
        Choices chosen = new(true, false, false);
        Assert.AreEqual(chosen, Choices.Read(chosen.Write()));
        Assert.AreEqual(Choices.Initial, Choices.Read(null));
        Assert.AreEqual(Choices.Initial, Choices.Read("not json"));
        Assert.AreEqual(Choices.Initial, Choices.Read("[]"));
        Assert.AreEqual(new Choices(true, true, true), Choices.Read("{\"tray\":true,\"keep_running\":\"yes\"}"));
    }

    [TestMethod]
    public void TheChoicesAreKeptInAPlainFileOfTheAppsOwnFolder()
    {
        string folder = Path.Combine(Path.GetTempPath(), "navree-choices-" + Guid.NewGuid().ToString("N"));
        try
        {
            ChoicesFile file = new(folder);
            Assert.AreEqual(Choices.Initial, file.Read());
            Assert.IsTrue(file.Write(new Choices(true, true, false)));
            Assert.AreEqual(new Choices(true, true, false), new ChoicesFile(folder).Read());
            File.WriteAllText(Path.Combine(folder, ChoicesFile.Name), "damaged");
            Assert.AreEqual(Choices.Initial, file.Read());
        }
        finally
        {
            if (Directory.Exists(folder))
            {
                Directory.Delete(folder, recursive: true);
            }
        }
    }
}

[TestClass]
public sealed class PlacementTests
{
    private static readonly Box Work = new(0, 0, 1920, 1040);

    [TestMethod]
    public void TheFirstWindowIsCentredInTheWorkArea()
    {
        Assert.AreEqual((460, 120), PlacementRules.Cascade(null, Work, 1000, 800));
    }

    [TestMethod]
    public void ANewWindowIsAStepDownAndAcrossFromTheOneBefore()
    {
        (int left, int top) = PlacementRules.Cascade(new Box(100, 100, 1100, 900), Work, 1000, 800);
        Assert.AreEqual(100 + PlacementRules.Step, left);
        Assert.AreEqual(100 + PlacementRules.Step, top);
    }

    [TestMethod]
    public void WhenTheNextStepWouldNotFitTheCascadeStartsAgainAtTheCorner()
    {
        Assert.AreEqual((0, 0), PlacementRules.Cascade(new Box(900, 300, 1900, 1100), Work, 1000, 800));
    }

    [TestMethod]
    public void ANewWindowIsAsLargeAsTheOneItIsOpenedFromAndNeverSmallerThanTheSmallest()
    {
        Assert.AreEqual((1000, 800), PlacementRules.OpenSize(new Box(0, 0, 1000, 800), 500, 400));
        Assert.AreEqual((500, 400), PlacementRules.OpenSize(new Box(0, 0, 300, 200), 500, 400));
    }

    [TestMethod]
    public async Task ABuildWithNoAuthenticatorConfirmsNobody()
    {
        IDeviceAuthenticator none = new NoAuthenticator();
        Assert.IsFalse(await none.IsAvailableAsync(CancellationToken.None));
        Assert.AreEqual(DeviceAnswer.Unsupported, await none.ConfirmAsync("Confirm the refund", CancellationToken.None));
    }
}
