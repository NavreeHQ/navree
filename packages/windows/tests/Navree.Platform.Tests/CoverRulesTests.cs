using System.Collections.Immutable;
using System.Text.Json.Nodes;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Platform.Tests;

/// <summary>
/// Locked covers are drawn across the whole main window with neither close button nor Escape dismissal, hide navigation and search,
/// never open as a ContentDialog or SidePane, and blank secondary windows until the cover goes.
/// </summary>
[TestClass]
public sealed class CoverRulesTests
{
    [TestMethod]
    public void Locked_modals_are_full_window_covers_never_dialogs_and_blank_secondary_windows()
    {
        ViewState ready = new(
            new AppPart { Name = "Charkha Supply Co.", User = new Models.User { Name = "Harpreet Sandhu", Key = "k1" } },
            new NavPart
            {
                Stage = Stage.Ready,
                Destinations = [new Destination { Label = "Orders", Href = "/orders", Stack = [1, 2] }],
                Selected = 0,
                Front = 2,
                CanGoBack = true,
            },
            ImmutableDictionary<long, ScreenView>.Empty
                .Add(1, new ScreenView { Id = 1, Url = "https://shop.example/orders", Title = "Orders" })
                .Add(2, new ScreenView { Id = 2, Url = "https://shop.example/orders/1", Title = "Order CS-20481" }),
            ImmutableList<Message>.Empty);

        CoverLayout normal = CoverRules.Decide(ready);
        Assert.IsFalse(normal.IsCovered);
        Assert.IsNull(normal.CoverScreenId);
        Assert.IsTrue(normal.CanGoBack);
        Assert.IsFalse(normal.BlankSecondaryWindow);

        // A locked sign-in modal goes up over a two-screen destination stack: the cover screen fills the main window, Back is disabled
        // because the cover's own stack has one screen, Dialogs skips locked modal screens, and secondary windows blank.
        ViewState covered = ready with
        {
            App = ready.App! with { User = null },
            Nav = ready.Nav with
            {
                Modals = [new Modal { Present = Models.Present.Full, Locked = true, Stack = [3] }],
                Front = 3,
                CanGoBack = false,
            },
            Screens = ready.Screens.Add(3, new ScreenView { Id = 3, Url = "https://shop.example/sign-in", Title = "Sign in", Chrome = false }),
        };

        CoverLayout singleStep = CoverRules.Decide(covered);
        Assert.IsTrue(singleStep.IsCovered);
        Assert.AreEqual(3L, singleStep.CoverScreenId);
        Assert.IsFalse(singleStep.CanGoBack, "Back on the root of a locked cover must not step back in the destination underneath");
        Assert.IsNull(singleStep.UnlockedModalAbove);
        Assert.IsTrue(singleStep.BlankSecondaryWindow);
        Assert.IsTrue(CoverRules.IsLockedModalScreen(covered, 3));
        Assert.IsFalse(CoverRules.IsLockedModalScreen(covered, 2));

        // A second step pushed inside the locked cover enables Back inside the cover while keeping everything underneath covered.
        ViewState secondStep = covered with
        {
            Nav = covered.Nav with
            {
                Modals = [new Modal { Present = Models.Present.Full, Locked = true, Stack = [3, 4] }],
                Front = 4,
                CanGoBack = true,
            },
            Screens = covered.Screens.Add(4, new ScreenView { Id = 4, Url = "https://shop.example/sign-in/otp", Title = "Enter code", Chrome = false }),
        };

        CoverLayout twoSteps = CoverRules.Decide(secondStep);
        Assert.IsTrue(twoSteps.IsCovered);
        Assert.AreEqual(4L, twoSteps.CoverScreenId);
        Assert.IsTrue(twoSteps.CanGoBack);
        Assert.IsTrue(twoSteps.BlankSecondaryWindow);
        Assert.IsTrue(CoverRules.IsLockedModalScreen(secondStep, 4));

        // An unlocked sheet opened from the cover itself sits above the cover as a dialog, without lifting the cover or unblanking secondary windows.
        ViewState helpAboveCover = covered with
        {
            Nav = covered.Nav with
            {
                Modals =
                [
                    new Modal { Present = Models.Present.Full, Locked = true, Stack = [3] },
                    new Modal { Present = Models.Present.Sheet, Locked = false, Stack = [5] },
                ],
                Front = 5,
                CanGoBack = true,
            },
            Screens = covered.Screens.Add(5, new ScreenView { Id = 5, Url = "https://shop.example/sign-in/help", Title = "Trouble signing in" }),
        };

        CoverLayout withUnlockedAbove = CoverRules.Decide(helpAboveCover);
        Assert.IsTrue(withUnlockedAbove.IsCovered);
        Assert.AreEqual(3L, withUnlockedAbove.CoverScreenId);
        Assert.AreEqual(5L, withUnlockedAbove.UnlockedModalAbove);
        Assert.IsTrue(withUnlockedAbove.CanGoBack);
        Assert.IsTrue(withUnlockedAbove.BlankSecondaryWindow);
        Assert.IsFalse(CoverRules.IsLockedModalScreen(helpAboveCover, 5));
    }

    [TestMethod]
    public async Task Core_sign_in_cover_and_failed_shell_conversations_hold_locked_cover_until_shell_answers()
    {
        string transcripts = Path.Combine(Repository.Root, "crates", "navree-native-core", "transcripts");
        foreach (string name in new[] { "sign-in-cover.json", "sign-in-shell-fails.json", "session-ended-outside.json" })
        {
            JsonNode transcript = JsonNode.Parse(await File.ReadAllTextAsync(Path.Combine(transcripts, name)))!;
            byte[] config = System.Text.Encoding.UTF8.GetBytes("""{"origin":"https://shop.example","client":{"platform":"android","form":"phone","vocab":0,"text_percent":100}}""");
            using CoreClient client = new(config, _ => { });
            bool sawCover = false;
            bool sawWaitingCover = false;

            foreach (JsonNode? step in transcript["steps"]!.AsArray())
            {
                JsonObject send = step!["send"]!.DeepClone().AsObject();
                if (step["body"] is JsonNode body)
                {
                    string text = await File.ReadAllTextAsync(Path.Combine(transcripts, "fixtures", body.GetValue<string>()));
                    send[send["t"]!.GetValue<string>() == "cache" ? "value" : "body"] = text;
                }

                Update update = await client.SendAsync(send.ToJsonString());
                CoverLayout layout = CoverRules.Decide(update.State);
                if (layout.IsCovered)
                {
                    sawCover = true;
                    Assert.IsNotNull(layout.CoverScreenId, name + ": covered state had no cover screen id");
                    Assert.IsTrue(layout.BlankSecondaryWindow, name + ": secondary windows were not blanked under a locked cover");
                    Assert.IsTrue(CoverRules.IsLockedModalScreen(update.State, layout.CoverScreenId.Value));
                    if (update.State.Screens.TryGetValue(layout.CoverScreenId.Value, out ScreenView? coverScreen)
                        && coverScreen.Content is BlankContent
                        && (coverScreen.Loading || coverScreen.Failure is not null))
                    {
                        sawWaitingCover = true;
                    }
                }
            }

            Expect.That(sawCover && sawWaitingCover, name + ": did not observe both the locked cover and the waiting cover");
            Assert.IsFalse(CoverRules.IsCovered(client.State), name + ": cover stayed up after the shell answered");
        }
    }
}
